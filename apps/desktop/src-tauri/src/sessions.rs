//! Agent sessions in the island: Claude Code and Codex working in a
//! project, streamed step by step, with permission questions answered in
//! the island and every change reviewable afterwards (see `review`).
//!
//! Claude Code runs as `claude -p` with stream-json in and out; its
//! permission questions come through Sidekick's MCP server
//! (`--permission-prompt-tool`). Codex runs as `codex app-server` and asks
//! for approvals as JSON-RPC requests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::agents::Agent;
use crate::review::{self, Baseline};
use crate::state::{AppState, lock};

pub const EVENT: &str = "agent://event";
/// An unanswered permission question is declined after this long.
const ASK_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Claude's context window, for the context ring.
const CLAUDE_WINDOW: u64 = 200_000;

/// How much the agent may do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Reads and plans; changes nothing.
    Plan,
    /// Asks before every change and command.
    Ask,
    /// Edits files on its own; asks before commands.
    Edit,
    /// Does everything without asking.
    Full,
}

impl Mode {
    fn claude(self) -> &'static str {
        match self {
            Mode::Plan => "plan",
            Mode::Ask => "default",
            Mode::Edit => "acceptEdits",
            Mode::Full => "bypassPermissions",
        }
    }

    /// Codex sandbox and approval policy.
    fn codex(self) -> (&'static str, &'static str) {
        match self {
            Mode::Plan => ("read-only", "never"),
            Mode::Ask => ("workspace-write", "untrusted"),
            Mode::Edit => ("workspace-write", "on-request"),
            Mode::Full => ("danger-full-access", "never"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub session: String,
    #[serde(flatten)]
    pub data: Value,
}

fn emit(app: &AppHandle, session: &str, data: Value) {
    let _ = app.emit(
        EVENT,
        Event {
            session: session.to_owned(),
            data,
        },
    );
}

enum Cmd {
    Send(String),
    Stop,
}

struct Handle {
    tx: mpsc::UnboundedSender<Cmd>,
    baseline: Option<Baseline>,
    agent: Agent,
    path: PathBuf,
    /// The agent's own session or thread id, for Open in terminal.
    resume: Option<String>,
}

static SESSIONS: Mutex<Option<HashMap<String, Handle>>> = Mutex::new(None);
/// Permission questions waiting for the user: (session, question id).
type Pending = HashMap<String, (String, oneshot::Sender<Answer>)>;
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
/// Claude tool_use ids seen per session, to know whose question it is.
static TOOL_OWNER: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn with<T, R>(m: &Mutex<Option<T>>, f: impl FnOnce(&mut T) -> R) -> R
where
    T: Default,
{
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(T::default))
}

/// The user's answer to a permission question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Answer {
    Allow,
    /// Allow this and the same kind of thing for the rest of the session.
    Always,
    Deny,
}

/// What `agent_start` returns.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub id: String,
    pub agent: &'static str,
    pub project: String,
    pub branch: Option<String>,
    /// Changes can be reviewed and undone (the project uses git).
    pub reviewable: bool,
}

/// Starts an agent in `path` with `prompt`.
pub async fn start(
    app: &AppHandle,
    agent: Agent,
    path: &Path,
    prompt: &str,
    mode: Mode,
) -> Result<Started, String> {
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    let settings = lock(&app.state::<AppState>().settings).clone();
    let exe = agent
        .resolve(&settings)
        .ok_or_else(|| format!("{} is not installed", agent.name()))?;
    let id = ulid::Ulid::new().to_string();
    let baseline = {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || review::snapshot(&path))
            .await
            .ok()
            .flatten()
    };
    let branch = branch(path);
    let (tx, rx) = mpsc::unbounded_channel();
    let started = Started {
        id: id.clone(),
        agent: agent.name(),
        project: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        branch,
        reviewable: baseline.is_some(),
    };
    with(&SESSIONS, |s| {
        s.insert(
            id.clone(),
            Handle {
                tx: tx.clone(),
                baseline,
                agent,
                path: path.to_owned(),
                resume: None,
            },
        )
    });
    let _ = tx.send(Cmd::Send(prompt.to_owned()));
    let (app2, id2, path2) = (app.clone(), id.clone(), path.to_owned());
    let model = match agent {
        Agent::ClaudeCode => Some(settings.ai.claude_code.model.clone()),
        Agent::Codex => Some(settings.ai.codex.model.clone()),
    }
    .filter(|m| !m.trim().is_empty());
    let mcp_config = app
        .state::<AppState>()
        .ai_workdir
        .join(crate::mcp::CONFIG_FILE);
    tauri::async_runtime::spawn(async move {
        let result = match agent {
            Agent::ClaudeCode => {
                run_claude(&app2, &id2, &exe, &path2, mode, model, &mcp_config, rx).await
            }
            Agent::Codex => run_codex(&app2, &id2, &exe, &path2, mode, model, rx).await,
        };
        let changes = review_count(&id2);
        emit(
            &app2,
            &id2,
            json!({ "kind": "ended", "error": result.err(), "changes": changes }),
        );
    });
    Ok(started)
}

