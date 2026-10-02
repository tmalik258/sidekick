//! Tools the local model can use in Ask mode, all on this PC: search your
//! files and history, read your day, see what just happened, and open
//! things. They answer in short plain text a small model can use.

use serde::Serialize;
use serde_json::{Value, json};
use sidekick_ai::ToolDef;
use sidekick_core::ActionRecord;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, executor, lock};

const SEARCH: &str = "search";
const TODAY: &str = "today";
const RECENT: &str = "recent";
const OPEN: &str = "open";
const SCREEN: &str = "screen_text";
const PROPOSE: &str = "propose";

pub const PROPOSAL_EVENT: &str = "ai://proposal";
const SKILL_ID: &str = "ask";

/// What Ask may offer to do. Each runs only after the user taps it, and
/// files it makes or moves can be undone.
const ASK_ACTIONS: &[&str] = &[
    "open_path",
    "reveal_path",
    "open_url",
    "open_folder",
    "open_in_editor",
    "launch_project",
    "copy_text",
    "convert",
    "extract_archive",
    "extract_text",
    "zip",
    "move_file",
    "git_pull",
    "install_deps",
    "create_env",
];

/// An action waiting for a tap, from one chat.
#[derive(Debug, Clone)]
pub struct Proposed {
    pub action: String,
    pub args: Value,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProposalNote<'a> {
    chat_id: &'a str,
    id: &'a str,
    label: &'a str,
}

/// What a tapped action did, for the answer it belongs to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ran {
    pub ok: bool,
    pub message: String,
    pub undo_id: Option<i64>,
    pub path: Option<String>,
}

/// Most screen text handed to the model.
const MAX_SCREEN_TEXT: usize = 4000;

/// How far back `recent` looks.
const RECENT_EVENTS: u32 = 40;

pub fn defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: SEARCH.into(),
            description: "Find the user's own files, downloads, screenshots, copied text, \
                pages they read, meeting notes and past answers on this PC. Use for any question about \
                their stuff."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": { "query": { "type": "string", "description": "A few words to look for" } },
                "required": ["query"],
            }),
        },
        ToolDef {
            name: TODAY.into(),
            description: "Today's meetings and how the user's time went, by app and project."
                .into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: RECENT.into(),
            description: "What just happened on the PC: downloads, copies, windows, \
                errors, Claude Code sessions. Newest first."
                .into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: SCREEN.into(),
            description: "Read the text in the window the user was just in (an error, a page, \
                a document). Use when they say this, here, or ask about their screen."
                .into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: PROPOSE.into(),
            description: "Offer to do something for the user as a button they tap: move_file \
                {path, to}, zip {paths, name}, convert {path, to: png|jpg|webp|pdf|mp3|mp4}, \
                extract_archive {path}, extract_text {path}, open_path {path}, reveal_path {path}, \
                open_url {url}, launch_project {path}, git_pull {path}, install_deps {path}. \
                Use full paths from search. Nothing happens until they tap it."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ASK_ACTIONS },
                    "args": { "type": "object" },
                    "label": { "type": "string", "description": "Button text, e.g. Move invoice.pdf to Invoices" }
                },
                "required": ["action", "args", "label"],
            }),
        },
        ToolDef {
            name: OPEN.into(),
            description: "Open a file, folder or web page for the user. Pass a full path \
                from search, or a URL."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": { "target": { "type": "string", "description": "Full path or URL" } },
                "required": ["target"],
            }),
        },
    ]
}

/// Runs a local tool, or `None` when `name` is not one of them.
pub async fn run(app: &AppHandle, chat_id: &str, name: &str, args: &Value) -> Option<String> {
    Some(match name {
        PROPOSE => propose(app, chat_id, args),
        SEARCH => search(app, args["query"].as_str().unwrap_or_default()).await,
        TODAY => today(app).await,
        RECENT => recent(app),
        OPEN => open(app, args["target"].as_str().unwrap_or_default()).await,
        SCREEN => match crate::ai::screenshot(app).await {
            Ok(png) => match screen_text(app, &png).await {
                Ok(t) if t.is_empty() => "No readable text on screen.".into(),
                Ok(t) => t,
                Err(e) => format!("Error: {e}"),
            },
            Err(e) => format!("Error: could not capture the screen: {e}"),
        },
        _ => return None,
    })
}

