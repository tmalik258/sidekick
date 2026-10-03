//! AI tiers wired to settings: chat in Ask mode (T2 with fallback) and
//! T1 decisions for ranking suggestions.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sidekick_ai::{
    AiProvider, Anthropic, CancellationToken, ChatRequest, ClaudeCode, Codex, Decider,
    LocalDecider, Message, OpenAiCompat, Router, SemIf, Sink,
};
use sidekick_core::{AiSettings, Settings};
use sidekick_sensors::classify::{ClipKind, clip_kind};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, lock};

pub const DELTA_EVENT: &str = "ai://delta";
pub const DONE_EVENT: &str = "ai://done";

/// Longest clipboard text attached to a chat.
const MAX_CLIP: usize = 8_000;
const SEMIF_TIMEOUT: Duration = Duration::from_secs(90);

const SYSTEM: &str = "You are Sidekick, the assistant on the user's Windows PC. You help \
with their own files, apps, day and whatever is on screen right now, and you get small \
things done. Rules:
- Answer in one or two short sentences. Lists only when they ask for one.
- Never introduce yourself or say what you are.
- Do it yourself with Sidekick's tools (named find_files, search, open, show_in_folder, \
recent, screen_text, web_search, read_page, browser, app_action, desktop, apps, recipes, remember, notifications, pc_status, pc_control, windows, propose, or the \
same with a sidekick_ prefix): find a file, then open it or show it in its folder; search the \
web and read pages for anything current or not on this PC, and link your sources; change volume, brightness or dark \
mode; switch to or start an app. Check pc_status before suggesting a Windows setting, and never \
offer to turn on what is already on. Do not use a shell or your own file access for this, and never \
tell the user to do something a tool can do. Say you cannot only after a tool failed.
- Look things up with tools instead of guessing. Never invent files, dates or facts.
- When the user tells you something to keep (their manager, signature, usual folder), save it with \
remember. To repeat a task later or on a schedule (\"every Friday at 5\"), save it with recipes.
- Tasks with several steps (reply and attach, find then send, fill a form): say the plan in one \
short line, then do one step at a time and check its result (read the page or window again) \
before the next. If something unexpected shows up (a login, a popup, a different page), deal with \
it or stop and ask. Stop after 12 steps and say where you got to. Anything that sends, posts, \
pays or deletes waits for the user's tap; prepare it and say so.
- Paths and links: write them as markdown links, [name](C:\\full\\path) or [name](https://...), \
so the user can click them.
- When there is a clear next step, end with up to three lines, each \"OPTION: \" and a short \
action in the user's words, like \"OPTION: Open invoice.pdf\".
- Code or commands only when asked, in fenced code blocks.
- Never use em dashes.";

/// Chat providers in the user's order. `all` includes switched-off ones
/// (for the status list in settings).
fn providers(app: &AppHandle, ai: &AiSettings, all: bool) -> Vec<Arc<dyn AiProvider>> {
    let state = app.state::<AppState>();
    let mut out: Vec<Arc<dyn AiProvider>> = Vec::new();
    for id in &ai.order {
        match id.as_str() {
            "claude_code" if all || ai.claude_code.enabled => out.push(Arc::new(ClaudeCode {
                path: Some(ai.claude_code.path.trim())
                    .filter(|p| !p.is_empty())
                    .map(Into::into),
                model: Some(ai.claude_code.model.clone()).filter(|m| !m.is_empty()),
                workdir: state.ai_workdir.clone(),
                mcp_config: Some(state.ai_workdir.join(crate::mcp::CONFIG_FILE)),
            })),
            "codex" if all || ai.codex.enabled => out.push(Arc::new(Codex {
                path: Some(ai.codex.path.trim())
                    .filter(|p| !p.is_empty())
                    .map(Into::into),
                model: Some(ai.codex.model.clone()).filter(|m| !m.is_empty()),
                workdir: state.ai_workdir.join("codex"),
                mcp: Some((
                    format!("http://127.0.0.1:{}/mcp", crate::mcp::PORT),
                    state.mcp_token.clone(),
                )),
            })),
            "anthropic" if all || ai.anthropic.enabled => {
                out.push(Arc::new(Anthropic::new(Some(ai.anthropic.model.clone()))))
            }
            "local" if all || ai.local.enabled => out.push(Arc::new(local_model(ai))),
            _ => {}
        }
    }
    out
}

pub(crate) fn local_model(ai: &AiSettings) -> OpenAiCompat {
    OpenAiCompat::new(
        Some(ai.local.base_url.clone()),
        Some(ai.local.model.clone()),
    )
}

/// The router for one Ask-mode chat: the local model always gets Sidekick's
/// own tools, and Composio's when they are set up and the chat may leave
/// the PC.
async fn chat_router(
    app: &AppHandle,
    chat_id: &str,
    handoff: &Arc<std::sync::Mutex<Option<String>>>,
    local_only: bool,
    prefer: Option<&str>,
) -> Router {
    let settings = lock(&app.state::<AppState>().settings).clone();
    let server = if local_only || !settings.ai.local.enabled {
        None
    } else {
        crate::composio::server(&settings.composio).await
    };
    let list = providers(app, &settings.ai, false)
        .into_iter()
        .map(|p| match p.id() {
            "local" => Arc::new(crate::composio::LocalWithTools {
                inner: local_model(&settings.ai),
                app: app.clone(),
                chat_id: chat_id.to_owned(),
                handoff: handoff.clone(),
                server: server.clone(),
                offline: local_only,
            }) as Arc<dyn AiProvider>,
            _ => p,
        })
        .collect();
    Router::new(prefer_first(list, prefer))
}

/// The picked provider first; the rest keep the user's order.
fn prefer_first(
    mut list: Vec<Arc<dyn AiProvider>>,
    prefer: Option<&str>,
) -> Vec<Arc<dyn AiProvider>> {
    if let Some(id) = prefer {
        list.sort_by_key(|p| p.id() != id);
    }
    list
}

fn semif(app: &AppHandle, ai: &AiSettings) -> SemIf {
    SemIf {
        command: ai.semif.command.clone(),
        mode: ai.semif.mode.clone(),
        backend: ai.semif.backend.clone(),
        model: ai.semif.model.clone(),
        revision: ai.semif.revision.clone(),
        gguf: Some(ai.semif.gguf.clone()).filter(|g| !g.is_empty()),
        scratch: app.state::<AppState>().scratch_dir.join("semif"),
        timeout: SEMIF_TIMEOUT,
    }
}

/// T1 deciders in order: SemIf when switched on, then the local model.
pub fn deciders(app: &AppHandle, settings: &Settings) -> Vec<Arc<dyn Decider>> {
    let ai = &settings.ai;
    let mut out: Vec<Arc<dyn Decider>> = Vec::new();
    if !ai.decisions {
        return out;
    }
    if ai.semif.enabled {
        out.push(Arc::new(semif(app, ai)));
    }
    if ai.local.enabled {
        out.push(Arc::new(LocalDecider {
            model: local_model(ai),
        }));
    }
    out
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    id: &'static str,
    available: bool,
    local: bool,
}

pub async fn status(app: &AppHandle) -> Vec<ProviderStatus> {
    let ai = lock(&app.state::<AppState>().settings).ai.clone();
    let mut out = Vec::new();
    for p in providers(app, &ai, true) {
        out.push(ProviderStatus {
            id: p.id(),
            available: p.available().await,
            local: p.is_local(),
        });
    }
    out.push(ProviderStatus {
        id: "semif",
        available: semif(app, &ai).available().await,
        local: true,
    });
    out
}

/// What the user chose to attach to a chat.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attach {
    pub window: bool,
    pub clipboard: bool,
    /// Text of the web page the conversation is about.
    #[serde(default)]
    pub page: Option<String>,
    /// The user is describing a new skill for Sidekick to learn.
    #[serde(default)]
    pub skill: bool,
    /// Send a screenshot of the window the user was in.
    #[serde(default)]
    pub screen: bool,
    /// The question was spoken; read the answer aloud.
    #[serde(default)]
    pub speak: bool,
    /// The model picked in Ask mode ("claude_code", "codex", "anthropic",
    /// "local"); it goes first, the others stay as a fallback.
    #[serde(default)]
    pub prefer: Option<String>,
}

/// What Sidekick can notice, for AI writing skills. Keep in sync with the
/// sensors.
const EVENT_CATALOG: &str = "\
file.download_completed: path, dir, name, ext, kind (image|video|audio|document|archive|installer|code|other), size, size_human, stem, duplicate_of, duplicate_name, signature (installers)
port.listening / port.closed: port, pid, process (lowercase, no .exe), address, url
clipboard.changed: kind (url|json|color|email|path|stack_trace|code|text|secret), preview, text (never for secret), entity (address|phone|date) with address+maps_url, number+whatsapp_url, or when+calendar_url
window.focused: app, exe, title, pid
claude.stop / claude.notification / codex.stop: project, cwd, session, message
claude.permission: id, project, tool, summary, seconds
browser.login_form / browser.long_read / browser.upwork_job: url, domain, title, text, words, tab
browser.site: domain, browser
browser.many_tabs: count, duplicates
system.disk_low: mount, free_human, total_human, percent_free
system.memory_high: percent, process, process_mb
focus.long_session: app, project, minutes, dnd (on, off, unknown)
file.screenshot: path, dir, name, ext, kind, size
dev.unsaved_work: count, names, first, first_path, changed, unpushed
time.day_summary: total_human, top, text
day.morning_brief: headline, text, reviews, first_url, first_title, routine_count, routine_text, item1..item3, offer_auto, auto
calendar.meeting_soon: title, minutes, start, join_url, location, attendees, details, doc_title, doc_url or doc_path (what matches it in history)
calendar.meeting_ended: title, start, start_utc, attendees
dev.repo_opened: name, path, branch, behind, ahead, changed, env_missing, docker_needed, docker_running, deps_needed
dev.stuck: preview, minutes (the same error copied again)
day.back: minutes, app, title, page_title, page_url, recap
time.late_night: time, night_light, dnd (each on, off or unknown)
time.week_summary: total_human, top, text
files.downloads_old: count, mb, loose, installers, summary, dir
system.monitor_connected: monitors
system.battery_low: percent, saver (on or off)
user.idle / user.active: idle_secs / away_secs";

const SKILL_SYSTEM: &str = "You write skills for Sidekick, a desktop assistant on Windows. \
A skill is one YAML file that matches an event and offers the user a few one-click options. \
Reply with one short sentence on what the skill does, then the complete skill in a single \
```yaml code block. Use only the events, fields and actions below. Do not set trust: auto. \
Use a lowercase id starting with \"my.\". Never use em dashes.";

/// The context Ask mode can offer to attach, captured when it opens.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub app: Option<String>,
    pub title: Option<String>,
    pub clipboard_kind: Option<String>,
    pub clipboard_preview: Option<String>,
    /// The clipboard holds something that looks like a secret; never attached.
    pub clipboard_secret: bool,
}

fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

pub fn context(app: &AppHandle) -> Context {
    let state = app.state::<AppState>();
    let window = lock(&state.last_window).clone();
    let mut ctx = Context {
        app: window
            .as_ref()
            .and_then(|w| w["app"].as_str().map(str::to_owned)),
        title: window
            .as_ref()
            .and_then(|w| w["title"].as_str().map(str::to_owned)),
        ..Context::default()
    };
    if let Some(text) = clipboard_text().filter(|t| !t.trim().is_empty()) {
        let kind = clip_kind(&text);
        if kind == ClipKind::Secret {
            ctx.clipboard_secret = true;
        } else {
            ctx.clipboard_kind = Some(kind.as_str().to_owned());
            ctx.clipboard_preview = Some(text.chars().take(160).collect());
        }
    }
    ctx
}

/// Longest page text attached to a chat.
const MAX_PAGE: usize = 20_000;

fn system_prompt(app: &AppHandle, attach: &Attach) -> String {
    if attach.skill {
        return format!(
            "{SKILL_SYSTEM}\n\nEvents and their fields:\n{EVENT_CATALOG}\n\nThe skill format:\n{}",
            sidekick_skills::FORMAT_GUIDE
        );
    }
    let mut system = format!(
        "{SYSTEM}\n\nNow: {}",
        chrono::Local::now().format("%A %-d %B %Y, %H:%M")
    );
    let memory = lock(&app.state::<AppState>().settings).memory.clone();
    if !memory.is_empty() {
        system.push_str("\n\nAbout the user (they asked you to remember):\n");
        for m in &memory {
            system.push_str(&format!("- {m}\n"));
        }
    }
    if attach.speak {
        system.push_str(
            "\n\nThe user asked by voice and your answer is read aloud. Answer in one to three short spoken sentences. No markdown, lists, tables, links or OPTION lines unless they ask for them; if code is needed, keep it to one short block.",
        );
    }
    if let Some(page) = attach.page.as_deref().filter(|p| !p.trim().is_empty()) {
        let clipped: String = page.chars().take(MAX_PAGE).collect();
        system.push_str(&format!(
            "\n\nThe web page the user is asking about:\n```\n{clipped}\n```"
        ));
    }
    let ctx = context(app);
    if attach.window
        && let Some(name) = &ctx.app
    {
        system.push_str(&format!(
            "\n\nThe user was just in {name}, window title: {}",
            ctx.title.as_deref().unwrap_or("(none)")
        ));
    }
    if attach.clipboard
        && !ctx.clipboard_secret
        && let Some(text) = clipboard_text()
    {
        let clipped: String = text.chars().take(MAX_CLIP).collect();
        system.push_str(&format!(
            "\n\nThe user's clipboard ({}):\n```\n{clipped}\n```",
            ctx.clipboard_kind.as_deref().unwrap_or("text")
        ));
    }
    system
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Delta<'a> {
    id: &'a str,
    text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Done {
    id: String,
    provider: Option<String>,
    error: Option<String>,
    /// Why the local model suggests continuing in Claude Code, if it does.
    handoff: Option<String>,
}

/// Starts a streamed chat. Text arrives as `ai://delta`, the end as `ai://done`.
pub fn chat(
    app: &AppHandle,
    id: String,
    mut messages: Vec<Message>,
    attach: Attach,
    local_only: bool,
) {
    // A recipe's name ("Send timesheet", "run Send timesheet") runs its
    // instruction; other questions are counted to offer saving repeats.
    if let Some(last) = messages
        .iter_mut()
        .rev()
        .find(|m| m.role == sidekick_ai::Role::User)
    {
        if let Some(prompt) = crate::recipes::expand(app, &last.content) {
            last.content = prompt;
        } else if !attach.skill {
            crate::recipes::note_question(app, &last.content);
        }
    }
    let cancel = CancellationToken::new();
    lock(&app.state::<AppState>().chats).insert(id.clone(), cancel.clone());
    crate::ask_tools::set_current_chat(&id);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let question = messages
            .iter()
            .rev()
            .find(|m| m.role == sidekick_ai::Role::User)
            .map(|m| m.content.clone());
        let image = if attach.screen {
            match screenshot(&app).await {
                Ok(png) => Some(png),
                Err(err) => {
                    lock(&app.state::<AppState>().chats).remove(&id);
                    let _ = app.emit(
                        DONE_EVENT,
                        Done {
                            id,
                            provider: None,
                            error: Some(format!("Could not capture the screen: {err}")),
                            handoff: None,
                        },
                    );
                    return;
                }
            }
        } else {
            None
        };
        let req = ChatRequest {
            system: system_prompt(&app, &attach),
            messages,
            image,
        };
        let handoff = Arc::new(std::sync::Mutex::new(None));
        // A task with several steps goes to the strongest agent in Auto.
        let prefer = attach.prefer.clone().or_else(|| {
            let q = question.as_deref().unwrap_or_default();
            (!local_only && looks_multistep(q))
                .then(|| strongest_agent(&app))
                .flatten()
        });
        let router = chat_router(&app, &id, &handoff, local_only, prefer.as_deref()).await;
        let speak = attach.speak && crate::voice::begin_answer(&app, &id);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let forward = {
            let app = app.clone();
            let id = id.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(text) = rx.recv().await {
                    if speak {
                        crate::voice::answer_text(&app, &id, &text);
                    }
                    let _ = app.emit(DELTA_EVENT, Delta { id: &id, text });
                }
            })
        };
        let sink = Sink::new(tx);
        let result = router.chat(&req, &sink, &cancel, local_only).await;
        drop(sink);
        let _ = forward.await;
        lock(&app.state::<AppState>().chats).remove(&id);
        if let (Ok(answer), Some(q)) = (&result, &question) {
            crate::search::index_chat(&app, &id, q, &answer.text);
        }
        let handoff = lock(&handoff).take();
        let done = match result {
            Ok(answer) => Done {
                id,
                handoff: handoff.filter(|_| answer.provider == "local"),
                provider: Some(answer.provider),
                error: None,
            },
            Err(err) => Done {
                id,
                provider: None,
                handoff,
                error: Some(match err {
                    sidekick_ai::AiError::NoProvider => no_provider_hint(local_only),
                    other => other.to_string(),
                }),
            },
        };
        if speak {
            crate::voice::answer_done(&app, &done.id, done.error.as_deref());
        }
        let _ = app.emit(DONE_EVENT, done);
    });
}

