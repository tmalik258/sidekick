//! Composio in Ask mode: the apps connected there (Jira, Trello, Slack,
//! Gmail, Notion and more) become tools for the local model.
//!
//! The local model may only read. A tool that would change something
//! (send, create, move, delete, run code) is refused and the chat offers to
//! continue in Claude Code instead, where its own permission prompts and the
//! island's Allow or Deny cover writes. Every tool call is logged.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use sidekick_ai::{
    AiError, AiProvider, CancellationToken, ChatRequest, McpClient, McpTool, OpenAiCompat, Sink,
    ToolDef, ToolRunner,
};
use sidekick_core::{ActionRecord, ComposioSettings};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, lock};

pub const TOOL_EVENT: &str = "ai://tool";
/// The tool the local model calls when a request is beyond it.
pub const HANDOFF_TOOL: &str = "continue_in_claude_code";
/// Most Composio tools offered to the local model at once; small models do
/// worse with long tool lists.
const MAX_TOOLS: usize = 16;
/// Most characters of tool definitions sent to the local model. Ollama's
/// default context is a few thousand tokens; tools past this budget would
/// push the question out of it.
const TOOL_BUDGET: usize = 12_000;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const SKILL_ID: &str = "chat.composio";

/// Words in a tool's name that mean it changes something.
const WRITE_WORDS: &[&str] = &[
    "ADD",
    "APPROVE",
    "ARCHIVE",
    "ASSIGN",
    "BASH",
    "CANCEL",
    "CLOSE",
    "COMMENT",
    "COPY",
    "CREATE",
    "DELETE",
    "DISABLE",
    "EDIT",
    "ENABLE",
    "EXECUTE",
    "FORWARD",
    "INITIATE",
    "INSERT",
    "INVITE",
    "LABEL",
    "MANAGE",
    "MARK",
    "MERGE",
    "MODIFY",
    "MOVE",
    "PATCH",
    "POST",
    "PUBLISH",
    "PUT",
    "REJECT",
    "REMOVE",
    "RENAME",
    "REPLY",
    "RUN",
    "SCHEDULE",
    "SEND",
    "SET",
    "SHARE",
    "STAR",
    "START",
    "STOP",
    "SUBMIT",
    "TRANSITION",
    "TRASH",
    "TRIGGER",
    "UPDATE",
    "UPLOAD",
    "UPSERT",
    "WORKBENCH",
    "WRITE",
];

/// Keys where meta tools (like COMPOSIO_MULTI_EXECUTE_TOOL) name the tool
/// they will run.
const SLUG_KEYS: &[&str] = &[
    "tool_slug",
    "slug",
    "tool",
    "action",
    "tool_name",
    "toolSlug",
];

/// Signed in with the Connect button: the key is in Credential Manager.
pub fn signed_in() -> bool {
    crate::secrets::get(crate::composio_api::KEY_NAME).is_some()
}

/// Composio is on and either signed in or given an MCP link by hand.
pub fn is_set_up(c: &ComposioSettings) -> bool {
    c.enabled && (signed_in() || manual(c).is_some())
}

/// The MCP link and headers chat uses: a session for the signed-in
/// account, else a link pasted or copied from Claude Code.
pub async fn server(c: &ComposioSettings) -> Option<(String, Vec<(String, String)>)> {
    if !c.enabled {
        return None;
    }
    if let Some(key) = crate::secrets::get(crate::composio_api::KEY_NAME) {
        match crate::composio_api::session(&key, &user_id(c)).await {
            Ok(s) if !s.mcp_url.is_empty() => {
                return Some((s.mcp_url, vec![("x-api-key".into(), key)]));
            }
            Ok(_) => log::warn!("Composio session has no MCP link"),
            Err(err) => log::warn!("Composio session failed: {err}"),
        }
    }
    manual(c)
}

pub fn user_id(c: &ComposioSettings) -> String {
    Some(c.user_id.trim())
        .filter(|u| !u.is_empty())
        .unwrap_or("default")
        .to_owned()
}

/// Runs one Composio tool for Sidekick itself (calendar, brief, notes).
pub async fn run_tool(c: &ComposioSettings, tool: &str, args: Value) -> Result<Value, String> {
    if !c.enabled {
        return Err("Composio is off".into());
    }
    let key =
        crate::secrets::get(crate::composio_api::KEY_NAME).ok_or("Composio is not connected")?;
    let session = crate::composio_api::session(&key, &user_id(c)).await?;
    crate::composio_api::execute(&key, &session, tool, args).await
}

