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

pub fn risky(label: &str) -> bool {
    RISKY.is_match(label)
}

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
pub fn needs_tap(action: &str, kind: &str, label: &str, key: &str) -> bool {
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

fn deny_list(app: &AppHandle) -> Vec<String> {
    lock(&app.state::<AppState>().settings).deny_sites.clone()
}

async fn ask(app: &AppHandle, mut cmd: Value) -> Result<Value, String> {
    let bridge = app.state::<AppState>().browser.clone();
    if !bridge.connected() {
        return Err(
            "the Sidekick browser extension is not connected (Settings > Apps > Browser)".into(),
        );
    }
    cmd["deny"] = json!(deny_list(app));
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
    if needs_tap(what, &kind, &label, text) {
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

async fn blocking<T: Send + 'static>(
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
            if needs_tap(&what, &kind, &name, "") {
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
            if uia::keys_send(&keys) {
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