/// Captures the window the user was in before Sidekick took focus.
pub(crate) async fn screenshot(app: &AppHandle) -> Result<Vec<u8>, String> {
    let pid = lock(&app.state::<AppState>().last_window)
        .as_ref()
        .and_then(|w| w["pid"].as_u64())
        .and_then(|p| u32::try_from(p).ok());
    tokio::task::spawn_blocking(move || crate::screen::capture(pid))
        .await
        .map_err(|e| e.to_string())?
}

fn no_provider_hint(local_only: bool) -> String {
    if local_only {
        "No local model is running. Start Ollama (ollama serve) and pull a model, e.g. ollama pull qwen3:1.7b.".into()
    } else {
        "No AI is set up yet. Install Claude Code and sign in, start Ollama, or set ANTHROPIC_API_KEY. See Settings > AI.".into()
    }
}

pub fn cancel(app: &AppHandle, id: &str) {
    if let Some(token) = lock(&app.state::<AppState>().chats).remove(id) {
        token.cancel();
    }
}

/// "Reply to Ali and attach the invoice", "find X then email it": a request
/// to do several things, not a question.
pub fn looks_multistep(q: &str) -> bool {
    let q = q.to_lowercase();
    let doing = [
        "send", "reply", "email", "book", "schedule", "fill", "post", "attach", "invite", "create",
        "move", "rename", "install", "order", "apply", "message", "forward",
    ];
    let joins = [" and ", " then ", " after that", ", then"];
    let acts = doing.iter().filter(|w| q.contains(*w)).count();
    acts >= 2 || (acts >= 1 && joins.iter().any(|j| q.contains(j)))
}