/// A link given by hand (or copied from Claude Code), when Composio is on.
pub fn manual(c: &ComposioSettings) -> Option<(String, Vec<(String, String)>)> {
    let url = c.url.trim();
    if !c.enabled || !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    let mut headers: Vec<(String, String)> = c
        .headers
        .iter()
        .filter(|(k, v)| !k.trim().is_empty() && !v.trim().is_empty())
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let has_key = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("x-api-key"));
    if !has_key
        && let Ok(key) = std::env::var("COMPOSIO_API_KEY")
        && !key.trim().is_empty()
    {
        headers.push(("x-api-key".into(), key.trim().to_owned()));
    }
    Some((url.to_owned(), headers))
}

fn words(name: &str) -> impl Iterator<Item = String> + '_ {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_uppercase)
}

/// The tool's name says it changes something.
pub fn is_write(name: &str) -> bool {
    words(name).any(|w| WRITE_WORDS.contains(&w.as_str()))
}

fn collect_slugs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if SLUG_KEYS.contains(&k.as_str())
                    && let Some(s) = val.as_str()
                {
                    out.push(s.to_owned());
                }
                collect_slugs(val, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| collect_slugs(i, out)),
        _ => {}
    }
}

/// Why the local model may not run this, or None when it only reads. A
/// meta tool that runs other tools is judged by the tools it would run,
/// and refused when it names none.
pub fn blocked(name: &str, args: &Value) -> Option<String> {
    let upper = name.to_ascii_uppercase();
    let runs_others = upper.contains("EXECUTE") && !upper.contains("BASH");
    if runs_others {
        let mut slugs = Vec::new();
        collect_slugs(args, &mut slugs);
        if slugs.is_empty() {
            return Some(format!("{name} without a named tool"));
        }
        return slugs.into_iter().find(|s| is_write(s));
    }
    is_write(name).then(|| name.to_owned())
}

/// The tools most related to the question, best first, plus the handoff.
pub fn pick_tools(tools: &[McpTool], question: &str) -> Vec<ToolDef> {
    let q: Vec<String> = question
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(str::to_lowercase)
        .collect();
    let score = |t: &McpTool| -> usize {
        let hay = format!("{} {}", t.name, t.description).to_lowercase();
        let mut s = q.iter().filter(|w| hay.contains(w.as_str())).count() * 2;
        // Tool search and schema lookups help a small model find its way.
        if t.name.to_ascii_uppercase().contains("SEARCH_TOOLS") {
            s += 3;
        }
        // Writes are refused anyway; offer reads first.
        if !is_write(&t.name) || t.name.to_ascii_uppercase().contains("EXECUTE") {
            s += 1;
        }
        s
    };
    let mut ranked: Vec<&McpTool> = tools.iter().collect();
    ranked.sort_by(|a, b| score(b).cmp(&score(a)).then_with(|| a.name.cmp(&b.name)));
    let handoff = handoff_tool();
    let size = |t: &ToolDef| t.name.len() + t.description.len() + t.parameters.to_string().len();
    let mut used = size(&handoff);
    let mut out: Vec<ToolDef> = Vec::new();
    for t in ranked {
        if out.len() == MAX_TOOLS {
            break;
        }
        let def = ToolDef {
            name: t.name.clone(),
            description: t.description.chars().take(400).collect(),
            parameters: t.input_schema.clone(),
        };
        // Skip a tool that does not fit; a smaller one further down might.
        if used + size(&def) > TOOL_BUDGET {
            continue;
        }
        used += size(&def);
        out.push(def);
    }
    out.push(handoff);
    out
}

fn handoff_tool() -> ToolDef {
    ToolDef {
        name: HANDOFF_TOOL.into(),
        description: "Use when the request needs changes in an app (sending, creating, \
            updating, moving, deleting), needs many steps, or you cannot do it well. The user \
            can then continue in Claude Code with this conversation."
            .into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "reason": { "type": "string", "description": "Why, in a few words" } },
            "required": ["reason"],
        }),
    }
}

pub const TOOLS_SYSTEM: &str = "\n\nYou can use the tools listed to read from the user's apps \
(through Composio). Only read: you cannot send, create, change or delete anything. When the \
request needs a change or is too complex, call continue_in_claude_code with a short reason, \
then tell the user in one sentence that Claude Code can finish it. Never invent data you did \
not read with a tool.";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolNote<'a> {
    id: &'a str,
    name: &'a str,
}

