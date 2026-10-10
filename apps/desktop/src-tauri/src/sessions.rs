//! Agent sessions in the island: Claude Code and Codex working in a
//! project, streamed step by step, with permission questions answered in
//! the island and every change reviewable afterwards (see `review`).
//!
//! Claude Code runs as `claude -p` with stream-json in and out; its
//! permission questions come through Sidekick's MCP server
//! (`--permission-prompt-tool`). Codex runs as `codex app-server` and asks
//! for approvals as JSON-RPC requests.

mod local;

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

/// Marks every in-flight tool step cancelled (Esc / Stop mid-command).
pub(super) fn cancel_running_steps(app: &AppHandle, id: &str) {
    emit(app, id, json!({ "kind": "cancel_steps" }));
}

enum Cmd {
    Send(String),
    Stop,
    /// Stop quietly: a new process carries on the same session.
    Restart,
}

/// What a quiet restart returns, so it is not reported as the end.
const RESTARTED: &str = "restarted";
/// Esc / Stop: the turn was cut off; the session stays open for the next send.
const INTERRUPTED: &str = "interrupted";

/// The model and thinking picked for one session in its chat box.
#[derive(Debug, Clone, Default)]
struct Tune {
    model: Option<String>,
    effort: Option<String>,
    /// Claude Code takes them only at start: restart on the next message.
    restart: bool,
}

static TUNES: Mutex<Option<HashMap<String, Tune>>> = Mutex::new(None);

/// Thinking budget for Claude Code by level.
fn thinking_tokens(effort: &str) -> Option<&'static str> {
    match effort {
        "off" => Some("0"),
        "low" => Some("4000"),
        "medium" => Some("10000"),
        "high" => Some("31999"),
        "max" => Some("63999"),
        _ => None,
    }
}

/// Sets the model and thinking for a session from its chat box. Codex
/// takes them with the next turn; Claude Code restarts in its own session
/// before the next message; Local switches model on the next turn.
pub fn tune(id: &str, model: Option<String>, effort: Option<String>) {
    let claude = with(&SESSIONS, |s| {
        s.get(id).map(|h| h.agent == Agent::ClaudeCode)
    })
    .unwrap_or(false);
    with(&TUNES, |t| {
        let e = t.entry(id.to_owned()).or_default();
        e.model = model.filter(|m| !m.trim().is_empty());
        e.effort = effort.filter(|m| !m.trim().is_empty());
        e.restart = claude;
    });
}

fn tuned(id: &str) -> Tune {
    with(&TUNES, |t| t.get(id).cloned()).unwrap_or_default()
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

/// Where projects without git get their private snapshot repositories.
fn shadow_root() -> Option<PathBuf> {
    SAVE_DIR
        .lock()
        .ok()
        .and_then(|d| d.clone())
        .map(|d| d.join("snapshots"))
}

/// Writes every session to disk (called after anything that matters changes).
fn save() {
    let Some(dir) = SAVE_DIR.lock().ok().and_then(|d| d.clone()) else {
        return;
    };
    let saved: Vec<Saved> = with(&SESSIONS, |s| {
        s.iter()
            .map(|(id, h)| Saved {
                id: id.clone(),
                agent: h.agent.id().into(),
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
            let agent = Agent::from_id(&v.agent).unwrap_or(Agent::ClaudeCode);
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
    /// Another session works in this repo, so this one got its own
    /// worktree on this branch; Finish merges it back.
    pub worktree: Option<String>,
}

/// A session's own worktree: the repo it came from and its branch.
#[derive(Debug, Clone)]
struct Worktree {
    repo: PathBuf,
    dir: PathBuf,
    branch: String,
}

static WORKTREES: Mutex<Option<HashMap<String, Worktree>>> = Mutex::new(None);

fn git_ok(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr)
            .lines()
            .last()
            .unwrap_or("git failed")
            .trim()
            .to_owned())
    }
}

/// A branch name from a task: "wt/fix-the-footer-year".
pub fn worktree_branch(prompt: &str, id: &str) -> String {
    let slug: String = prompt
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|w| !w.is_empty())
        .take(5)
        .collect::<Vec<_>>()
        .join("-");
    let tail = &id[id.len().saturating_sub(4)..];
    format!(
        "wt/{}-{}",
        if slug.is_empty() { "task" } else { &slug },
        tail.to_lowercase()
    )
}

/// A second agent in a repo already in use gets its own worktree next to
/// it, so the two never edit the same files.
fn own_worktree(path: &Path, prompt: &str, id: &str) -> Option<Worktree> {
    let busy = with(&SESSIONS, |s| {
        s.values().any(|h| !h.tx.is_closed() && h.path == path)
    });
    if !busy || !path.join(".git").exists() {
        return None;
    }
    let branch = worktree_branch(prompt, id);
    let name = path.file_name()?.to_string_lossy().into_owned();
    let dir = path
        .parent()?
        .join(".sidekick-worktrees")
        .join(format!("{name}-{}", branch.trim_start_matches("wt/")));
    std::fs::create_dir_all(dir.parent()?).ok()?;
    git_ok(
        path,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            &branch,
            &dir.to_string_lossy(),
        ],
    )
    .ok()?;
    Some(Worktree {
        repo: path.to_owned(),
        dir,
        branch,
    })
}

