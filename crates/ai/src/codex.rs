use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::pool::Pool;
use crate::{
    AiError, AiProvider, CancellationToken, ChatRequest, Message, Role, Sink, hide_console,
    transcript,
};

/// Chat through the user's own Codex CLI (`codex exec`), so answers come from
/// their ChatGPT plan or OpenAI key. Like Claude Code, Sidekick never reads
/// Codex's credentials; it only runs the CLI the user already signed in to.
/// It runs read-only in an empty folder, so it cannot change anything.
/// A `codex app-server` session per chat is kept warm; older CLIs without
/// it fall back to one `codex exec` per message.
pub struct Codex {
    /// Explicit path to `codex`; otherwise it is looked up on PATH.
    pub path: Option<PathBuf>,
    /// Optional `--model`.
    pub model: Option<String>,
    /// Folder the CLI runs in. Kept empty so it has no project to touch.
    pub workdir: PathBuf,
    /// Sidekick's own MCP server (link and token) for search and notes.
    pub mcp: Option<(String, String)>,
}

const SCREENSHOT: &str = "screen.png";
/// Codex reads the token for Sidekick's server from this variable, so it
/// never appears on a command line.
const TOKEN_VAR: &str = "SIDEKICK_MCP_TOKEN";

impl Codex {
    pub fn resolve(&self) -> Option<PathBuf> {
        self.path
            .clone()
            .filter(|p| p.is_file())
            .or_else(|| which::which("codex").ok())
    }

    fn args(&self, image: bool) -> Vec<String> {
        let mut args: Vec<String> = [
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--color",
            "never",
        ]
        .map(String::from)
        .to_vec();
        if let Some(model) = self.model.as_deref().filter(|m| valid_model(m)) {
            args.push("--model".into());
            args.push(model.into());
        }
        args.extend(self.mcp_args());
        if image {
            args.push("--image".into());
            args.push(SCREENSHOT.into());
        }
        // The prompt comes on stdin.
        args.push("-".into());
        args
    }

    /// `-c` settings that add Sidekick's MCP server.
    fn mcp_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some((url, _)) = self
            .mcp
            .as_ref()
            .filter(|(u, _)| u.starts_with("http://127.0.0.1"))
        {
            args.push("-c".into());
            args.push(format!("mcp_servers.sidekick.url=\"{url}\""));
            args.push("-c".into());
            args.push(format!(
                "mcp_servers.sidekick.bearer_token_env_var=\"{TOKEN_VAR}\""
            ));
            // `codex exec` never asks, so a tool that needs approval is
            // refused. Sidekick's tools are safe (changes wait for a tap).
            args.push("-c".into());
            args.push("mcp_servers.sidekick.default_tools_approval_mode=\"approve\"".into());
        }
        args
    }

    fn command(&self, exe: &Path, args: &[String]) -> tokio::process::Command {
        let _ = std::fs::create_dir_all(&self.workdir);
        let mut cmd = tokio::process::Command::new(exe);
        cmd.args(args)
            .current_dir(&self.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some((_, token)) = &self.mcp {
            cmd.env(TOKEN_VAR, token);
        }
        hide_console(&mut cmd);
        cmd
    }
}

fn valid_model(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 64
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
}

/// What one line of `codex exec --json` means for us. Both the current
/// event names (`item.completed`) and the older ones (`msg`) are read.
#[derive(Debug, PartialEq)]
enum Line {
    Delta(String),
    Message(String),
    Done(Option<String>),
    Error(String),
    Other,
}

fn parse_line(line: &str) -> Line {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Line::Other;
    };
    let text = |x: &Value| x.as_str().unwrap_or_default().to_owned();
    match v["type"].as_str() {
        Some("item.completed") if v["item"]["type"] == "agent_message" => {
            Line::Message(text(&v["item"]["text"]))
        }
        Some("turn.completed") => Line::Done(None),
        Some("turn.failed") => Line::Error(
            v["error"]["message"]
                .as_str()
                .unwrap_or("Codex failed")
                .to_owned(),
        ),
        Some("error") => Line::Error(text(&v["message"])),
        _ => {
            let m = &v["msg"];
            match m["type"].as_str() {
                Some("agent_message_delta") => Line::Delta(text(&m["delta"])),
                Some("agent_message") => Line::Message(text(&m["message"])),
                Some("task_complete") => {
                    Line::Done(m["last_agent_message"].as_str().map(str::to_owned))
                }
                Some("error") => Line::Error(text(&m["message"])),
                _ => Line::Other,
            }
        }
    }
}

