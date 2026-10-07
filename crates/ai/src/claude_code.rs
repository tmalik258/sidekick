use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::pool::Pool;
use crate::{
    AiError, AiProvider, CancellationToken, ChatRequest, Message, Role, Sink, hide_console,
    transcript,
};

/// Chat through the user's own Claude Code install (`claude -p`), so answers
/// come from their subscription. One session per chat is kept warm. Sidekick never reads Claude Code's
/// credential files; it only runs the CLI the user already signed in to.
pub struct ClaudeCode {
    /// Explicit path to `claude`; otherwise it is looked up on PATH.
    pub path: Option<PathBuf>,
    /// Optional `--model` (an alias like `opus` or a full model id).
    pub model: Option<String>,
    /// Folder the CLI runs in. Kept empty so it has no project to touch.
    pub workdir: PathBuf,
    /// An MCP config file (Sidekick's own server) to load for the chat. Its
    /// tools are pre-approved; they only search, notify and open links.
    pub mcp_config: Option<PathBuf>,
}

impl ClaudeCode {
    pub fn resolve(&self) -> Option<PathBuf> {
        self.path
            .clone()
            .filter(|p| p.is_file())
            .or_else(|| which::which("claude").ok())
    }

    fn args(&self) -> Vec<String> {
        let mut args: Vec<String> = [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
        ]
        .map(String::from)
        .to_vec();
        if let Some(model) = self.model.as_deref().filter(|m| valid_model(m)) {
            args.push("--model".into());
            args.push(model.into());
        }
        if let Some(config) = self.mcp_config.as_ref().filter(|p| p.is_file()) {
            args.push("--mcp-config".into());
            args.push(config.to_string_lossy().into_owned());
            args.push("--allowedTools".into());
            args.push("mcp__sidekick,WebSearch,WebFetch".into());
        } else {
            // Claude Code's own web tools, which `-p` refuses unless allowed.
            args.push("--allowedTools".into());
            args.push("WebSearch,WebFetch".into());
        }
        args
    }
}

const SCREENSHOT: &str = "screen.png";

fn valid_model(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 64
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '[' | ']'))
}

/// Until when Claude Code is out of usage: Auto skips it and asks the next
/// model instead of failing every question.
static LIMITED_UNTIL: Mutex<Option<std::time::SystemTime>> = Mutex::new(None);

/// How long to skip Claude Code when its message gives no reset time.
const LIMIT_PAUSE: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Claude Code's "out of usage" messages ("Claude AI usage limit reached",
/// "You've hit your limit · resets 3pm", "Credit balance is too low").
pub fn is_usage_limit(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.len() < 400
        && [
            "usage limit",
            "hit your limit",
            "limit reached",
            "rate limit",
            "out of extra usage",
            "credit balance is too low",
            "weekly limit",
            "5-hour limit",
        ]
        .iter()
        .any(|k| t.contains(k))
}

fn mark_limited(resets_at: Option<u64>) {
    let now = std::time::SystemTime::now();
    let until = resets_at
        .map(|secs| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
        .filter(|t| *t > now)
        .unwrap_or(now + LIMIT_PAUSE);
    *LIMITED_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) = Some(until);
}

/// Claude Code is out of usage for now (see [`LIMITED_UNTIL`]).
pub fn limited() -> bool {
    LIMITED_UNTIL
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some_and(|t| t > std::time::SystemTime::now())
}

/// The error for a full plan, which the router answers with the next model.
fn limit_error(text: &str) -> AiError {
    AiError::Failed(format!("Claude Code usage limit: {}", text.trim()))
}

/// What one line of `--output-format stream-json` means for us.
#[derive(Debug, PartialEq)]
enum Line {
    Delta(String),
    /// A whole assistant message (sent when partial messages are off).
    Message(String),
    Done(String),
    Error(String),
    /// The plan's usage is used up; the reset time, when given.
    Limited(Option<u64>),
    Other,
}

