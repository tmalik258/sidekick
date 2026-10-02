//! Routines: what you open in the first hour of each day, learned so the
//! morning card can offer it back as one "Open all". Only first opens are
//! kept (an app's exe, a site's domain, never a page or a title), and only
//! on this PC. Ignored apps and sites never reach here (privacy filter).
//!
//! An item belongs to the routine when it was opened on at least 3 of the
//! last 5 same weekdays; with too few of those, the last 5 days of any kind
//! stand in (an everyday routine).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sidekick_core::{Event, RoutineOpen};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, executor, lock};

/// Opens after this long from the first activity of the day do not count.
const FIRST_HOUR: i64 = 60 * 60;
/// Before this local hour it is still last night.
const DAY_STARTS: u32 = 5;
const LOOK_BACK: usize = 5;
const MIN_DAYS: usize = 3;
/// Items on the card; "Open all" plus these fit Alt 1 to 4.
pub const MAX_SHOWN: usize = 3;
const MAX_ITEMS: usize = 8;
/// "Not today" three times in a row drops an item.
const MAX_SKIPS: u32 = 3;
/// After this many Open alls, offer to do it without asking.
const OFFER_AUTO_AFTER: u32 = 5;
/// Days of history kept.
const KEEP_DAYS: i64 = 60;
/// Gap between launches, so windows come up in the usual order.
const LAUNCH_GAP: Duration = Duration::from_millis(700);
/// Time for the windows to appear before the layout is put back.
const SETTLE: Duration = Duration::from_secs(6);

/// Shell windows, lock screens and Sidekick itself are not things you open.
const SKIP_EXES: &[&str] = &[
    "explorer.exe",
    "lockapp.exe",
    "searchhost.exe",
    "searchapp.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "applicationframehost.exe",
    "textinputhost.exe",
    "sidekick.exe",
    "sidekick-desktop.exe",
    "taskmgr.exe",
    "systemsettings.exe",
    "",
];

const BROWSERS: &[(&str, &str)] = &[
    ("chrome.exe", "chrome"),
    ("msedge.exe", "edge"),
    ("firefox.exe", "firefox"),
    ("brave.exe", "brave"),
    ("opera.exe", "opera"),
    ("zen.exe", "zen"),
    ("vivaldi.exe", "vivaldi"),
];

pub fn is_browser(exe: &str) -> bool {
    BROWSERS.iter().any(|(e, _)| *e == exe)
}

/// The browser id `open_url` knows, from the name the extension reports.
fn browser_id(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    BROWSERS
        .iter()
        .map(|(_, id)| *id)
        .find(|id| lower.contains(id))
        .unwrap_or_default()
        .to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub kind: String,
    pub key: String,
    pub label: String,
    pub target: String,
    pub browser: String,
    /// On how many of the days looked at.
    pub days: usize,
}

impl Item {
    pub fn id(&self) -> String {
        format!("{}:{}", self.kind, self.key)
    }
}

/// Small state that outlives a restart: skips per item, Open all count,
/// and the day the card was put off.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Memory {
    pub skips: BTreeMap<String, u32>,
    pub open_all: u32,
    pub hidden_day: String,
}

fn memory_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("routines.json"))
}

