//! Composio in Ask mode: the apps connected there (Jira, Trello, Slack,
//! Gmail, Notion and more) become tools for the local model.
//!
//! The local model may only read. A tool that would change something
//! (send, create, move, delete, run code) is refused and the chat offers to
//! continue in Claude Code instead, where its own permission prompts and the
//! island's Allow or Deny cover writes. Every tool call is logged.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

use crate::composio_api as api;
use crate::state::{AppState, lock};

pub const TOOL_EVENT: &str = "ai://tool";
/// The tool the local model calls when a request is beyond it.
pub const HANDOFF_TOOL: &str = "continue_in_claude_code";
/// Most Composio tools offered to the local model at once; small models do
/// worse with long tool lists.
const MAX_TOOLS: usize = 6;
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

/// Connected: an OAuth grant from Connect Composio, or a consumer key.
pub fn signed_in() -> bool {
    crate::secrets::get(api::GRANT_NAME).is_some()
        || crate::secrets::get(api::CONSUMER_KEY_NAME).is_some()
}

/// Composio is on and either connected or given a link and headers by hand.
pub fn is_set_up(c: &ComposioSettings) -> bool {
    c.enabled && (signed_in() || manual(c).is_some())
}

/// The MCP link: Composio Connect unless another was set by hand.
pub fn mcp_url(c: &ComposioSettings) -> String {
    let url = c.url.trim();
    if url.starts_with("https://") || url.starts_with("http://") {
        url.to_owned()
    } else {
        api::CONNECT_URL.to_owned()
    }
}

static TOKEN: std::sync::LazyLock<crate::mcp_oauth::Cached> =
    std::sync::LazyLock::new(Default::default);

fn grant() -> Option<crate::mcp_oauth::Grant> {
    crate::secrets::get(api::GRANT_NAME).and_then(|g| serde_json::from_str(&g).ok())
}

/// An access token that is good for at least another minute.
async fn access_token(force: bool) -> Result<String, String> {
    if !force && let Some(t) = TOKEN.get() {
        return Ok(t);
    }
    let mut g = grant().ok_or("Connect Composio first")?;
    if g.refresh_token.is_empty() {
        return Some(g.access_token)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| "The Composio sign-in ran out. Connect again.".into());
    }
    let (access, rotated, life) = crate::mcp_oauth::refresh(&g).await?;
    if let Some(r) = rotated {
        g.refresh_token = r;
        if let Ok(json) = serde_json::to_string(&g) {
            let _ = crate::secrets::set(api::GRANT_NAME, &json);
        }
    }
    TOKEN.set(access.clone(), life);
    Ok(access)
}

/// Headers that sign Sidekick in: the consumer key, the OAuth token, or
/// headers given by hand.
async fn auth_headers(c: &ComposioSettings, force: bool) -> Result<Vec<(String, String)>, String> {
    if let Some(key) = crate::secrets::get(api::CONSUMER_KEY_NAME) {
        return Ok(vec![("x-consumer-api-key".into(), key)]);
    }
    if grant().is_some() {
        let t = access_token(force).await?;
        return Ok(vec![("authorization".into(), format!("Bearer {t}"))]);
    }
    manual(c)
        .map(|(_, h)| h)
        .ok_or_else(|| "Connect Composio first".into())
}

/// The MCP link and headers chat uses, when Composio is set up.
pub async fn server(c: &ComposioSettings) -> Option<(String, Vec<(String, String)>)> {
    if !is_set_up(c) {
        return None;
    }
    match auth_headers(c, false).await {
        Ok(h) => Some((mcp_url(c), h)),
        Err(err) => {
            log::warn!("Composio: {err}");
            None
        }
    }
}

/// One connected client, reused for a while so each tool call does not
/// start over.
static CLIENT: tokio::sync::Mutex<Option<(std::time::Instant, Arc<McpClient>)>> =
    tokio::sync::Mutex::const_new(None);
const CLIENT_TTL: Duration = Duration::from_secs(10 * 60);

fn forget_client() {
    if let Ok(mut g) = CLIENT.try_lock() {
        *g = None;
    }
}