fn branch(path: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|b| !b.is_empty())
}

fn review_count(id: &str) -> usize {
    let baseline = with(&SESSIONS, |s| s.get(id).and_then(|h| h.baseline.clone()));
    baseline
        .and_then(|b| review::changes(&b).ok())
        .map_or(0, |c| c.len())
}

/// Sends a follow-up or a steer to a running session.
pub fn send(id: &str, text: &str) -> Result<(), String> {
    with(&SESSIONS, |s| {
        s.get(id)
            .ok_or_else(|| "That session has ended.".to_owned())
            .and_then(|h| {
                h.tx.send(Cmd::Send(text.to_owned()))
                    .map_err(|_| "That session has ended.".to_owned())
            })
    })
}

pub fn stop(id: &str) {
    with(&SESSIONS, |s| {
        if let Some(h) = s.get(id) {
            let _ = h.tx.send(Cmd::Stop);
        }
    });
}

/// Answers a permission question.
pub fn answer(question: &str, answer: Answer) -> Result<(), String> {
    let tx = with(&PENDING, |p| p.remove(question))
        .ok_or("That question was already answered.")?
        .1;
    tx.send(answer)
        .map_err(|_| "The agent stopped waiting.".into())
}

fn baseline(id: &str) -> Result<Baseline, String> {
    with(&SESSIONS, |s| s.get(id).and_then(|h| h.baseline.clone()))
        .ok_or_else(|| "Changes can only be reviewed in a git project.".to_owned())
}

pub fn changes(id: &str) -> Result<Vec<review::FileChange>, String> {
    review::changes(&baseline(id)?)
}

pub fn undo(id: &str, path: Option<&str>, hunk: Option<usize>) -> Result<(), String> {
    let b = baseline(id)?;
    match (path, hunk) {
        (Some(p), Some(h)) => review::undo_hunk(&b, p, h),
        (Some(p), None) => review::undo_file(&b, p),
        (None, _) => review::undo_all(&b).map(|_| ()),
    }
}

/// Closes a session: stops the agent and forgets its review.
pub fn close(id: &str) {
    stop(id);
    with(&SESSIONS, |s| s.remove(id));
}

/// Opens the session in a terminal, to carry on in the CLI itself.
pub fn open_terminal(app: &AppHandle, id: &str) -> Result<(), String> {
    let (agent, path, resume) = with(&SESSIONS, |s| {
        s.get(id)
            .map(|h| (h.agent, h.path.clone(), h.resume.clone()))
    })
    .ok_or("That session has ended.")?;
    let settings = lock(&app.state::<AppState>().settings).clone();
    let exe = agent
        .resolve(&settings)
        .ok_or_else(|| format!("{} is not installed", agent.name()))?;
    let args: Vec<String> = match (agent, resume) {
        (Agent::ClaudeCode, Some(r)) => vec!["--resume".into(), r],
        (Agent::Codex, Some(r)) => vec!["resume".into(), r],
        _ => Vec::new(),
    };
    crate::agents::open_terminal(&path, &exe, &args)
}

