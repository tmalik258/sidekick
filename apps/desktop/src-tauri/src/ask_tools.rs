//! Tools the local model can use in Ask mode, all on this PC: search your
//! files and history, read your day, see what just happened, and open
//! things. They answer in short plain text a small model can use.

use serde::Serialize;
use std::path::Path;

use serde_json::{Value, json};
use sidekick_ai::ToolDef;
use sidekick_core::ActionRecord;
use std::collections::HashMap;
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
const STORAGE: &str = "storage";
const DOCTOR: &str = "app_doctor";
const GIT: &str = "git";
const GITHUB: &str = "github";
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
    "trash_download",
    "install_app",
    "set_compat",
    "clear_compat",
    "git_commit",
    "git_delete_branches",
];

/// An action waiting for a tap, from one chat.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Proposed {
    pub action: String,
    pub args: Value,
    pub label: String,
    /// The chat it was offered in, so the same button is not offered twice.
    #[serde(default)]
    pub chat_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProposalNote<'a> {
    chat_id: &'a str,
    id: &'a str,
    label: &'a str,
    /// One step of a task, shown with the others as a plan to run in order.
    step: bool,
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
                close_app {name}, sleep_pc {}, empty_recycle_bin {}, \
                trash_download {path} (a file in Downloads, to the Recycle Bin), \
                open_system_page {page}. \
                Use full paths from search. Nothing happens until they tap it. For a task \
                that takes several actions in order (move files, then zip them), offer each \
                one with step: true, in order; they show as a plan with one Run."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ASK_ACTIONS },
                    "args": { "type": "object" },
                    "label": { "type": "string", "description": "Button text, e.g. Move invoice.pdf to Invoices" },
                    "step": { "type": "boolean", "description": "One step of a task, run in order with the other steps" }
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
                Read before acting and after. For \"how do I use this\" or \"where is\", read, \
                answer in at most three short steps, then act do:point on the first step's \
                control to outline it on screen without pressing it."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["read", "act", "keys", "selection", "type_here", "click_text"] },
                    "app": { "type": "string", "description": "App or window name; the app the user was in when left out" },
                    "ref": { "type": "string" },
                    "do": { "type": "string", "enum": ["click", "type", "select", "focus", "point"] },
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
            name: STORAGE.into(),
            description: "What is taking space on this PC and what can be cleared: free space \
                on each drive, the biggest folders and files, old files and installers in \
                Downloads. Use for low disk space, a full drive, or what to delete."
                .into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: DOCTOR.into(),
            description: "Why an app crashes, freezes or will not start: reads the last 14 days \
                of crash records, names the likely cause and the fix Sidekick can run. Offer \
                fixes with propose, never run them yourself. When no cause is clear, web_search \
                the exact error code and module with Reddit, PCGamingWiki, Steam forums and the \
                app's own forums, and say what people found worked. Never suggest crack, repack \
                or pirated files. A file the user downloads themselves needs their go-ahead and \
                a Defender scan first."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": { "app": { "type": "string", "description": "App or game name; empty for all recent crashes" } }
            }),
        },
        ToolDef {
            name: GIT.into(),
            description: "A git project on this PC (path; the most recent project when left \
                out): changes shows what a commit would hold, so write a short commit message \
                (imperative subject, why in the body) and offer it with propose git_commit \
                {path, message}; branch shows this branch against main, for a PR description; \
                clean lists merged branches, offered with propose git_delete_branches {path, \
                branches}; conflicts lists files with merge conflicts, then offer to hand them \
                to a coding agent."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["changes", "branch", "clean", "conflicts"] },
                    "path": { "type": "string", "description": "Project folder" }
                },
                "required": ["action"],
            }),
        },
        ToolDef {
            name: GITHUB.into(),
            description: "GitHub through the user's gh CLI: waiting lists reviews asked of \
                them, their open PRs and assigned issues; ci shows the latest runs of a project \
                and the failing log, so explain the cause and the fix; review {number} reads a \
                pull request to review it (bugs first, then risks, then nits)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["waiting", "ci", "review"] },
                    "path": { "type": "string", "description": "Project folder; the most recent project when left out" },
                    "number": { "type": "integer", "description": "Pull request number" }
                },
                "required": ["action"],
            }),
        },
        ToolDef {
            name: PC.into(),
            description: "Change an everyday Windows setting right away: volume_up, volume_down, \
                mute, set_volume {level}, brightness {level}, dark_mode_on, dark_mode_off, \
                bluetooth_on, bluetooth_off, wifi_on, wifi_off, dnd_on, dnd_off (Do Not Disturb), \
                hotspot_on, hotspot_off (this PC's own Mobile hotspot, sharing its internet; \
                \"hotspot\" always means this PC's, never a phone's), airplane_on, airplane_off, \
                night_light_on, night_light_off, lock, \
                audio_outputs (lists speakers and headphones), audio_output {page: device name} \
                plays sound there, display {page: internal|clone|extend|external} sets screens, \
                open_settings {page} (no page opens Windows Settings itself). Pages: home, display, nightlight, sound, notifications, focus \
                (Do Not Disturb), bluetooth, wifi, network, battery, power, storage, apps, \
                default_apps, startup_apps, colors, background, mouse, keyboard, printers, updates, \
                privacy, accounts, time, language, about, hotspot, airplane. Switch a setting \
                instead of opening its page when the user asks to turn it on or off."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "what": { "type": "string", "enum": [
                        "volume_up", "volume_down", "mute", "set_volume", "brightness",
                        "dark_mode_on", "dark_mode_off", "bluetooth_on", "bluetooth_off",
                        "wifi_on", "wifi_off", "dnd_on", "dnd_off", "hotspot_on", "hotspot_off",
                        "airplane_on", "airplane_off", "night_light_on", "night_light_off",
                        "lock", "open_settings",
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
            description: "Apps and windows on this PC: find {name} (is an app installed? \
                spelling is forgiven), list (the open windows), focus {name} (bring one to the \
                front), launch {name} (start an installed app, e.g. Spotify), close {name} (like \
                pressing its X; the app still asks to save unsaved work, so just do it)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["find", "list", "focus", "launch", "close"] },
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

/// What a tool call is doing, in a few words for the steps list: "Searching
/// the web for flight prices", "Reading WhatsApp", "Clicking Send".
pub fn step_label(name: &str, args: &Value) -> String {
    let arg = |k: &str| {
        args[k]
            .as_str()
            .map(|s| s.trim().chars().take(48).collect::<String>())
            .filter(|s| !s.is_empty())
    };
    let host = |url: String| {
        url.split("://")
            .nth(1)
            .unwrap_or(&url)
            .split('/')
            .next()
            .unwrap_or_default()
            .trim_start_matches("www.")
            .to_owned()
    };
    let quoted = |verb: &str, what: Option<String>| match what {
        Some(w) => format!("{verb} \u{201c}{w}\u{201d}"),
        None => verb.to_owned(),
    };
    match name {
        WEB_SEARCH => quoted("Searching the web for", arg("query")),
        READ_PAGE => match arg("url") {
            Some(u) => format!("Reading {}", host(u)),
            None => "Reading the page".into(),
        },
        SEARCH => quoted("Searching your PC for", arg("query")),
        FIND => quoted("Looking for", arg("query").or_else(|| arg("name"))),
        SCREEN => "Reading your screen".into(),
        NOTIFS => "Checking your notifications".into(),
        PC_STATUS => "Checking your PC".into(),
        STORAGE => "Measuring what takes space".into(),
        DOCTOR => "Reading crash records".into(),
        GIT => "Reading the project's git".into(),
        GITHUB => "Checking GitHub".into(),
        PC => {
            let what = arg("what").unwrap_or_default();
            let thing = |k: &str| match k {
                "dnd" => "Do Not Disturb".to_owned(),
                "wifi" => "Wi-Fi".to_owned(),
                "bluetooth" => "Bluetooth".to_owned(),
                "hotspot" => "Mobile hotspot".to_owned(),
                "airplane" => "airplane mode".to_owned(),
                other => other.replace('_', " "),
            };
            if let Some(k) = what.strip_suffix("_on") {
                format!("Turning on {}", thing(k))
            } else if let Some(k) = what.strip_suffix("_off") {
                format!("Turning off {}", thing(k))
            } else if what == "open_settings" {
                "Opening Settings".into()
            } else if what.is_empty() {
                "Changing a setting".into()
            } else {
                format!("Changing {}", thing(&what))
            }
        }
        WINDOWS => match args["action"].as_str().unwrap_or("list") {
            "find" => quoted("Looking for", arg("name")),
            "launch" => quoted("Opening", arg("name")),
            "focus" => quoted("Switching to", arg("name")),
            "close" => quoted("Closing", arg("name")),
            _ => "Looking at your windows".into(),
        },
        OPEN => quoted("Opening", arg("target")),
        REVEAL => "Showing it in its folder".into(),
        DESKTOP => {
            let app = arg("app").unwrap_or_else(|| "the app".into());
            match args["action"].as_str().unwrap_or("read") {
                "read" => format!("Reading {app}"),
                "keys" => format!("Pressing keys in {app}"),
                "type_here" => "Typing".into(),
                "click_text" => quoted("Clicking", arg("text")),
                _ => match args["do"].as_str() {
                    Some("type") => format!("Typing in {app}"),
                    _ => format!("Working in {app}"),
                },
            }
        }
        BROWSER => match args["action"].as_str().unwrap_or("read") {
            "open" => match arg("url") {
                Some(u) => format!("Opening {}", host(u)),
                None => "Opening a page".into(),
            },
            "read" | "tabs" => "Reading the page".into(),
            _ => "Working in your browser".into(),
        },
        APP_ACTION => "Preparing the change".into(),
        APPS => quoted("Looking up", arg("name")),
        OFFICE => match args["action"].as_str().unwrap_or_default() {
            "email_draft" => "Drafting the email".into(),
            "excel_read" => "Reading the spreadsheet".into(),
            "excel_write" => "Filling the spreadsheet".into(),
            "word_create" | "to_pdf" => "Making the document".into(),
            _ => "Working in Office".into(),
        },
        RECIPES => "Updating recipes".into(),
        REMEMBER => "Remembering that".into(),
        TODAY => "Checking your day".into(),
        RECENT => "Looking at what just happened".into(),
        PROPOSE => "Preparing an action".into(),
        _ => String::new(),
    }
}

/// Tools that only read; the model may run several of them at once.
pub fn reads_only(name: &str) -> bool {
    matches!(
        name,
        SEARCH
            | FIND
            | TODAY
            | RECENT
            | SCREEN
            | PC_STATUS
            | STORAGE
            | DOCTOR
            | GIT
            | GITHUB
            | WEB_SEARCH
            | READ_PAGE
            | NOTIFS
    )
}

/// One of Sidekick's own tools (not Composio's).
pub fn is_own(name: &str) -> bool {
    matches!(
        name,
        SEARCH
            | TODAY
            | RECENT
            | OPEN
            | SCREEN
            | PROPOSE
            | FIND
            | REVEAL
            | PC_STATUS
            | STORAGE
            | DOCTOR
            | GIT
            | GITHUB
            | PC
            | WINDOWS
            | WEB_SEARCH
            | READ_PAGE
            | NOTIFS
            | BROWSER
            | APP_ACTION
            | DESKTOP
            | APPS
            | OFFICE
            | RECIPES
            | REMEMBER
    )
}

/// Always offered: what nearly every question needs.
/// Greetings and writing help: answered from the words alone, so no tool
/// list for a small model to read first.
pub fn needs_no_tools(question: &str) -> bool {
    let q = question.trim().to_lowercase();
    let first = q.split_whitespace().next().unwrap_or_default();
    let small_talk = q.split_whitespace().count() <= 4
        && [
            "hi", "hello", "hey", "thanks", "thank", "ok", "okay", "yo", "can", "good", "bye",
        ]
        .contains(&first.trim_matches(|c: char| !c.is_alphanumeric()))
        && !q.contains("file")
        && !q.contains("open");
    let writing = [
        "rewrite",
        "rephrase",
        "reword",
        "translate",
        "summarize",
        "proofread",
    ]
    .contains(&first)
        || (first == "write" && !q.contains("file"));
    small_talk || writing
}

const CORE: &[&str] = &[SEARCH, FIND, OPEN, PROPOSE];
/// Tools offered beyond the core, picked by meaning.
const PICKED: usize = 4;
/// Each tool's name and the embedding of its description.
type ToolVectors = Vec<(String, Vec<f32>)>;
/// Tool descriptions embedded once per embedding model.
static TOOL_VECTORS: tokio::sync::Mutex<Option<(String, ToolVectors)>> =
    tokio::sync::Mutex::const_new(None);

/// About eight tools instead of twenty: the core ones plus the closest in
/// meaning to the question. Small models pick better from fewer. Without an
/// embedding model every tool is offered.
pub async fn pick(app: &AppHandle, question: &str, defs: Vec<ToolDef>) -> Vec<ToolDef> {
    if needs_no_tools(question) {
        return Vec::new();
    }
    let Some((client, model)) = crate::search::embedder(app) else {
        return by_words(question, defs);
    };
    let embed = async {
        let mut cached = TOOL_VECTORS.lock().await;
        if cached.as_ref().is_none_or(|(m, _)| *m != model) {
            let inputs: Vec<String> = defs
                .iter()
                .map(|d| {
                    crate::search::prefixed(
                        &model,
                        "search_document",
                        &format!("{}: {}", d.name, d.description),
                    )
                })
                .collect();
            let vectors = client.embed(&model, &inputs).await.ok()?;
            *cached = Some((
                model.clone(),
                defs.iter().map(|d| d.name.clone()).zip(vectors).collect(),
            ));
        }
        let q = client
            .embed(
                &model,
                &[crate::search::prefixed(&model, "search_query", question)],
            )
            .await
            .ok()?
            .pop()?;
        Some((q, cached.as_ref()?.1.clone()))
    };
    let Ok(Some((q, tools))) = tokio::time::timeout(PICK_TIMEOUT, embed).await else {
        return by_words(question, defs);
    };
    let names = closest(&q, &tools, PICKED);
    defs.into_iter()
        .filter(|d| CORE.contains(&d.name.as_str()) || names.contains(&d.name))
        .collect()
}

/// Without an embedding model: the tools whose name or description share
/// the most words with the question, plus the core ones. Every tool when
/// nothing matches, so a question is never left without the right one.
pub fn by_words(question: &str, defs: Vec<ToolDef>) -> Vec<ToolDef> {
    let words: Vec<String> = question
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 4)
        .map(str::to_lowercase)
        .collect();
    let mut scored: Vec<(usize, String)> = defs
        .iter()
        .filter(|d| !CORE.contains(&d.name.as_str()))
        .map(|d| {
            let text = format!("{} {}", d.name.replace('_', " "), d.description).to_lowercase();
            (
                words.iter().filter(|w| text.contains(w.as_str())).count(),
                d.name.clone(),
            )
        })
        .filter(|(n, _)| *n > 0)
        .collect();
    // Nothing matches ("hi", "rewrite this"): the core tools only, not all
    // twenty. Every tool is prompt the model must read before its first word.
    if scored.is_empty() {
        return defs
            .into_iter()
            .filter(|d| CORE.contains(&d.name.as_str()))
            .collect();
    }
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    let names: Vec<String> = scored.into_iter().take(PICKED).map(|(_, n)| n).collect();
    defs.into_iter()
        .filter(|d| CORE.contains(&d.name.as_str()) || names.contains(&d.name))
        .collect()
}

/// Embedding the question may take this long before every tool is offered.
const PICK_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(600);

/// The `n` tool names closest to `q`, core tools left out.
fn closest(q: &[f32], tools: &[(String, Vec<f32>)], n: usize) -> Vec<String> {
    let mut scored: Vec<(f32, &String)> = tools
        .iter()
        .filter(|(name, _)| !CORE.contains(&name.as_str()))
        .map(|(name, v)| (sidekick_core::storage::cosine(q, v), name))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().take(n).map(|(_, n)| n.clone()).collect()
}

/// Tools that reach the internet or other apps, left out for "This PC only".
pub fn is_web(name: &str) -> bool {
    matches!(
        name,
        WEB_SEARCH | READ_PAGE | BROWSER | APP_ACTION | APPS | GITHUB
    )
}

/// Runs a local tool, or `None` when `name` is not one of them.
/// How long a look-up tool may take before the model hears it timed out.
const TOOL_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn run(app: &AppHandle, chat_id: &str, name: &str, args: &Value) -> Option<String> {
    // A stuck look-up (OCR, a slow page) must not hold the answer forever.
    if reads_only(name) {
        return match tokio::time::timeout(TOOL_LIMIT, run_inner(app, chat_id, name, args)).await {
            Ok(out) => out,
            Err(_) => Some(format!(
                "Error: {name} took over {} s and was stopped. Answer with what you have.",
                TOOL_LIMIT.as_secs()
            )),
        };
    }
    run_inner(app, chat_id, name, args).await
}

async fn run_inner(app: &AppHandle, chat_id: &str, name: &str, args: &Value) -> Option<String> {
    // An answer started at a pause in speech may look things up, but acts
    // only once the question is final.
    if is_own(name) && !reads_only(name) && !crate::ai::wait_released(chat_id).await {
        return Some("Error: the question changed; stop.".into());
    }
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
        STORAGE => tokio::task::spawn_blocking(crate::disk::report)
            .await
            .unwrap_or_else(|e| format!("Error: {e}")),
        GIT | GITHUB => {
            let path = args["path"]
                .as_str()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(std::path::PathBuf::from)
                .or_else(|| crate::projects::list(app).into_iter().next());
            let action = args["action"].as_str().unwrap_or_default().to_owned();
            let number = args["number"].as_u64();
            blocking(move || git_tool(&action, path.as_deref(), number)).await
        }
        DOCTOR => {
            let app = args["app"].as_str().unwrap_or_default().trim().to_owned();
            blocking(move || doctor_report((!app.is_empty()).then_some(app.as_str()))).await
        }
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
                "close" => blocking(move || pc::close_window(&name).map(|o| o.message)).await,
                "launch" => blocking(move || pc::launch_app(&name).map(|o| o.message)).await,
                "find" => {
                    blocking(move || {
                        pc::find_apps(&name).map(|found| match found {
                            pc::Found::Matches(names) => format!("Installed: {}", names.join(", ")),
                            pc::Found::Closest(names) if !names.is_empty() => format!(
                                "No app named {name}. Closest installed: {}. Ask the user which \
                                 one they mean before opening it.",
                                names.join(", ")
                            ),
                            pc::Found::Closest(_) => format!("No installed app like {name}."),
                        })
                    })
                    .await
                }
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
        SCREEN => match screen_now(app).await {
            Ok(t) if t.is_empty() => "No readable text on screen.".into(),
            Ok(t) => t,
            Err(e) => format!("Error: {e}"),
        },
        _ => return None,
    })
}