/// How long `codex app-server` may take to start and open a thread.
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// A running `codex app-server` (JSON-RPC, one message per line) with one
/// open thread.
struct Session {
    _child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    next_id: u64,
    thread: String,
}

static SESSIONS: Pool<Session> = Pool::new();

/// Closes every warm Codex session.
pub fn close_sessions() {
    SESSIONS.clear();
}

pub(crate) fn sweep_sessions() {
    SESSIONS.sweep();
}

/// What one app-server line means while a turn runs.
#[derive(Debug, PartialEq)]
enum Event {
    Delta(String),
    /// A finished agent message (the whole text).
    Message(String),
    Done,
    Failed(String),
    /// The server asks something (an approval); it is declined.
    Request(Value),
    Other,
}

fn parse_event(v: &Value) -> Event {
    let p = &v["params"];
    let text = |x: &Value| x.as_str().unwrap_or_default().to_owned();
    match v["method"].as_str() {
        Some(_) if v.get("id").is_some() => Event::Request(v["id"].clone()),
        Some("item/agentMessage/delta") => Event::Delta(text(&p["delta"])),
        Some("item/completed") if p["item"]["type"] == "agentMessage" => {
            Event::Message(text(&p["item"]["text"]))
        }
        Some("turn/completed") => match p["turn"]["status"].as_str() {
            Some("failed") => Event::Failed(
                p["turn"]["error"]["message"]
                    .as_str()
                    .unwrap_or("Codex failed")
                    .to_owned(),
            ),
            Some("interrupted") => Event::Failed("Codex stopped".into()),
            _ => Event::Done,
        },
        Some("error") if p["willRetry"] != true => Event::Failed(text(&p["error"]["message"])),
        _ => Event::Other,
    }
}

impl Session {
    async fn send(&mut self, v: &Value) -> Result<(), AiError> {
        let mut line = v.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| AiError::Failed(format!("Codex stopped: {e}")))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| AiError::Failed(e.to_string()))
    }

    async fn next(&mut self) -> Result<Value, AiError> {
        loop {
            match self.lines.next_line().await {
                Ok(Some(line)) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        return Ok(v);
                    }
                }
                _ => return Err(AiError::Failed("Codex stopped".into())),
            }
        }
    }

    /// Sends a request and waits for its answer; notifications in between
    /// are skipped.
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, AiError> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "id": id, "method": method, "params": params }))
            .await?;
        loop {
            let v = self.next().await?;
            if v["id"] == id && v.get("method").is_none() {
                if let Some(err) = v["error"]["message"].as_str() {
                    return Err(AiError::Failed(login_hint(err)));
                }
                return Ok(v["result"].clone());
            }
            if let Event::Request(rid) = parse_event(&v) {
                self.decline(rid).await?;
            }
        }
    }

    async fn decline(&mut self, id: Value) -> Result<(), AiError> {
        self.send(&json!({ "id": id, "error": { "code": -32601, "message": "Sidekick runs Codex read-only" } }))
            .await
    }

    /// Runs one turn and streams the answer.
    async fn turn(
        &mut self,
        input: Vec<Value>,
        sink: &Sink,
        cancel: &CancellationToken,
    ) -> Result<String, AiError> {
        self.next_id += 1;
        let id = self.next_id;
        let thread = self.thread.clone();
        self.send(&json!({ "id": id, "method": "turn/start",
            "params": { "threadId": thread, "input": input } }))
            .await?;
        let mut full = String::new();
        let mut streamed = false;
        loop {
            let v = tokio::select! {
                _ = cancel.cancelled() => return Err(AiError::Cancelled),
                v = self.next() => v?,
            };
            if v["id"] == id && v.get("method").is_none() {
                if let Some(err) = v["error"]["message"].as_str() {
                    return Err(AiError::Failed(login_hint(err)));
                }
                continue;
            }
            match parse_event(&v) {
                Event::Delta(text) => {
                    streamed = true;
                    sink.send(&text);
                    full.push_str(&text);
                }
                // Several messages (a plan, then the answer) without deltas.
                Event::Message(text) if !streamed && !text.is_empty() => {
                    if !full.is_empty() {
                        sink.send("\n\n");
                        full.push_str("\n\n");
                    }
                    sink.send(&text);
                    full.push_str(&text);
                }
                Event::Message(_) => streamed = false,
                Event::Done => return Ok(full),
                Event::Failed(msg) if !full.is_empty() => {
                    log::warn!("Codex ended with an error after answering: {msg}");
                    return Ok(full);
                }
                Event::Failed(msg) => return Err(AiError::Failed(login_hint(&msg))),
                Event::Request(rid) => self.decline(rid).await?,
                Event::Other => {}
            }
        }
    }
}

