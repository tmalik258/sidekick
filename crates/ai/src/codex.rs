use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::{AiError, AiProvider, CancellationToken, ChatRequest, Sink, hide_console, transcript};

/// T2 through the user's own Codex CLI (`codex exec`), so answers come from
/// their ChatGPT plan or OpenAI key. Like Claude Code, Sidekick never reads
/// Codex's credentials; it only runs the CLI the user already signed in to.
/// It runs read-only in an empty folder, so it cannot change anything.
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
        }
        if image {
            args.push("--image".into());
            args.push(SCREENSHOT.into());
        }
        // The prompt comes on stdin.
        args.push("-".into());
        args
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
        let mut cmd = tokio::process::Command::new(&exe);
        // The prompt goes in on stdin, never as an argument.
        cmd.args(self.args(req.image.is_some()))
            .current_dir(&self.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some((_, token)) = &self.mcp {
            cmd.env(TOKEN_VAR, token);
        }
        hide_console(&mut cmd);
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
    fn runs_read_only_with_the_prompt_on_stdin() {
        let c = Codex {
            path: None,
            model: Some("gpt-5-codex".into()),
            workdir: PathBuf::from("."),
            mcp: Some(("http://127.0.0.1:47823/mcp".into(), "secret".into())),
        };
        let args = c.args(true);
        let joined = args.join(" ");
        assert!(joined.contains("--sandbox read-only"));
        assert!(joined.contains("--model gpt-5-codex"));
        assert!(joined.contains("--image screen.png"));
        assert!(
            !joined.contains("secret"),
            "the token stays off the command line"
        );
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(!valid_model("x; rm"));
    }
}
