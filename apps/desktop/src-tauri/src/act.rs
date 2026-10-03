//! Acting outside the PC's own files: on web pages (through the Sidekick
//! extension, in the user's signed-in browser) and in connected apps
//! (through Composio).
//!
//! Reading and reversible steps run at once. A step that sends, posts,
//! pays, deletes or submits becomes a button the user taps (or Alt 1), so
//! nothing goes out without a yes. Text on a page or in an email can never
//! skip that, since only the user's tap runs it.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use regex::Regex;
use serde_json::{Value, json};
use sidekick_actions::Outcome;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

/// Words on a button or in a tool name that mean something leaves the PC
/// or cannot be taken back.
static RISKY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(send|submit|post|publish|pay|buy|order|purchase|checkout|check out|place order|delete|remove|discard|confirm|book|reply|tweet|apply|transfer|withdraw|sign up|subscribe|unsubscribe|invite|share|upload|accept|decline|approve|merge|deploy|transfer|donate)\b")
        .expect("risky words")
});

fn risky(label: &str) -> bool {
    RISKY.is_match(label)
}

/// Money leaving or something gone for good: always asks, whatever the
/// settings say.
static IRREVERSIBLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(pay|buy|purchase|checkout|check out|place order|order now|delete|remove|discard|transfer|withdraw|donate|uninstall|empty)\b")
        .expect("irreversible words")
});

/// How risky one step is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    /// Typing, opening, a link, a tab.
    Step,
    /// Sends, posts, submits or shares.
    Outward,
    /// Pays or deletes.
    Irreversible,
}

pub fn risk(action: &str, kind: &str, label: &str, key: &str) -> Risk {
    if !needs_tap(action, kind, label, key) {
        return Risk::Step;
    }
    if IRREVERSIBLE.is_match(label) {
        Risk::Irreversible
    } else {
        Risk::Outward
    }
}

/// What to do with one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Run,
    Tap,
    Refuse,
}

/// The user's permission for a place (an app name or a site), matching a
/// site's subdomains too.
pub fn place_rule(agent: &sidekick_core::AgentSettings, place: &str) -> Option<String> {
    let place = place.trim().to_lowercase();
    if place.is_empty() {
        return None;
    }
    agent
        .places
        .iter()
        .find(|(k, _)| {
            let k = k.trim().to_lowercase();
            !k.is_empty() && (place == k || place.ends_with(&format!(".{k}")) || place.contains(&k))
        })
        .map(|(_, v)| v.clone())
}

/// Runs, waits for a tap, or refuses, from the step's risk and the user's
/// settings. Paying and deleting always wait.
pub fn decide(agent: &sidekick_core::AgentSettings, place: &str, risk: Risk) -> Gate {
    match place_rule(agent, place).as_deref() {
        Some("never") => return Gate::Refuse,
        Some("allow") => {
            return if risk == Risk::Irreversible {
                Gate::Tap
            } else {
                Gate::Run
            };
        }
        _ => {}
    }
    let tap = match agent.ask.as_str() {
        "each" => true,
        "irreversible" => risk == Risk::Irreversible,
        _ => risk >= Risk::Outward,
    };
    if tap { Gate::Tap } else { Gate::Run }
}

pub(crate) fn agent_settings(app: &AppHandle) -> sidekick_core::AgentSettings {
    lock(&app.state::<AppState>().settings).agent.clone()
}

/// Sites the user keeps Sidekick out of: the ignore list and Never.
fn never_sites(app: &AppHandle) -> Vec<String> {
    let state = app.state::<AppState>();
    let s = lock(&state.settings);
    let mut v = s.deny_sites.clone();
    v.extend(
        s.agent
            .places
            .iter()
            .filter(|(k, v)| v.as_str() == "never" && k.contains('.'))
            .map(|(k, _)| k.clone()),
    );
    v
}

/// The site each tab showed when last read.
static TAB_HOST: LazyLock<Mutex<HashMap<i64, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const WAIT: Duration = Duration::from_secs(25);