fn load(path: &Path) -> Memory {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn memory(app: &AppHandle) -> Memory {
    memory_path(app).map(|p| load(&p)).unwrap_or_default()
}

fn save(app: &AppHandle, m: &Memory) {
    if let (Some(p), Ok(json)) = (memory_path(app), serde_json::to_string(m)) {
        let _ = std::fs::write(p, json);
    }
}

/// The routine for `weekday` (0 is Monday) from stored opens, usual order
/// first. Items with `MAX_SKIPS` skips in a row are left out.
pub fn learn(rows: &[RoutineOpen], weekday: u32, skips: &BTreeMap<String, u32>) -> Vec<Item> {
    let mut all_days: Vec<&str> = rows.iter().map(|r| r.day.as_str()).collect();
    all_days.sort_unstable();
    all_days.dedup();
    all_days.reverse();
    let mut same: Vec<&str> = rows
        .iter()
        .filter(|r| r.weekday == weekday)
        .map(|r| r.day.as_str())
        .collect();
    same.sort_unstable();
    same.dedup();
    same.reverse();
    let days: Vec<&str> = if same.len() >= MIN_DAYS {
        same.into_iter().take(LOOK_BACK).collect()
    } else {
        all_days.into_iter().take(LOOK_BACK).collect()
    };
    if days.len() < MIN_DAYS {
        return Vec::new();
    }
    // (item, seq total) per id; the newest row wins for label and target.
    let mut found: BTreeMap<String, (Item, u32)> = BTreeMap::new();
    for r in rows.iter().rev().filter(|r| days.contains(&r.day.as_str())) {
        let entry = found.entry(format!("{}:{}", r.kind, r.key)).or_insert((
            Item {
                kind: r.kind.clone(),
                key: r.key.clone(),
                label: r.label.clone(),
                target: r.target.clone(),
                browser: r.browser.clone(),
                days: 0,
            },
            0,
        ));
        entry.0.days += 1;
        entry.1 += r.seq;
    }
    let mut items: Vec<(Item, f32)> = found
        .into_iter()
        .filter(|(id, (item, _))| {
            item.days >= MIN_DAYS && skips.get(id).copied().unwrap_or(0) < MAX_SKIPS
        })
        .map(|(_, (item, seq))| {
            let avg = seq as f32 / item.days as f32;
            (item, avg)
        })
        .collect();
    items.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut items: Vec<Item> = items.into_iter().map(|(i, _)| i).collect();
    // Sites open their browser anyway.
    let site_browsers: Vec<String> = items
        .iter()
        .filter(|i| i.kind == "site")
        .map(|i| browser_id(&i.browser))
        .collect();
    if !site_browsers.is_empty() {
        items.retain(|i| {
            i.kind != "app"
                || !BROWSERS
                    .iter()
                    .any(|(exe, id)| *exe == i.key && site_browsers.iter().any(|b| b == id))
        });
    }
    items.truncate(MAX_ITEMS);
    items
}

/// Today's routine, as learned so far.
pub fn today(app: &AppHandle) -> Vec<Item> {
    let since = (Local::now() - chrono::Duration::days(KEEP_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    let now = Local::now().format("%Y-%m-%d").to_string();
    // Today is still being written; only finished days teach.
    let rows: Vec<RoutineOpen> = lock(&app.state::<AppState>().storage)
        .opens_since(&since)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.day < now)
        .collect();
    learn(
        &rows,
        Local::now().weekday().num_days_from_monday(),
        &memory(app).skips,
    )
}

/// Fields for the morning card: `routine_count`, `routine_text`,
/// `item1`..`item3`, `offer_auto` and `auto`.
pub fn card_fields(items: &[Item], m: &Memory, auto: bool) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    out.insert("routine_count".into(), items.len().into());
    out.insert(
        "routine_text".into(),
        items
            .iter()
            .map(|i| i.label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
            .into(),
    );
    for n in 0..MAX_SHOWN {
        let label = items.get(n).map(|i| i.label.clone()).unwrap_or_default();
        out.insert(format!("item{}", n + 1), label.into());
    }
    let offer = !items.is_empty() && !auto && m.open_all >= OFFER_AUTO_AFTER;
    out.insert("offer_auto".into(), if offer { "1" } else { "" }.into());
    out.insert("auto".into(), if auto { "1" } else { "" }.into());
    out
}

struct Today {
    day: String,
    started: DateTime<Local>,
    seq: u32,
}

static TODAY: LazyLock<Mutex<Option<Today>>> = LazyLock::new(|| Mutex::new(None));
/// The domain in the active tab, for time per site.
static SITE: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new()));

/// The site in the browser's active tab, as last reported.
pub fn current_site() -> String {
    SITE.lock().map(|s| s.clone()).unwrap_or_default()
}

/// The day an event at `now` belongs to, and whether it is still in that
/// day's first hour. The first activity after 5 am starts the day.
fn in_first_hour(slot: &mut Option<Today>, now: DateTime<Local>) -> Option<(String, u32)> {
    if now.hour() < DAY_STARTS {
        return None;
    }
    let day = now.format("%Y-%m-%d").to_string();
    if slot.as_ref().is_none_or(|t| t.day != day) {
        *slot = Some(Today {
            day: day.clone(),
            started: now,
            seq: 0,
        });
    }
    let t = slot.as_mut()?;
    if (now - t.started).num_seconds() > FIRST_HOUR {
        return None;
    }
    t.seq += 1;
    Some((day, t.seq))
}

/// What a window or page event says was opened: (kind, key, label, target,
/// browser).
fn opened(event: &Event) -> Option<(String, String, String, String, String)> {
    let p = &event.payload;
    let s = |k: &str| p[k].as_str().unwrap_or_default().trim().to_owned();
    match event.kind.as_str() {
        "window.focused" => {
            let exe = s("exe");
            if SKIP_EXES.contains(&exe.as_str()) {
                return None;
            }
            let label = s("app");
            let label = if label.is_empty() {
                exe.trim_end_matches(".exe").to_owned()
            } else {
                label
            };
            Some(("app".into(), exe, label, s("path"), String::new()))
        }
        "browser.site" => {
            let domain = s("domain");
            if domain.is_empty() || !domain.contains('.') {
                return None;
            }
            Some((
                "site".into(),
                domain.clone(),
                domain.clone(),
                format!("https://{domain}/"),
                s("browser"),
            ))
        }
        _ => None,
    }
}

