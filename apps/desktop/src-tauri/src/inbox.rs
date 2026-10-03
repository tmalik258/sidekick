//! The notification inbox. With Windows silenced (Do Not Disturb), Sidekick
//! reads each notification as it lands in Notification Center, sorts it,
//! and only brings up what matters:
//!
//! - now: login codes, VIPs, security, payments, meetings. A card right away.
//! - soon: messages and mentions. One card at a quiet moment.
//! - digest: promos, updates, chatter. Kept for "what did I miss?".
//! - never: apps the user muted. Dropped.
//!
//! The local model decides, with context: what reached the user in the last
//! hours, earlier notifications about the same thing (so the Gmail copy of
//! an Upwork alert folds into the first), and what the user did with cards
//! before. Rules are only a safety net: login codes and muted apps, and the
//! whole decision when no model is running.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Serialize;
use serde_json::json;
use sidekick_core::{Event, NotificationSettings};
use sidekick_sensors::toasts::{self, Toast};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

pub const NOW_EVENT: &str = "notification.now";
pub const SOON_EVENT: &str = "notification.soon";
/// Tells Settings the inbox changed, so it refreshes without polling.
pub const CHANGED_EVENT: &str = "inbox://changed";
const POLL: Duration = Duration::from_secs(2);
/// Messages wait at most this long before one card gathers them.
const SOON_EVERY: Duration = Duration::from_secs(10 * 60);
const KEEP: usize = 300;
/// "Not now" this many times in a row on one app moves it down a level.
const DISMISSALS_TO_DEMOTE: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Now,
    Soon,
    Digest,
    Never,
}

impl Level {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "now" => Some(Level::Now),
            "soon" => Some(Level::Soon),
            "digest" => Some(Level::Digest),
            "never" => Some(Level::Never),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Now => "now",
            Level::Soon => "soon",
            Level::Digest => "digest",
            Level::Never => "never",
        }
    }

    fn lower(self) -> Self {
        match self {
            Level::Now => Level::Soon,
            Level::Soon | Level::Digest => Level::Digest,
            Level::Never => Level::Never,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: i64,
    pub app: String,
    pub title: String,
    pub body: String,
    pub ts: String,
    pub level: Level,
    pub why: String,
    pub code: Option<String>,
    /// Mirrored from the phone (Phone Link).
    pub phone: bool,
    /// Other apps that brought the same thing ("Gmail" for an Upwork alert).
    pub also: Vec<String>,
}

impl Item {
    /// "WhatsApp" or "Gmail (phone)".
    pub fn source(&self) -> String {
        source(&self.app, self.phone)
    }
}

fn source(app: &str, phone: bool) -> String {
    if phone {
        format!("{app} (phone)")
    } else {
        app.to_owned()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sorted {
    pub level: Level,
    pub why: &'static str,
    pub code: Option<String>,
    /// No rule was sure (used when no model is running).
    pub unsure: bool,
    /// The model's one-line reason, shown on the card.
    pub reason: Option<String>,
}

static CODE_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(code|otp|verification|verify|passcode|pin|2fa|one[- ]time|security code|sign[- ]in|login)\b")
        .expect("code words")
});
static CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:[A-Z]-)?(\d{4,8})\b").expect("code digits"));
static SECURITY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(security alert|unusual (sign|activity)|new sign-in|password (was )?(changed|reset)|suspicious|virus|threat|ransomware|account (locked|suspended))")
        .expect("security")
});
static MONEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(payment (failed|declined)|card (declined|expired)|insufficient|overdue|chargeback|refund issued)")
        .expect("money")
});
static MEETING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(starts in|starting now|is starting|reminder:|meeting (in|now)|incoming call|is calling|missed call)")
        .expect("meeting")
});
static PROMO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\d+% off|\bsale\b|\bdeal(s)?\b|\boffer\b|discount|coupon|newsletter|unsubscribe|limited time|free trial|liked your|reacted to|new follower|recommended for you|trending)")
        .expect("promo")
});
static UPDATES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(update (is )?(available|ready|installed)|downloaded|restart to (update|finish)|new version|backup (complete|finished)|sync(ed)? complete)")
        .expect("updates")
});
static MENTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(mentioned you|replied to you|@here|@channel|sent you|direct message|assigned to you|review requested)")
        .expect("mention")
});