/// Finish: commit the worktree's changes, merge its branch into the
/// repo's current branch, and remove the worktree.
pub fn finish(id: &str) -> Result<String, String> {
    let wt = with(&WORKTREES, |w| w.get(id).cloned())
        .ok_or("This session has no worktree of its own.")?;
    let dirty = !git_ok(&wt.dir, &["status", "--porcelain"])?.is_empty();
    if dirty {
        git_ok(&wt.dir, &["add", "-A"])?;
        git_ok(
            &wt.dir,
            &[
                "commit",
                "-q",
                "-m",
                &format!("Agent work on {}", wt.branch),
            ],
        )?;
    }
    if let Err(e) = git_ok(&wt.repo, &["merge", "--no-edit", "-q", &wt.branch]) {
        let _ = git_ok(&wt.repo, &["merge", "--abort"]);
        return Err(format!(
            "Could not merge {} by itself ({e}). It is kept, so you can merge it in your editor.",
            wt.branch
        ));
    }
    let _ = git_ok(
        &wt.repo,
        &["worktree", "remove", "--force", &wt.dir.to_string_lossy()],
    );
    let _ = git_ok(&wt.repo, &["branch", "-d", &wt.branch]);
    with(&WORKTREES, |w| w.remove(id));
    Ok(format!("Merged {} back", wt.branch))
}

/// Starts an agent in `path` with `prompt`.
pub async fn start(
    app: &AppHandle,
    agent: Agent,
    path: &Path,
    prompt: &str,
    mode: Mode,
) -> Result<Started, String> {
    start_tuned(app, agent, path, prompt, mode, None, None).await
}

/// Starts an agent with the model and thinking picked in New's chat box.
pub async fn start_tuned(
    app: &AppHandle,
    agent: Agent,
    path: &Path,
    prompt: &str,
    mode: Mode,
    model: Option<String>,
    effort: Option<String>,
) -> Result<Started, String> {
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    let settings = lock(&app.state::<AppState>().settings).clone();
    let exe = agent
        .resolve(&settings)
        .ok_or_else(|| format!("{} is not installed", agent.name()))?;
    let id = ulid::Ulid::new().to_string();
    if model.is_some() || effort.is_some() {
        with(&TUNES, |t| {
            t.insert(
                id.clone(),
                Tune {
                    model: model.filter(|m| !m.trim().is_empty()),
                    effort: effort.filter(|e| !e.trim().is_empty()),
                    restart: false,
                },
            )
        });
    }
    let wt = {
        let (path, prompt, id) = (path.to_owned(), prompt.to_owned(), id.clone());
        tokio::task::spawn_blocking(move || own_worktree(&path, &prompt, &id))
            .await
            .ok()
            .flatten()
    };
    let wt_dir = wt.as_ref().map(|w| w.dir.clone());
    let path: &Path = wt_dir.as_deref().unwrap_or(path);
    let baseline = {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || review::snapshot(&path, shadow_root().as_deref()))
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
        worktree: wt.as_ref().map(|w| w.branch.clone()),
    };
    if let Some(w) = wt {
        with(&WORKTREES, |m| m.insert(id.clone(), w));
    }
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
    let tune = tuned(id);
    let model = match agent {
        _ if tune.model.is_some() => tune.model.clone(),
        Agent::ClaudeCode => Some(settings.ai.claude_code.model.clone()),
        Agent::Codex => Some(settings.ai.codex.model.clone()),
        Agent::Copilot | Agent::Cursor => None,
        Agent::Local => Some(settings.ai.local.model.clone()),
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
                    tune.effort.as_deref(),
                    &mcp_config,
                    resume,
                    rx,
                )
                .await
            }
            Agent::Codex => run_codex(&app2, &id2, &exe, &path2, mode, model, resume, rx).await,
            Agent::Copilot | Agent::Cursor => {
                run_plain(&app2, &id2, agent, &exe, &path2, mode, rx).await
            }
            Agent::Local => {
                local::run(
                    &app2,
                    &id2,
                    &path2,
                    mode,
                    model,
                    &settings.ai.local.base_url,
                    rx,
                )
                .await
            }
        };
        if result.as_ref().err().map(String::as_str) == Some(RESTARTED) {
            return;
        }
        // Esc interrupt: leave the session idle; the next send auto-resumes.
        if result.as_ref().err().map(String::as_str) == Some(INTERRUPTED) {
            cancel_running_steps(&app2, &id2);
            let id = id2.clone();
            tokio::task::spawn_blocking(move || turn_ended(&id));
            emit(&app2, &id2, json!({ "kind": "turn" }));
            return;
        }
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