/// Called for every event that passed the privacy filter.
pub fn observe(app: &AppHandle, event: &Event) {
    if event.kind == "browser.site" {
        let domain = event.payload["domain"].as_str().unwrap_or_default();
        if let Ok(mut s) = SITE.lock() {
            *s = domain.to_owned();
        }
        // Split the browser's time at the new site.
        if let Some(w) = lock(&app.state::<AppState>().last_window).clone()
            && is_browser(w["exe"].as_str().unwrap_or_default())
        {
            crate::timetrack::on_window(app, &w);
        }
    }
    if !lock(&app.state::<AppState>().settings).routines {
        return;
    }
    let Some((kind, key, label, target, browser)) = opened(event) else {
        return;
    };
    let now = Local::now();
    let Some((day, seq)) = TODAY
        .lock()
        .ok()
        .and_then(|mut t| in_first_hour(&mut t, now))
    else {
        return;
    };
    let open = RoutineOpen {
        day,
        weekday: now.weekday().num_days_from_monday(),
        kind,
        key,
        label,
        target,
        browser,
        seq,
    };
    let storage = app.state::<AppState>().storage.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(err) = lock(&storage).record_open(&open) {
            log::warn!("could not record routine: {err}");
        }
    });
}

async fn open_item(app: &AppHandle, item: &Item) -> Result<(), String> {
    let exec = executor(&app.state::<AppState>());
    let result = match item.kind.as_str() {
        "site" => {
            let mut args = serde_json::json!({ "url": item.target });
            let id = browser_id(&item.browser);
            if !id.is_empty() && exec.capabilities().browser(&id).is_some() {
                args["browser"] = id.into();
            }
            exec.run("open_url", &args).await
        }
        _ if item.target.is_empty() => {
            return Err(format!(
                "{} has not been seen with its path yet",
                item.label
            ));
        }
        _ => {
            exec.run("open_path", &serde_json::json!({ "path": item.target }))
                .await
        }
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

/// Opens one item of today's routine (1-based, as shown on the card).
pub async fn open_one(app: &AppHandle, index: usize) -> Result<String, String> {
    let items = today(app);
    let item = items
        .get(index.saturating_sub(1))
        .ok_or("That item is no longer part of your routine")?;
    open_item(app, item).await?;
    let mut m = memory(app);
    m.skips.remove(&item.id());
    save(app, &m);
    Ok(format!("Opened {}", item.label))
}

/// Opens the whole routine in the usual order, then puts the windows back
/// where they were.
pub async fn open_all(app: &AppHandle, counted: bool) -> Result<String, String> {
    let items = today(app);
    if items.is_empty() {
        return Err("No routine yet. It takes three mornings to learn one.".into());
    }
    let mut failed = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(LAUNCH_GAP).await;
        }
        if let Err(err) = open_item(app, item).await {
            log::warn!("routine: {err}");
            failed.push(item.label.clone());
        }
    }
    let mut m = memory(app);
    for item in &items {
        m.skips.remove(&item.id());
    }
    if counted {
        m.open_all += 1;
    }
    save(app, &m);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SETTLE).await;
        if let Err(err) = crate::layout::restore(&app2).await {
            log::info!("routine layout: {err}");
        }
    });
    let opened = items.len() - failed.len();
    Ok(if failed.is_empty() {
        format!("Opened your {opened} usual things")
    } else {
        format!("Opened {opened}; could not open {}", failed.join(", "))
    })
}

/// "Not today": the card stays away until tomorrow, and each item takes a
/// skip. Three in a row and it leaves the routine.
pub fn skip_today(app: &AppHandle) -> String {
    let items = today(app);
    let mut m = memory(app);
    for item in &items {
        *m.skips.entry(item.id()).or_default() += 1;
    }
    m.hidden_day = Local::now().format("%Y-%m-%d").to_string();
    save(app, &m);
    "Not today. See you tomorrow.".into()
}

pub fn set_auto(app: &AppHandle, on: bool) -> Result<String, String> {
    let mut next = lock(&app.state::<AppState>().settings).clone();
    next.routines_auto = on;
    crate::commands::apply_settings(app, next)?;
    Ok(if on {
        "Your usual setup opens by itself each morning. Turn off in Settings > Privacy.".into()
    } else {
        "Back to asking first".into()
    })
}

/// Forgets every routine (Settings > Privacy).
pub fn forget(app: &AppHandle) -> Result<usize, String> {
    save(app, &Memory::default());
    lock(&app.state::<AppState>().storage)
        .clear_opens(None)
        .map_err(|e| e.to_string())
}