/// Asks the user and waits for the answer (Deny after a while).
async fn ask(app: &AppHandle, session: &str, label: &str, detail: &str) -> Answer {
    let question = ulid::Ulid::new().to_string();
    let (tx, rx) = oneshot::channel();
    with(&PENDING, |p| {
        p.insert(question.clone(), (session.to_owned(), tx))
    });
    emit(
        app,
        session,
        json!({ "kind": "ask", "question": question, "label": label, "detail": detail }),
    );
    let answer = tokio::time::timeout(ASK_TIMEOUT, rx)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(Answer::Deny);
    with(&PENDING, |p| p.remove(&question));
    emit(
        app,
        session,
        json!({ "kind": "answered", "question": question }),
    );
    answer
}

// ---------- Claude Code ----------

/// What Claude Code's permission prompt tool gets: called by the CLI (not
/// the model) through Sidekick's MCP server.
pub async fn claude_permission(app: &AppHandle, args: &Value) -> String {
    let tool = args["tool_name"].as_str().unwrap_or("a tool");
    let input = &args["input"];
    let owner = args["tool_use_id"]
        .as_str()
        .and_then(|t| with(&TOOL_OWNER, |o| o.get(t).cloned()))
        .or_else(|| with(&SESSIONS, |s| s.keys().last().cloned()));
    let Some(session) = owner else {
        return json!({ "behavior": "deny", "message": "No Sidekick session is waiting for this." })
            .to_string();
    };
    let (label, detail) = describe_tool(tool, input);
    match ask(app, &session, &label, &detail).await {
        Answer::Allow | Answer::Always => {
            json!({ "behavior": "allow", "updatedInput": input }).to_string()
        }
        Answer::Deny => {
            json!({ "behavior": "deny", "message": "The user said no. Ask them what to do instead." })
                .to_string()
        }
    }
}

/// A tool call in words: ("Edit src/main.rs", "3 lines").
pub fn describe_tool(tool: &str, input: &Value) -> (String, String) {
    let s = |k: &str| input[k].as_str().unwrap_or_default().to_owned();
    let file = || {
        let p = s("file_path");
        let p = if p.is_empty() { s("path") } else { p };
        p.rsplit(['/', '\\']).next().unwrap_or_default().to_owned()
    };
    match tool {
        "Read" => (format!("Read {}", file()), String::new()),
        "Edit" | "MultiEdit" => (format!("Edit {}", file()), String::new()),
        "Write" => (format!("Write {}", file()), String::new()),
        "Bash" => {
            let cmd = s("command");
            ("Run a command".into(), cmd.chars().take(160).collect())
        }
        "Grep" => (format!("Search for {}", s("pattern")), String::new()),
        "Glob" => (format!("Find files {}", s("pattern")), String::new()),
        "WebFetch" => (format!("Read {}", s("url")), String::new()),
        "WebSearch" => (format!("Search the web for {}", s("query")), String::new()),
        "TodoWrite" => ("Update the plan".into(), String::new()),
        other => (
            other
                .trim_start_matches("mcp__")
                .replace("__", " ")
                .replace('_', " "),
            String::new(),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_claude(
    app: &AppHandle,
    id: &str,
    exe: &Path,
    path: &Path,
    mode: Mode,
    model: Option<String>,
    mcp_config: &Path,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-mode",
        mode.claude(),
    ]);
    if let Some(m) = model {
        cmd.args(["--model", &m]);
    }
    if mcp_config.is_file() {
        cmd.arg("--mcp-config").arg(mcp_config);
        if mode != Mode::Full {
            cmd.args([
                "--permission-prompt-tool",
                "mcp__sidekick__sidekick_permission",
            ]);
        }
    }
    cmd.current_dir(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    crate::agents::hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start Claude Code: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let mut lines = BufReader::new(child.stdout.take().ok_or("no stdout")?).lines();
    loop {
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(Cmd::Send(text)) => {
                    let mut line = json!({
                        "type": "user",
                        "message": { "role": "user", "content": [{ "type": "text", "text": text }] },
                    })
                    .to_string();
                    line.push('\n');
                    stdin.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
                    stdin.flush().await.map_err(|e| e.to_string())?;
                    emit(app, id, json!({ "kind": "working" }));
                }
                Some(Cmd::Stop) | None => {
                    let _ = child.kill().await;
                    return Ok(());
                }
            },
            line = lines.next_line() => {
                let Ok(Some(line)) = line else {
                    return Err("Claude Code stopped.".into());
                };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                for data in claude_events(&v) {
                    if data["kind"] == "session" {
                        let resume = data["resume"].as_str().map(str::to_owned);
                        with(&SESSIONS, |s| if let Some(h) = s.get_mut(id) { h.resume = resume });
                        continue;
                    }
                    if data["kind"] == "step" && data["state"] == "running"
                        && let Some(t) = data["id"].as_str()
                    {
                        with(&TOOL_OWNER, |o| o.insert(t.to_owned(), id.to_owned()));
                    }
                    emit(app, id, data);
                }
            }
        }
    }
}

