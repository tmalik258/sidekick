//! Agent sessions in the island: Claude Code and Codex working in a
//! project, streamed step by step, with permission questions answered in
//! the island and every change reviewable afterwards (see `review`).
//!
//! Claude Code runs as `claude -p` with stream-json in and out; its
//! permission questions come through Sidekick's MCP server
//! (`--permission-prompt-tool`). Codex runs as `codex app-server` and asks
//! for approvals as JSON-RPC requests.

use sidekick_sensors::classify::mask;
use std::borrow::Cow;
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
    mode: Mode,
    /// The agent's own session or thread id, for Open in terminal and Resume.
    resume: Option<String>,
    /// The project before each of the user's messages, for Rewind.
    checkpoints: Vec<Baseline>,
    /// Each changed file's fingerprint when the agent last finished, so an
    /// undo never overwrites edits made after that.
    after: HashMap<String, u64>,
}

/// What is kept on disk for each session, so Review, Undo and Resume work
/// after Sidekick restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    id: String,
    agent: String,
    path: PathBuf,
    mode: Mode,
    resume: Option<String>,
    baseline: Option<Baseline>,
    #[serde(default)]
    checkpoints: Vec<Baseline>,
    #[serde(default)]
    after: HashMap<String, u64>,
}

const SAVED_FILE: &str = "agent-sessions.json";
static SAVE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Writes every session to disk (called after anything that matters changes).
fn save() {
    let Some(dir) = SAVE_DIR.lock().ok().and_then(|d| d.clone()) else {
        return;
    };
    let saved: Vec<Saved> = with(&SESSIONS, |s| {
        s.iter()
            .map(|(id, h)| Saved {
                id: id.clone(),
                agent: match h.agent {
                    Agent::ClaudeCode => "claude_code".into(),
                    Agent::Codex => "codex".into(),
                },
                path: h.path.clone(),
                mode: h.mode,
                resume: h.resume.clone(),
                baseline: h.baseline.clone(),
                checkpoints: h.checkpoints.clone(),
                after: h.after.clone(),
            })
            .collect()
    });
    if let Ok(bytes) = serde_json::to_vec(&saved) {
        let _ = std::fs::write(dir.join(SAVED_FILE), bytes);
    }
}

/// Loads the sessions from before a restart; they can be reviewed, undone and
/// resumed, but nothing runs until the user resumes one.
pub fn restore(dir: &Path) {
    if let Ok(mut d) = SAVE_DIR.lock() {
        *d = Some(dir.to_owned());
    }
    let Ok(bytes) = std::fs::read(dir.join(SAVED_FILE)) else {
        return;
    };
    let saved: Vec<Saved> = serde_json::from_slice(&bytes).unwrap_or_default();
    with(&SESSIONS, |s| {
        for v in saved {
            let agent = if v.agent == "codex" {
                Agent::Codex
            } else {
                Agent::ClaudeCode
            };
            // Nothing is running: sends fail with "That session has ended".
            let (tx, _) = mpsc::unbounded_channel();
            s.entry(v.id).or_insert(Handle {
                tx,
                baseline: v.baseline,
                agent,
                path: v.path,
                mode: v.mode,
                resume: v.resume,
                checkpoints: v.checkpoints,
                after: v.after,
            });
        }
    });
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
                checkpoints: baseline.iter().cloned().collect(),
                baseline,
                agent,
                path: path.to_owned(),
                mode,
                resume: None,
                after: HashMap::new(),
            },
        )
    });
    save();
    let _ = tx.send(Cmd::Send(prompt.to_owned()));
    spawn_run(app, &id, agent, exe, path, mode, None, rx);
    Ok(started)
}

/// Runs the agent's process for session `id` until it ends; `resume` carries
/// on the agent's own earlier session.
#[allow(clippy::too_many_arguments)]
fn spawn_run(
    app: &AppHandle,
    id: &str,
    agent: Agent,
    exe: PathBuf,
    path: &Path,
    mode: Mode,
    resume: Option<String>,
    rx: mpsc::UnboundedReceiver<Cmd>,
) {
    let settings = lock(&app.state::<AppState>().settings).clone();
    let (app2, id2, path2) = (app.clone(), id.to_owned(), path.to_owned());
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
                run_claude(
                    &app2,
                    &id2,
                    &exe,
                    &path2,
                    mode,
                    model,
                    &mcp_config,
                    resume,
                    rx,
                )
                .await
            }
            Agent::Codex => run_codex(&app2, &id2, &exe, &path2, mode, model, resume, rx).await,
        };
        let changes = review_count(&id2);
        emit(
            &app2,
            &id2,
            json!({ "kind": "ended", "error": result.err(), "changes": changes }),
        );
    });
}