/// Apps where a notification is usually a person writing to you.
const MESSAGING: &[&str] = &[
    "WhatsApp",
    "Slack",
    "Teams",
    "Outlook",
    "Mail",
    "Telegram",
    "Discord",
    "Signal",
    "Messenger",
];

/// Where one notification goes, before any model looks at it.
pub fn sort(app: &str, title: &str, body: &str, s: &NotificationSettings) -> Sorted {
    let text = format!("{title}\n{body}");
    let user = s.apps.get(app).and_then(|l| Level::parse(l));
    let pick = |level, why| Sorted {
        level,
        why,
        code: None,
        unsure: false,
        reason: None,
    };
    if user == Some(Level::Never) {
        return pick(Level::Never, "muted app");
    }
    if CODE_WORDS.is_match(&text)
        && let Some(c) = CODE.captures(&text)
    {
        return Sorted {
            level: Level::Now,
            why: "login code",
            code: Some(c[1].to_owned()),
            unsure: false,
            reason: None,
        };
    }
    if s.vip
        .iter()
        .map(|v| v.trim().to_lowercase())
        .filter(|v| v.len() >= 2)
        .any(|v| title.to_lowercase().contains(&v) || body.to_lowercase().contains(&v))
    {
        return pick(Level::Now, "from a VIP");
    }
    if let Some(level) = user {
        return pick(level, "your choice for this app");
    }
    if SECURITY.is_match(&text) {
        return pick(Level::Now, "security");
    }
    if MONEY.is_match(&text) {
        return pick(Level::Now, "payment");
    }
    if MEETING.is_match(&text) {
        return pick(Level::Now, "meeting or call");
    }
    if PROMO.is_match(&text) {
        return pick(Level::Digest, "promotion");
    }
    if UPDATES.is_match(&text) {
        return pick(Level::Digest, "update");
    }
    if MENTION.is_match(&text) {
        return pick(Level::Soon, "mention");
    }
    if MESSAGING.contains(&app) {
        return pick(Level::Soon, "message");
    }
    Sorted {
        level: Level::Digest,
        why: "everything else",
        code: None,
        unsure: true,
        reason: None,
    }
}

#[derive(Default)]
struct Inbox {
    items: VecDeque<Item>,
    readable: bool,
    error: Option<String>,
    pending: Vec<Item>,
    last_soon: Option<Instant>,
    dismissals: HashMap<String, u32>,
    /// What the user did with notification cards, newest last.
    lessons: VecDeque<String>,
    /// Meaning vectors of recent items, by id, for spotting repeats.
    vectors: HashMap<i64, Vec<f32>>,
}

/// Lessons kept for the model.
const LESSONS: usize = 20;
/// How far back a repeat is looked for.
const REPEAT_WINDOW: chrono::Duration = chrono::Duration::hours(3);
/// The model has this long per notification; then the rules decide.
const JUDGE_WAIT: Duration = Duration::from_secs(6);
/// Notifications judged by the model from one read.
const JUDGED_PER_POLL: usize = 5;

/// Something the user did with a notification card ("Dismissed: Sara on
/// Slack"); the model reads these as examples of what matters to them.
pub fn lesson(text: String) {
    let mut i = inbox();
    i.lessons.push_back(text);
    while i.lessons.len() > LESSONS {
        i.lessons.pop_front();
    }
}

static INBOX: LazyLock<Mutex<Inbox>> = LazyLock::new(|| Mutex::new(Inbox::default()));

fn inbox() -> std::sync::MutexGuard<'static, Inbox> {
    INBOX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Words that carry meaning, for comparing two notifications.
fn words(text: &str) -> HashSet<String> {
    const SKIP: &[&str] = &[
        "the",
        "and",
        "for",
        "you",
        "your",
        "with",
        "this",
        "that",
        "from",
        "new",
        "has",
        "have",
        "are",
        "was",
        "via",
        "notification",
        "notifications",
        "alert",
    ];
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.len() >= 3 && !SKIP.contains(&w.as_str()))
        .collect()
}