/// What one stream-json line means for the island.
pub fn claude_events(v: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    match v["type"].as_str() {
        Some("system") if v["subtype"] == "init" => {
            if let Some(s) = v["session_id"].as_str() {
                out.push(json!({ "kind": "session", "resume": s }));
            }
        }
        Some("stream_event") => {
            let e = &v["event"];
            if e["type"] == "content_block_delta" && e["delta"]["type"] == "text_delta" {
                out.push(json!({ "kind": "text", "text": e["delta"]["text"] }));
            }
        }
        Some("assistant") => {
            for b in v["message"]["content"].as_array().into_iter().flatten() {
                if b["type"] != "tool_use" {
                    continue;
                }
                let name = b["name"].as_str().unwrap_or_default();
                if name == "TodoWrite" {
                    let items: Vec<Value> = b["input"]["todos"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|t| json!({ "text": t["content"], "status": t["status"] }))
                        .collect();
                    out.push(json!({ "kind": "plan", "items": items }));
                    continue;
                }
                let (label, detail) = describe_tool(name, &b["input"]);
                out.push(json!({
                    "kind": "step", "id": b["id"], "tool": name, "label": label,
                    "detail": detail, "state": "running",
                }));
            }
        }
        Some("user") => {
            for b in v["message"]["content"].as_array().into_iter().flatten() {
                if b["type"] == "tool_result" {
                    let failed = b["is_error"].as_bool().unwrap_or(false);
                    out.push(json!({
                        "kind": "step", "id": b["tool_use_id"],
                        "state": if failed { "failed" } else { "done" },
                    }));
                }
            }
        }
        Some("result") => {
            let u = &v["usage"];
            let used = [
                "input_tokens",
                "cache_read_input_tokens",
                "cache_creation_input_tokens",
            ]
            .iter()
            .filter_map(|k| u[k].as_u64())
            .sum::<u64>();
            if used > 0 {
                out.push(json!({ "kind": "usage", "used": used, "window": CLAUDE_WINDOW }));
            }
            let failed = v["is_error"].as_bool().unwrap_or(false);
            out.push(json!({
                "kind": "turn",
                "error": if failed { v["result"].clone() } else { Value::Null },
            }));
        }
        _ => {}
    }
    out
}

// ---------- Codex ----------

async fn write_line(stdin: &mut tokio::process::ChildStdin, v: Value) -> Result<(), String> {
    let mut line = v.to_string();
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())
}