/// Runs PC work off the async runtime; errors read as text for the model.
/// The git and github tools' reads, as text for the model.
fn git_tool(
    action: &str,
    path: Option<&std::path::Path>,
    number: Option<u64>,
) -> Result<String, sidekick_actions::ActionError> {
    use sidekick_actions::{ActionError, gitflow};
    let need =
        || path.ok_or_else(|| ActionError::Invalid("no project found; ask which folder".into()));
    match action {
        "changes" => gitflow::changes(need()?),
        "branch" => gitflow::branch(need()?),
        "clean" => {
            let p = need()?;
            let b = gitflow::merged(p)?;
            Ok(if b.is_empty() {
                "No merged branches to clean.".into()
            } else {
                format!(
                    "Merged into main, safe to delete in {}:\n{}",
                    p.display(),
                    b.join("\n")
                )
            })
        }
        "conflicts" => {
            let c = gitflow::conflicts(need()?)?;
            Ok(if c.is_empty() {
                "No merge conflicts.".into()
            } else {
                format!("Files with conflicts:\n{}", c.join("\n"))
            })
        }
        "waiting" => gitflow::waiting(),
        "ci" => gitflow::ci(need()?),
        "review" => match number {
            Some(n) => gitflow::pr(need()?, n),
            None => Err(ActionError::Invalid(
                "say which pull request (number)".into(),
            )),
        },
        other => Err(ActionError::Invalid(format!("unknown git action {other}"))),
    }
}