fn login_hint(msg: &str) -> String {
    let lower = msg.to_lowercase();
    if lower.contains("login") || lower.contains("auth") || lower.contains("401") {
        format!("{msg}. Run codex login once in a terminal.")
    } else {
        msg.to_owned()
    }
}

#[async_trait]
impl AiProvider for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn is_local(&self) -> bool {
        false
    }

    async fn available(&self) -> bool {
        self.resolve().is_some()
    }

    async fn warm(&self) {
        let Some(exe) = self.resolve() else { return };
        let key = self.key(&exe);
        if SESSIONS.has_spare(&key) {
            return;
        }
        match self.start(&exe).await {
            Ok(s) => SESSIONS.put(key, Vec::new(), s),
            Err(err) => log::info!("could not warm Codex: {err}"),
        }
    }

    async fn chat(
        &self,
        req: &ChatRequest,
        sink: &Sink,
        cancel: &CancellationToken,
    ) -> Result<String, AiError> {
        let exe = self
            .resolve()
            .ok_or_else(|| AiError::Failed("Codex (codex) is not installed".into()))?;
        let _ = std::fs::create_dir_all(&self.workdir);
        if let Some(png) = &req.image {
            std::fs::write(self.workdir.join(SCREENSHOT), png)
                .map_err(|e| AiError::Failed(format!("could not save the screenshot: {e}")))?;
        }
        let key = self.key(&exe);
        let earlier = &req.messages[..req.messages.len().saturating_sub(1)];
        let pooled = SESSIONS.take(&key, earlier);
        let from_pool = pooled.is_some();
        let (mut session, continuing) = match pooled {
            Some(p) => p,
            None => match self.start(&exe).await {
                Ok(s) => (s, false),
                // No app-server in this Codex: one process per message.
                Err(err) => {
                    log::info!("Codex app-server unavailable, using codex exec: {err}");
                    return self.exec_chat(&exe, req, sink, cancel).await;
                }
            },
        };
        let input = |continuing: bool| {
            let text = if continuing {
                req.messages
                    .last()
                    .map(|m| m.content.clone())
                    .unwrap_or_default()
            } else {
                transcript(req)
            };
            let mut input = vec![json!({ "type": "text", "text": text })];
            if req.image.is_some() {
                input.push(json!({
                    "type": "localImage",
                    "path": self.workdir.join(SCREENSHOT).to_string_lossy(),
                }));
            }
            input
        };
        let answer = match session.turn(input(continuing), sink, cancel).await {
            Err(AiError::Failed(err)) if from_pool && !sink.has_sent() => {
                log::info!("warm Codex session failed, starting a new one: {err}");
                session = self.start(&exe).await?;
                session.turn(input(false), sink, cancel).await?
            }
            other => other?,
        };
        let mut history = req.messages.clone();
        history.push(Message {
            role: Role::Assistant,
            content: answer.clone(),
        });
        SESSIONS.put(key, history, session);
        Ok(answer)
    }
}