/// Runs Composio tools for one chat.
pub struct Runner {
    client: McpClient,
    app: AppHandle,
    chat_id: String,
    handoff: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl ToolRunner for Runner {
    fn started(&self, name: &str) {
        let _ = self.app.emit(
            TOOL_EVENT,
            ToolNote {
                id: &self.chat_id,
                name,
            },
        );
    }

    async fn run(&self, name: &str, arguments: &Value) -> String {
        if name == HANDOFF_TOOL {
            let reason = arguments["reason"]
                .as_str()
                .filter(|r| !r.trim().is_empty())
                .unwrap_or("needs more than the local model can do");
            *lock(&self.handoff) = Some(reason.chars().take(160).collect());
            return "Noted. Tell the user in one sentence that Claude Code can finish this, \
                    with the button below the answer."
                .into();
        }
        if let Some(what) = blocked(name, arguments) {
            *lock(&self.handoff) = Some(format!("needs a change ({what})"));
            log_call(&self.app, name, false, "refused: changes something");
            return format!(
                "Refused: {what} would change something, and you may only read. Tell the user \
                 Claude Code can do it."
            );
        }
        match self.client.call_tool(name, arguments).await {
            Ok(text) => {
                log_call(&self.app, name, !text.starts_with("Error:"), &text);
                text
            }
            Err(err) => {
                log_call(&self.app, name, false, &err.to_string());
                format!("Error: {err}")
            }
        }
    }
}

fn log_call(app: &AppHandle, name: &str, ok: bool, message: &str) {
    log::info!("Composio tool {name}: {}", if ok { "ok" } else { "failed" });
    let record = ActionRecord {
        id: 0,
        ts: Utc::now().to_rfc3339(),
        skill_id: SKILL_ID.into(),
        action: name.chars().take(80).collect(),
        label: format!("Composio: {name}"),
        ok,
        message: message.chars().take(200).collect(),
        auto: false,
        undo_path: None,
        undone: false,
    };
    if let Err(err) = lock(&app.state::<AppState>().storage).log_action(&record) {
        log::warn!("could not log the Composio call: {err}");
    }
}

/// The local model, with Composio's tools when they are set up. Falls back
/// to a plain answer when Composio cannot be reached.
pub struct LocalWithTools {
    pub inner: OpenAiCompat,
    pub app: AppHandle,
    pub chat_id: String,
    pub handoff: Arc<Mutex<Option<String>>>,
    pub server: (String, Vec<(String, String)>),
}

#[async_trait]
impl AiProvider for LocalWithTools {
    fn id(&self) -> &'static str {
        "local"
    }

    fn is_local(&self) -> bool {
        self.inner.is_local()
    }

    async fn available(&self) -> bool {
        self.inner.available().await
    }

    async fn chat(
        &self,
        req: &ChatRequest,
        sink: &Sink,
        cancel: &CancellationToken,
    ) -> Result<String, AiError> {
        let (url, headers) = &self.server;
        let connect = tokio::time::timeout(CONNECT_TIMEOUT, async {
            let client = McpClient::connect(url, headers.clone()).await?;
            let tools = client.list_tools().await?;
            Ok::<_, AiError>((client, tools))
        });
        let (client, tools) = match tokio::select! {
            _ = cancel.cancelled() => return Err(AiError::Cancelled),
            r = connect => r,
        } {
            Ok(Ok(found)) => found,
            Ok(Err(err)) => {
                log::warn!("Composio not reachable, answering without it: {err}");
                return self.inner.chat(req, sink, cancel).await;
            }
            Err(_) => {
                log::warn!("Composio did not answer in time, answering without it");
                return self.inner.chat(req, sink, cancel).await;
            }
        };
        let question = req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == sidekick_ai::Role::User)
            .map(|m| m.content.as_str())
            .unwrap_or_default();
        let defs = pick_tools(&tools, question);
        let mut with_tools = req.clone();
        with_tools.system.push_str(TOOLS_SYSTEM);
        let runner = Runner {
            client,
            app: self.app.clone(),
            chat_id: self.chat_id.clone(),
            handoff: self.handoff.clone(),
        };
        let end = self
            .inner
            .chat_with_tools(&with_tools, &defs, &runner, sink, cancel)
            .await?;
        if end.out_of_steps {
            lock(&self.handoff).get_or_insert_with(|| "took too many steps".into());
        }
        if end.text.trim().is_empty() {
            lock(&self.handoff).get_or_insert_with(|| "the local model gave no answer".into());
        }
        Ok(end.text)
    }
}