/// Crash records with their likely causes and fixes, as text for the model.
fn doctor_report(app: Option<&str>) -> Result<String, sidekick_actions::ActionError> {
    use sidekick_actions::doctor;
    let crashes = doctor::crashes(app)?;
    if crashes.is_empty() {
        return Ok(match app {
            Some(a) => format!(
                "No crash records for {a} in 14 days. Ask what happens when it fails, then web_search that."
            ),
            None => "No crash records in 14 days.".into(),
        });
    }
    let mut out = String::new();
    for c in crashes.iter().take(8) {
        out.push_str(&format!(
            "{} {} {} (module {}, code {}) {}\n",
            c.when, c.kind, c.app, c.module, c.code, c.detail
        ));
        for cause in doctor::causes(c) {
            out.push_str(&format!("  cause: {}\n", cause.what));
            if let Some((action, args, label)) = cause.fix {
                out.push_str(&format!("  fix: {label} (action {action}, args {args})\n"));
            }
        }
    }
    Ok(out)
}

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
    let mut action_args = args["args"].clone();
    for key in ["path", "to"] {
        if let Some(p) = action_args[key].as_str() {
            action_args[key] = json!(normalize_target(p));
        }
    }
    if let Some(paths) = action_args["paths"].as_array_mut() {
        for p in paths.iter_mut() {
            if let Some(s) = p.as_str() {
                *p = json!(normalize_target(s));
            }
        }
    }
    // Models sometimes pass the action's own name ("close_app") as the text.
    let label: String = args["label"]
        .as_str()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.contains('_'))
        .map_or_else(|| button_text(action, &action_args), str::to_owned)
        .chars()
        .take(60)
        .collect();
    if action == "open_path" && action_args["path"].as_str().is_some_and(runs_code) {
        return "Refused: that file runs a program. Tell the user to open it themselves.".into();
    }
    let step = args["step"].as_bool().unwrap_or(false);
    offer_as(app, chat_id, action, action_args, &label, step);
    if step {
        format!(
            "Added to the plan as \"{label}\". Offer the other steps, then say in one sentence what the plan does."
        )
    } else {
        format!(
            "Shown to the user as a button \"{label}\". Say in one short sentence what it will do."
        )
    }
}