/// Claude Code, else Codex, when one is turned on.
fn strongest_agent(app: &AppHandle) -> Option<String> {
    let ai = lock(&app.state::<AppState>().settings).ai.clone();
    if ai.claude_code.enabled {
        Some("claude_code".into())
    } else if ai.codex.enabled {
        Some("codex".into())
    } else {
        None
    }
}

/// Stops every answer in progress (the global Stop). True when one was running.
pub fn cancel_all(app: &AppHandle) -> bool {
    let tokens: Vec<_> = lock(&app.state::<AppState>().chats)
        .drain()
        .map(|(_, t)| t)
        .collect();
    for t in &tokens {
        t.cancel();
    }
    !tokens.is_empty()
}

const READY_CHECK_EVERY: Duration = Duration::from_secs(60);

/// Keeps `AppState::ai_ready` current, so AI options only show when an
/// enabled provider can answer.
pub fn watch_readiness(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            refresh_readiness(&app).await;
            tokio::time::sleep(READY_CHECK_EVERY).await;
        }
    });
}

pub async fn refresh_readiness(app: &AppHandle) {
    let ai = lock(&app.state::<AppState>().settings).ai.clone();
    let mut ready = false;
    for p in providers(app, &ai, false) {
        if p.available().await {
            ready = true;
            break;
        }
    }
    app.state::<AppState>()
        .ai_ready
        .store(ready, std::sync::atomic::Ordering::Relaxed);
}