/// Finds a Composio server in Claude Code's config (`~/.claude.json`), so
/// the user does not have to paste it again.
pub fn from_claude_config(claude_json: &str) -> Option<(String, BTreeMap<String, String>)> {
    let v: Value = serde_json::from_str(claude_json).ok()?;
    let mut servers: Vec<&Value> = Vec::new();
    if let Some(m) = v["mcpServers"].as_object() {
        servers.extend(m.values());
    }
    // Project-scoped servers live under each project.
    if let Some(projects) = v["projects"].as_object() {
        for p in projects.values() {
            if let Some(m) = p["mcpServers"].as_object() {
                servers.extend(m.values());
            }
        }
    }
    servers.into_iter().find_map(|s| {
        let url = s["url"].as_str()?;
        if !url.to_ascii_lowercase().contains("composio") {
            return None;
        }
        let headers = s["headers"]
            .as_object()
            .map(|h| {
                h.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        Some((url.to_owned(), headers))
    })
}

/// A file with the chat so far, for Claude Code to pick up.
pub fn handoff_markdown(
    messages: &[sidekick_ai::Message],
    reason: Option<&str>,
    context: &crate::ai::Context,
) -> String {
    let mut md = String::from(
        "# Continued from Sidekick\n\nThe user started this in Sidekick (a desktop assistant) \
         with a small local model, which handed it over to you. Read the conversation, then \
         continue it: answer the latest request, using your tools (including Composio) \
         where needed. Ask before changing anything.\n",
    );
    if let Some(r) = reason {
        md.push_str(&format!("\nWhy it was handed over: {r}\n"));
    }
    if let Some(app) = &context.app {
        md.push_str(&format!(
            "\nThe user was in {app}{}.\n",
            context
                .title
                .as_deref()
                .map(|t| format!(" (window: {t})"))
                .unwrap_or_default()
        ));
    }
    md.push_str("\n## Conversation\n");
    for m in messages {
        let who = match m.role {
            sidekick_ai::Role::User => "User",
            sidekick_ai::Role::Assistant => "Sidekick",
        };
        md.push_str(&format!("\n**{who}:**\n\n{}\n", m.content.trim()));
    }
    md
}

/// Opens Claude Code in a new terminal with the conversation so far, in a
/// folder holding only that conversation (and Composio's MCP config when
/// Claude Code does not have Composio yet). The folder stays the same, so
/// Claude Code asks to trust it only once. Returns the folder.
pub async fn open_in_claude_code(
    app: &AppHandle,
    messages: &[sidekick_ai::Message],
    reason: Option<&str>,
) -> Result<std::path::PathBuf, String> {
    let state = app.state::<AppState>();
    let settings = lock(&state.settings).clone();
    let dir = state.ai_workdir.join("handoff");
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not make the handoff folder: {e}"))?;
    let context = crate::ai::context(app);
    std::fs::write(
        dir.join("conversation.md"),
        handoff_markdown(messages, reason, &context),
    )
    .map_err(|e| format!("could not save the conversation: {e}"))?;

    let claude_json = dirs::home_dir()
        .map(|h| std::fs::read_to_string(h.join(".claude.json")).unwrap_or_default())
        .unwrap_or_default();
    let mut mcp_config = None;
    if let Some((url, headers)) = server(&settings.composio).await
        && from_claude_config(&claude_json).is_none()
    {
        let headers: serde_json::Map<String, Value> = headers
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        let cfg = serde_json::json!({ "mcpServers": { "composio": {
            "type": "http", "url": url, "headers": headers,
        }}});
        let path = dir.join("composio-mcp.json");
        std::fs::write(&path, cfg.to_string()).map_err(|e| e.to_string())?;
        mcp_config = Some(path);
    } else {
        // Do not leave an old copy of the key behind.
        let _ = std::fs::remove_file(dir.join("composio-mcp.json"));
    }
    let claude = Some(settings.ai.claude_code.path.trim().to_owned())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "claude".into());
    launch(&dir, &claude, mcp_config.as_deref())?;
    Ok(dir)
}

#[cfg_attr(not(windows), allow(dead_code))]
const HANDOFF_PROMPT: &str =
    "Read conversation.md in this folder. It is a conversation from Sidekick; continue it.";

#[cfg(windows)]
fn launch(
    dir: &std::path::Path,
    claude: &str,
    mcp: Option<&std::path::Path>,
) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let mut cmd = format!(
        "Set-Location -LiteralPath {}; & {}",
        q(&dir.to_string_lossy()),
        q(claude)
    );
    if let Some(m) = mcp {
        cmd.push_str(&format!(" --mcp-config {}", q(&m.to_string_lossy())));
    }
    cmd.push_str(&format!(" {}", q(HANDOFF_PROMPT)));
    std::process::Command::new("powershell.exe")
        .args(["-NoExit", "-NoLogo", "-NoProfile", "-Command", &cmd])
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open a terminal: {e}"))
}