/// What worked where: per site or app, the controls used successfully
/// before, newest first. Shown with the next read, so a repeat task finds
/// its way faster. Kept in the app's data folder.
/// Site or app to the controls used there.
type KnowHow = HashMap<String, Vec<String>>;

static KNOW_HOW: LazyLock<Mutex<Option<KnowHow>>> = LazyLock::new(|| Mutex::new(None));
const KNOW_HOW_KEEP: usize = 12;

fn know_how_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("know-how.json"))
}

fn with_know_how<T>(app: &AppHandle, f: impl FnOnce(&mut KnowHow) -> T) -> T {
    let mut guard = KNOW_HOW
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let map = guard.get_or_insert_with(|| {
        know_how_path(app)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    });
    f(map)
}

/// Forgets every remembered path (Settings > Privacy).
pub fn forget_know_how(app: &AppHandle) {
    with_know_how(app, HashMap::clear);
    if let Some(path) = know_how_path(app) {
        let _ = std::fs::remove_file(path);
    }
}

/// Remembers that `label` worked on `place` (a site or an app).
pub fn learned(app: &AppHandle, place: &str, label: &str) {
    let (place, label) = (place.trim().to_lowercase(), label.trim());
    if place.is_empty() || label.is_empty() {
        return;
    }
    let snapshot = with_know_how(app, |m| {
        let list = m.entry(place).or_default();
        list.retain(|l| l != label);
        list.insert(0, label.to_owned());
        list.truncate(KNOW_HOW_KEEP);
        m.clone()
    });
    if let (Some(path), Ok(text)) = (know_how_path(app), serde_json::to_string(&snapshot)) {
        let _ = std::fs::write(path, text);
    }
}

fn know_how(app: &AppHandle, place: &str) -> String {
    let used =
        with_know_how(app, |m| m.get(&place.trim().to_lowercase()).cloned()).unwrap_or_default();
    if used.is_empty() {
        String::new()
    } else {
        format!("\nWorked here before: {}", used.join(", "))
    }
}

fn host(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_owned())
        })
        .unwrap_or_default()
}

/// "Inbox - name@x.com - Outlook" is Outlook.
fn app_of(window: &str) -> String {
    window
        .rsplit(" - ")
        .next()
        .unwrap_or(window)
        .trim()
        .to_owned()
}

static LAST_WINDOW: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new()));

/// What each element number on a page was, from the last read, so a click
/// on "12" can be checked against its label. Keyed by tab.
/// Element number to (kind, label).
type Elements = HashMap<String, (String, String)>;

static ELEMENTS: LazyLock<Mutex<HashMap<i64, Elements>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\[(\d+)\] (\S+) "([^"]*)""#).expect("element line"));

fn remember(tab: i64, elements: &str) {
    let map: Elements = elements
        .lines()
        .filter_map(|l| LINE.captures(l))
        .map(|c| (c[1].to_owned(), (c[2].to_owned(), c[3].to_owned())))
        .collect();
    if let Ok(mut m) = ELEMENTS.lock() {
        m.insert(tab, map);
    }
}

fn element(tab: i64, r: &str) -> Option<(String, String)> {
    ELEMENTS.lock().ok()?.get(&tab)?.get(r).cloned()
}

/// Whether this step needs the user's tap first.
fn needs_tap(action: &str, kind: &str, label: &str, key: &str) -> bool {
    match action {
        "click" => risky(label) || kind == "button" && label.is_empty(),
        // Enter in a message box sends it; in a search box it only searches.
        "press" => {
            let enter = key.is_empty() || key.eq_ignore_ascii_case("enter");
            enter && !kind.contains("search") && !label.to_lowercase().contains("search")
        }
        _ => false,
    }
}

async fn ask(app: &AppHandle, mut cmd: Value) -> Result<Value, String> {
    let bridge = app.state::<AppState>().browser.clone();
    if !bridge.connected() {
        return Err(
            "the Sidekick browser extension is not connected (Settings > Apps > Browser)".into(),
        );
    }
    cmd["deny"] = json!(never_sites(app));
    bridge.request(cmd, WAIT).await
}