/// Readable button text for an action offered without one.
fn button_text(action: &str, args: &Value) -> String {
    let what = ["name", "path", "url"]
        .iter()
        .find_map(|k| args[k].as_str())
        .map(|v| {
            v.trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(v)
                .to_owned()
        })
        .unwrap_or_default();
    let verb = match action {
        "close_app" => "Close",
        "open_path" | "open_url" => "Open",
        "reveal_path" => "Show in folder",
        "move_file" => "Move",
        "zip" => "Zip",
        "convert" => "Convert",
        "extract_archive" | "extract_text" => "Extract",
        "launch_project" => "Open project",
        "git_pull" => "Pull",
        "install_deps" => "Install packages",
        "sleep_pc" => "Put the PC to sleep",
        "empty_recycle_bin" => "Empty the Recycle Bin",
        "trash_download" => "Remove",
        other => return other.replace('_', " "),
    };
    if what.is_empty() {
        verb.to_owned()
    } else {
        format!("{verb} {what}")
    }
}

/// A button under the answer that runs `action` when tapped. Sidekick's own
/// code uses this for steps that need the user's yes (send, post, pay).
const PROPOSALS_FILE: &str = "ask-buttons.json";
/// Buttons kept for a tap; older ones are let go.
const KEEP_PROPOSALS: usize = 60;