#[cfg(not(windows))]
fn launch(
    dir: &std::path::Path,
    _claude: &str,
    _mcp: Option<&std::path::Path>,
) -> Result<(), String> {
    Err(format!(
        "Opening Claude Code is only available on Windows. The conversation is in {}",
        dir.display()
    ))
}

pub const CHANGED_EVENT: &str = "composio://changed";
const SIGN_IN_WAIT: Duration = Duration::from_secs(10 * 60);
const CONNECT_WAIT: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Changed {
    ok: bool,
    message: String,
}

fn changed(app: &AppHandle, ok: bool, message: impl Into<String>) {
    let _ = app.emit(
        CHANGED_EVENT,
        Changed {
            ok,
            message: message.into(),
        },
    );
}

async fn open_url(app: &AppHandle, url: &str) -> Result<(), String> {
    crate::state::executor(&app.state::<AppState>())
        .run("open_url", &serde_json::json!({ "url": url }))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Starts signing in: opens Composio in the browser and waits there for the
/// user to allow Sidekick. Returns the code the page shows, so the user can
/// check it matches.
pub async fn sign_in(app: &AppHandle) -> Result<String, String> {
    let login = crate::composio_api::begin_login().await?;
    open_url(app, &login.url).await?;
    let app = app.clone();
    let id = login.id.clone();
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        while started.elapsed() < SIGN_IN_WAIT {
            tokio::time::sleep(Duration::from_secs(2)).await;
            match crate::composio_api::poll_login(&id).await {
                Ok(Some((key, who))) => return finish_sign_in(&app, &key, &who).await,
                Ok(None) => {}
                Err(err) => log::warn!("Composio sign-in check failed: {err}"),
            }
        }
        changed(
            &app,
            false,
            "Composio sign-in timed out. Press Connect again.",
        );
    });
    Ok(login.code)
}

async fn finish_sign_in(app: &AppHandle, key: &str, who: &str) {
    if let Err(err) = crate::secrets::set(crate::composio_api::KEY_NAME, key) {
        return changed(app, false, err);
    }
    crate::composio_api::forget_session();
    let user = crate::composio_api::guess_user_id(key).await;
    let mut settings = lock(&app.state::<AppState>().settings).clone();
    settings.composio.enabled = true;
    settings.composio.account = who.to_owned();
    settings.composio.user_id = user;
    if let Err(err) = crate::commands::apply_settings(app, settings) {
        return changed(app, false, err);
    }
    let who = if who.is_empty() { "your account" } else { who };
    changed(app, true, format!("Composio connected as {who}"));
}

pub fn sign_out(app: &AppHandle) -> Result<(), String> {
    crate::secrets::delete(crate::composio_api::KEY_NAME);
    crate::composio_api::forget_session();
    if let Ok(mut connected) = CONNECTED.write() {
        connected.clear();
    }
    let mut settings = lock(&app.state::<AppState>().settings).clone();
    settings.composio.account.clear();
    settings.composio.user_id.clear();
    crate::commands::apply_settings(app, settings).map(|_| ())
}

/// Sidekick's apps and which are connected on Composio.
pub async fn apps(c: &ComposioSettings) -> Result<Vec<crate::composio_api::App>, String> {
    let key =
        crate::secrets::get(crate::composio_api::KEY_NAME).ok_or("Composio is not connected")?;
    let session = crate::composio_api::session(&key, &user_id(c)).await?;
    let list = crate::composio_api::apps(&key, &session).await?;
    if let Ok(mut connected) = CONNECTED.write() {
        *connected = list
            .iter()
            .filter(|a| a.connected)
            .map(|a| a.slug.clone())
            .collect();
    }
    Ok(list)
}