/// Carries on a session that ended or was cut off by a restart, in the
/// agent's own session, with the same review baseline.
pub fn resume(app: &AppHandle, id: &str) -> Result<(), String> {
    let (agent, path, mode, resume, running) = with(&SESSIONS, |s| {
        s.get(id).map(|h| {
            (
                h.agent,
                h.path.clone(),
                h.mode,
                h.resume.clone(),
                !h.tx.is_closed(),
            )
        })
    })
    .ok_or("That session is gone.")?;
    if running {
        return Ok(());
    }
    let resume = resume.ok_or("This session ended before it started, so it cannot be resumed.")?;
    let settings = lock(&app.state::<AppState>().settings).clone();
    let exe = agent
        .resolve(&settings)
        .ok_or_else(|| format!("{} is not installed", agent.name()))?;
    let (tx, rx) = mpsc::unbounded_channel();
    with(&SESSIONS, |s| {
        if let Some(h) = s.get_mut(id) {
            h.tx = tx;
        }
    });
    spawn_run(app, id, agent, exe, &path, mode, Some(resume), rx);
    Ok(())
}

/// When the agent finishes a turn: fingerprint what it changed, so an undo
/// later can tell the user's own edits apart.
fn turn_ended(id: &str) {
    let Some(b) = with(&SESSIONS, |s| s.get(id).and_then(|h| h.baseline.clone())) else {
        return;
    };
    let prints: HashMap<String, u64> = review::changes(&b)
        .unwrap_or_default()
        .iter()
        .filter_map(|f| review::fingerprint(&b.root, &f.path).map(|h| (f.path.clone(), h)))
        .collect();
    with(&SESSIONS, |s| {
        if let Some(h) = s.get_mut(id) {
            h.after = prints;
        }
    });
    save();
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

/// Sends a follow-up or a steer to a running session, after a checkpoint
/// of the project so Rewind can come back to this point.
pub fn send(id: &str, text: &str) -> Result<(), String> {
    let path = with(&SESSIONS, |s| s.get(id).map(|h| h.path.clone()));
    if let Some(snap) = path.as_deref().and_then(review::snapshot) {
        with(&SESSIONS, |s| {
            if let Some(h) = s.get_mut(id) {
                h.checkpoints.push(snap);
            }
        });
        save();
    }
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

/// The changes to review, with secrets in them masked. Undo reads the files
/// again, so masking here never changes what gets undone.
pub fn changes(id: &str) -> Result<Vec<review::FileChange>, String> {
    let mut files = review::changes(&baseline(id)?)?;
    for line in files
        .iter_mut()
        .flat_map(|f| f.hunks.iter_mut())
        .flat_map(|h| h.lines.iter_mut())
    {
        if let Cow::Owned(m) = mask(line) {
            *line = m;
        }
    }
    Ok(files)
}

pub fn undo(id: &str, path: Option<&str>, hunk: Option<usize>) -> Result<(), String> {
    let b = baseline(id)?;
    let after = with(&SESSIONS, |s| s.get(id).map(|h| h.after.clone())).unwrap_or_default();
    // A file edited after the agent finished: undo would lose those edits.
    let edited_since = |p: &str| {
        after
            .get(p)
            .is_some_and(|h| review::fingerprint(&b.root, p) != Some(*h))
    };
    let refuse = |p: &str| {
        format!(
            "{p} changed after the agent finished. Undoing now would lose those edits, so it was left as it is."
        )
    };
    match (path, hunk) {
        (Some(p), _) if edited_since(p) => return Err(refuse(p)),
        (None, _) => {
            if let Some(p) = after.keys().find(|p| edited_since(p)) {
                return Err(refuse(p));
            }
        }
        _ => {}
    }
    match (path, hunk) {
        (Some(p), Some(h)) => review::undo_hunk(&b, p, h)?,
        (Some(p), None) => review::undo_file(&b, p)?,
        (None, _) => {
            review::undo_all(&b)?;
        }
    }
    // What is on disk now is Sidekick's own doing: the new fingerprint.
    with(&SESSIONS, |s| {
        if let Some(h) = s.get_mut(id) {
            let keys: Vec<String> = match path {
                Some(p) => vec![p.to_owned()],
                None => h.after.keys().cloned().collect(),
            };
            for k in keys {
                match review::fingerprint(&b.root, &k) {
                    Some(f) => h.after.insert(k, f),
                    None => h.after.remove(&k),
                };
            }
        }
    });
    save();
    Ok(())
}

/// How many files Rewind to before message `index` would put back.
pub fn rewind_preview(id: &str, index: usize) -> Result<usize, String> {
    let cp = checkpoint(id, index)?;
    Ok(review::changes(&cp)?.len())
}

/// Puts the project back to how it was before the user's message `index`
/// (0 is the first), and forgets the checkpoints after it.
pub fn rewind(id: &str, index: usize) -> Result<usize, String> {
    let cp = checkpoint(id, index)?;
    let n = review::undo_all(&cp)?;
    with(&SESSIONS, |s| {
        if let Some(h) = s.get_mut(id) {
            h.checkpoints.truncate(index + 1);
        }
    });
    turn_ended(id);
    Ok(n)
}

fn checkpoint(id: &str, index: usize) -> Result<Baseline, String> {
    with(&SESSIONS, |s| {
        s.get(id).and_then(|h| h.checkpoints.get(index).cloned())
    })
    .ok_or_else(|| "Rewind needs a git project.".to_owned())
}

/// Files in the session's project whose path has every word of `query`,
/// shortest paths first, for @ in the composer.
pub fn files(id: &str, query: &str) -> Vec<String> {
    let Some(root) = with(&SESSIONS, |s| s.get(id).map(|h| h.path.clone())) else {
        return Vec::new();
    };
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let mut cmd = std::process::Command::new("git");
    cmd.args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(&root);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let listed = cmd
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let all: Vec<String> = match listed {
        Some(text) => text.lines().map(str::to_owned).collect(),
        None => crate::find::find(
            std::slice::from_ref(&root),
            query,
            Some(false),
            std::time::Duration::from_millis(300),
        )
        .into_iter()
        .filter_map(|f| {
            f.path
                .strip_prefix(&root)
                .ok()
                .map(|p| p.display().to_string())
        })
        .collect(),
    };
    let mut hits: Vec<String> = all
        .into_iter()
        .filter(|p| {
            let l = p.to_lowercase();
            words.iter().all(|w| l.contains(w.as_str()))
        })
        .collect();
    // A match in the file name beats one in a folder name; then shorter paths.
    let name_hit = |p: &str| {
        let name = p.rsplit('/').next().unwrap_or(p).to_lowercase();
        !words.iter().all(|w| name.contains(w.as_str()))
    };
    hits.sort_by_key(|p| (name_hit(p), p.len()));
    hits.truncate(8);
    hits
}

/// A command for / in the composer.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub group: String,
}

/// The session's own commands, then the project's from .claude/commands.
pub fn commands(id: &str) -> Vec<SlashCommand> {
    let Some((agent, path)) = with(&SESSIONS, |s| s.get(id).map(|h| (h.agent, h.path.clone())))
    else {
        return Vec::new();
    };
    let cmd = |name: &str, description: &str, group: &str| SlashCommand {
        name: name.into(),
        description: description.into(),
        group: group.into(),
    };
    let mut out = Vec::new();
    if agent == Agent::ClaudeCode {
        out.push(cmd(
            "/compact",
            "Summarize the chat to free context",
            "Session",
        ));
    }
    out.push(cmd("/clear", "Start fresh in the same project", "Session"));
    out.push(cmd(
        "/rewind",
        "Go back to an earlier message, code included",
        "Session",
    ));
    if agent == Agent::ClaudeCode {
        out.extend(project_commands(&path.join(".claude").join("commands")));
    }
    out
}

/// Markdown files in a commands folder: the file name is the command, the
/// frontmatter description (or first line) says what it does.
pub fn project_commands(dir: &Path) -> Vec<SlashCommand> {
    let mut out: Vec<SlashCommand> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .filter_map(|e| {
            let name = e.path().file_stem()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(e.path()).unwrap_or_default();
            let description = text
                .lines()
                .find_map(|l| {
                    l.strip_prefix("description:")
                        .map(|d| d.trim().trim_matches('"').to_owned())
                })
                .or_else(|| {
                    text.lines()
                        .map(|l| l.trim().trim_start_matches('#').trim())
                        .find(|l| !l.is_empty() && *l != "---")
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            Some(SlashCommand {
                name: format!("/{name}"),
                description: format!("{description} (.claude/commands/{name}.md)")
                    .trim()
                    .to_owned(),
                group: "This project".into(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Closes a session: stops the agent and forgets its review.
pub fn close(id: &str) {
    stop(id);
    with(&SESSIONS, |s| s.remove(id));
    save();
}

/// The session's project folder, to open in the user's editor.
pub fn project(id: &str) -> Option<PathBuf> {
    with(&SESSIONS, |s| s.get(id).map(|h| h.path.clone()))
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
        json!({ "kind": "ask", "question": question, "label": label, "detail": mask(detail) }),
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
    resume: Option<String>,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(exe);
    if let Some(r) = &resume {
        cmd.args(["--resume", r]);
    }
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
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::agents::hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start Claude Code: {e}"))?;
    let stderr = keep_stderr(&mut child);
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
                    let _ = child.wait().await;
                    return Err(why_stopped("Claude Code", &lock(&stderr)));
                };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                for data in claude_events(&v) {
                    if data["kind"] == "session" {
                        let resume = data["resume"].as_str().map(str::to_owned);
                        with(&SESSIONS, |s| if let Some(h) = s.get_mut(id) { h.resume = resume });
                        save();
                        continue;
                    }
                    if data["kind"] == "step" && data["state"] == "running"
                        && let Some(t) = data["id"].as_str()
                    {
                        with(&TOOL_OWNER, |o| o.insert(t.to_owned(), id.to_owned()));
                    }
                    if data["kind"] == "turn" {
                        let id = id.to_owned();
                        tokio::task::spawn_blocking(move || turn_ended(&id));
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
                    let text = match &b["content"] {
                        Value::String(t) => t.clone(),
                        Value::Array(parts) => parts
                            .iter()
                            .filter_map(|p| p["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        _ => String::new(),
                    };
                    out.push(json!({
                        "kind": "step", "id": b["tool_use_id"],
                        "state": if failed { "failed" } else { "done" },
                        "output": tail(&text),
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

/// The end of a command's output, for the folded block under a step.
/// The end of a CLI's error output, read as it comes so a full pipe never
/// blocks it.
fn keep_stderr(child: &mut tokio::process::Child) -> std::sync::Arc<Mutex<String>> {
    let kept = std::sync::Arc::new(Mutex::new(String::new()));
    if let Some(err) = child.stderr.take() {
        let kept = kept.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut k = lock(&kept);
                k.push_str(&line);
                k.push('\n');
                if k.len() > 4000 {
                    let cut = k.len() - 2000;
                    let cut = (cut..k.len()).find(|i| k.is_char_boundary(*i)).unwrap_or(0);
                    k.drain(..cut);
                }
            }
        });
    }
    kept
}

/// Why an agent stopped, in words: a CLI too old for a flag or method
/// Sidekick uses says how to update it.
pub fn why_stopped(agent: &str, stderr: &str) -> String {
    let e = stderr.to_lowercase();
    let old = [
        "unknown option",
        "unexpected argument",
        "unrecognized option",
        "unrecognized argument",
        "unrecognized subcommand",
        "unknown command",
        "unknown subcommand",
        "method not found",
        "unknown variant",
    ];
    if old.iter().any(|o| e.contains(o)) {
        let update = if agent == "Codex" {
            "npm install -g @openai/codex@latest"
        } else {
            "claude update"
        };
        return format!(
            "{agent} is too old for this. Update it (run {update} in a terminal), then try again."
        );
    }
    match stderr.lines().rev().find(|l| !l.trim().is_empty()) {
        Some(last) => format!("{agent} stopped: {}", mask(last.trim())),
        None => format!("{agent} stopped."),
    }
}

pub fn tail(text: &str) -> String {
    const LINES: usize = 12;
    const MAX: usize = 1_500;
    let text = mask(text);
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let kept = lines[lines.len().saturating_sub(LINES)..].join("\n");
    let start = kept.len().saturating_sub(MAX);
    let start = (start..=kept.len())
        .find(|i| kept.is_char_boundary(*i))
        .unwrap_or(0);
    kept[start..].to_owned()
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

#[allow(clippy::too_many_arguments)]
async fn run_codex(
    app: &AppHandle,
    id: &str,
    exe: &Path,
    path: &Path,
    mode: Mode,
    model: Option<String>,
    resume: Option<String>,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.arg("app-server")
        .current_dir(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::agents::hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start Codex: {e}"))?;
    let stderr = keep_stderr(&mut child);
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
    // Resume picks up the same thread; otherwise a new one.
    let (method, params) = match &resume {
        Some(t) => ("thread/resume", json!({ "threadId": t })),
        None => ("thread/start", params),
    };
    write_line(
        &mut stdin,
        json!({ "id": thread_req, "method": method, "params": params }),
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
                    let _ = child.wait().await;
                    return Err(why_stopped("Codex", &lock(&stderr)));
                };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                if v["id"] == thread_req && v.get("method").is_none() {
                    if let Some(err) = v["error"]["message"].as_str() {
                        return Err(why_stopped("Codex", err));
                    }
                    let t = v["result"]["thread"]["id"].as_str().map(str::to_owned);
                    with(&SESSIONS, |s| if let Some(h) = s.get_mut(id) { h.resume = t.clone() });
                    save();
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
                    if data["kind"] == "turn" {
                        let id = id.to_owned();
                        tokio::task::spawn_blocking(move || turn_ended(&id));
                    }
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
                "output": tail(item["aggregatedOutput"].as_str().unwrap_or_default()),
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

    fn git(dir: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// A session in a throwaway repo, as if started, without an agent.
    fn session(name: &str) -> Option<(String, PathBuf)> {
        let dir = std::env::temp_dir().join(format!("sidekick-sess-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "t@example.com"],
            &["config", "user.name", "t"],
        ] {
            git(&dir, args).then_some(())?;
        }
        std::fs::write(dir.join("a.txt"), "one\n").ok()?;
        (git(&dir, &["add", "."]) && git(&dir, &["commit", "-q", "-m", "start"])).then_some(())?;
        let b = review::snapshot(&dir)?;
        let id = format!("test-{name}");
        let (tx, _rx) = mpsc::unbounded_channel();
        with(&SESSIONS, |s| {
            s.insert(
                id.clone(),
                Handle {
                    tx,
                    checkpoints: vec![b.clone()],
                    baseline: Some(b),
                    agent: Agent::ClaudeCode,
                    path: dir.clone(),
                    mode: Mode::Edit,
                    resume: None,
                    after: HashMap::new(),
                },
            )
        });
        Some((id, dir))
    }

    #[test]
    fn rewinds_to_before_a_message() {
        let Some((id, dir)) = session("rewind") else {
            return; // no git here
        };
        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        // The second message: a checkpoint, then the agent's next edit.
        let _ = send(&id, "also log it");
        std::fs::write(dir.join("a.txt"), "three\n").unwrap();
        std::fs::write(dir.join("new.txt"), "x").unwrap();
        assert_eq!(rewind_preview(&id, 1).unwrap(), 2);
        rewind(&id, 1).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "two\n");
        assert!(!dir.join("new.txt").exists());
        close(&id);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn undo_leaves_files_edited_after_the_agent_finished() {
        let Some((id, dir)) = session("edited") else {
            return;
        };
        std::fs::write(dir.join("a.txt"), "agent\n").unwrap();
        turn_ended(&id);
        std::fs::write(dir.join("a.txt"), "agent\nmine\n").unwrap();
        let err = undo(&id, Some("a.txt"), None).unwrap_err();
        assert!(err.contains("changed after the agent finished"), "{err}");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "agent\nmine\n"
        );
        close(&id);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finds_project_commands() {
        let dir = std::env::temp_dir().join(format!("sidekick-cmds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("release.md"),
            "---\ndescription: Cut a release\n---\nSteps",
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "not a command").unwrap();
        let cmds = project_commands(&dir);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name, "/release");
        assert!(cmds[0].description.starts_with("Cut a release"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn says_when_a_cli_is_too_old() {
        let m = why_stopped(
            "Claude Code",
            "error: unknown option '--include-partial-messages'\n",
        );
        assert!(m.contains("too old") && m.contains("claude update"), "{m}");
        let m = why_stopped("Codex", "error: unrecognized subcommand 'app-server'");
        assert!(m.contains("@openai/codex@latest"), "{m}");
        assert!(why_stopped("Codex", "Method not found: thread/resume").contains("too old"));
        assert_eq!(
            why_stopped("Codex", "\nnot logged in\n\n"),
            "Codex stopped: not logged in"
        );
        assert_eq!(why_stopped("Codex", ""), "Codex stopped.");
    }

    #[test]
    fn keeps_the_end_of_long_output() {
        let text: String = (1..=40).map(|n| format!("line {n}\n")).collect();
        let t = tail(&text);
        assert!(t.starts_with("line 29") && t.ends_with("line 40"), "{t}");
        assert_eq!(tail(""), "");
        assert_eq!(
            tail("export TOKEN ghp_abcdefghijklmnopqrstuvwxyz0123456789AB"),
            "export TOKEN \u{2022}\u{2022}\u{2022}\u{2022}"
        );
    }

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