/// How much of the shorter text the longer one repeats (0 to 1), so a
/// short app alert and the longer email about it still match.
pub fn overlap(a: &str, b: &str) -> f32 {
    let (a, b) = (words(a), words(b));
    let small = a.len().min(b.len());
    if small < 3 {
        return 0.0;
    }
    a.intersection(&b).count() as f32 / small as f32
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// The meaning vector of a notification, when an embedding model is set.
async fn embed(app: &AppHandle, text: &str) -> Option<Vec<f32>> {
    let (client, model) = crate::search::embedder(app)?;
    let input = vec![text.chars().take(500).collect::<String>()];
    tokio::time::timeout(Duration::from_secs(3), client.embed(&model, &input))
        .await
        .ok()?
        .ok()?
        .into_iter()
        .next()
}

/// Earlier notifications that may be about the same thing, best first.
fn candidates(new_text: &str, vector: Option<&[f32]>) -> Vec<(Item, f32)> {
    let cutoff = chrono::Utc::now() - REPEAT_WINDOW;
    let i = inbox();
    let mut found: Vec<(Item, f32)> = i
        .items
        .iter()
        .filter(|it| {
            chrono::DateTime::parse_from_rfc3339(&it.ts)
                .is_ok_and(|t| t.with_timezone(&chrono::Utc) > cutoff)
        })
        .filter_map(|it| {
            let words = overlap(new_text, &format!("{} {}", it.title, it.body));
            let meaning = vector
                .zip(i.vectors.get(&it.id))
                .map_or(0.0, |(a, b)| cosine(a, b));
            let score = words.max(meaning);
            (words >= 0.5 || meaning >= 0.8).then(|| (it.clone(), score))
        })
        .collect();
    found.sort_by(|a, b| b.1.total_cmp(&a.1));
    found.truncate(3);
    found
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub level: Level,
    pub same_as: Option<i64>,
    pub why: String,
}

/// Reads the model's JSON answer.
pub fn parse_verdict(answer: &str) -> Option<Verdict> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    let v: serde_json::Value = serde_json::from_str(answer.get(start..=end)?).ok()?;
    let level = match v["level"].as_str()?.to_lowercase().as_str() {
        "now" => Level::Now,
        "soon" => Level::Soon,
        "digest" => Level::Digest,
        "skip" | "never" => Level::Never,
        _ => return None,
    };
    Some(Verdict {
        level,
        same_as: v["same_as"].as_i64(),
        why: v["why"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(80)
            .collect(),
    })
}

const JUDGE_SYSTEM: &str = "You decide which desktop notifications reach the user. Reply with JSON only: \
{\"level\":\"now|soon|digest|skip\",\"same_as\":<id or null>,\"why\":\"<at most 8 words>\"}. \
now: they need it right away (a person waiting on them, money, security, a meeting or deadline now, \
something they were waiting for). soon: worth seeing today (messages, mentions, things to act on, \
leads and job alerts they care about). digest: fine to skim later (updates, newsletters, social). \
skip: no value (empty 'you have new messages' pings, ads, duplicates of nothing useful). \
same_as: the id of an earlier notification about the same thing (the same job, message, order or \
event), even from another app or in other words; it then folds into that one. Learn from what the \
user did before.";

/// Asks the local model about one notification, with what came before.
async fn judge(
    app: &AppHandle,
    t: &Toast,
    rule: &Sorted,
    similar: &[(Item, f32)],
) -> Option<Verdict> {
    let settings = lock(&app.state::<AppState>().settings).clone();
    if !settings.ai.local.enabled {
        return None;
    }
    let (recent, lessons) = {
        let i = inbox();
        let recent: Vec<String> = i
            .items
            .iter()
            .take(12)
            .map(|it| {
                format!(
                    "- {} ({}): {}: {}",
                    it.source(),
                    it.level.as_str(),
                    it.title,
                    it.body.chars().take(80).collect::<String>()
                )
            })
            .collect();
        (recent, i.lessons.iter().cloned().collect::<Vec<_>>())
    };
    let mut prompt = String::new();
    if !settings.notifications.vip.is_empty() {
        prompt.push_str(&format!(
            "People who always matter: {}\n",
            settings.notifications.vip.join(", ")
        ));
    }
    if !lessons.is_empty() {
        prompt.push_str(&format!(
            "What the user did with earlier cards:\n{}\n",
            lessons.join("\n")
        ));
    }
    if !recent.is_empty() {
        prompt.push_str(&format!("Recent notifications:\n{}\n", recent.join("\n")));
    }
    if !similar.is_empty() {
        prompt.push_str("Possibly the same thing:\n");
        for (it, _) in similar {
            prompt.push_str(&format!(
                "- id {}: {}: {}: {}\n",
                it.id,
                it.source(),
                it.title,
                it.body.chars().take(160).collect::<String>()
            ));
        }
    }
    prompt.push_str(&format!(
        "Time: {}\nRules alone would say: {}\nNew notification from {}:\nTitle: {}\nText: {}",
        chrono::Local::now().format("%a %H:%M"),
        rule.level.as_str(),
        source(&t.app, t.phone),
        t.title,
        t.body.chars().take(400).collect::<String>()
    ));
    let model = crate::ai::local_model(&settings.ai);
    let req = sidekick_ai::ChatRequest {
        system: JUDGE_SYSTEM.into(),
        messages: vec![sidekick_ai::Message::user(prompt)],
        image: None,
    };
    let answer = tokio::time::timeout(JUDGE_WAIT, model.complete(&req, json!({})))
        .await
        .ok()?
        .ok()?;
    parse_verdict(&answer)
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(path) = toasts::db_path() else {
            let mut i = inbox();
            i.readable = false;
            i.error = Some("Notifications can be read on Windows only.".into());
            return;
        };
        let mut last: Option<i64> = None;
        let mut seen = None;
        loop {
            tokio::time::sleep(POLL).await;
            let settings = lock(&app.state::<AppState>().settings).clone();
            if !settings.notifications.enabled {
                // Turned back on later: start from then, never replay.
                last = None;
                continue;
            }
            // Nothing changed on disk: nothing new to read (and no copy of
            // a locked database to make).
            let now_stamp = toasts::stamp(&path);
            if last.is_some() && now_stamp.is_some() && now_stamp == seen {
                flush_soon(&app, &settings);
                continue;
            }
            seen = now_stamp;
            let p = path.clone();
            let since = last;
            let read = tokio::task::spawn_blocking(move || match since {
                None => toasts::latest_id(&p).map(|id| (id, Vec::new())),
                Some(after) => {
                    toasts::read_since(&p, after).map(|t| (t.last().map_or(after, |x| x.id), t))
                }
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            let new = match read {
                Ok((id, new)) => {
                    let mut i = inbox();
                    i.readable = true;
                    i.error = None;
                    last = Some(id);
                    new
                }
                Err(err) => {
                    let mut i = inbox();
                    if i.error.as_deref() != Some(err.as_str()) {
                        log::warn!("notifications: {err}");
                    }
                    i.readable = false;
                    i.error = Some(err);
                    continue;
                }
            };
            // A burst (after a sync, or waking the PC) is judged up to a
            // point; past it the rules decide, so nothing waits long.
            for (n, t) in new.into_iter().enumerate() {
                take(&app, &settings, t, n < JUDGED_PER_POLL).await;
            }
            flush_soon(&app, &settings);
        }
    });
}

fn denied(app_name: &str, s: &sidekick_core::Settings) -> bool {
    let lower = app_name.to_lowercase();
    lower == "sidekick"
        || s.deny_apps
            .iter()
            .any(|d| d.trim_end_matches(".exe").eq_ignore_ascii_case(&lower))
}

async fn take(app: &AppHandle, settings: &sidekick_core::Settings, t: Toast, think: bool) {
    if denied(&t.app, settings) {
        return;
    }
    let mut sorted = sort(&t.app, &t.title, &t.body, &settings.notifications);
    // Muted apps and login codes need no judgement (and codes no delay).
    let firm = matches!(sorted.why, "muted app" | "login code") || !think;
    let text = format!("{} {}", t.title, t.body);
    let vector = if firm { None } else { embed(app, &text).await };
    let similar = if firm {
        Vec::new()
    } else {
        candidates(&text, vector.as_deref())
    };
    let mut repeat_of: Option<i64> = None;
    if !firm {
        match judge(app, &t, &sorted, &similar).await {
            Some(v) => {
                repeat_of = v
                    .same_as
                    .filter(|id| similar.iter().any(|(it, _)| it.id == *id));
                // The user's own pick for an app or a VIP keeps its level.
                if !matches!(sorted.why, "your choice for this app" | "from a VIP") {
                    sorted.level = v.level;
                    sorted.why = "Sidekick's judgement";
                }
                sorted.reason = Some(v.why);
            }
            // No model: a near copy of something recent is still a repeat.
            None => {
                repeat_of = similar
                    .first()
                    .filter(|(_, score)| *score >= 0.75)
                    .map(|(it, _)| it.id);
            }
        }
    }
    if let Some(first) = repeat_of {
        let mut i = inbox();
        let also = source(&t.app, t.phone);
        if let Some(it) = i.items.iter_mut().find(|it| it.id == first)
            && it.source() != also
            && !it.also.contains(&also)
        {
            it.also.push(also.clone());
        }
        if let Some(it) = i.pending.iter_mut().find(|it| it.id == first)
            && !it.also.contains(&also)
        {
            it.also.push(also);
        }
        log::debug!("notification {} folded into {first}", t.id);
        return;
    }
    let item = Item {
        id: t.id,
        app: t.app.clone(),
        title: t.title.clone(),
        body: t.body.clone(),
        ts: t.arrived.to_rfc3339(),
        level: sorted.level,
        why: sorted.reason.clone().unwrap_or_else(|| sorted.why.into()),
        code: sorted.code.clone(),
        phone: t.phone,
        also: Vec::new(),
    };
    if let Some(v) = vector {
        let mut i = inbox();
        i.vectors.insert(item.id, v);
        if i.vectors.len() > KEEP {
            let keep: HashSet<i64> = i.items.iter().map(|it| it.id).collect();
            i.vectors.retain(|id, _| keep.contains(id));
        }
    }
    {
        let mut i = inbox();
        if sorted.level != Level::Never {
            i.items.push_front(item.clone());
            i.items.truncate(KEEP);
        }
        if sorted.level == Level::Soon {
            i.pending.push(item.clone());
        }
    }
    let _ = tauri::Emitter::emit(app, CHANGED_EVENT, ());
    if sorted.level != Level::Never {
        crate::recipes::on_notification(app, item.id, &item.app, &item.title, &item.body);
    }
    let paused = settings.pause.is_active(chrono::Utc::now());
    if sorted.level == Level::Now && !paused {
        app.state::<AppState>().bus.publish(Event::new(
            NOW_EVENT,
            "notifications",
            json!({
                "id": item.id,
                "app": item.app,
                "source": item.source(),
                "title": item.title,
                "body": clip(&item.body, 160),
                "why": item.why,
                "code": item.code,
            }),
        ));
    }
}

/// Messages gathered since the last card, as one card, at most every
/// `SOON_EVERY`. During a meeting the card waits (suggestions handle that).
fn flush_soon(app: &AppHandle, settings: &sidekick_core::Settings) {
    let pending = {
        let mut i = inbox();
        let due = i.last_soon.is_none_or(|t| t.elapsed() >= SOON_EVERY);
        if i.pending.is_empty() || !due || settings.pause.is_active(chrono::Utc::now()) {
            return;
        }
        i.last_soon = Some(Instant::now());
        std::mem::take(&mut i.pending)
    };
    let (count_text, summary) = digest(&pending);
    let first_app = pending.first().map(|i| i.app.clone()).unwrap_or_default();
    app.state::<AppState>().bus.publish(Event::new(
        SOON_EVENT,
        "notifications",
        json!({
            "count": pending.len(),
            "count_text": count_text,
            "summary": summary,
            "first_app": first_app,
        }),
    ));
}

/// The waiting card's title and text: one message reads as itself ("Ali
/// Khan on WhatsApp" / what he wrote); several show each sender's latest
/// message, one per line.
fn digest(pending: &[Item]) -> (String, String) {
    if let [only] = pending {
        let title = if only.title.is_empty() {
            only.source()
        } else {
            format!("{} on {}", only.title, only.source())
        };
        return (title, clip(&only.body, 200));
    }
    let mut latest: Vec<(String, String)> = Vec::new();
    for it in pending.iter().rev() {
        let who = if it.title.is_empty() {
            it.source()
        } else {
            format!("{} ({})", it.title, it.source())
        };
        if !latest.iter().any(|(w, _)| *w == who) {
            latest.push((who, it.body.clone()));
        }
    }
    let mut lines: Vec<String> = latest
        .iter()
        .take(3)
        .map(|(who, body)| {
            if body.trim().is_empty() {
                who.clone()
            } else {
                format!("{who}: {}", clip(body.trim(), 70))
            }
        })
        .collect();
    if latest.len() > 3 {
        lines.push(format!("and {} more", latest.len() - 3));
    }
    (
        format!("{} messages waiting", pending.len()),
        lines.join("\n"),
    )
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        s.chars().take(n).collect::<String>() + "..."
    }
}