/// Apps last seen connected, for skills that need one (`app:fathom`).
static CONNECTED: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

pub fn app_connected(slug: &str) -> bool {
    CONNECTED
        .read()
        .map(|c| c.iter().any(|s| s == slug))
        .unwrap_or(false)
}

/// Opens the sign-in page for one app and waits for it to be connected.
pub async fn connect_app(app: &AppHandle, slug: &str) -> Result<(), String> {
    if !crate::composio_api::APPS.iter().any(|a| a.0 == slug) {
        return Err("Unknown app".into());
    }
    let c = lock(&app.state::<AppState>().settings).composio.clone();
    let key = crate::secrets::get(crate::composio_api::KEY_NAME).ok_or("Connect Composio first")?;
    let session = crate::composio_api::session(&key, &user_id(&c)).await?;
    let url = crate::composio_api::link(&key, &session, slug).await?;
    open_url(app, &url).await?;
    let app = app.clone();
    let slug = slug.to_owned();
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        while started.elapsed() < CONNECT_WAIT {
            tokio::time::sleep(Duration::from_secs(3)).await;
            if let Ok(list) = crate::composio_api::apps(&key, &session).await
                && let Some(a) = list.iter().find(|a| a.slug == slug && a.connected)
            {
                return changed(&app, true, format!("{} connected", a.name));
            }
        }
        changed(
            &app,
            false,
            "Still waiting for the app to connect. Check the browser and press Refresh.",
        );
    });
    Ok(())
}

/// How the Composio connection went, for the Test button.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub tools: usize,
    pub reads: usize,
    pub sample: Vec<String>,
}

