//! Tools the local model can use in Ask mode, all on this PC: search your
//! files and history, read your day, see what just happened, and open
//! things. They answer in short plain text a small model can use.

use serde::Serialize;
use serde_json::{Value, json};
use sidekick_ai::ToolDef;
use sidekick_core::ActionRecord;
use tauri::{AppHandle, Emitter, Manager};

use sidekick_actions::pc;

use crate::state::{AppState, executor, lock};

const SEARCH: &str = "search";
const TODAY: &str = "today";
const RECENT: &str = "recent";
const OPEN: &str = "open";
const SCREEN: &str = "screen_text";
const PROPOSE: &str = "propose";
const FIND: &str = "find_files";
const REVEAL: &str = "show_in_folder";
const PC_STATUS: &str = "pc_status";
const PC: &str = "pc_control";
const WINDOWS: &str = "windows";
const WEB_SEARCH: &str = "web_search";
const READ_PAGE: &str = "read_page";
const NOTIFS: &str = "notifications";
const BROWSER: &str = "browser";
const APP_ACTION: &str = "app_action";
const DESKTOP: &str = "desktop";
const APPS: &str = "apps";
const OFFICE: &str = "office";
const RECIPES: &str = "recipes";
const REMEMBER: &str = "remember";

/// The Ask chat answering right now, so tools called through Sidekick's
/// MCP server (by Claude Code or Codex) put their buttons in it.
static CURRENT_CHAT: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub fn set_current_chat(id: &str) {
    if let Ok(mut c) = CURRENT_CHAT.lock() {
        *c = id.to_owned();
    }
}