/// "Not now" on a notification card: three in a row for one app moves the
/// app down a level, so its next ones interrupt less.
pub fn on_dismiss(app: &AppHandle, from: &str) -> Option<String> {
    if from.is_empty() {
        return None;
    }
    let n = {
        let mut i = inbox();
        let n = i.dismissals.entry(from.to_owned()).or_default();
        *n += 1;
        *n
    };
    if n < DISMISSALS_TO_DEMOTE {
        return None;
    }
    inbox().dismissals.remove(from);
    let current = lock(&app.state::<AppState>().settings)
        .notifications
        .apps
        .get(from)
        .and_then(|l| Level::parse(l))
        .unwrap_or(Level::Now);
    set_level(app, from, current.lower()).ok()
}

/// Back to letting Sidekick decide for this app.
pub fn clear_level(app: &AppHandle, from: &str) -> Result<String, String> {
    let mut next = lock(&app.state::<AppState>().settings).clone();
    next.notifications.apps.remove(from);
    crate::commands::apply_settings(app, next)?;
    Ok(format!("Sidekick decides for {from}"))
}

/// The user's level for one app, from a card ("Less from WhatsApp") or Settings.
pub fn set_level(app: &AppHandle, from: &str, level: Level) -> Result<String, String> {
    let mut next = lock(&app.state::<AppState>().settings).clone();
    next.notifications
        .apps
        .insert(from.to_owned(), level.as_str().to_owned());
    crate::commands::apply_settings(app, next)?;
    inbox().dismissals.remove(from);
    lesson(format!("Set {from} to {}", level.as_str()));
    Ok(match level {
        Level::Now => format!("{from} always comes through"),
        Level::Soon => format!("{from} waits for a quiet moment"),
        Level::Digest => format!("{from} goes to the digest"),
        Level::Never => format!("{from} is muted"),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppLevel {
    pub app: String,
    /// The user's pick; None lets Sidekick decide per notification.
    pub level: Option<Level>,
    pub count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub readable: bool,
    pub error: Option<String>,
    pub items: Vec<Item>,
    pub apps: Vec<AppLevel>,
}

pub fn status(app: &AppHandle) -> Status {
    let s = lock(&app.state::<AppState>().settings)
        .notifications
        .clone();
    let i = inbox();
    let mut apps: Vec<AppLevel> = Vec::new();
    for it in &i.items {
        match apps.iter_mut().find(|a| a.app == it.app) {
            Some(a) => a.count += 1,
            None => apps.push(AppLevel {
                app: it.app.clone(),
                level: s.apps.get(&it.app).and_then(|l| Level::parse(l)),
                count: 1,
            }),
        }
    }
    for (name, level) in &s.apps {
        if !apps.iter().any(|a| &a.app == name)
            && let Some(level) = Level::parse(level)
        {
            apps.push(AppLevel {
                app: name.clone(),
                level: Some(level),
                count: 0,
            });
        }
    }
    Status {
        readable: i.readable,
        error: i.error.clone(),
        items: i.items.iter().take(50).cloned().collect(),
        apps,
    }
}

/// The inbox as text for a model: "what did I miss?".
pub fn describe(level: Option<&str>, from: Option<&str>, minutes: Option<u64>) -> String {
    let since = minutes.map(|m| chrono::Utc::now() - chrono::Duration::minutes(m as i64));
    let important = level == Some("important");
    let i = inbox();
    if !i.readable && i.items.is_empty() {
        return i.error.clone().unwrap_or_else(|| {
            "The notification inbox is off (Settings > Home > Notifications).".into()
        });
    }
    let lines: Vec<String> = i
        .items
        .iter()
        .filter(|it| !important || matches!(it.level, Level::Now | Level::Soon))
        .filter(|it| {
            from.is_none_or(|f| {
                it.app.eq_ignore_ascii_case(f)
                    || it.title.to_lowercase().contains(&f.to_lowercase())
            })
        })
        .filter(|it| {
            since.is_none_or(|s| chrono::DateTime::parse_from_rfc3339(&it.ts).is_ok_and(|t| t >= s))
        })
        .take(40)
        .map(|it| {
            let when = chrono::DateTime::parse_from_rfc3339(&it.ts)
                .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .unwrap_or_default();
            format!(
                "- {when} [{}] {}: {} {}",
                it.level.as_str(),
                it.app,
                it.title,
                clip(&it.body, 140)
            )
        })
        .collect();
    if lines.is_empty() {
        "No notifications match.".into()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(app: &str, title: &str, body: &str) -> Item {
        Item {
            id: 0,
            app: app.into(),
            title: title.into(),
            body: body.into(),
            ts: String::new(),
            level: Level::Soon,
            why: String::new(),
            code: None,
            phone: false,
            also: Vec::new(),
        }
    }

    #[test]
    fn waiting_card_shows_the_messages() {
        let one = [item("WhatsApp", "Ali Khan", "Can you send the invoice?")];
        assert_eq!(
            digest(&one),
            (
                "Ali Khan on WhatsApp".into(),
                "Can you send the invoice?".into()
            )
        );
        let many = [
            item("WhatsApp", "Ali Khan", "Hi"),
            item("Slack", "#dev", "Build failed on main"),
            item("WhatsApp", "Ali Khan", "Can you send the invoice?"),
        ];
        let (title, text) = digest(&many);
        assert_eq!(title, "3 messages waiting");
        assert_eq!(
            text,
            "Ali Khan (WhatsApp): Can you send the invoice?\n#dev (Slack): Build failed on main",
            "each sender's latest message, newest first"
        );
    }

    fn s() -> NotificationSettings {
        NotificationSettings::default()
    }

    #[test]
    fn sorts_the_clear_cases() {
        let code = sort(
            "Chrome",
            "Google",
            "G-482913 is your verification code",
            &s(),
        );
        assert_eq!(code.level, Level::Now);
        assert_eq!(code.code.as_deref(), Some("482913"));
        assert_eq!(
            sort(
                "Outlook",
                "Security alert",
                "New sign-in to your account",
                &s()
            )
            .level,
            Level::Now
        );
        assert_eq!(
            sort("Teams", "Standup", "Starts in 5 minutes", &s()).level,
            Level::Now
        );
        assert_eq!(
            sort("Chrome", "Daraz", "Flash sale: 50% off today", &s()).level,
            Level::Digest
        );
        assert_eq!(
            sort(
                "Windows Update",
                "Updates",
                "Restart to finish installing",
                &s()
            )
            .level,
            Level::Digest
        );
        assert_eq!(
            sort("WhatsApp", "Ali Khan", "Can you send the invoice?", &s()).level,
            Level::Soon
        );
        assert_eq!(
            sort("Slack", "#dev", "Sara mentioned you", &s()).level,
            Level::Soon
        );
        let other = sort("Spotify", "Now playing", "Song by Band", &s());
        assert_eq!(other.level, Level::Digest);
        assert!(other.unsure);
        // "Order 12345 shipped" has digits but no code words.
        assert_eq!(
            sort("Chrome", "Shop", "Order 12345 shipped", &s()).code,
            None
        );
    }

    #[test]
    fn the_users_picks_win() {
        let mut p = s();
        p.vip.push("Ali".into());
        p.apps.insert("WhatsApp".into(), "digest".into());
        p.apps.insert("Discord".into(), "never".into());
        assert_eq!(
            sort("WhatsApp", "Ali Khan", "hi", &p).level,
            Level::Now,
            "VIP beats the app level"
        );
        assert_eq!(sort("WhatsApp", "Sara", "hi", &p).level, Level::Digest);
        assert_eq!(
            sort("Discord", "Bot", "Your code is 1234", &p).level,
            Level::Never
        );
        assert_eq!(Level::Now.lower(), Level::Soon);
        assert_eq!(Level::Digest.lower(), Level::Digest);
    }
}
