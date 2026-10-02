//! Tools the local model can use in Ask mode, all on this PC: search your
//! files and history, read your day, see what just happened, and open
//! things. They answer in short plain text a small model can use.

use serde_json::{Value, json};
use sidekick_ai::ToolDef;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, executor, lock};

const SEARCH: &str = "search";
const TODAY: &str = "today";
const RECENT: &str = "recent";
const OPEN: &str = "open";

/// How far back `recent` looks.
const RECENT_EVENTS: u32 = 40;

pub fn defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: SEARCH.into(),
            description: "Find the user's own files, downloads, screenshots, copied text, \
                notes and past answers on this PC. Use for any question about their stuff."
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
pub async fn run(app: &AppHandle, name: &str, args: &Value) -> Option<String> {
    Some(match name {
        SEARCH => {
            let query = args["query"].as_str().unwrap_or_default();
            crate::mcp::call_text(
                app,
                "sidekick_search",
                &json!({ "query": query, "limit": 6 }),
            )
            .await
        }
        TODAY => today(app).await,
        RECENT => recent(app),
        OPEN => open(app, args["target"].as_str().unwrap_or_default()).await,
        _ => return None,
    })
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
        assert_eq!(names, [SEARCH, TODAY, RECENT, OPEN]);
    }
}