pub async fn test(settings: &ComposioSettings) -> Result<Check, String> {
    let mut on = settings.clone();
    on.enabled = true;
    let (url, headers) = server(&on).await.ok_or("Connect Composio first")?;
    let found = tokio::time::timeout(CONNECT_TIMEOUT, async {
        let client = McpClient::connect(&url, headers).await?;
        client.list_tools().await
    })
    .await
    .map_err(|_| "Composio did not answer in time".to_string())?
    .map_err(|e| e.to_string())?;
    Ok(Check {
        tools: found.len(),
        reads: found.iter().filter(|t| !is_write(&t.name)).count(),
        sample: found.iter().take(6).map(|t| t.name.clone()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tells_reads_from_writes() {
        for read in [
            "JIRA_SEARCH_ISSUES",
            "GMAIL_FETCH_EMAILS",
            "SLACK_LIST_CHANNELS",
            "GITHUB_GET_ADDRESS_INFO",
            "COMPOSIO_SEARCH_TOOLS",
        ] {
            assert!(!is_write(read), "{read}");
        }
        for write in [
            "SLACK_SEND_MESSAGE",
            "JIRA_TRANSITION_ISSUE",
            "GMAIL_CREATE_DRAFT",
            "TRELLO_MOVE_CARD",
            "COMPOSIO_REMOTE_BASH_TOOL",
            "COMPOSIO_MANAGE_CONNECTIONS",
            "notion-update-page",
        ] {
            assert!(is_write(write), "{write}");
        }
    }

    #[test]
    fn judges_meta_tools_by_what_they_run() {
        let reads = json!({ "tools": [{ "tool_slug": "JIRA_SEARCH_ISSUES", "arguments": {} }] });
        assert_eq!(blocked("COMPOSIO_MULTI_EXECUTE_TOOL", &reads), None);
        let writes = json!({ "tools": [
            { "tool_slug": "JIRA_SEARCH_ISSUES" },
            { "tool_slug": "SLACK_SEND_MESSAGE" },
        ]});
        assert_eq!(
            blocked("COMPOSIO_MULTI_EXECUTE_TOOL", &writes).as_deref(),
            Some("SLACK_SEND_MESSAGE")
        );
        assert!(blocked("COMPOSIO_MULTI_EXECUTE_TOOL", &json!({})).is_some());
        assert!(blocked("COMPOSIO_REMOTE_BASH_TOOL", &json!({ "command": "ls" })).is_some());
        assert_eq!(blocked("GMAIL_FETCH_EMAILS", &json!({})), None);
    }

    #[test]
    fn picks_related_tools_and_adds_the_handoff() {
        let tool = |n: &str, d: &str| McpTool {
            name: n.into(),
            description: d.into(),
            input_schema: json!({ "type": "object" }),
        };
        let mut tools: Vec<McpTool> = (0..30)
            .map(|i| tool(&format!("OTHER_{i}"), "unrelated"))
            .collect();
        tools.push(tool(
            "JIRA_SEARCH_ISSUES",
            "Search Jira issues assigned to a user",
        ));
        tools.push(tool("COMPOSIO_SEARCH_TOOLS", "Find tools"));
        let picked = pick_tools(&tools, "what jira issues are assigned to me?");
        assert_eq!(picked.len(), MAX_TOOLS + 1);
        assert_eq!(picked[0].name, "JIRA_SEARCH_ISSUES");
        assert!(picked.iter().any(|t| t.name == "COMPOSIO_SEARCH_TOOLS"));
        assert_eq!(picked.last().unwrap().name, HANDOFF_TOOL);
    }

    #[test]
    fn keeps_tool_definitions_within_budget() {
        let big = json!({ "type": "object", "properties": { "x": { "description": "y".repeat(5_000) } } });
        let tools: Vec<McpTool> = (0..10)
            .map(|i| McpTool {
                name: format!("BIG_{i}"),
                description: "list".into(),
                input_schema: big.clone(),
            })
            .chain(std::iter::once(McpTool {
                name: "SMALL_LIST".into(),
                description: "list".into(),
                input_schema: json!({ "type": "object" }),
            }))
            .collect();
        let picked = pick_tools(&tools, "list");
        let total: usize = picked
            .iter()
            .map(|t| t.name.len() + t.description.len() + t.parameters.to_string().len())
            .sum();
        assert!(total <= TOOL_BUDGET, "{total}");
        assert!(
            picked.iter().any(|t| t.name == "SMALL_LIST"),
            "a small tool still fits"
        );
        assert_eq!(picked.last().unwrap().name, HANDOFF_TOOL);
    }

    #[test]
    fn needs_a_link_and_the_switch() {
        let mut c = ComposioSettings {
            enabled: true,
            url: "https://backend.composio.dev/v3/mcp/abc/mcp?user_id=me".into(),
            headers: BTreeMap::from([
                ("x-api-key".into(), "k".into()),
                ("empty".into(), " ".into()),
            ]),
            ..Default::default()
        };
        let (url, headers) = manual(&c).unwrap();
        assert!(url.contains("composio"));
        assert_eq!(headers, vec![("x-api-key".to_owned(), "k".to_owned())]);
        c.enabled = false;
        assert!(manual(&c).is_none());
        c.enabled = true;
        c.url = "file:///etc/passwd".into();
        assert!(manual(&c).is_none());
    }

    #[test]
    fn finds_composio_in_claude_config() {
        let cfg = json!({
            "mcpServers": { "other": { "type": "http", "url": "http://127.0.0.1:47823/mcp" } },
            "projects": { "C:/code/x": { "mcpServers": {
                "composio": { "type": "http", "url": "https://mcp.composio.dev/abc", "headers": { "x-api-key": "k" } }
            }}}
        })
        .to_string();
        let (url, headers) = from_claude_config(&cfg).unwrap();
        assert_eq!(url, "https://mcp.composio.dev/abc");
        assert_eq!(headers.get("x-api-key").map(String::as_str), Some("k"));
        assert!(from_claude_config(r#"{"mcpServers":{}}"#).is_none());
    }

    #[test]
    fn handoff_file_carries_the_conversation() {
        let messages = vec![
            sidekick_ai::Message::user("Move my Jira ticket ABC-1 to Done"),
            sidekick_ai::Message {
                role: sidekick_ai::Role::Assistant,
                content: "I can only read; Claude Code can do it.".into(),
            },
        ];
        let ctx = crate::ai::Context {
            app: Some("Visual Studio Code".into()),
            ..Default::default()
        };
        let md = handoff_markdown(
            &messages,
            Some("needs a change (JIRA_TRANSITION_ISSUE)"),
            &ctx,
        );
        assert!(md.contains("ABC-1 to Done"));
        assert!(md.contains("JIRA_TRANSITION_ISSUE"));
        assert!(md.contains("Visual Studio Code"));
        assert!(md.find("**User:**").unwrap() < md.find("**Sidekick:**").unwrap());
    }
}