/// Drops history older than `KEEP_DAYS`.
pub fn prune(app: &AppHandle) {
    let before = (Local::now() - chrono::Duration::days(KEEP_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    let _ = lock(&app.state::<AppState>().storage).clear_opens(Some(&before));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(day: &str, weekday: u32, kind: &str, key: &str, seq: u32) -> RoutineOpen {
        RoutineOpen {
            day: day.into(),
            weekday,
            kind: kind.into(),
            key: key.into(),
            label: key.trim_end_matches(".exe").into(),
            target: format!("C:/{key}"),
            browser: if kind == "site" {
                "Chrome".into()
            } else {
                String::new()
            },
            seq,
        }
    }

    #[test]
    fn learns_what_comes_up_three_of_five_mondays() {
        let mut rows = Vec::new();
        for (n, day) in ["2026-09-07", "2026-09-14", "2026-09-21", "2026-09-28"]
            .iter()
            .enumerate()
        {
            rows.push(row(day, 0, "app", "code.exe", 2));
            rows.push(row(day, 0, "site", "github.com", 3));
            rows.push(row(day, 0, "app", "chrome.exe", 1));
            rows.push(row(day, 0, "app", "slack.exe", 1));
            if n == 0 {
                rows.push(row(day, 0, "app", "spotify.exe", 4));
            }
        }
        // A Tuesday habit does not count on Mondays.
        for day in ["2026-09-08", "2026-09-15", "2026-09-22"] {
            rows.push(row(day, 1, "app", "teams.exe", 1));
        }
        let items = learn(&rows, 0, &BTreeMap::new());
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["slack.exe", "code.exe", "github.com"]);
        assert_eq!(items[0].days, 4);

        let mut skips = BTreeMap::new();
        skips.insert("app:slack.exe".to_owned(), 3);
        assert_eq!(learn(&rows, 0, &skips).len(), 2);
    }

    #[test]
    fn falls_back_to_every_day_and_needs_three() {
        let rows: Vec<RoutineOpen> = ["2026-09-29", "2026-09-30", "2026-10-01"]
            .iter()
            .enumerate()
            .map(|(n, d)| row(d, n as u32 + 1, "app", "code.exe", 1))
            .collect();
        assert_eq!(learn(&rows, 4, &BTreeMap::new()).len(), 1);
        assert!(learn(&rows[..2], 4, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn counts_only_the_first_hour_after_five() {
        let at = |h, m| {
            Local::now()
                .with_hour(h)
                .unwrap()
                .with_minute(m)
                .unwrap()
                .with_second(0)
                .unwrap()
        };
        let mut slot = None;
        assert!(in_first_hour(&mut slot, at(2, 0)).is_none());
        assert_eq!(in_first_hour(&mut slot, at(8, 0)).unwrap().1, 1);
        assert_eq!(in_first_hour(&mut slot, at(8, 59)).unwrap().1, 2);
        assert!(in_first_hour(&mut slot, at(9, 1)).is_none());
    }

    #[test]
    fn reads_opens_from_events() {
        let w = Event::new(
            "window.focused",
            "window",
            serde_json::json!({ "app": "Visual Studio Code", "exe": "code.exe", "path": "C:/code.exe" }),
        );
        let (kind, key, label, target, _) = opened(&w).unwrap();
        assert_eq!(
            (kind.as_str(), key.as_str(), label.as_str(), target.as_str()),
            ("app", "code.exe", "Visual Studio Code", "C:/code.exe")
        );
        let shell = Event::new(
            "window.focused",
            "window",
            serde_json::json!({ "exe": "explorer.exe" }),
        );
        assert!(opened(&shell).is_none());
        let site = Event::new(
            "browser.site",
            "browser",
            serde_json::json!({ "domain": "github.com", "browser": "Edge" }),
        );
        let (_, _, _, target, browser) = opened(&site).unwrap();
        assert_eq!(target, "https://github.com/");
        assert_eq!(browser_id(&browser), "edge");
    }

    #[test]
    fn card_offers_always_after_five_open_alls() {
        let items = vec![Item {
            kind: "app".into(),
            key: "code.exe".into(),
            label: "Code".into(),
            target: String::new(),
            browser: String::new(),
            days: 3,
        }];
        let m = Memory {
            open_all: 5,
            ..Default::default()
        };
        let f = card_fields(&items, &m, false);
        assert_eq!(f["offer_auto"], "1");
        assert_eq!(f["item1"], "Code");
        assert_eq!(f["item2"], "");
        assert_eq!(card_fields(&items, &m, true)["offer_auto"], "");
    }
}