pub fn current_chat() -> String {
    CURRENT_CHAT.lock().map(|c| c.clone()).unwrap_or_default()
}

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
    "open_system_page",
    "launch_app",
    "close_app",
    "sleep_pc",
    "empty_recycle_bin",
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
            name: FIND.into(),
            description: "Find files or folders on this PC by name (Desktop, Documents, \
                Downloads, code folders, app data). Use for \"find\", \"where is\" or \"open my ...\" \
                when you need a path. Returns full paths, newest info included."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Words in the name, e.g. sidekick or invoice sept" },
                    "kind": { "type": "string", "enum": ["any", "file", "folder"] }
                },
                "required": ["name"],
            }),
        },
        ToolDef {
            name: REVEAL.into(),
            description: "Show a file or folder in File Explorer, selected. Pass a full path."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
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
                open_url {url}, launch_project {path}, git_pull {path}, install_deps {path}, \
                close_app {name}, sleep_pc {}, empty_recycle_bin {}. \
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
            name: WEB_SEARCH.into(),
            description: "Search the web for anything not on this PC: news, docs, prices, \
                how-tos, facts that may have changed. Returns titles, links and snippets; use \
                read_page on a link for the details."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"],
            }),
        },
        ToolDef {
            name: READ_PAGE.into(),
            description: "Read a web page as text, from a link the user gave or one web_search \
                found."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": { "url": { "type": "string" } },
                "required": ["url"],
            }),
        },
        ToolDef {
            name: BROWSER.into(),
            description: "Use the user's own browser (signed in to their sites): tabs lists open \
                pages, open {url} opens one, read shows a page's text and its buttons and fields \
                as numbered elements, act {ref, do: click|type|select|press|scroll, text} acts on \
                one, extract {what: tables|links|text} pulls data out. Read before acting, and read \
                again after. Sending, submitting or paying becomes a button the user taps."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["tabs", "open", "read", "act", "extract", "switch", "close", "back"] },
                    "url": { "type": "string" },
                    "tab": { "type": "integer", "description": "Tab number; the active tab when left out" },
                    "ref": { "type": "string", "description": "Element number from read" },
                    "do": { "type": "string", "enum": ["click", "type", "select", "press", "scroll"] },
                    "text": { "type": "string", "description": "What to type or pick, the key to press, or up/down to scroll" },
                    "what": { "type": "string", "enum": ["tables", "links", "text"] }
                },
                "required": ["action"],
            }),
        },
        ToolDef {
            name: APP_ACTION.into(),
            description: "Change something in a connected app (send an email, post to Slack, \
                create an event or ticket) by Composio tool name and arguments. It becomes a \
                button with a preview that the user taps; nothing runs before that."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tool": { "type": "string", "description": "Composio tool slug, e.g. GMAIL_SEND_EMAIL" },
                    "arguments": { "type": "object" }
                },
                "required": ["tool", "arguments"],
            }),
        },
        ToolDef {
            name: DESKTOP.into(),
            description: "Use any Windows app (the one the user was in, or one named by app): \
                read lists its buttons, fields, menus and lists as numbered controls; act {ref, \
                do: click|type|select|focus, text} uses one; keys {text} sends shortcuts like ^s \
                or {TAB} when an app shows no controls; selection reads the selected text; \
                type_here {text} puts text into the field the user is in (to rewrite a \
                selection, read it, then type_here the new text); click_text {text} finds those \
                words on the screen and clicks them, for apps whose read shows no controls. \
                Read before acting and after."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["read", "act", "keys", "selection", "type_here", "click_text"] },
                    "app": { "type": "string", "description": "App or window name; the app the user was in when left out" },
                    "ref": { "type": "string" },
                    "do": { "type": "string", "enum": ["click", "type", "select", "focus"] },
                    "text": { "type": "string" }
                },
                "required": ["action"],
            }),
        },
        ToolDef {
            name: APPS.into(),
            description: "Apps and networks: search {name} finds apps to install (winget), \
                install or update {name: winget id} becomes a button the user taps, \
                wifi_networks lists networks in range, wifi_connect {name} joins a saved one."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["search", "install", "update", "wifi_networks", "wifi_connect"] },
                    "name": { "type": "string" }
                },
                "required": ["action"],
            }),
        },
        ToolDef {
            name: OFFICE.into(),
            description: "Office on this PC (the desktop apps): email_draft {to, subject, body, \
                attach [full paths]} opens an Outlook draft for the user to send; excel_read \
                {path, sheet?, range?} returns cells (all used cells when no range); excel_write \
                {path, sheet?, range (start cell), values (rows by line, cells by tab)} fills \
                cells and saves; word_create {path ending .docx or .pdf, text} makes a new \
                document; to_pdf {path} saves a Word file as PDF next to it. Find files first \
                with find_files to get full paths."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["email_draft", "excel_read", "excel_write", "word_create", "to_pdf"] },
                    "path": { "type": "string" },
                    "to": { "type": "string" },
                    "subject": { "type": "string" },
                    "body": { "type": "string" },
                    "attach": { "type": "array", "items": { "type": "string" } },
                    "sheet": { "type": "string" },
                    "range": { "type": "string" },
                    "values": { "type": "string" },
                    "text": { "type": "string" }
                },
                "required": ["action"],
            }),
        },
        ToolDef {
            name: RECIPES.into(),
            description: "Saved tasks the user can run again: list; create {name, prompt (the \
                instruction in their words), when: manual|time|notification|download|meeting_ended|\
                app_opened, time HH:MM, days [mon..sun], app, contains, kind}; run {name}; delete \
                {name}. \"Every Friday at 5 email my timesheet\" is create with when time, time \
                17:00, days [fri]."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "create", "run", "delete"] },
                    "name": { "type": "string" },
                    "prompt": { "type": "string" },
                    "when": { "type": "string", "enum": ["manual", "time", "notification", "download", "meeting_ended", "app_opened"] },
                    "time": { "type": "string", "description": "HH:MM, 24 hour" },
                    "days": { "type": "array", "items": { "type": "string" } },
                    "app": { "type": "string" },
                    "contains": { "type": "string" },
                    "kind": { "type": "string", "description": "pdf, image, document..." }
                },
                "required": ["action"],
            }),
        },
        ToolDef {
            name: REMEMBER.into(),
            description: "Keep a fact about the user for every future answer (\"my manager is \
                Sara\", \"sign emails as Tayyab\"), or forget one. Only what they ask you to keep."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "fact": { "type": "string" },
                    "forget": { "type": "boolean" }
                },
                "required": ["fact"],
            }),
        },
        ToolDef {
            name: NOTIFS.into(),
            description: "The user's recent Windows notifications, sorted by importance (now, \
                soon, digest). Use for \"what did I miss\", \"anything from Ali\" or messages \
                waiting. level important skips the digest."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "level": { "type": "string", "enum": ["important", "all"] },
                    "from": { "type": "string", "description": "An app or a person" },
                    "minutes": { "type": "integer", "description": "Only the last N minutes" }
                },
            }),
        },
        ToolDef {
            name: PC_STATUS.into(),
            description: "What is on right now on this Windows PC: night light, Do Not Disturb, \
                dark mode, battery, brightness, Wi-Fi. Check this before suggesting a change, and \
                never offer to turn on what is already on."
                .into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: PC.into(),
            description: "Change an everyday Windows setting right away: volume_up, volume_down, \
                mute, set_volume {level}, brightness {level}, dark_mode_on, dark_mode_off, \
                bluetooth_on, bluetooth_off, wifi_on, wifi_off, dnd_on, dnd_off (Do Not Disturb), lock, \
                audio_outputs (lists speakers and headphones), audio_output {page: device name} \
                plays sound there, display {page: internal|clone|extend|external} sets screens, \
                open_settings {page} (no page opens Windows Settings itself). Pages: home, display, nightlight, sound, notifications, focus \
                (Do Not Disturb), bluetooth, wifi, network, battery, power, storage, apps, \
                default_apps, startup_apps, colors, background, mouse, keyboard, printers, updates, \
                privacy, accounts, time, language, about. Night light has no switch: open its page."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "what": { "type": "string", "enum": [
                        "volume_up", "volume_down", "mute", "set_volume", "brightness",
                        "dark_mode_on", "dark_mode_off", "bluetooth_on", "bluetooth_off",
                        "wifi_on", "wifi_off", "dnd_on", "dnd_off", "lock", "open_settings",
                        "audio_outputs", "audio_output", "display"
                    ] },
                    "level": { "type": "integer", "minimum": 0, "maximum": 100 },
                    "page": { "type": "string" }
                },
                "required": ["what"],
            }),
        },
        ToolDef {
            name: WINDOWS.into(),
            description: "Apps and windows: list (the open windows), focus {name} (bring one \
                to the front), launch {name} (start an installed app, e.g. Spotify). To close one, \
                use propose with close_app."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "focus", "launch"] },
                    "name": { "type": "string", "description": "App or window name" }
                },
                "required": ["action"],
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