async fn open_client(c: &ComposioSettings, force: bool) -> Result<Arc<McpClient>, String> {
    let headers = auth_headers(c, force).await?;
    let url = mcp_url(c);
    let client = tokio::time::timeout(CONNECT_TIMEOUT, McpClient::connect(&url, headers))
        .await
        .map_err(|_| "Composio did not answer in time".to_string())?
        .map_err(|e| e.to_string())?;
    Ok(Arc::new(client))
}

/// A connected client; a failed connection is tried once more with a new
/// token (the old one may have run out).
async fn client(c: &ComposioSettings) -> Result<Arc<McpClient>, String> {
    if !c.enabled {
        return Err("Composio is off".into());
    }
    let mut g = CLIENT.lock().await;
    if let Some((at, cl)) = g.as_ref()
        && at.elapsed() < CLIENT_TTL
    {
        return Ok(cl.clone());
    }
    let cl = match open_client(c, false).await {
        Ok(cl) => cl,
        Err(err) if grant().is_some() => {
            log::info!("Composio: retrying with a new token ({err})");
            TOKEN.clear();
            open_client(c, true).await.map_err(friendly)?
        }
        Err(err) => return Err(friendly(err)),
    };
    *g = Some((std::time::Instant::now(), cl.clone()));
    Ok(cl)
}

fn friendly(err: String) -> String {
    let lower = err.to_lowercase();
    if lower.contains("401") || lower.contains("unauthorized") || lower.contains("invalid consumer")
    {
        "Composio did not accept the sign-in. Connect again.".into()
    } else if lower.contains("dns") || lower.contains("connect") || lower.contains("timed out") {
        "Composio is not reachable right now. Check the internet and try again.".into()
    } else {
        err
    }
}

/// Calls one of Composio Connect's own tools and reads its JSON answer.
async fn call(c: &ComposioSettings, tool: &str, args: Value) -> Result<Value, String> {
    let cl = client(c).await?;
    let text = match cl.call_tool(tool, &args).await {
        Ok(t) => t,
        Err(err) => {
            // The session may have expired on the server: start over once.
            forget_client();
            let cl = client(c).await?;
            cl.call_tool(tool, &args)
                .await
                .map_err(|_| err.to_string())?
        }
    };
    api::parse_text(&text)
}

/// Runs one app tool for Sidekick itself (calendar, brief, notes).
pub async fn run_tool(c: &ComposioSettings, tool: &str, args: Value) -> Result<Value, String> {
    let v = call(c, api::EXECUTE_TOOL, api::execute_args(tool, args)).await?;
    api::first_result(&v).map_err(|e| format!("{tool}: {e}"))
}

/// A link given by hand (or copied from Claude Code) with its headers.
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
    // A link with no way in needs the Connect button instead.
    (!headers.is_empty()).then(|| (url.to_owned(), headers))
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

/// Composio's own plumbing: connecting accounts, its sandbox, feedback.
/// Sidekick handles connections in Settings, so the model never sees these.
fn plumbing(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "MANAGE_CONNECTIONS",
        "WAIT_FOR_CONNECTIONS",
        "REMOTE_BASH",
        "REMOTE_WORKBENCH",
        "MANAGE_SKILL",
        "SUBMIT_FEEDBACK",
    ]
    .iter()
    .any(|p| upper.contains(p))
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
    let mut ranked: Vec<&McpTool> = tools.iter().filter(|t| !plumbing(&t.name)).collect();
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

pub const TOOLS_SYSTEM: &str = "\n\nTools: search finds the user's files and history on this \
PC, today gives meetings and time, recent shows what just happened, open opens a file, folder \
or page, propose offers an action (move, zip, convert, open) as a button the user taps; \
never say you did something you only proposed. Other tools read and change the user's apps; a change (send, create, update, delete) is \
prepared as a button the user taps, so call the tool as usual and say it waits for their tap. \
browser reads and acts on web pages in their browser. Look things up before answering. When \
the request needs more than you can do, call continue_in_claude_code with a short reason. \
Never invent data you did not read with a tool.";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolNote<'a> {
    id: &'a str,
    name: &'a str,
    /// What the step does, in words ("Searching the web for flights").
    label: String,
}

/// Runs Composio tools for one chat.
pub struct Runner {
    client: Option<Arc<McpClient>>,
    app: AppHandle,
    chat_id: String,
    handoff: Arc<Mutex<Option<String>>>,
    /// This PC only: tools that reach the internet are refused even when the
    /// model names one it was not given.
    offline: bool,
}