fn parse_line(line: &str) -> Line {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Line::Other;
    };
    match v["type"].as_str() {
        Some("stream_event") => {
            let e = &v["event"];
            if e["type"] == "content_block_delta" && e["delta"]["type"] == "text_delta" {
                Line::Delta(e["delta"]["text"].as_str().unwrap_or_default().to_owned())
            } else {
                Line::Other
            }
        }
        Some("assistant") => {
            let text: String = v["message"]["content"]
                .as_array()
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter(|b| b["type"] == "text")
                        .filter_map(|b| b["text"].as_str())
                        .collect()
                })
                .unwrap_or_default();
            Line::Message(text)
        }
        Some("rate_limit_event") if v["rate_limit_info"]["status"] == "rejected" => {
            Line::Limited(v["rate_limit_info"]["resetsAt"].as_u64())
        }
        Some("result") => {
            let text = v["result"].as_str().unwrap_or_default().to_owned();
            if v["is_error"].as_bool().unwrap_or(false) {
                Line::Error(if text.is_empty() {
                    v["subtype"]
                        .as_str()
                        .unwrap_or("Claude Code failed")
                        .to_owned()
                } else {
                    text
                })
            } else {
                Line::Done(text)
            }
        }
        _ => Line::Other,
    }
}

/// One running `claude -p --input-format stream-json`: it takes a message
/// per line on stdin and answers each with a stream ending in `result`.
struct Session {
    _child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    stderr: Arc<Mutex<String>>,
}

static SESSIONS: Pool<Session> = Pool::new();

/// Closes every warm Claude Code session.
pub fn close_sessions() {
    SESSIONS.clear();
}

pub(crate) fn sweep_sessions() {
    SESSIONS.sweep();
}

impl ClaudeCode {
    fn key(&self, exe: &Path) -> String {
        format!(
            "{}|{}|{}",
            exe.display(),
            self.args().join(" "),
            self.workdir.display()
        )
    }

    fn spawn(&self, exe: &Path) -> Result<Session, AiError> {
        let _ = std::fs::create_dir_all(&self.workdir);
        let mut cmd = tokio::process::Command::new(exe);
        // Messages go in on stdin, never as arguments, so no text from the
        // user or the screen ever reaches a command line.
        cmd.args(self.args())
            .args(["--input-format", "stream-json"])
            .current_dir(&self.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        hide_console(&mut cmd);
        let mut child = cmd
            .spawn()
            .map_err(|e| AiError::Failed(format!("could not start Claude Code: {e}")))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
        let stderr = Arc::new(Mutex::new(String::new()));
        let mut err = child.stderr.take().expect("piped stderr");
        let keep = stderr.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            while let Ok(n) = err.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let mut s = keep.lock().unwrap_or_else(|e| e.into_inner());
                s.push_str(&String::from_utf8_lossy(&buf[..n]));
                if s.len() > 4096 {
                    let cut = s.len() - 4096;
                    let cut = (cut..s.len()).find(|i| s.is_char_boundary(*i)).unwrap_or(0);
                    s.drain(..cut);
                }
            }
        });
        Ok(Session {
            _child: child,
            stdin,
            lines,
            stderr,
        })
    }
}

/// The stream-json line for one user message.
fn user_line(text: &str) -> String {
    let mut line = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": [{ "type": "text", "text": text }] },
    })
    .to_string();
    line.push('\n');
    line
}

/// Sends one message and streams the answer. An error leaves the session
/// unusable, so the caller drops it.
async fn turn(
    s: &mut Session,
    text: &str,
    sink: &Sink,
    cancel: &CancellationToken,
) -> Result<String, AiError> {
    s.stdin
        .write_all(user_line(text).as_bytes())
        .await
        .map_err(|e| AiError::Failed(format!("Claude Code stopped: {e}")))?;
    s.stdin
        .flush()
        .await
        .map_err(|e| AiError::Failed(e.to_string()))?;
    let mut full = String::new();
    loop {
        let line = tokio::select! {
            _ = cancel.cancelled() => return Err(AiError::Cancelled),
            line = s.lines.next_line() => line,
        };
        let Ok(Some(line)) = line else { break };
        match parse_line(&line) {
            Line::Delta(text) => {
                sink.send(&text);
                full.push_str(&text);
            }
            // Not an answer: say nothing so Auto can ask the next model.
            Line::Limited(resets) => {
                mark_limited(resets);
                return Err(limit_error("the plan's usage is used up"));
            }
            Line::Message(text) | Line::Done(text) if full.is_empty() && is_usage_limit(&text) => {
                mark_limited(None);
                return Err(limit_error(&text));
            }
            Line::Error(msg) if is_usage_limit(&msg) => {
                mark_limited(None);
                return Err(limit_error(&msg));
            }
            Line::Message(text) if full.is_empty() => {
                sink.send(&text);
                full.push_str(&text);
            }
            Line::Done(text) => {
                if full.is_empty() {
                    sink.send(&text);
                    full = text;
                }
                return Ok(full);
            }
            Line::Error(msg) => return Err(AiError::Failed(msg)),
            Line::Message(_) | Line::Other => {}
        }
    }
    if !full.is_empty() {
        return Ok(full);
    }
    let detail = s
        .stderr
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("no output")
        .trim()
        .to_owned();
    Err(AiError::Failed(format!("Claude Code failed: {detail}")))
}