/// Searches everything on this PC (files, downloads, screenshots, clipboard,
/// pages read, meeting notes, past answers), by words and by meaning.
async fn search(app: &AppHandle, query: &str) -> String {
    let hits = crate::search::hybrid(app, query, crate::search::LOCAL, 6).await;
    if hits.is_empty() {
        return format!("Nothing found for \"{query}\".");
    }
    hits.iter()
        .enumerate()
        .map(|(i, h)| {
            format!(
                "{}. [{}] {} ({}, {})\n   {}",
                i + 1,
                h.source,
                h.title,
                h.reference,
                short_time(&h.ts),
                clip(&h.snippet, 200)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Keeps an action as a button under the answer; it runs on a tap.
fn propose(app: &AppHandle, chat_id: &str, args: &Value) -> String {
    let action = args["action"].as_str().unwrap_or_default();
    if !ASK_ACTIONS.contains(&action) {
        return format!("Error: {action} is not something you can offer.");
    }
    let label: String = args["label"]
        .as_str()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or(action)
        .chars()
        .take(60)
        .collect();
    let action_args = args["args"].clone();
    if action == "open_path" && action_args["path"].as_str().is_some_and(runs_code) {
        return "Refused: that file runs a program. Tell the user to open it themselves.".into();
    }
    let id = ulid::Ulid::new().to_string();
    lock(&app.state::<AppState>().ask_proposals).insert(
        id.clone(),
        Proposed {
            action: action.into(),
            args: action_args,
            label: label.clone(),
        },
    );
    let _ = app.emit(
        PROPOSAL_EVENT,
        ProposalNote {
            chat_id,
            id: &id,
            label: &label,
        },
    );
    format!("Shown to the user as a button \"{label}\". Say in one short sentence what it will do.")
}

/// Runs a proposal the user tapped, logs it, and keeps Undo for files it
/// made or moved.
pub async fn run_proposal(app: &AppHandle, id: &str) -> Result<Ran, String> {
    let proposed = lock(&app.state::<AppState>().ask_proposals)
        .remove(id)
        .ok_or("That button has expired; ask again")?;
    let exec = executor(&app.state::<AppState>());
    let result = exec.run(&proposed.action, &proposed.args).await;
    let (ok, message, path) = match &result {
        Ok(o) => (true, o.message.clone(), o.path.clone()),
        Err(e) => (false, e.to_string(), None),
    };
    let undo_path = match (&result, proposed.action.as_str()) {
        (Ok(o), "move_file") => o
            .path
            .as_deref()
            .zip(proposed.args["path"].as_str())
            .map(|(now, from)| crate::undo::move_back(now, from)),
        (Ok(o), action) => crate::undo::undo_path(action, o.path.as_deref()),
        _ => None,
    };
    let record = ActionRecord {
        id: 0,
        ts: chrono::Utc::now().to_rfc3339(),
        skill_id: SKILL_ID.into(),
        action: proposed.action.clone(),
        label: proposed.label.clone(),
        ok,
        message: message.clone(),
        auto: false,
        undo_path: undo_path.clone(),
        undone: false,
    };
    let undo_id = lock(&app.state::<AppState>().storage)
        .log_action(&record)
        .ok()
        .filter(|_| undo_path.is_some());
    Ok(Ran {
        ok,
        message,
        undo_id,
        path,
    })
}

/// The text in a screenshot, read on this PC with Tesseract.
pub async fn screen_text(app: &AppHandle, png: &[u8]) -> Result<String, String> {
    let state = app.state::<AppState>();
    std::fs::create_dir_all(&state.scratch_dir).map_err(|e| e.to_string())?;
    let path = state.scratch_dir.join("screen-ocr.png");
    std::fs::write(&path, png).map_err(|e| e.to_string())?;
    let exec = executor(&state);
    let text = exec.read_text(&path).await.map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&path);
    Ok(clip(&text?, MAX_SCREEN_TEXT))
}

async fn today(app: &AppHandle) -> String {
    let meetings = {
        let state = app.state::<AppState>();
        let c = lock(&state.calendar);
        let day = chrono::Local::now().date_naive();
        sidekick_sensors::calendar::on_day(&c.meetings, day)
            .iter()
            .map(|m| {
                format!(
                    "{} to {}: {}",
                    m.start.with_timezone(&chrono::Local).format("%H:%M"),
                    m.end.with_timezone(&chrono::Local).format("%H:%M"),
                    m.title
                )
            })
            .collect::<Vec<_>>()
    };
    let time = crate::mcp::call_text(app, "sidekick_time_today", &json!({})).await;
    let meetings = if meetings.is_empty() {
        "No meetings today.".to_owned()
    } else {
        format!("Meetings:\n{}", meetings.join("\n"))
    };
    format!(
        "Now: {}\n{meetings}\nTime today:\n{time}",
        chrono::Local::now().format("%A %H:%M")
    )
}

fn recent(app: &AppHandle) -> String {
    let events = lock(&app.state::<AppState>().storage)
        .recent_events(RECENT_EVENTS)
        .unwrap_or_default();
    let lines: Vec<String> = events
        .iter()
        // Secrets never reach a model, and focus changes are noise here.
        .filter(|e| e.sensitivity != "secret" && e.kind != "window.focused")
        .take(15)
        .map(|e| {
            format!(
                "{} {}: {}",
                short_time(&e.ts),
                e.kind,
                brief(e.payload.as_ref())
            )
        })
        .collect();
    if lines.is_empty() {
        "Nothing recent.".into()
    } else {
        lines.join("\n")
    }
}

fn short_time(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

/// The few fields of an event worth reading.
fn brief(payload: Option<&Value>) -> String {
    let Some(p) = payload else {
        return String::new();
    };
    [
        "name", "title", "app", "project", "path", "url", "preview", "message",
    ]
    .iter()
    .filter_map(|k| p[*k].as_str().map(|v| format!("{k}={}", clip(v, 80))))
    .take(3)
    .collect::<Vec<_>>()
    .join(", ")
}

fn clip(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push_str("...");
    }
    out
}

async fn open(app: &AppHandle, target: &str) -> String {
    let target = target.trim();
    if target.is_empty() {
        return "Error: nothing to open.".into();
    }
    if runs_code(target) {
        return "Refused: that file runs a program. Tell the user to open it themselves.".into();
    }
    let (action, args) = if target.starts_with("http://") || target.starts_with("https://") {
        ("open_url", json!({ "url": target }))
    } else {
        ("open_path", json!({ "path": target }))
    };
    let exec = executor(&app.state::<AppState>());
    match exec.run(action, &args).await {
        Ok(o) => o.message,
        Err(e) => format!("Error: {e}"),
    }
}

/// Files that run something when opened. The model only opens documents,
/// folders and pages; a program starts only when the user starts it.
fn runs_code(target: &str) -> bool {
    const RUNS: &[&str] = &[
        "exe", "msi", "bat", "cmd", "com", "ps1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "scr",
        "lnk", "url", "hta", "cpl", "msc", "jar", "reg", "appx", "msix",
    ];
    if target.starts_with("http://") || target.starts_with("https://") {
        return false;
    }
    std::path::Path::new(target)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| RUNS.contains(&e.to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn briefs_keep_only_useful_fields() {
        let p = json!({ "name": "invoice.pdf", "size": 1200, "path": "C:/Users/me/Downloads/invoice.pdf" });
        assert_eq!(
            brief(Some(&p)),
            "name=invoice.pdf, path=C:/Users/me/Downloads/invoice.pdf"
        );
        assert_eq!(brief(None), "");
        assert_eq!(clip("abcdef", 3), "abc...");
    }

    #[test]
    fn never_opens_programs() {
        assert!(runs_code("C:/Users/me/Downloads/setup.EXE"));
        assert!(runs_code("C:/x/run.ps1"));
        assert!(!runs_code("C:/Users/me/Downloads/invoice.pdf"));
        assert!(!runs_code("C:/Users/me/Projects"));
        assert!(!runs_code("https://example.com/setup.exe"));
    }

    #[test]
    fn tools_have_short_names() {
        let names: Vec<_> = defs().into_iter().map(|d| d.name).collect();
        assert_eq!(names, [SEARCH, TODAY, RECENT, SCREEN, PROPOSE, OPEN]);
    }
}