impl Codex {
    fn key(&self, exe: &Path) -> String {
        format!(
            "{}|{:?}|{}|{}",
            exe.display(),
            self.model,
            self.mcp_args().join(" "),
            self.workdir.display()
        )
    }

    /// Starts `codex app-server` and opens a read-only thread in the empty
    /// folder.
    async fn start(&self, exe: &Path) -> Result<Session, AiError> {
        let mut args = vec!["app-server".to_owned()];
        args.extend(self.mcp_args());
        let mut child = self
            .command(exe, &args)
            .spawn()
            .map_err(|e| AiError::Failed(format!("could not start Codex: {e}")))?;
        let mut s = Session {
            stdin: child.stdin.take().expect("piped stdin"),
            lines: BufReader::new(child.stdout.take().expect("piped stdout")).lines(),
            _child: child,
            next_id: 0,
            thread: String::new(),
        };
        let start = async {
            s.request(
                "initialize",
                json!({ "clientInfo": { "name": "sidekick", "version": env!("CARGO_PKG_VERSION") } }),
            )
            .await?;
            s.send(&json!({ "method": "initialized" })).await?;
            let mut params = json!({
                "cwd": self.workdir.to_string_lossy(),
                "sandbox": "read-only",
                "approvalPolicy": "never",
                "ephemeral": true,
            });
            if let Some(model) = self.model.as_deref().filter(|m| valid_model(m)) {
                params["model"] = json!(model);
            }
            let thread = s.request("thread/start", params).await?;
            s.thread = thread["thread"]["id"]
                .as_str()
                .ok_or_else(|| AiError::Failed("Codex did not open a thread".into()))?
                .to_owned();
            Ok::<_, AiError>(())
        };
        tokio::time::timeout(START_TIMEOUT, start)
            .await
            .map_err(|_| AiError::Failed("Codex did not start in time".into()))??;
        Ok(s)
    }

    /// One `codex exec` per message: the way older CLIs are run.
    async fn exec_chat(
        &self,
        exe: &Path,
        req: &ChatRequest,
        sink: &Sink,
        cancel: &CancellationToken,
    ) -> Result<String, AiError> {
        // The prompt goes in on stdin, never as an argument.
        let mut cmd = self.command(exe, &self.args(req.image.is_some()));
        cmd.stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| AiError::Failed(format!("could not start Codex: {e}")))?;

        let mut stdin = child.stdin.take().expect("piped stdin");
        stdin
            .write_all(transcript(req).as_bytes())
            .await
            .map_err(|e| AiError::Failed(e.to_string()))?;
        drop(stdin);

        let mut stderr = child.stderr.take().expect("piped stderr");
        let err_task = tokio::spawn(async move {
            let mut s = String::new();
            let _ = stderr.read_to_string(&mut s).await;
            s
        });