#[async_trait]
impl ToolRunner for Runner {
    fn started(&self, name: &str, arguments: &Value) {
        let _ = self.app.emit(
            TOOL_EVENT,
            ToolNote {
                id: &self.chat_id,
                name,
                label: crate::ask_tools::step_label(name, arguments),
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
        if self.offline && crate::ask_tools::is_web(name) {
            return "Error: this question is set to This PC only, so nothing goes online. \
                    Answer from this PC, or say it needs the internet."
                .into();
        }
        if plumbing(name) {
            return "Error: app connections are managed in Sidekick's Settings > Apps, not here."
                .into();
        }
        if let Some(what) = blocked(name, arguments) {
            // A change in an app: prepared as a button, run on the user's tap.
            log_call(&self.app, name, true, "prepared for a tap");
            if what.contains("without a named tool") {
                return format!("Error: {what}; name the tool to run.");
            }
            return crate::act::offer_app_change(&self.app, &self.chat_id, name, arguments);
        }
        if let Some(out) = crate::ask_tools::run(&self.app, &self.chat_id, name, arguments).await {
            return out;
        }
        let Some(client) = &self.client else {
            return format!("Error: there is no tool called {name}.");
        };
        match client.call_tool(name, arguments).await {
            Ok(text) => {
                log_call(&self.app, name, !text.starts_with("Error:"), &text);
                text
            }
            Err(err) => {
                // The kept connection may have expired; the next chat opens a new one.
                forget_chat_client().await;
                log_call(&self.app, name, false, &err.to_string());
                format!("Error: {err}")
            }
        }
    }

    fn parallel(&self, name: &str) -> bool {
        if crate::ask_tools::is_own(name) || name == HANDOFF_TOOL {
            return crate::ask_tools::reads_only(name);
        }
        // A Composio tool that only reads (no meta tool that runs others).
        let upper = name.to_ascii_uppercase();
        !plumbing(name) && !is_write(name) && !upper.contains("EXECUTE") && !upper.contains("BASH")
    }
}

/// Runs one Composio tool now (after the user tapped its button).
pub async fn run_tapped(app: &AppHandle, name: &str, arguments: &Value) -> Result<String, String> {
    let settings = lock(&app.state::<AppState>().settings).composio.clone();
    let (url, headers) = server(&settings)
        .await
        .ok_or("Composio is not connected (Settings > Apps)")?;
    let client = McpClient::connect(&url, headers)
        .await
        .map_err(|e| e.to_string())?;
    let text = client
        .call_tool(name, arguments)
        .await
        .map_err(|e| e.to_string())?;
    log_call(app, name, !text.starts_with("Error:"), &text);
    if text.starts_with("Error:") {
        return Err(text);
    }
    Ok(text)
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

/// The local model with Sidekick's own tools (search, today, recent, open),
/// plus Composio's when they are set up and reachable.
pub struct LocalWithTools {
    pub inner: OpenAiCompat,
    pub app: AppHandle,
    pub chat_id: String,
    pub handoff: Arc<Mutex<Option<String>>>,
    pub server: Option<(String, Vec<(String, String)>)>,
    /// This PC only: no web tools, so nothing leaves the PC.
    pub offline: bool,
}

#[async_trait]
impl AiProvider for LocalWithTools {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn is_local(&self) -> bool {
        self.inner.is_local()
    }

    async fn available(&self) -> bool {
        self.inner.available().await
    }

    /// Loads the model and opens the Composio connection while the user types.
    async fn warm(&self) {
        let model = self.inner.warm();
        let tools = async {
            if let Some(server) = &self.server {
                let _ = connect(server, &CancellationToken::new()).await;
            }
        };
        tokio::join!(model, tools);
    }

    async fn chat(
        &self,
        req: &ChatRequest,
        sink: &Sink,
        cancel: &CancellationToken,
    ) -> Result<String, AiError> {
        let (client, tools) = match &self.server {
            Some(server) => connect(server, cancel).await?,
            None => (None, Vec::new()),
        };
        let question = req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == sidekick_ai::Role::User)
            .map(|m| m.content.as_str())
            .unwrap_or_default();
        let started = std::time::Instant::now();
        // The local model picks tools by words: an embedding call first
        // would make Ollama swap models on every question.
        let mut defs = if self.inner.id() == "local" {
            if crate::ask_tools::needs_no_tools(question) {
                Vec::new()
            } else {
                crate::ask_tools::by_words(question, crate::ask_tools::defs())
            }
        } else {
            crate::ask_tools::pick(&self.app, question, crate::ask_tools::defs()).await
        };
        if self.offline {
            defs.retain(|d| !crate::ask_tools::is_web(&d.name));
        }
        // Small talk and writing get no tools at all.
        let chatty = defs.is_empty();
        if !chatty {
            if client.is_some() {
                defs.extend(pick_tools(&tools, question));
            } else {
                defs.push(handoff_tool());
            }
        }
        log::info!(
            "ask: {} tools for {} after {} ms",
            defs.len(),
            self.inner.id(),
            started.elapsed().as_millis()
        );
        let mut with_tools = req.clone();
        // Right after the fixed rules, so the start of the prompt stays the
        // same from message to message and the local server reuses its cache.
        match with_tools.system.find(crate::ai::FIXED_END) {
            _ if chatty => {}
            Some(at) => with_tools.system.insert_str(at, TOOLS_SYSTEM),
            None => with_tools.system.push_str(TOOLS_SYSTEM),
        }
        // A screenshot: a vision model sees it when one is set; otherwise
        // the text model gets the screen's text, read here with OCR.
        if let Some(png) = req.image.as_ref() {
            let ai = lock(&self.app.state::<AppState>().settings)
                .ai
                .local
                .clone();
            let vision_name = ai.vision_model.trim();
            // The local vision model only stands in for the local text model.
            if self.inner.id() == "local"
                && !vision_name.is_empty()
                && !vision_name.eq_ignore_ascii_case("off")
            {
                let vision = OpenAiCompat::new(Some(ai.base_url), Some(ai.vision_model));
                return vision.chat(req, sink, cancel).await;
            }
            with_tools.image = None;
            let text = match crate::ask_tools::cached_screen(&self.app) {
                Some(text) => Ok(text),
                None => crate::ask_tools::screen_text(&self.app, png).await,
            };
            match text {
                Ok(text) if !text.is_empty() => with_tools.system.push_str(&format!(
                    "\n\nText on the user's screen (read with OCR, layout lost):\n```\n{text}\n```"
                )),
                Ok(_) => with_tools.system.push_str(
                    "\n\nThe screenshot has no readable text; say so and ask what they need.",
                ),
                Err(err) => with_tools.system.push_str(&format!(
                    "\n\nThe screen could not be read ({err}); say so in one sentence."
                )),
            }
        }
        let runner = Runner {
            client,
            app: self.app.clone(),
            chat_id: self.chat_id.clone(),
            handoff: self.handoff.clone(),
            offline: self.offline,
        };
        let end = match self
            .inner
            .chat_with_tools(&with_tools, &defs, &runner, sink, cancel)
            .await
        {
            // Some models (Gemma, older Llama) cannot call tools: answer plainly.
            Err(AiError::Failed(err))
                if !sink.has_sent() && err.to_lowercase().contains("tools") =>
            {
                log::info!("local model has no tool support, answering without tools: {err}");
                return self.inner.chat(req, sink, cancel).await;
            }
            other => other?,
        };
        if end.out_of_steps {
            lock(&self.handoff).get_or_insert_with(|| "took too many steps".into());
        }
        if end.text.trim().is_empty() {
            lock(&self.handoff).get_or_insert_with(|| "the local model gave no answer".into());
        } else if looks_like_tool_text(&end.text) {
            // A small model that writes its tool call as text instead of
            // making it: not an answer, so offer a stronger model.
            lock(&self.handoff)
                .get_or_insert_with(|| "the local model wrote a tool call as text".into());
        }
        Ok(end.text)
    }
}

/// `{"name": "notifications", "arguments": {...}}` or "SEARCH: ... RESULT:"
/// written out as the answer.
fn looks_like_tool_text(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with('{') && t.contains("\"name\"") && t.contains("\"arguments\""))
        || t.lines().any(|l| {
            let l = l.trim_start();
            l.starts_with("SEARCH:") || l.starts_with("RESULT:") || l.starts_with("ACTION:")
        })
}

#[cfg(test)]
mod tool_text_tests {
    #[test]
    fn spots_tool_calls_written_as_text() {
        assert!(super::looks_like_tool_text(
            r#"{"name": "notifications", "arguments": {"level": "important"}}"#
        ));
        assert!(super::looks_like_tool_text(
            "SEARCH: \"aapl\"\nRESULT: https://x"
        ));
        assert!(!super::looks_like_tool_text("Bluetooth is on."));
    }
}

/// The chat connection and its tool list, kept between messages: (link,
/// connected at, client, tools).
type ChatClient = (String, std::time::Instant, Arc<McpClient>, Vec<McpTool>);
static CHAT_CLIENT: tokio::sync::Mutex<Option<ChatClient>> = tokio::sync::Mutex::const_new(None);
/// After this the tool list is refreshed in the background.
const TOOLS_FRESH: Duration = Duration::from_secs(5 * 60);

async fn forget_chat_client() {
    *CHAT_CLIENT.lock().await = None;
}

async fn open_chat_client(
    url: &str,
    headers: &[(String, String)],
) -> Result<(Arc<McpClient>, Vec<McpTool>), AiError> {
    let client = McpClient::connect(url, headers.to_vec()).await?;
    let tools = client.list_tools().await?;
    Ok((Arc::new(client), tools))
}

/// Connects to Composio for a chat, reusing the last connection. Unreachable
/// is not an error: the chat goes on with Sidekick's own tools.
async fn connect(
    (url, headers): &(String, Vec<(String, String)>),
    cancel: &CancellationToken,
) -> Result<(Option<Arc<McpClient>>, Vec<McpTool>), AiError> {
    {
        let cached = CHAT_CLIENT.lock().await;
        if let Some((u, at, client, tools)) = cached.as_ref()
            && u == url
            && at.elapsed() < CLIENT_TTL
        {
            if at.elapsed() > TOOLS_FRESH {
                let (url, headers) = (url.clone(), headers.clone());
                tauri::async_runtime::spawn(async move {
                    if let Ok(Ok((c, t))) =
                        tokio::time::timeout(CONNECT_TIMEOUT, open_chat_client(&url, &headers))
                            .await
                    {
                        *CHAT_CLIENT.lock().await = Some((url, std::time::Instant::now(), c, t));
                    }
                });
            }
            return Ok((Some(client.clone()), tools.clone()));
        }
    }
    let connect = tokio::time::timeout(CONNECT_TIMEOUT, open_chat_client(url, headers));
    match tokio::select! {
        _ = cancel.cancelled() => return Err(AiError::Cancelled),
        r = connect => r,
    } {
        Ok(Ok((client, tools))) => {
            *CHAT_CLIENT.lock().await = Some((
                url.clone(),
                std::time::Instant::now(),
                client.clone(),
                tools.clone(),
            ));
            Ok((Some(client), tools))
        }
        Ok(Err(err)) => {
            log::warn!("Composio not reachable, using local tools only: {err}");
            Ok((None, Vec::new()))
        }
        Err(_) => {
            log::warn!("Composio did not answer in time, using local tools only");
            Ok((None, Vec::new()))
        }
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

pub const CHANGED_EVENT: &str = "composio://changed";
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

/// Connect Composio: the browser sign-in Claude Desktop uses. Returns at
/// once; the result comes as a `composio://changed` event.
pub async fn sign_in(app: &AppHandle) -> Result<String, String> {
    let url = mcp_url(&lock(&app.state::<AppState>().settings).composio);
    let (opened_tx, opened_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut opened_tx = Some(opened_tx);
        let result = crate::mcp_oauth::sign_in(&url, |page| {
            let app = app2.clone();
            let tx = opened_tx.take();
            async move {
                let r = open_url(&app, &page).await;
                if let Some(tx) = tx {
                    let _ = tx.send(r.clone());
                }
                r
            }
        })
        .await;
        match result {
            Ok((grant, access, life)) => finish_sign_in(&app2, &grant, access, life).await,
            Err(err) => {
                if let Some(tx) = opened_tx.take() {
                    let _ = tx.send(Err(err.clone()));
                }
                changed(&app2, false, err);
            }
        }
    });
    // Wait only until the browser is open, so a failure there shows now.
    match tokio::time::timeout(Duration::from_secs(30), opened_rx).await {
        Ok(Ok(Ok(()))) => Ok("Composio opened in your browser".into()),
        Ok(Ok(Err(err))) => Err(err),
        _ => Err("Composio is not reachable right now. Try again in a moment.".into()),
    }
}

async fn finish_sign_in(
    app: &AppHandle,
    grant: &crate::mcp_oauth::Grant,
    access: String,
    life: u64,
) {
    let json = match serde_json::to_string(grant) {
        Ok(j) => j,
        Err(err) => return changed(app, false, err.to_string()),
    };
    if let Err(err) = crate::secrets::set(api::GRANT_NAME, &json) {
        return changed(app, false, err);
    }
    crate::secrets::delete(api::CONSUMER_KEY_NAME);
    crate::secrets::delete(api::LEGACY_KEY_NAME);
    TOKEN.set(access, life);
    forget_client();
    connected_now(app, "Composio Connect").await;
}

/// Uses a consumer key (`ck_...`) instead of signing in, after checking it.
pub async fn use_key(app: &AppHandle, key: &str) -> Result<String, String> {
    let key = key.trim();
    if key.len() < 8 {
        return Err("Paste the whole key".into());
    }
    let mut c = lock(&app.state::<AppState>().settings).composio.clone();
    c.enabled = true;
    let url = mcp_url(&c);
    let headers = vec![("x-consumer-api-key".to_owned(), key.to_owned())];
    tokio::time::timeout(CONNECT_TIMEOUT, McpClient::connect(&url, headers))
        .await
        .map_err(|_| "Composio did not answer in time".to_string())?
        .map_err(|e| friendly(e.to_string()))?;
    crate::secrets::set(api::CONSUMER_KEY_NAME, key)?;
    crate::secrets::delete(api::GRANT_NAME);
    TOKEN.clear();
    forget_client();
    connected_now(app, "Composio key").await;
    Ok("Composio connected".into())
}

async fn connected_now(app: &AppHandle, how: &str) {
    let mut settings = lock(&app.state::<AppState>().settings).clone();
    settings.composio.enabled = true;
    settings.composio.account = how.to_owned();
    settings.composio.user_id.clear();
    if let Err(err) = crate::commands::apply_settings(app, settings.clone()) {
        return changed(app, false, err);
    }
    let n = apps(&settings.composio)
        .await
        .map(|a| a.iter().filter(|x| x.connected).count())
        .unwrap_or(0);
    changed(
        app,
        true,
        match n {
            0 => "Composio connected".to_owned(),
            1 => "Composio connected with 1 app".to_owned(),
            n => format!("Composio connected with {n} apps"),
        },
    );
}

pub fn sign_out(app: &AppHandle) -> Result<(), String> {
    for name in [
        api::GRANT_NAME,
        api::CONSUMER_KEY_NAME,
        api::LEGACY_KEY_NAME,
    ] {
        crate::secrets::delete(name);
    }
    TOKEN.clear();
    forget_client();
    if let Ok(mut connected) = CONNECTED.write() {
        connected.clear();
    }
    let mut settings = lock(&app.state::<AppState>().settings).clone();
    settings.composio.account.clear();
    settings.composio.user_id.clear();
    crate::commands::apply_settings(app, settings).map(|_| ())
}

/// Every app connected in the account: Sidekick's own (connected or not)
/// first, then the rest. The last good answer is kept, so Settings still
/// shows it when Composio is slow.
pub async fn apps(c: &ComposioSettings) -> Result<Vec<api::App>, String> {
    let previous: std::collections::BTreeSet<String> = CONNECTED
        .read()
        .map(|c| c.iter().cloned().collect())
        .unwrap_or_default();
    let cl = client(c).await?;
    let mut connected = std::collections::BTreeSet::new();
    if let Ok(tools) = cl.list_tools().await
        && let Some(search) = tools.iter().find(|t| t.name == api::SEARCH_TOOL)
    {
        connected = api::connected_from_description(&search.description);
    }
    match call(c, api::CONNECTIONS_TOOL, api::list_args()).await {
        Ok(v) => {
            let listed = api::connected_from_list(&v);
            let inactive = api::inactive_from_list(&v);
            // Union description + active list. Keep what we already knew unless
            // this list explicitly marks it inactive — a fresh connect can
            // disappear from the next poll for a second or two.
            connected.extend(listed);
            for s in previous {
                if !inactive.contains(&s) || recently_pinned(&s) {
                    connected.insert(s);
                }
            }
            for s in &inactive {
                if !recently_pinned(s) {
                    connected.remove(s);
                }
            }
        }
        Err(err) if connected.is_empty() && previous.is_empty() => return Err(err),
        Err(err) => {
            log::info!("Composio connections list failed, using the summary: {err}");
            connected.extend(previous);
        }
    }
    if let Ok(mut g) = CONNECTED.write() {
        *g = connected.iter().cloned().collect();
    }
    Ok(api::merge_apps(&connected))
}

/// After a successful connect poll, ignore "not active" list answers for a bit.
const PIN_GRACE: Duration = Duration::from_secs(45);
static PINNED: std::sync::RwLock<Vec<(String, Instant)>> = std::sync::RwLock::new(Vec::new());

/// Remembers one app as connected (e.g. right after a successful connect poll).
fn pin_connected(slug: &str) {
    if let Ok(mut g) = CONNECTED.write()
        && !g.iter().any(|s| s == slug)
    {
        g.push(slug.to_owned());
    }
    if let Ok(mut p) = PINNED.write() {
        p.retain(|(s, at)| s != slug && at.elapsed() < PIN_GRACE);
        p.push((slug.to_owned(), Instant::now()));
    }
}

fn recently_pinned(slug: &str) -> bool {
    PINNED
        .read()
        .map(|p| {
            p.iter()
                .any(|(s, at)| s == slug && at.elapsed() < PIN_GRACE)
        })
        .unwrap_or(false)
}

/// The apps last seen connected, without asking Composio.
pub fn cached_apps() -> Vec<api::App> {
    let set: std::collections::BTreeSet<String> = CONNECTED
        .read()
        .map(|c| c.iter().cloned().collect())
        .unwrap_or_default();
    api::merge_apps(&set)
}

static CONNECTED: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

pub fn app_connected(slug: &str) -> bool {
    CONNECTED
        .read()
        .map(|c| c.iter().any(|s| s == slug))
        .unwrap_or(false)
}

/// Connects one more app (most are connected already through the account).
pub async fn connect_app(app: &AppHandle, slug: &str) -> Result<(), String> {
    if slug.is_empty() || !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("Unknown app".into());
    }
    let c = lock(&app.state::<AppState>().settings).composio.clone();
    let v = call(&c, api::CONNECTIONS_TOOL, api::add_args(slug)).await?;
    let url = api::redirect_url(&v).ok_or("Composio did not return a sign-in page")?;
    open_url(app, &url).await?;
    let app = app.clone();
    let slug = slug.to_owned();
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        while started.elapsed() < CONNECT_WAIT {
            tokio::time::sleep(Duration::from_secs(4)).await;
            if let Ok(v) = call(&c, api::CONNECTIONS_TOOL, api::list_args()).await
                && api::connected_from_list(&v).contains(&slug)
            {
                // Pin before/after the full refresh so a laggy list cannot
                // clear the app we just saw as active.
                pin_connected(&slug);
                let _ = apps(&c).await;
                pin_connected(&slug);
                return changed(&app, true, format!("{} connected", api::app_name(&slug)));
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
        tools.push(tool(
            "COMPOSIO_MANAGE_CONNECTIONS",
            "Manage jira connections assigned to me",
        ));
        let picked = pick_tools(&tools, "what jira issues are assigned to me?");
        assert!(!picked.iter().any(|t| t.name.contains("MANAGE_CONNECTIONS")));
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
        assert_eq!(mcp_url(&c), crate::composio_api::CONNECT_URL);
        // A link with nothing to sign in with is left to the Connect button.
        c.url = "https://connect.composio.dev/mcp".into();
        c.headers.clear();
        assert!(manual(&c).is_none());
        assert_eq!(mcp_url(&c), "https://connect.composio.dev/mcp");
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