/// Buttons still waiting from before a restart, so they keep working.
pub fn load_proposals(dir: &std::path::Path) -> HashMap<String, Proposed> {
    std::fs::read(dir.join(PROPOSALS_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_proposals(app: &AppHandle, all: &mut HashMap<String, Proposed>) {
    // Ids are ULIDs, so the smallest are the oldest.
    while all.len() > KEEP_PROPOSALS {
        let Some(oldest) = all.keys().min().cloned() else {
            break;
        };
        all.remove(&oldest);
    }
    if let Ok(dir) = app.path().app_data_dir()
        && let Ok(bytes) = serde_json::to_vec(&*all)
    {
        let _ = std::fs::write(dir.join(PROPOSALS_FILE), bytes);
    }
}

pub fn offer(app: &AppHandle, chat_id: &str, action: &str, action_args: Value, label: &str) {
    offer_as(app, chat_id, action, action_args, label, false);
}

/// Like `offer`; a step shows with the other steps as a plan, run in order.
fn offer_as(
    app: &AppHandle,
    chat_id: &str,
    action: &str,
    action_args: Value,
    label: &str,
    step: bool,
) {
    let label: String = label.chars().take(60).collect();
    let id = ulid::Ulid::new().to_string();
    {
        let state = app.state::<AppState>();
        let mut all = lock(&state.ask_proposals);
        // Small models sometimes call the same thing twice in one answer.
        let repeat = all
            .values()
            .any(|p| p.chat_id == chat_id && p.action == action && p.args == action_args);
        if repeat {
            return;
        }
        all.insert(
            id.clone(),
            Proposed {
                action: action.into(),
                args: action_args,
                label: label.clone(),
                chat_id: chat_id.to_owned(),
            },
        );
        save_proposals(app, &mut all);
    }
    let _ = app.emit(
        PROPOSAL_EVENT,
        ProposalNote {
            chat_id,
            id: &id,
            label: &label,
            step,
        },
    );
}

/// Runs a proposal the user tapped, logs it, and keeps Undo for files it
/// made or moved.
pub async fn run_proposal(app: &AppHandle, id: &str) -> Result<Ran, String> {
    let proposed = {
        let state = app.state::<AppState>();
        let mut all = lock(&state.ask_proposals);
        let found = all.remove(id);
        save_proposals(app, &mut all);
        found
    }
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
/// Screen text read in the background when Ask opened: (window pid, when,
/// text). Only kept in memory, and only briefly.
static SCREEN_CACHE: std::sync::Mutex<Option<(Option<u32>, std::time::Instant, String)>> =
    std::sync::Mutex::new(None);
/// Background screen text older than this is read again.
const SCREEN_FRESH: std::time::Duration = std::time::Duration::from_secs(90);

fn last_pid(app: &AppHandle) -> Option<u32> {
    lock(&app.state::<AppState>().last_window)
        .as_ref()
        .and_then(|w| w["pid"].as_u64())
        .and_then(|p| u32::try_from(p).ok())
}

/// The screen text read when Ask opened, while it is still about the same
/// window.
pub fn cached_screen(app: &AppHandle) -> Option<String> {
    let pid = last_pid(app);
    let cache = SCREEN_CACHE.lock().ok()?;
    let (p, at, text) = cache.as_ref()?;
    (*p == pid && at.elapsed() < SCREEN_FRESH).then(|| text.clone())
}

/// Reads the screen text in the background, so a question about the
/// screen does not wait for the capture and OCR.
pub async fn prefetch_screen(app: &AppHandle) {
    if cached_screen(app).is_some() {
        return;
    }
    let pid = last_pid(app);
    let Ok(png) = crate::ai::screenshot(app).await else {
        return;
    };
    if let Ok(text) = screen_text(app, &png).await
        && let Ok(mut cache) = SCREEN_CACHE.lock()
    {
        *cache = Some((pid, std::time::Instant::now(), text));
    }
}

/// The text of the window the user was in: the background read when it is
/// fresh, else a new capture.
pub async fn screen_now(app: &AppHandle) -> Result<String, String> {
    if let Some(text) = cached_screen(app) {
        return Ok(text);
    }
    let png = crate::ai::screenshot(app)
        .await
        .map_err(|e| format!("could not capture the screen: {e}"))?;
    screen_text(app, &png).await
}

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
        // Every drive, from the name index; the live walk until it exists
        // or when it has nothing (a file made since the last build).
        let indexed: Vec<crate::find::Found> = crate::names::search(&query, 40)
            .unwrap_or_default()
            .into_iter()
            .filter(|h| folders.is_none_or(|f| f == h.folder))
            .take(crate::find::MAX_RESULTS)
            .map(|h| crate::find::Found {
                modified: h.path.metadata().and_then(|m| m.modified()).ok(),
                path: h.path,
                folder: h.folder,
            })
            .collect();
        if indexed.is_empty() {
            crate::find::find(&roots, &query, folders, crate::find::BUDGET)
        } else {
            indexed
        }
    })
    .await
    .unwrap_or_default();
    crate::find::describe(&found, &name)
}

async fn reveal(app: &AppHandle, path: &str) -> String {
    let path = normalize_target(path);
    let path = path.as_str();
    if path.is_empty() {
        return "Error: no path.".into();
    }
    if !Path::new(path).exists() {
        return missing(path);
    }
    let exec = executor(&app.state::<AppState>());
    match exec.run("reveal_path", &json!({ "path": path })).await {
        Ok(o) => o.message,
        Err(e) => format!("Error: {e}"),
    }
}

/// A path the model made up or guessed: tell it to look, not the user.
fn missing(path: &str) -> String {
    format!(
        "Error: {path} does not exist. Get real paths from find_files (or storage) and try \
         again; do not mention this error to the user."
    )
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
    expand_home(&expand_vars(&t.replace("%20", " ")))
}

/// What models write for the user's folders, made real: ~, %VAR%, $VAR,
/// $(VAR) and ${VAR} (USERPROFILE, HOME, USERNAME, APPDATA...).
pub fn expand_vars(t: &str) -> String {
    let var = |name: &str| -> Option<String> {
        let n = name.trim();
        std::env::var(n).ok().or_else(|| {
            (n.eq_ignore_ascii_case("home") || n.eq_ignore_ascii_case("userprofile"))
                .then(|| dirs::home_dir().map(|h| h.display().to_string()))
                .flatten()
        })
    };
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    if let Some(r) = rest.strip_prefix('~')
        && (r.is_empty() || r.starts_with(['/', '\\']))
        && let Some(home) = dirs::home_dir()
    {
        out.push_str(&home.display().to_string());
        rest = r;
    }
    while let Some(i) = rest.find(['%', '$']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let (name, used) = if let Some(t) = tail.strip_prefix('%') {
            t.find('%').map_or((None, 1), |j| (Some(&t[..j]), j + 2))
        } else if let Some(t) = tail.strip_prefix("$(").or_else(|| tail.strip_prefix("${")) {
            t.find([')', '}'])
                .map_or((None, 2), |j| (Some(&t[..j]), j + 3))
        } else {
            let t = &tail[1..];
            let j = t
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(t.len());
            ((j > 0).then(|| &t[..j]), j + 1)
        };
        match name.and_then(var) {
            Some(v) => out.push_str(&v),
            None => out.push_str(&tail[..used.min(tail.len())]),
        }
        rest = &tail[used.min(tail.len())..];
    }
    out.push_str(rest);
    out
}

/// A made-up user folder (C:\\Users\\John, C:\\Users\\<username>) becomes the
/// real one when the made-up one does not exist.
pub fn expand_home(t: &str) -> String {
    let b = t.as_bytes();
    if b.len() < 10 || !b[0].is_ascii_alphabetic() || b[1] != b':' {
        return t.to_owned();
    }
    let rest = &t[3..];
    let Some(after) = rest
        .strip_prefix("Users")
        .or_else(|| rest.strip_prefix("users"))
    else {
        return t.to_owned();
    };
    let after = after.trim_start_matches(['/', '\\']);
    let (user, tail) = after.split_once(['/', '\\']).unwrap_or((after, ""));
    let Some(home) = dirs::home_dir() else {
        return t.to_owned();
    };
    let shared = ["public", "default", "all users"].contains(&user.to_ascii_lowercase().as_str());
    let users = Path::new(&t[..3]).join("Users");
    if user.is_empty() || shared || !users.is_dir() || users.join(user).exists() {
        return t.to_owned();
    }
    if tail.is_empty() {
        home.display().to_string()
    } else {
        home.join(tail).display().to_string()
    }
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
    let web = target.starts_with("http://") || target.starts_with("https://");
    if !web && !Path::new(target).exists() {
        return missing(target);
    }
    let (action, args) = if web {
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
pub fn runs_code(target: &str) -> bool {
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

/// Whether Windows (or this OS) has a default app that can open `path`.
/// No extension, or no ProgId / open command → Ask should ask where to open it.
pub fn has_file_association(path: &str) -> bool {
    let p = std::path::Path::new(path);
    if p.is_dir() {
        return true;
    }
    let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    #[cfg(windows)]
    {
        association_prog_id(ext).is_some_and(|id| has_shell_open(&id))
    }
    #[cfg(not(windows))]
    {
        let _ = ext;
        true
    }
}

#[cfg(windows)]
fn association_prog_id(ext: &str) -> Option<String> {
    let dot = format!(".{}", ext.to_ascii_lowercase());
    reg_value(
        &format!(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{dot}\UserChoice"
        ),
        "ProgId",
    )
    .filter(|id| !id.is_empty())
    .or_else(|| reg_default(&format!(r"HKCR\{dot}")).filter(|id| !id.is_empty()))
}

#[cfg(windows)]
fn has_shell_open(prog_id: &str) -> bool {
    reg_default(&format!(r"HKCR\{prog_id}\shell\open\command")).is_some_and(|cmd| !cmd.is_empty())
}

#[cfg(windows)]
fn reg_value(key: &str, name: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("reg")
        .args(["query", key, "/v", name])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let needle = name.to_ascii_lowercase();
    text.lines()
        .find(|l| l.to_ascii_lowercase().contains(&needle))
        .and_then(|l| l.split_whitespace().last())
        .map(str::to_owned)
}

#[cfg(windows)]
fn reg_default(key: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("reg")
        .args(["query", key, "/ve"])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().find(|l| l.contains("REG_")).and_then(|l| {
        // "    (Default)    REG_SZ    value" — value may be missing.
        let mut parts = l.split_whitespace();
        let _ = parts.next()?; // (Default)
        let _ = parts.next()?; // REG_SZ
        let rest: Vec<_> = parts.collect();
        if rest.is_empty() {
            None
        } else {
            Some(rest.join(" "))
        }
    })
}

#[cfg(test)]
mod no_tools_tests {
    #[test]
    fn greetings_and_writing_skip_tools() {
        assert!(super::needs_no_tools("hi"));
        assert!(super::needs_no_tools("can you hear me"));
        assert!(super::needs_no_tools(
            "rewrite this to sound friendlier: send me the report"
        ));
        assert!(super::needs_no_tools(
            "write a short reply saying I'll be late"
        ));
        assert!(!super::needs_no_tools("what's my battery at"));
        assert!(!super::needs_no_tools("open the file report.docx"));
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn picks_tools_by_words_without_embeddings() {
        let all = defs();
        let picked = by_words("turn on the bluetooth hotspot", all.clone());
        assert!(picked.len() < all.len());
        assert!(picked.len() <= CORE.len() + PICKED);
        assert_eq!(
            by_words("hi", all.clone()).len(),
            CORE.len(),
            "nothing matches: the core tools only"
        );
    }

    use super::*;

    #[test]
    fn made_up_user_folders_become_the_real_one() {
        let home = dirs::home_dir().unwrap();
        let t = expand_home("C:/Users/NoSuchUserX9/Documents");
        if std::path::Path::new("C:/Users").is_dir() {
            assert_eq!(t, home.join("Documents").display().to_string());
        }
        assert_eq!(
            expand_home("C:/Users/Public/Documents"),
            "C:/Users/Public/Documents"
        );
        assert_eq!(expand_home("D:/work/x"), "D:/work/x");
    }

    /// This PC only keeps exactly these: each reads or acts on this PC. A new
    /// tool that goes online must be added to `is_web`, or this fails.
    #[test]
    fn this_pc_only_keeps_tools_that_stay_on_the_pc() {
        let offline: Vec<String> = defs()
            .into_iter()
            .map(|d| d.name)
            .filter(|n| !is_web(n))
            .collect();
        let mut expected = vec![
            SEARCH, TODAY, RECENT, OPEN, SCREEN, PROPOSE, FIND, REVEAL, PC_STATUS, STORAGE, DOCTOR,
            GIT, PC, WINDOWS, NOTIFS, DESKTOP, OFFICE, RECIPES, REMEMBER,
        ];
        let mut got: Vec<&str> = offline.iter().map(String::as_str).collect();
        expected.sort_unstable();
        got.sort_unstable();
        assert_eq!(got, expected);
        for web in [WEB_SEARCH, READ_PAGE, BROWSER, APP_ACTION, APPS, GITHUB] {
            assert!(is_web(web), "{web} goes online");
        }
    }

    #[test]
    fn every_tool_is_known_and_reads_are_marked() {
        for d in defs() {
            assert!(is_own(&d.name), "{} is missing from is_own", d.name);
        }
        assert!(reads_only(SEARCH) && reads_only(WEB_SEARCH));
        assert!(!reads_only(PROPOSE) && !reads_only(DESKTOP) && !reads_only(OPEN));
    }

    #[test]
    fn picks_the_closest_tools_besides_the_core() {
        let tools = vec![
            (SEARCH.to_owned(), vec![1.0, 0.0]),
            (WEB_SEARCH.to_owned(), vec![0.9, 0.1]),
            (OFFICE.to_owned(), vec![0.0, 1.0]),
            (TODAY.to_owned(), vec![0.5, 0.5]),
        ];
        assert_eq!(closest(&[1.0, 0.0], &tools, 2), vec![WEB_SEARCH, TODAY]);
    }

    #[test]
    fn buttons_never_show_action_names() {
        assert_eq!(
            button_text("close_app", &json!({ "name": "Notepad" })),
            "Close Notepad"
        );
        assert_eq!(
            button_text(
                "open_path",
                &json!({ "path": "C:\\Users\\me\\invoice.pdf" })
            ),
            "Open invoice.pdf"
        );
        assert_eq!(button_text("sleep_pc", &json!({})), "Put the PC to sleep");
    }

    #[test]
    fn steps_say_what_they_do() {
        assert_eq!(
            step_label(WEB_SEARCH, &json!({ "query": "flight prices to Dubai" })),
            "Searching the web for \u{201c}flight prices to Dubai\u{201d}"
        );
        assert_eq!(
            step_label(READ_PAGE, &json!({ "url": "https://www.bbc.com/news/x" })),
            "Reading bbc.com"
        );
        assert_eq!(
            step_label(DESKTOP, &json!({ "action": "read", "app": "WhatsApp" })),
            "Reading WhatsApp"
        );
        assert_eq!(
            step_label(PC, &json!({ "what": "dnd_on" })),
            "Turning on Do Not Disturb"
        );
        assert_eq!(
            step_label(PC, &json!({ "what": "dark_mode_off" })),
            "Turning off dark mode"
        );
        assert_eq!(step_label("SOME_COMPOSIO_TOOL", &json!({})), "");
    }

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
        assert_eq!(normalize_target("/C:/Projects/app"), "C:/Projects/app");
        assert_eq!(normalize_target("file:///C:/a%20b/x.pdf"), "C:/a b/x.pdf");
        assert_eq!(normalize_target("<https://x.dev>"), "https://x.dev");
        assert_eq!(normalize_target("C:\\x"), "C:\\x");
        // Folders the model writes as variables become the real ones.
        let home = dirs::home_dir().unwrap().display().to_string();
        assert_eq!(expand_vars("~/Videos"), format!("{home}/Videos"));
        assert_eq!(expand_vars("$(USERPROFILE)/Music"), format!("{home}/Music"));
        assert_eq!(expand_vars("${HOME}/a"), format!("{home}/a"));
        assert_eq!(expand_vars("100% done $5"), "100% done $5");
        assert_eq!(expand_vars("%NO_SUCH_VAR_X%/a"), "%NO_SUCH_VAR_X%/a");
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
    fn files_without_an_extension_need_a_choice() {
        assert!(!super::has_file_association("C:/Users/me/NOTES"));
    }

    #[test]
    fn tools_have_short_names() {
        let names: Vec<_> = defs().into_iter().map(|d| d.name).collect();
        assert_eq!(
            names,
            [
                SEARCH, FIND, REVEAL, TODAY, RECENT, SCREEN, PROPOSE, WEB_SEARCH, READ_PAGE,
                BROWSER, APP_ACTION, DESKTOP, APPS, OFFICE, RECIPES, REMEMBER, NOTIFS, PC_STATUS,
                STORAGE, DOCTOR, GIT, GITHUB, PC, WINDOWS, OPEN
            ]
        );
    }
}