/// Claude Code with a new model or thinking: stop the process quietly and
/// carry on the same session with the new settings.
fn restart(app: &AppHandle, id: &str) -> Result<(), String> {
    with(&TUNES, |t| {
        if let Some(e) = t.get_mut(id) {
            e.restart = false;
        }
    });
    let (agent, path, mode, resume) = with(&SESSIONS, |s| {
        s.get(id)
            .map(|h| (h.agent, h.path.clone(), h.mode, h.resume.clone()))
    })
    .ok_or("That session is gone.")?;
    // Not started yet: the first process already has what it needs.
    let Some(resume) = resume else { return Ok(()) };
    let settings = lock(&app.state::<AppState>().settings).clone();
    let exe = agent
        .resolve(&settings)
        .ok_or_else(|| format!("{} is not installed", agent.name()))?;
    let (tx, rx) = mpsc::unbounded_channel();
    with(&SESSIONS, |s| {
        if let Some(h) = s.get_mut(id) {
            let _ = h.tx.send(Cmd::Restart);
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
pub fn send(app: &AppHandle, id: &str, text: &str) -> Result<(), String> {
    if tuned(id).restart {
        restart(app, id)?;
    }
    // After Esc interrupt the CLI is gone; bring it back without a Resume click.
    let running = with(&SESSIONS, |s| s.get(id).is_some_and(|h| !h.tx.is_closed()));
    if !running {
        resume(app, id)?;
    }
    send_text(id, text)
}

fn send_text(id: &str, text: &str) -> Result<(), String> {
    let path = with(&SESSIONS, |s| s.get(id).map(|h| h.path.clone()));
    if let Some(snap) = path
        .as_deref()
        .and_then(|p| review::snapshot(p, shadow_root().as_deref()))
    {
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
    with(&SESSIONS, |s| s.get(id).and_then(|h| h.baseline.clone())).ok_or_else(|| {
        "This project was too big to track, so its changes cannot be reviewed here.".to_owned()
    })
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
    .ok_or_else(|| "This project was too big to track, so it cannot be rewound.".to_owned())
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
    if agent == Agent::Local {
        return Err("The local agent runs inside Sidekick, not in a terminal.".into());
    }
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

/// Each running session's CLI process, for its memory use.
static PIDS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);

/// Memory the session's CLI uses, with the processes it started, in bytes.
pub fn memory(id: &str) -> Option<u64> {
    let root = with(&PIDS, |p| p.get(id).copied())?;
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let procs = sys.processes();
    procs.get(&sysinfo::Pid::from_u32(root))?;
    // The CLI and everything under it (node, shells, test runners).
    let under = |mut pid: sysinfo::Pid| {
        for _ in 0..16 {
            if pid.as_u32() == root {
                return true;
            }
            match procs.get(&pid).and_then(|p| p.parent()) {
                Some(parent) => pid = parent,
                None => return false,
            }
        }
        false
    };
    Some(
        procs
            .iter()
            .filter(|(pid, _)| under(**pid))
            .map(|(_, p)| p.memory())
            .sum(),
    )
}

/// Tools the user allowed for the rest of a Claude Code session, by session.
/// Codex keeps this itself (acceptForSession). Never saved: a restart asks
/// again.
static SESSION_ALLOWED: Mutex<Option<HashMap<String, std::collections::HashSet<String>>>> =
    Mutex::new(None);

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
    // Allowed for this session already (Allow this session).
    if with(&SESSION_ALLOWED, |a| {
        a.get(&session).is_some_and(|t| t.contains(tool))
    }) {
        return json!({ "behavior": "allow", "updatedInput": input }).to_string();
    }
    let (label, detail) = describe_tool(tool, input);
    match ask(app, &session, &label, &detail).await {
        Answer::Allow => json!({ "behavior": "allow", "updatedInput": input }).to_string(),
        Answer::Always => {
            with(&SESSION_ALLOWED, |a| {
                a.entry(session.clone())
                    .or_default()
                    .insert(tool.to_owned())
            });
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
    effort: Option<&str>,
    mcp_config: &Path,
    resume: Option<String>,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(exe);
    if let Some(t) = effort.and_then(thinking_tokens) {
        cmd.env("MAX_THINKING_TOKENS", t);
    }
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
    if let Some(pid) = child.id() {
        with(&PIDS, |p| p.insert(id.to_owned(), pid));
    }
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
                Some(Cmd::Restart) => {
                    let _ = child.kill().await;
                    return Err(RESTARTED.into());
                }
                Some(Cmd::Stop) => {
                    let _ = child.kill().await;
                    return Err(INTERRUPTED.into());
                }
                None => {
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
            // What this call read is how full the context is now. The
            // result's usage adds up every call in the turn, so it reads high.
            let u = &v["message"]["usage"];
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
        // The plan's usage limits: "allowed", "allowed_warning" near the
        // limit, "rejected" at it.
        Some("rate_limit_event") => {
            let r = &v["rate_limit_info"];
            if let Some(status) = r["status"].as_str() {
                out.push(json!({
                    "kind": "limit",
                    "status": status,
                    "window": r["rateLimitType"],
                    "resetsAt": r["resetsAt"],
                    "used": r["utilization"],
                }));
            }
        }
        Some("result") => {
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
    if let Some(pid) = child.id() {
        with(&PIDS, |p| p.insert(id.to_owned(), pid));
    }
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
                        None => {
                            let mut params = json!({ "threadId": t, "input": input });
                            let tune = tuned(id);
                            if let Some(m) = tune.model { params["model"] = json!(m); }
                            if let Some(e) = tune.effort.filter(|e| e != "off") { params["effort"] = json!(if e == "max" { "xhigh" } else { e.as_str() }); }
                            json!({ "id": next_id, "method": "turn/start", "params": params })
                        }
                    };
                    write_line(&mut stdin, msg).await?;
                    emit(app, id, json!({ "kind": "working" }));
                }
                Some(Cmd::Restart) => {
                    let _ = child.kill().await;
                    return Err(RESTARTED.into());
                }
                Some(Cmd::Stop) => {
                    let _ = child.kill().await;
                    return Err(INTERRUPTED.into());
                }
                None => {
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
                        ("Run a command".to_owned(), shown_command(p["command"].as_str().unwrap_or_default()))
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

/// One turn's arguments for a CLI run once per message (Copilot, Cursor).
/// They cannot ask in the island, so the mode decides up front what they
/// may do; review and undo still cover every change.
fn plain_args(agent: Agent, mode: Mode, text: &str, again: bool) -> Vec<String> {
    let mut a: Vec<String> = vec!["-p".into(), text.into()];
    match agent {
        Agent::Copilot => {
            if again {
                a.push("--continue".into());
            }
            match mode {
                Mode::Full => a.push("--allow-all-tools".into()),
                Mode::Edit | Mode::Ask => {
                    a.extend(["--allow-all-tools", "--deny-tool", "shell"].map(String::from))
                }
                Mode::Plan => a.extend(
                    [
                        "--allow-all-tools",
                        "--deny-tool",
                        "write",
                        "--deny-tool",
                        "shell",
                    ]
                    .map(String::from),
                ),
            }
        }
        _ => {
            // Cursor streams each step as JSON, so its sessions are as live as Codex.
            a.extend(["--output-format", "stream-json"].map(String::from));
            if matches!(mode, Mode::Full | Mode::Edit) {
                a.push("--force".into());
            }
        }
    }
    a
}

/// Runs a CLI that takes one prompt per process, streaming what it prints.
/// A message sent mid-turn runs as the next turn.
async fn run_plain(
    app: &AppHandle,
    id: &str,
    agent: Agent,
    exe: &Path,
    path: &Path,
    mode: Mode,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
) -> Result<(), String> {
    let mut turns = 0usize;
    // Cursor's own chat id, so later turns carry on the same chat.
    let mut chat: Option<String> = None;
    let mut queued: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    loop {
        let text = match queued.pop_front() {
            Some(t) => t,
            None => match rx.recv().await {
                Some(Cmd::Send(t)) => t,
                // Already idle: Esc is a no-op; keep waiting for a message.
                Some(Cmd::Stop) => continue,
                Some(Cmd::Restart) | None => return Ok(()),
            },
        };
        emit(app, id, json!({ "kind": "working" }));
        let mut cmd = tokio::process::Command::new(exe);
        cmd.args(plain_args(agent, mode, &text, turns > 0));
        if let Some(c) = chat.as_ref().filter(|_| agent == Agent::Cursor) {
            cmd.args(["--resume", c]);
        }
        cmd.current_dir(path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::agents::hide_console(&mut cmd);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", agent.name()))?;
        turns += 1;
        let stderr = keep_stderr(&mut child);
        if let Some(pid) = child.id() {
            with(&PIDS, |p| p.insert(id.to_owned(), pid));
        }
        let mut lines = BufReader::new(child.stdout.take().ok_or("no stdout")?).lines();
        let mut stopped = false;
        loop {
            tokio::select! {
                line = lines.next_line() => match line {
                    Ok(Some(line)) if agent == Agent::Cursor => {
                        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                        if let Some(c) = v["session_id"].as_str() {
                            chat = Some(c.to_owned());
                        }
                        for e in cursor_events(&v) {
                            emit(app, id, e);
                        }
                    }
                    Ok(Some(line)) => emit(app, id, json!({ "kind": "text", "text": format!("{line}\n") })),
                    _ => break,
                },
                cmd = rx.recv() => match cmd {
                    Some(Cmd::Send(more)) => queued.push_back(more),
                    Some(Cmd::Stop) => {
                        let _ = child.kill().await;
                        stopped = true;
                        break;
                    }
                    Some(Cmd::Restart) | None => {
                        let _ = child.kill().await;
                        return Ok(());
                    }
                },
            }
        }
        let ok = !stopped && child.wait().await.is_ok_and(|s| s.success());
        let error = (!stopped && !ok).then(|| why_stopped(agent.name(), &lock(&stderr)));
        if stopped {
            cancel_running_steps(app, id);
        }
        emit(app, id, json!({ "kind": "turn", "error": error }));
        let sid = id.to_owned();
        tokio::task::spawn_blocking(move || turn_ended(&sid));
    }
}

/// What one line of `cursor-agent --output-format stream-json` means for
/// the island.
pub fn cursor_events(v: &Value) -> Vec<Value> {
    match v["type"].as_str() {
        Some("assistant") => v["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["text"].as_str())
            .map(|t| json!({ "kind": "text", "text": t }))
            .collect(),
        Some("tool_call") => {
            let call = v["tool_call"].as_object();
            let (name, body) = call
                .and_then(|o| o.iter().next())
                .map(|(k, b)| (k.trim_end_matches("ToolCall").to_owned(), b.clone()))
                .unwrap_or_default();
            let args = &body["args"];
            let detail = args["path"]
                .as_str()
                .or_else(|| args["command"].as_str())
                .or_else(|| args["pattern"].as_str())
                .unwrap_or("");
            let label = match name.as_str() {
                "read" => "Read",
                "edit" | "write" => "Edit",
                "shell" => "Run",
                "grep" | "glob" | "ls" => "Search",
                _ => "Step",
            };
            let done = v["subtype"] == "completed";
            let failed = done && body["result"].get("error").is_some();
            vec![json!({
                "kind": "step",
                "id": v["call_id"].as_str().unwrap_or(""),
                "tool": name,
                "label": label,
                "detail": shown_command(detail),
                "state": if failed { "failed" } else if done { "done" } else { "running" },
            })]
        }
        _ => Vec::new(),
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
                    shown_command(item["command"].as_str().unwrap_or_default()),
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

/// The command as the user would type it. Codex wraps each one as
/// `"C:\\...\\pwsh.exe" -Command "..."`; only the inner part matters.
fn shown_command(cmd: &str) -> String {
    let cmd = cmd.trim();
    let lower = cmd.to_ascii_lowercase();
    let wrapped = lower.starts_with('"')
        || lower.contains("powershell")
        || lower.contains("pwsh")
        || lower.contains("cmd.exe");
    let inner = ["-command ", "-c ", "/c "]
        .iter()
        .filter(|_| wrapped)
        .find_map(|flag| lower.find(flag).map(|i| &cmd[i + flag.len()..]))
        .unwrap_or(cmd)
        .trim();
    let inner = inner
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(inner);
    inner.replace("\\\\", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_command_unwraps_the_shell() {
        let raw = r#""C:\\Program Files\\WindowsApps\\PowerShell\\pwsh.exe" -Command "git status --short""#;
        assert_eq!(shown_command(raw), "git status --short");
        assert_eq!(shown_command("pnpm test"), "pnpm test");
    }

    #[test]
    fn names_worktree_branches() {
        assert_eq!(
            worktree_branch("Fix the footer year, it says 2024!", "01ABCDWXYZ"),
            "wt/fix-the-footer-year-it-wxyz"
        );
        assert_eq!(worktree_branch("???", "01AB"), "wt/task-01ab");
    }

    #[test]
    fn plain_agents_get_what_the_mode_allows() {
        let a = plain_args(Agent::Copilot, Mode::Plan, "fix it", false);
        assert_eq!(a[..2], ["-p", "fix it"]);
        assert!(a.windows(2).any(|w| w == ["--deny-tool", "write"]));
        assert!(
            plain_args(Agent::Copilot, Mode::Full, "x", true).contains(&"--continue".to_owned())
        );
        assert!(plain_args(Agent::Cursor, Mode::Edit, "x", false).contains(&"--force".to_owned()));
        assert!(!plain_args(Agent::Cursor, Mode::Plan, "x", false).contains(&"--force".to_owned()));
    }

    #[test]
    fn reads_cursor_stream() {
        let text =
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"On it"}]}});
        assert_eq!(cursor_events(&text)[0]["text"], "On it");
        let started = json!({"type":"tool_call","subtype":"started","call_id":"c1",
            "tool_call":{"readToolCall":{"args":{"path":"src/a.ts"}}}});
        let e = &cursor_events(&started)[0];
        assert_eq!(
            (e["label"].as_str(), e["state"].as_str()),
            (Some("Read"), Some("running"))
        );
        let done = json!({"type":"tool_call","subtype":"completed","call_id":"c1",
            "tool_call":{"readToolCall":{"args":{"path":"src/a.ts"},"result":{"success":{}}}}});
        assert_eq!(cursor_events(&done)[0]["state"], "done");
        assert!(cursor_events(&json!({"type":"result"})).is_empty());
    }

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
        let b = review::snapshot(&dir, None)?;
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
        let _ = send_text(&id, "also log it");
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

        let said: Value = serde_json::from_str(
            r#"{"type":"assistant","message":{"content":[],"usage":{"input_tokens":10,"cache_read_input_tokens":90}}}"#,
        )
        .unwrap();
        assert_eq!(claude_events(&said)[0]["used"], 100);

        let done: Value = serde_json::from_str(
            r#"{"type":"result","is_error":false,"result":"x","usage":{"input_tokens":10,"cache_read_input_tokens":90}}"#,
        )
        .unwrap();
        let ev = claude_events(&done);
        assert_eq!(ev[0]["kind"], "turn");
        assert!(ev[0]["error"].is_null());

        let limit: Value = serde_json::from_str(
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","rateLimitType":"five_hour","resetsAt":1760000000,"utilization":0.8}}"#,
        )
        .unwrap();
        let ev = claude_events(&limit);
        assert_eq!(ev[0]["kind"], "limit");
        assert_eq!(ev[0]["status"], "allowed_warning");
        assert_eq!(ev[0]["window"], "five_hour");
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