        let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
        let mut full = String::new();
        let mut streamed = false;
        let mut result: Option<Result<String, AiError>> = None;
        loop {
            let line = tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = child.kill().await;
                    return Err(AiError::Cancelled);
                }
                line = lines.next_line() => line,
            };
            let Ok(Some(line)) = line else { break };
            match parse_line(&line) {
                Line::Delta(text) => {
                    streamed = true;
                    sink.send(&text);
                    full.push_str(&text);
                }
                // Codex may send a few messages (a plan, then the answer);
                // the last one is the answer.
                Line::Message(text) if !streamed && !text.is_empty() => {
                    if !full.is_empty() {
                        sink.send("\n\n");
                        full.push_str("\n\n");
                    }
                    sink.send(&text);
                    full.push_str(&text);
                }
                Line::Done(last) => {
                    if full.is_empty()
                        && let Some(text) = last
                    {
                        sink.send(&text);
                        full = text;
                    }
                    result = Some(Ok(full.clone()));
                }
                Line::Error(msg) if !msg.is_empty() => result = Some(Err(AiError::Failed(msg))),
                _ => {}
            }
        }
        let status = child.wait().await.ok();
        let stderr = err_task.await.unwrap_or_default();
        match result {
            Some(Err(e)) if !full.is_empty() => {
                log::warn!("Codex ended with an error after answering: {e}");
                Ok(full)
            }
            Some(r) => r,
            None if !full.is_empty() => Ok(full),
            None => {
                let detail = stderr
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("no output")
                    .trim()
                    .to_owned();
                let code = status
                    .and_then(|s| s.code())
                    .map(|c| format!(" (exit {c})"))
                    .unwrap_or_default();
                let hint = if detail.to_lowercase().contains("login")
                    || detail.to_lowercase().contains("auth")
                {
                    ". Run codex login once in a terminal."
                } else {
                    ""
                };
                Err(AiError::Failed(format!(
                    "Codex failed{code}: {detail}{hint}"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_current_events() {
        assert_eq!(
            parse_line(
                r#"{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"Hi"}}"#
            ),
            Line::Message("Hi".into())
        );
        assert_eq!(
            parse_line(r#"{"type":"item.completed","item":{"type":"reasoning","text":"hmm"}}"#),
            Line::Other
        );
        assert_eq!(
            parse_line(r#"{"type":"turn.completed","usage":{}}"#),
            Line::Done(None)
        );
        assert_eq!(
            parse_line(r#"{"type":"turn.failed","error":{"message":"rate limited"}}"#),
            Line::Error("rate limited".into())
        );
    }

    #[test]
    fn reads_older_events() {
        assert_eq!(
            parse_line(r#"{"id":"0","msg":{"type":"agent_message_delta","delta":"He"}}"#),
            Line::Delta("He".into())
        );
        assert_eq!(
            parse_line(r#"{"id":"0","msg":{"type":"task_complete","last_agent_message":"Hello"}}"#),
            Line::Done(Some("Hello".into()))
        );
        assert_eq!(parse_line("Reading prompt from stdin..."), Line::Other);
    }

    #[test]
    fn reads_app_server_events() {
        let ev = |s: &str| parse_event(&serde_json::from_str(s).unwrap());
        assert_eq!(
            ev(
                r#"{"method":"item/agentMessage/delta","params":{"delta":"Hi","itemId":"i","threadId":"t","turnId":"u"}}"#
            ),
            Event::Delta("Hi".into())
        );
        assert_eq!(
            ev(
                r#"{"method":"item/completed","params":{"item":{"type":"agentMessage","text":"Hello"}}}"#
            ),
            Event::Message("Hello".into())
        );
        assert_eq!(
            ev(
                r#"{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"u","items":[],"status":"completed"}}}"#
            ),
            Event::Done
        );
        assert_eq!(
            ev(
                r#"{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"u","items":[],"status":"failed","error":{"message":"rate limited"}}}}"#
            ),
            Event::Failed("rate limited".into())
        );
        assert_eq!(
            ev(
                r#"{"method":"error","params":{"error":{"message":"busy"},"threadId":"t","turnId":"u","willRetry":true}}"#
            ),
            Event::Other
        );
        assert_eq!(
            ev(r#"{"id":7,"method":"item/commandExecution/requestApproval","params":{}}"#),
            Event::Request(serde_json::json!(7))
        );
    }

    #[test]
    fn runs_read_only_with_the_prompt_on_stdin() {
        let c = Codex {
            path: None,
            model: Some("gpt-6-luna".into()),
            workdir: PathBuf::from("."),
            mcp: Some(("http://127.0.0.1:47823/mcp".into(), "secret".into())),
        };
        let args = c.args(true);
        let joined = args.join(" ");
        assert!(joined.contains("--sandbox read-only"));
        assert!(joined.contains("--model gpt-6-luna"));
        assert!(joined.contains("--image screen.png"));
        assert!(joined.contains("default_tools_approval_mode=\"approve\""));
        assert!(
            !joined.contains("secret"),
            "the token stays off the command line"
        );
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(!valid_model("x; rm"));
    }
}