async fn run_codex(
    app: &AppHandle,
    id: &str,
    exe: &Path,
    path: &Path,
    mode: Mode,
    model: Option<String>,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.arg("app-server")
        .current_dir(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    crate::agents::hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start Codex: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let mut lines = BufReader::new(child.stdout.take().ok_or("no stdout")?).lines();
    let mut next_id = 0u64;
    next_id += 1;
    write_line(
        &mut stdin,
        json!({ "id": next_id, "method": "initialize",
                "params": { "clientInfo": { "name": "sidekick", "version": env!("CARGO_PKG_VERSION") } } }),
    )
    .await?;
    write_line(&mut stdin, json!({ "method": "initialized" })).await?;
    let (sandbox, approval) = mode.codex();
    let mut params =
        json!({ "cwd": path.to_string_lossy(), "sandbox": sandbox, "approvalPolicy": approval });
    if let Some(m) = model {
        params["model"] = json!(m);
    }
    next_id += 1;
    let thread_req = next_id;
    write_line(
        &mut stdin,
        json!({ "id": thread_req, "method": "thread/start", "params": params }),
    )
    .await?;
    let mut thread: Option<String> = None;
    let mut turn: Option<String> = None;
    // Messages typed before the thread opened.
    let mut queued: Vec<String> = Vec::new();
    loop {
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(Cmd::Send(text)) => {
                    let Some(t) = thread.clone() else { queued.push(text); continue };
                    next_id += 1;
                    let input = json!([{ "type": "text", "text": text }]);
                    let msg = match &turn {
                        Some(active) => json!({ "id": next_id, "method": "turn/steer",
                            "params": { "threadId": t, "expectedTurnId": active, "input": input } }),
                        None => json!({ "id": next_id, "method": "turn/start",
                            "params": { "threadId": t, "input": input } }),
                    };
                    write_line(&mut stdin, msg).await?;
                    emit(app, id, json!({ "kind": "working" }));
                }
                Some(Cmd::Stop) | None => {
                    let _ = child.kill().await;
                    return Ok(());
                }
            },
            line = lines.next_line() => {
                let Ok(Some(line)) = line else {
                    return Err("Codex stopped.".into());
                };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                if v["id"] == thread_req && v.get("method").is_none() {
                    if let Some(err) = v["error"]["message"].as_str() {
                        return Err(err.to_owned());
                    }
                    let t = v["result"]["thread"]["id"].as_str().map(str::to_owned);
                    with(&SESSIONS, |s| if let Some(h) = s.get_mut(id) { h.resume = t.clone() });
                    thread = t;
                    for text in queued.drain(..) {
                        next_id += 1;
                        write_line(&mut stdin, json!({ "id": next_id, "method": "turn/start",
                            "params": { "threadId": thread, "input": [{ "type": "text", "text": text }] } })).await?;
                    }
                    continue;
                }
                // Approval requests: ask in the island, answer Codex.
                if let (Some(method), Some(rid)) = (v["method"].as_str(), v.get("id"))
                    && method.ends_with("requestApproval")
                {
                    let p = &v["params"];
                    let (label, detail) = if method.contains("commandExecution") {
                        ("Run a command".to_owned(), p["command"].as_str().unwrap_or_default().to_owned())
                    } else {
                        ("Change files".to_owned(), p["reason"].as_str().unwrap_or_default().to_owned())
                    };
                    let decision = match ask(app, id, &label, &detail).await {
                        Answer::Allow => "accept",
                        Answer::Always => "acceptForSession",
                        Answer::Deny => "decline",
                    };
                    write_line(&mut stdin, json!({ "id": rid, "result": { "decision": decision } })).await?;
                    continue;
                }
                if v["method"] == "turn/started" {
                    turn = v["params"]["turn"]["id"].as_str().map(str::to_owned);
                }
                if v["method"] == "turn/completed" {
                    turn = None;
                }
                for data in codex_events(&v) {
                    emit(app, id, data);
                }
            }
        }
    }
}