/// Tools that reach the internet or other apps, left out for "This PC only".
pub fn is_web(name: &str) -> bool {
    matches!(name, WEB_SEARCH | READ_PAGE | BROWSER | APP_ACTION | APPS)
}

/// Runs a local tool, or `None` when `name` is not one of them.
pub async fn run(app: &AppHandle, chat_id: &str, name: &str, args: &Value) -> Option<String> {
    Some(match name {
        PROPOSE => propose(app, chat_id, args),
        FIND => find(app, args).await,
        REVEAL => reveal(app, args["path"].as_str().unwrap_or_default()).await,
        SEARCH => search(app, args["query"].as_str().unwrap_or_default()).await,
        TODAY => today(app).await,
        BROWSER => crate::act::browser(app, chat_id, args).await,
        DESKTOP => crate::act::desktop(app, chat_id, args).await,
        APPS => crate::act::apps(app, chat_id, args).await,
        OFFICE => crate::office::tool(app, chat_id, args).await,
        RECIPES => crate::recipes::tool(app, args),
        REMEMBER => crate::recipes::remember(
            app,
            args["fact"].as_str().unwrap_or_default(),
            args["forget"].as_bool().unwrap_or(false),
        )
        .unwrap_or_else(|e| format!("Error: {e}")),
        APP_ACTION => {
            let tool = args["tool"].as_str().unwrap_or_default();
            if tool.is_empty() {
                "Error: name the app tool, e.g. GMAIL_SEND_EMAIL.".into()
            } else {
                crate::act::offer_app_change(app, chat_id, tool, &args["arguments"])
            }
        }
        NOTIFS => crate::inbox::describe(
            args["level"].as_str(),
            args["from"].as_str().filter(|s| !s.is_empty()),
            args["minutes"].as_u64(),
        ),
        WEB_SEARCH => crate::web::search(args["query"].as_str().unwrap_or_default())
            .await
            .unwrap_or_else(|e| format!("Error: {e}")),
        READ_PAGE => crate::web::read(args["url"].as_str().unwrap_or_default())
            .await
            .unwrap_or_else(|e| format!("Error: {e}")),
        PC_STATUS => blocking(|| Ok(pc::describe(&pc::read_state()))).await,
        PC => {
            let what = args["what"].as_str().unwrap_or_default().to_owned();
            let level = args["level"].as_u64().and_then(|v| u8::try_from(v).ok());
            let page = args["page"].as_str().map(str::to_owned);
            blocking(move || pc::control(&what, level, page.as_deref()).map(|o| o.message)).await
        }
        WINDOWS => {
            let name = args["name"].as_str().unwrap_or_default().to_owned();
            match args["action"].as_str().unwrap_or("list") {
                "focus" => blocking(move || pc::focus_window(&name).map(|o| o.message)).await,
                "launch" => blocking(move || pc::launch_app(&name).map(|o| o.message)).await,
                _ => {
                    blocking(|| {
                        pc::windows().map(|w| {
                            if w.is_empty() {
                                "No open windows.".to_owned()
                            } else {
                                w.iter()
                                    .map(|w| format!("- {}: {}", w.app, w.title))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            }
                        })
                    })
                    .await
                }
            }
        }
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

/// Runs PC work off the async runtime; errors read as text for the model.
async fn blocking(
    f: impl FnOnce() -> Result<String, sidekick_actions::ActionError> + Send + 'static,
) -> String {
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(text)) => text,
        Ok(Err(e)) => format!("Error: {e}"),
        Err(e) => format!("Error: {e}"),
    }
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
    offer(app, chat_id, action, action_args, &label);
    format!("Shown to the user as a button \"{label}\". Say in one short sentence what it will do.")
}

/// A button under the answer that runs `action` when tapped. Sidekick's own
/// code uses this for steps that need the user's yes (send, post, pay).
pub fn offer(app: &AppHandle, chat_id: &str, action: &str, action_args: Value, label: &str) {
    let label: String = label.chars().take(60).collect();
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
}

/// Runs a proposal the user tapped, logs it, and keeps Undo for files it
/// made or moved.
pub async fn run_proposal(app: &AppHandle, id: &str) -> Result<Ran, String> {
    let proposed = lock(&app.state::<AppState>().ask_proposals)
        .remove(id)
        .ok_or("That button has expired; ask again")?;
    let result = match crate::act::run(app, &proposed.action, &proposed.args).await {
        Some(r) => r,
        None => {
            let exec = executor(&app.state::<AppState>());
            exec.run(&proposed.action, &proposed.args)
                .await
                .map_err(|e| e.to_string())
        }
    };
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

async fn find(app: &AppHandle, args: &Value) -> String {
    let name = args["name"]
        .as_str()
        .or(args["query"].as_str())
        .unwrap_or_default()
        .to_owned();
    let folders = match args["kind"].as_str() {
        Some("file") => Some(false),
        Some("folder") => Some(true),
        _ => None,
    };
    let configured: Vec<std::path::PathBuf> = lock(&app.state::<AppState>().settings)
        .code_folders
        .iter()
        .map(std::path::PathBuf::from)
        .collect();
    let code = if configured.is_empty() {
        sidekick_sensors::repos::ReposSensor::default_roots()
    } else {
        configured
    };
    let Some(home) = dirs::home_dir() else {
        return "Error: no home folder.".into();
    };
    let roots = crate::find::roots(&home, &code);
    let query = name.clone();
    let found = tokio::task::spawn_blocking(move || {
        crate::find::find(&roots, &query, folders, crate::find::BUDGET)
    })
    .await
    .unwrap_or_default();
    crate::find::describe(&found, &name)
}

async fn reveal(app: &AppHandle, path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return "Error: no path.".into();
    }
    let exec = executor(&app.state::<AppState>());
    match exec.run("reveal_path", &json!({ "path": path })).await {
        Ok(o) => o.message,
        Err(e) => format!("Error: {e}"),
    }
}

/// Opens a file, folder or link the user clicked in an answer.
pub async fn open_target(app: &AppHandle, target: &str) -> Result<String, String> {
    let out = open(app, target).await;
    if out.starts_with("Error") || out.starts_with("Refused") {
        Err(out)
    } else {
        Ok(out)
    }
}

/// A path as models write it: "file:///C:/x", "/C:/x" or "C:\\x" all mean
/// C:\x.
pub fn normalize_target(target: &str) -> String {
    let t = target.trim().trim_matches(['<', '>', '"', '\'']);
    if t.starts_with("http://") || t.starts_with("https://") {
        return t.to_owned();
    }
    let t = t
        .strip_prefix("file:///")
        .or_else(|| t.strip_prefix("file://"))
        .unwrap_or(t);
    let b = t.as_bytes();
    let t = if b.len() > 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        &t[1..]
    } else {
        t
    };
    t.replace("%20", " ")
}

async fn open(app: &AppHandle, target: &str) -> String {
    let target = normalize_target(target);
    let target = target.as_str();
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
    fn reads_paths_the_way_models_write_them() {
        assert_eq!(
            normalize_target("/C:/Users/talha/AppData"),
            "C:/Users/talha/AppData"
        );
        assert_eq!(normalize_target("file:///C:/a%20b/x.pdf"), "C:/a b/x.pdf");
        assert_eq!(normalize_target("<https://x.dev>"), "https://x.dev");
        assert_eq!(normalize_target("C:\\x"), "C:\\x");
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
        assert_eq!(
            names,
            [
                SEARCH, FIND, REVEAL, TODAY, RECENT, SCREEN, PROPOSE, WEB_SEARCH, READ_PAGE,
                BROWSER, APP_ACTION, DESKTOP, APPS, OFFICE, RECIPES, REMEMBER, NOTIFS, PC_STATUS,
                PC, WINDOWS, OPEN
            ]
        );
    }
}