#[async_trait]
impl AiProvider for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude_code"
    }

    fn is_local(&self) -> bool {
        false
    }

    async fn available(&self) -> bool {
        !limited() && self.resolve().is_some()
    }

    /// Starts a session in the background, so the first message does not
    /// wait for the CLI to load.
    async fn warm(&self) {
        let Some(exe) = self.resolve() else { return };
        let key = self.key(&exe);
        if SESSIONS.has_spare(&key) {
            return;
        }
        match self.spawn(&exe) {
            Ok(s) => SESSIONS.put(key, Vec::new(), s),
            Err(err) => log::warn!("could not warm Claude Code: {err}"),
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
            .ok_or_else(|| AiError::Failed("Claude Code (claude) is not installed".into()))?;
        let key = self.key(&exe);
        let earlier = &req.messages[..req.messages.len().saturating_sub(1)];
        let latest = req
            .messages
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or_default();
        if let Some(png) = &req.image {
            // Claude Code reads images from files; it runs in this folder.
            let _ = std::fs::create_dir_all(&self.workdir);
            std::fs::write(self.workdir.join(SCREENSHOT), png)
                .map_err(|e| AiError::Failed(format!("could not save the screenshot: {e}")))?;
        }
        let prompt = |continuing: bool| {
            // A session that already has the chat only needs the new message.
            let text = if continuing {
                latest.to_owned()
            } else {
                transcript(req)
            };
            if req.image.is_some() {
                format!(
                    "A screenshot of the user's screen is saved as {SCREENSHOT} in the current folder. Read it first.\n\n{text}"
                )
            } else {
                text
            }
        };
        let pooled = SESSIONS.take(&key, earlier);
        let from_pool = pooled.is_some();
        let (mut session, continuing) = match pooled {
            Some(p) => p,
            None => (self.spawn(&exe)?, false),
        };
        let answer = match turn(&mut session, &prompt(continuing), sink, cancel).await {
            // A kept session may have died while it waited: start over once.
            Err(AiError::Failed(err)) if from_pool && !sink.has_sent() => {
                log::info!("warm Claude Code session failed, starting a new one: {err}");
                session = self.spawn(&exe)?;
                turn(&mut session, &prompt(false), sink, cancel).await?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_partial_text_deltas() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}}"#;
        assert_eq!(parse_line(line), Line::Delta("Hel".into()));
    }

    #[test]
    fn reads_results_and_errors() {
        assert_eq!(
            parse_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"Hi"}"#),
            Line::Done("Hi".into())
        );
        assert_eq!(
            parse_line(
                r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1760000000}}"#
            ),
            Line::Limited(Some(1_760_000_000))
        );
        assert!(is_usage_limit("Claude AI usage limit reached|1760000000"));
        assert!(is_usage_limit(
            "You've hit your limit · resets 3pm (Asia/Karachi)"
        ));
        assert!(!is_usage_limit(
            "Here is how rate limiting works in nginx: ..."
                .repeat(20)
                .as_str()
        ));
        assert_eq!(
            parse_line(r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#),
            Line::Error("error_during_execution".into())
        );
        assert_eq!(parse_line("not json"), Line::Other);
    }

    #[test]
    fn whole_messages_join_text_blocks() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"a"},{"type":"tool_use"},{"type":"text","text":"b"}]}}"#;
        assert_eq!(parse_line(line), Line::Message("ab".into()));
    }

    #[test]
    fn selected_model_is_passed_explicitly_to_the_cli() {
        let provider = ClaudeCode {
            path: None,
            model: Some("claude-haiku-4-5-20251001".into()),
            workdir: PathBuf::new(),
            mcp_config: None,
        };
        let args = provider.args();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--model", "claude-haiku-4-5-20251001"])
        );
    }

    #[test]
    fn user_messages_are_one_json_line() {
        let line = user_line("two\nlines");
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        let v: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(v["message"]["content"][0]["text"], "two\nlines");
    }

    #[test]
    fn model_names_are_checked() {
        assert!(valid_model("opus"));
        assert!(valid_model("claude-opus-5-5"));
        assert!(!valid_model("opus; rm -rf"));
        assert!(!valid_model(""));
    }
}