fn tab_of(v: &Value) -> i64 {
    v["tab"].as_i64().unwrap_or_default()
}

/// The `browser` tool: read and act on web pages in the user's browser.
pub async fn browser(app: &AppHandle, chat_id: &str, args: &Value) -> String {
    let action = args["action"].as_str().unwrap_or("read");
    let tab = args["tab"].as_i64();
    let out = match action {
        "tabs" => ask(app, json!({ "type": "tabs" })).await.map(|v| {
            let tabs = v["tabs"].as_array().cloned().unwrap_or_default();
            if tabs.is_empty() {
                return "No web pages open.".to_owned();
            }
            tabs.iter()
                .map(|t| {
                    format!(
                        "- tab {}{}: {} ({})",
                        t["tab"],
                        if t["active"] == true { " (active)" } else { "" },
                        t["title"].as_str().unwrap_or_default(),
                        t["url"].as_str().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }),
        "open" => {
            let url = args["url"].as_str().unwrap_or_default();
            ask(
                app,
                json!({ "type": "open", "url": url, "newTab": args["new_tab"].as_bool().unwrap_or(true) }),
            )
            .await
            .map(|v| {
                format!(
                    "Opened tab {}: {} ({}). Read it to see what is on the page.",
                    v["tab"],
                    v["title"].as_str().unwrap_or_default(),
                    v["url"].as_str().unwrap_or_default()
                )
            })
        }
        "read" => ask(app, json!({ "type": "read", "tab": tab })).await.map(|v| {
            let elements = v["elements"].as_str().unwrap_or_default();
            remember(tab_of(&v), elements);
            let url = v["url"].as_str().unwrap_or_default();
            if let Ok(mut m) = TAB_HOST.lock() {
                m.insert(tab_of(&v), host(url));
            }
            format!(
                "Tab {}: {}\n{}{}\n\nElements (act on them by number):\n{}\n\nPage text:\n{}",
                v["tab"],
                v["title"].as_str().unwrap_or_default(),
                url,
                know_how(app, &host(url)),
                elements,
                v["text"].as_str().unwrap_or_default()
            )
        }),
        "act" => return act(app, chat_id, args).await,
        "extract" => ask(
            app,
            json!({ "type": "extract", "tab": tab, "what": args["what"].as_str().unwrap_or("text") }),
        )
        .await
        .map(|v| {
            if let Some(tables) = v["tables"].as_array() {
                tables
                    .iter()
                    .enumerate()
                    .map(|(i, t)| format!("Table {}:\n{}", i + 1, t.as_str().unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            } else if let Some(links) = v["links"].as_array() {
                links
                    .iter()
                    .map(|l| format!("- {} ({})", l["text"].as_str().unwrap_or_default(), l["href"].as_str().unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                v["text"].as_str().unwrap_or_default().to_owned()
            }
        }),
        "switch" | "close" | "back" => ask(app, json!({ "type": action, "tab": tab }))
            .await
            .map(|v| format!("Done. {}", v["title"].as_str().unwrap_or_default())),
        other => Err(format!("unknown browser action {other}")),
    };
    out.unwrap_or_else(|e| format!("Error: {e}"))
}

async fn act(app: &AppHandle, chat_id: &str, args: &Value) -> String {
    let what = args["do"].as_str().unwrap_or("click");
    let r = args["ref"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| args["ref"].as_i64().map(|n| n.to_string()))
        .unwrap_or_default();
    let text = args["text"].as_str().unwrap_or_default();
    let tab = args["tab"].as_i64();
    let known = tab
        .or_else(|| ELEMENTS.lock().ok().and_then(|m| m.keys().last().copied()))
        .and_then(|t| element(t, &r));
    let (kind, label) = known.unwrap_or_default();
    if r.is_empty() && what != "scroll" {
        return "Error: say which element (its number from read).".into();
    }
    let cmd = json!({ "type": "act", "tab": tab, "ref": r, "do": what, "text": text });
    let place = tab
        .or_else(|| TAB_HOST.lock().ok().and_then(|m| m.keys().last().copied()))
        .and_then(|t| TAB_HOST.lock().ok().and_then(|m| m.get(&t).cloned()))
        .unwrap_or_default();
    let gate = decide(
        &agent_settings(app),
        &place,
        risk(what, &kind, &label, text),
    );
    if gate == Gate::Refuse {
        return format!(
            "Error: the user keeps Sidekick out of {place} (Settings > Skills > Agent)."
        );
    }
    if gate == Gate::Tap {
        let shown = if label.is_empty() {
            "the button".to_owned()
        } else {
            format!("\"{label}\"")
        };
        let button = match what {
            "press" => format!("Send ({})", if label.is_empty() { "Enter" } else { &label }),
            _ => format!("Click {}", if label.is_empty() { "it" } else { &label }),
        };
        crate::ask_tools::offer(app, chat_id, "browser_act", cmd, &button);
        return format!(
            "Not done yet: {shown} sends or changes something, so it is a button the user taps. \
             Tell them in one sentence what tapping it will do."
        );
    }
    match ask(app, cmd).await {
        Ok(v) if what == "click" || what == "type" || what == "select" => {
            learned(app, &host(v["url"].as_str().unwrap_or_default()), &label);
            format!(
                "{}. Now on: {} ({}). Read the page again to see the result.",
                v["note"].as_str().unwrap_or("Done"),
                v["title"].as_str().unwrap_or_default(),
                v["url"].as_str().unwrap_or_default()
            )
        }
        Ok(v) => format!(
            "{}. Now on: {} ({}). Read the page again to see the result.",
            v["note"].as_str().unwrap_or("Done"),
            v["title"].as_str().unwrap_or_default(),
            v["url"].as_str().unwrap_or_default()
        ),
        Err(e) => format!("Error: {e}"),
    }
}

/// The app the user was in before Sidekick (or one they named).
fn desktop_target(app: &AppHandle, args: &Value) -> sidekick_actions::uia::Target {
    let named = args["app"].as_str().filter(|a| !a.trim().is_empty());
    let pid = lock(&app.state::<AppState>().last_window)
        .as_ref()
        .and_then(|w| w["pid"].as_u64())
        .and_then(|p| u32::try_from(p).ok());
    sidekick_actions::uia::Target {
        pid: if named.is_some() { None } else { pid },
        app: named.map(str::to_owned),
    }
}

/// Controls seen in the last desktop read: number to (kind, name).
static DESKTOP: LazyLock<Mutex<Elements>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, sidekick_actions::ActionError> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// The `desktop` tool: read and act inside any Windows app.
pub async fn desktop(app: &AppHandle, chat_id: &str, args: &Value) -> String {
    use sidekick_actions::uia;
    let target = desktop_target(app, args);
    let action = args["action"].as_str().unwrap_or("read");
    let out = match action {
        "read" => {
            let t = target.clone();
            blocking(move || uia::snapshot(&t)).await.map(|s| {
                if let Ok(mut m) = DESKTOP.lock() {
                    *m = s
                        .controls
                        .iter()
                        .map(|c| (c.n.to_string(), (c.kind.to_lowercase(), c.name.clone())))
                        .collect();
                }
                let place = app_of(&s.window);
                if let Ok(mut w) = LAST_WINDOW.lock() {
                    *w = place.clone();
                }
                if place_rule(&agent_settings(app), &place).as_deref() == Some("never") {
                    return format!("The user keeps Sidekick out of {place}; do not use it.");
                }
                format!("{}{}", s.describe(), know_how(app, &place))
            })
        }
        "act" => {
            let r = args["ref"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| args["ref"].as_i64().map(|n| n.to_string()))
                .unwrap_or_default();
            let what = args["do"].as_str().unwrap_or("click").to_owned();
            let text = args["text"].as_str().unwrap_or_default().to_owned();
            let (kind, name) = DESKTOP
                .lock()
                .ok()
                .and_then(|m| m.get(&r).cloned())
                .unwrap_or_default();
            let Ok(n) = r.parse::<usize>() else {
                return "Error: say which control (its number from read).".into();
            };
            let place = target
                .app
                .clone()
                .unwrap_or_else(|| LAST_WINDOW.lock().map(|w| w.clone()).unwrap_or_default());
            let gate = decide(&agent_settings(app), &place, risk(&what, &kind, &name, ""));
            if gate == Gate::Refuse {
                return format!(
                    "Error: the user keeps Sidekick out of {place} (Settings > Skills > Agent)."
                );
            }
            if gate == Gate::Tap {
                let cmd = json!({ "pid": target.pid, "app": target.app, "ref": n, "name": name, "do": what, "text": text });
                let label = format!("Click {}", if name.is_empty() { "it" } else { &name });
                crate::ask_tools::offer(app, chat_id, "desktop_act", cmd, &label);
                return format!(
                    "Not done yet: \"{name}\" sends or changes something, so it is a button the \
                     user taps. Tell them in one sentence what it will do."
                );
            }
            let t = target.clone();
            let label = name.clone();
            let done = blocking(move || uia::act(&t, n, &name, &what, &text)).await;
            if done.is_ok() {
                let place = LAST_WINDOW.lock().map(|w| w.clone()).unwrap_or_default();
                learned(app, &place, &label);
            }
            done.map(|o| format!("{}. Read the window again to see the result.", o.message))
        }
        "keys" => {
            let keys = args["text"].as_str().unwrap_or_default().to_owned();
            let place = target
                .app
                .clone()
                .unwrap_or_else(|| LAST_WINDOW.lock().map(|w| w.clone()).unwrap_or_default());
            let level = if uia::keys_send(&keys) {
                Risk::Outward
            } else {
                Risk::Step
            };
            let gate = decide(&agent_settings(app), &place, level);
            if gate == Gate::Refuse {
                return format!(
                    "Error: the user keeps Sidekick out of {place} (Settings > Skills > Agent)."
                );
            }
            if gate == Gate::Tap {
                let cmd = json!({ "pid": target.pid, "app": target.app, "keys": keys });
                crate::ask_tools::offer(
                    app,
                    chat_id,
                    "desktop_keys",
                    cmd,
                    &format!("Press {keys}"),
                );
                return "Not done yet: those keys may send something, so it is a button the user \
                        taps."
                    .into();
            }
            let t = target.clone();
            blocking(move || uia::keys(&t, &keys))
                .await
                .map(|o| o.message)
        }
        "click_text" => {
            // For apps that show no controls (games, remote desktops, some
            // Electron and Java apps): find the words on screen and click
            // them.
            let want = args["text"].as_str().unwrap_or_default().trim().to_owned();
            if want.is_empty() {
                return "Error: say which words to click.".into();
            }
            let place = target
                .app
                .clone()
                .unwrap_or_else(|| LAST_WINDOW.lock().map(|w| w.clone()).unwrap_or_default());
            let gate = decide(
                &agent_settings(app),
                &place,
                risk("click", "button", &want, ""),
            );
            if gate == Gate::Refuse {
                return format!(
                    "Error: the user keeps Sidekick out of {place} (Settings > Skills > Agent)."
                );
            }
            match find_on_screen(app, &want).await {
                Ok((x, y, how)) => {
                    let cmd = json!({ "x": x, "y": y, "what": want });
                    if gate == Gate::Tap {
                        crate::ask_tools::offer(
                            app,
                            chat_id,
                            "screen_click",
                            cmd,
                            &format!("Click {want}"),
                        );
                        return format!(
                            "Found \"{want}\" ({how}). Not clicked yet: it is a button the user taps."
                        );
                    }
                    tokio::task::spawn_blocking(move || crate::screen::click(x, y))
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r)
                        .map(|()| {
                            format!("Clicked \"{want}\" ({how}). Read again to see the result.")
                        })
                }
                Err(e) => Err(e),
            }
        }
        "selection" => {
            let t = target.clone();
            blocking(move || uia::selection(&t))
                .await
                .map(|s| format!("Selected text:\n{s}"))
        }
        "type_here" => {
            let text = args["text"].as_str().unwrap_or_default().to_owned();
            let t = target.clone();
            blocking(move || uia::type_here(&t, &text))
                .await
                .map(|o| o.message)
        }
        other => Err(format!("unknown desktop action {other}")),
    };
    out.unwrap_or_else(|e| format!("Error: {e}"))
}

/// Where words are on the user's window, as a desktop point: read with OCR,
/// or by the vision model when OCR finds nothing and one is set.
async fn find_on_screen(app: &AppHandle, want: &str) -> Result<(i32, i32, &'static str), String> {
    let pid = lock(&app.state::<AppState>().last_window)
        .as_ref()
        .and_then(|w| w["pid"].as_u64())
        .and_then(|p| u32::try_from(p).ok());
    let (png, ox, oy) = tokio::task::spawn_blocking(move || crate::screen::capture_at(pid))
        .await
        .map_err(|e| e.to_string())??;
    let (exec, path, ai) = {
        let state = app.state::<AppState>();
        std::fs::create_dir_all(&state.scratch_dir).map_err(|e| e.to_string())?;
        (
            crate::state::executor(&state),
            state.scratch_dir.join("screen-find.png"),
            lock(&state.settings).ai.local.clone(),
        )
    };
    std::fs::write(&path, &png).map_err(|e| e.to_string())?;
    let found = exec.find_text(&path, want).await;
    let _ = std::fs::remove_file(&path);
    if let Ok(Some((x, y, w, h))) = found {
        return Ok((ox + x + w / 2, oy + y + h / 2, "read on screen"));
    }
    let vision = ai.vision_model.trim().to_owned();
    if vision.is_empty() {
        return Err(match found {
            Err(e) => format!("could not read the screen ({e})"),
            Ok(_) => format!("\"{want}\" is not on the screen as text"),
        });
    }
    let (w, h) = crate::screen::png_size(&png).ok_or("the screenshot is unreadable")?;
    let model = sidekick_ai::OpenAiCompat::new(Some(ai.base_url), Some(vision));
    let req = sidekick_ai::ChatRequest {
        system: format!(
            "You locate things in a {w}x{h} screenshot. Answer only JSON: {{\"x\":<int>,\"y\":<int>}} \
             for the center of the thing asked about, in image pixels, or {{\"none\":true}}."
        ),
        messages: vec![sidekick_ai::Message::user(format!("Where is: {want}"))],
        image: Some(png),
    };
    let answer = tokio::time::timeout(Duration::from_secs(40), model.complete(&req, json!({})))
        .await
        .map_err(|_| "the vision model took too long".to_owned())?
        .map_err(|e| e.to_string())?;
    let (x, y) =
        point_in(&answer, w, h).ok_or_else(|| format!("\"{want}\" was not found on the screen"))?;
    Ok((ox + x, oy + y, "seen by the vision model"))
}

/// The `{"x":..,"y":..}` in a model's answer, when it lies in the image.
fn point_in(answer: &str, w: u32, h: u32) -> Option<(i32, i32)> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    let v: Value = serde_json::from_str(answer.get(start..=end)?).ok()?;
    let x = v["x"].as_f64()?;
    let y = v["y"].as_f64()?;
    (x >= 0.0 && y >= 0.0 && x < f64::from(w) && y < f64::from(h)).then_some((x as i32, y as i32))
}

/// The `apps` tool: find and install apps with winget, Wi-Fi networks.
pub async fn apps(app: &AppHandle, chat_id: &str, args: &Value) -> String {
    use sidekick_actions::pc;
    let name = args["name"].as_str().unwrap_or_default().to_owned();
    let out = match args["action"].as_str().unwrap_or("search") {
        "search" => blocking(move || pc::app_search(&name)).await,
        "install" | "update" => {
            let update = args["action"] == "update";
            if !pc::valid_app_id(&name) {
                return "Error: pass the winget id from search, e.g. Spotify.Spotify.".into();
            }
            let action = if update { "update_app" } else { "install_app" };
            let label = format!("{} {name}", if update { "Update" } else { "Install" });
            crate::ask_tools::offer(app, chat_id, action, json!({ "id": name }), &label);
            Ok(format!(
                "\"{label}\" is a button the user taps; installs need their yes."
            ))
        }
        "wifi_networks" => blocking(pc::wifi_networks).await.map(|n| {
            if n.is_empty() {
                "No Wi-Fi networks found.".into()
            } else {
                n.join("\n")
            }
        }),
        "wifi_connect" => blocking(move || pc::wifi_connect(&name))
            .await
            .map(|o| o.message),
        other => Err(format!("unknown apps action {other}")),
    };
    out.unwrap_or_else(|e| format!("Error: {e}"))
}

/// A short, human preview of an app change: "Gmail send email to ali@x.com:
/// Invoice".
pub fn preview(tool: &str, args: &Value) -> String {
    let words: Vec<String> = tool
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let w = w.to_lowercase();
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect();
    let mut out = words.join(" ");
    let pick = |keys: &[&str]| {
        keys.iter().find_map(|k| {
            args[*k]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
    };
    if let Some(to) = pick(&[
        "to",
        "recipient_email",
        "recipient",
        "channel",
        "email",
        "attendees",
    ]) {
        out.push_str(&format!(" to {to}"));
    }
    if let Some(what) = pick(&[
        "subject", "title", "summary", "name", "text", "body", "message",
    ]) {
        let short: String = what.chars().take(40).collect();
        out.push_str(&format!(": {short}"));
    }
    out.chars().take(60).collect()
}

/// A change in a connected app the model asked for: a button with a
/// preview; it runs only on the user's tap.
pub fn offer_app_change(app: &AppHandle, chat_id: &str, tool: &str, arguments: &Value) -> String {
    let label = preview(tool, arguments);
    crate::ask_tools::offer(
        app,
        chat_id,
        "app_action",
        json!({ "tool": tool, "arguments": arguments }),
        &label,
    );
    format!(
        "Prepared, not done: \"{label}\" is a button the user taps to do it. Tell them in one \
         sentence what it will do and that it waits for their tap."
    )
}

/// Runs a tapped button that is a browser or app step. None for other actions.
pub async fn run(app: &AppHandle, action: &str, args: &Value) -> Option<Result<Outcome, String>> {
    let msg = |m: String| Outcome {
        message: m,
        path: None,
    };
    Some(match action {
        "browser_act" => ask(app, args.clone()).await.map(|v| {
            msg(format!(
                "{} on {}",
                v["note"].as_str().unwrap_or("Done"),
                v["title"].as_str().unwrap_or("the page")
            ))
        }),
        "desktop_act" => {
            let target = sidekick_actions::uia::Target {
                pid: args["pid"].as_u64().and_then(|p| u32::try_from(p).ok()),
                app: args["app"].as_str().map(str::to_owned),
            };
            let n = args["ref"].as_u64().unwrap_or_default() as usize;
            let name = args["name"].as_str().unwrap_or_default().to_owned();
            let what = args["do"].as_str().unwrap_or("click").to_owned();
            let text = args["text"].as_str().unwrap_or_default().to_owned();
            blocking(move || sidekick_actions::uia::act(&target, n, &name, &what, &text)).await
        }
        "excel_write" => crate::office::run_write(args).await,
        "screen_click" => {
            let (x, y) = (
                args["x"].as_i64().unwrap_or_default() as i32,
                args["y"].as_i64().unwrap_or_default() as i32,
            );
            let what = args["what"].as_str().unwrap_or("it").to_owned();
            tokio::task::spawn_blocking(move || crate::screen::click(x, y))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
                .map(|()| msg(format!("Clicked {what}")))
        }
        "desktop_keys" => {
            let target = sidekick_actions::uia::Target {
                pid: args["pid"].as_u64().and_then(|p| u32::try_from(p).ok()),
                app: args["app"].as_str().map(str::to_owned),
            };
            let keys = args["keys"].as_str().unwrap_or_default().to_owned();
            blocking(move || sidekick_actions::uia::keys(&target, &keys)).await
        }
        "app_action" => {
            let tool = args["tool"].as_str().unwrap_or_default();
            crate::composio::run_tapped(app, tool, &args["arguments"])
                .await
                .map(|text| {
                    let first = text
                        .lines()
                        .next()
                        .unwrap_or("Done")
                        .chars()
                        .take(120)
                        .collect();
                    msg(first)
                })
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_points_from_a_vision_answer() {
        assert_eq!(
            point_in("Sure: {\"x\": 120, \"y\": 40.6}", 800, 600),
            Some((120, 40))
        );
        assert_eq!(point_in("{\"none\":true}", 800, 600), None);
        assert_eq!(
            point_in("{\"x\":900,\"y\":10}", 800, 600),
            None,
            "outside the image"
        );
        assert_eq!(point_in("no idea", 800, 600), None);
    }

    #[test]
    fn sending_waits_for_a_tap() {
        assert!(needs_tap("click", "button", "Send", ""));
        assert!(needs_tap("click", "button", "Place order", ""));
        assert!(
            needs_tap("click", "button", "", ""),
            "an unnamed button could be anything"
        );
        assert!(!needs_tap("click", "link", "Inbox", ""));
        assert!(!needs_tap("click", "tab", "Settings", ""));
        assert!(needs_tap("press", "textbox", "Type a message", "Enter"));
        assert!(!needs_tap("press", "searchbox", "Search", "Enter"));
        assert!(!needs_tap("type", "textbox", "To", ""));
    }

    #[test]
    fn settings_decide_what_waits() {
        let mut a = sidekick_core::AgentSettings::default();
        assert_eq!(risk("click", "button", "Send", ""), Risk::Outward);
        assert_eq!(
            risk("click", "button", "Place order", ""),
            Risk::Irreversible
        );
        assert_eq!(risk("type", "textbox", "To", ""), Risk::Step);
        assert_eq!(decide(&a, "mail.google.com", Risk::Step), Gate::Run);
        assert_eq!(decide(&a, "mail.google.com", Risk::Outward), Gate::Tap);
        a.places.insert("google.com".into(), "allow".into());
        a.places.insert("whatsapp".into(), "never".into());
        assert_eq!(decide(&a, "mail.google.com", Risk::Outward), Gate::Run);
        assert_eq!(
            decide(&a, "mail.google.com", Risk::Irreversible),
            Gate::Tap,
            "paying always asks"
        );
        assert_eq!(decide(&a, "WhatsApp", Risk::Step), Gate::Refuse);
        a.ask = "each".into();
        assert_eq!(decide(&a, "slack", Risk::Step), Gate::Tap);
        a.ask = "irreversible".into();
        assert_eq!(decide(&a, "slack", Risk::Outward), Gate::Run);
    }

    #[test]
    fn remembers_elements_from_a_read() {
        remember(
            7,
            "[1] link \"Inbox\"\n[2] button \"Send\" (disabled)\n[3] textbox(email) \"To\" = \"a@b.c\"",
        );
        assert_eq!(element(7, "2"), Some(("button".into(), "Send".into())));
        assert_eq!(element(7, "3").unwrap().0, "textbox(email)");
        assert_eq!(element(7, "9"), None);
    }

    #[test]
    fn names_places() {
        assert_eq!(app_of("Inbox - ali@x.com - Outlook"), "Outlook");
        assert_eq!(app_of("Files"), "Files");
        assert_eq!(host("https://www.upwork.com/jobs/1"), "upwork.com");
        assert!(super::super::ai::looks_multistep(
            "reply to Ali and attach the invoice"
        ));
        assert!(!super::super::ai::looks_multistep("what is on my screen"));
    }

    #[test]
    fn previews_app_changes() {
        let p = preview(
            "GMAIL_SEND_EMAIL",
            &json!({ "recipient_email": "ali@x.com", "subject": "Invoice for September" }),
        );
        assert_eq!(p, "Gmail Send Email to ali@x.com: Invoice for September");
        assert!(
            preview(
                "SLACK_SEND_MESSAGE",
                &json!({ "channel": "#dev", "text": "hi" })
            )
            .starts_with("Slack Send Message to #dev")
        );
    }
}