/// What one app-server notification means for the island.
pub fn codex_events(v: &Value) -> Vec<Value> {
    let p = &v["params"];
    let item = &p["item"];
    let mut out = Vec::new();
    match v["method"].as_str() {
        Some("item/agentMessage/delta") => out.push(json!({ "kind": "text", "text": p["delta"] })),
        Some(m @ ("item/started" | "item/completed")) => {
            let done = m == "item/completed";
            let (tool, label, detail) = match item["type"].as_str() {
                Some("commandExecution") => (
                    "Bash",
                    "Run a command".to_owned(),
                    item["command"].as_str().unwrap_or_default().to_owned(),
                ),
                Some("fileChange") => {
                    let files: Vec<String> = item["changes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|c| c["path"].as_str())
                        .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_owned())
                        .collect();
                    ("Edit", format!("Edit {}", files.join(", ")), String::new())
                }
                Some("mcpToolCall") => (
                    "Mcp",
                    item["tool"].as_str().unwrap_or("tool").replace('_', " "),
                    String::new(),
                ),
                Some("webSearch") => (
                    "WebSearch",
                    format!(
                        "Search the web for {}",
                        item["query"].as_str().unwrap_or_default()
                    ),
                    String::new(),
                ),
                _ => return out,
            };
            let failed = done
                && (item["status"] == "failed"
                    || item["exitCode"].as_i64().is_some_and(|c| c != 0));
            out.push(json!({
                "kind": "step", "id": item["id"], "tool": tool, "label": label, "detail": detail,
                "state": if !done { "running" } else if failed { "failed" } else { "done" },
            }));
        }
        Some("turn/plan/updated") => {
            let items: Vec<Value> = p["plan"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|s| {
                    let status = match s["status"].as_str() {
                        Some("inProgress") => "in_progress",
                        Some(other) => other,
                        None => "pending",
                    };
                    json!({ "text": s["step"], "status": status })
                })
                .collect();
            out.push(json!({ "kind": "plan", "items": items }));
        }
        Some("thread/tokenUsage/updated") => {
            let u = &p["tokenUsage"];
            let used = u["last"]["inputTokens"].as_u64().unwrap_or(0);
            let window = u["modelContextWindow"].as_u64().unwrap_or(0);
            if used > 0 && window > 0 {
                out.push(json!({ "kind": "usage", "used": used, "window": window }));
            }
        }
        Some("turn/completed") => {
            let failed = p["turn"]["status"] == "failed";
            out.push(json!({
                "kind": "turn",
                "error": if failed { p["turn"]["error"]["message"].clone() } else { Value::Null },
            }));
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_claude_steps_plans_and_turns() {
        let tool: Value = serde_json::from_str(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ok"},{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"C:\\code\\app\\src\\main.rs"}}]}}"#,
        )
        .unwrap();
        let ev = claude_events(&tool);
        assert_eq!(ev[0]["label"], "Edit main.rs");
        assert_eq!(ev[0]["state"], "running");

        let todo: Value = serde_json::from_str(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t2","name":"TodoWrite","input":{"todos":[{"content":"Add retries","status":"in_progress"}]}}]}}"#,
        )
        .unwrap();
        assert_eq!(claude_events(&todo)[0]["items"][0]["text"], "Add retries");

        let result: Value = serde_json::from_str(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","is_error":true}]}}"#,
        )
        .unwrap();
        assert_eq!(claude_events(&result)[0]["state"], "failed");

        let done: Value = serde_json::from_str(
            r#"{"type":"result","is_error":false,"result":"x","usage":{"input_tokens":10,"cache_read_input_tokens":90}}"#,
        )
        .unwrap();
        let ev = claude_events(&done);
        assert_eq!(ev[0]["used"], 100);
        assert_eq!(ev[1]["kind"], "turn");
        assert!(ev[1]["error"].is_null());
    }

    #[test]
    fn reads_codex_items_and_plans() {
        let cmd: Value = serde_json::from_str(
            r#"{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"i1","command":"cargo test","exitCode":1,"status":"completed"}}}"#,
        )
        .unwrap();
        let ev = codex_events(&cmd);
        assert_eq!(ev[0]["detail"], "cargo test");
        assert_eq!(ev[0]["state"], "failed");

        let plan: Value = serde_json::from_str(
            r#"{"method":"turn/plan/updated","params":{"plan":[{"step":"Read","status":"completed"},{"step":"Fix","status":"inProgress"}]}}"#,
        )
        .unwrap();
        let ev = codex_events(&plan);
        assert_eq!(ev[0]["items"][1]["status"], "in_progress");
    }

    #[test]
    fn describes_tools_in_words() {
        assert_eq!(
            describe_tool("Bash", &json!({ "command": "npm test" })),
            ("Run a command".into(), "npm test".into())
        );
        assert_eq!(
            describe_tool("mcp__sidekick__sidekick_search", &json!({})).0,
            "sidekick sidekick search"
        );
    }

    #[test]
    fn modes_map_to_each_cli() {
        assert_eq!(Mode::Edit.claude(), "acceptEdits");
        assert_eq!(Mode::Plan.codex(), ("read-only", "never"));
        assert_eq!(Mode::Ask.codex().1, "untrusted");
    }
}
