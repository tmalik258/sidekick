use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::{AiError, AiProvider, CancellationToken, ChatRequest, Sink, hide_console, transcript};

/// Chat through the user's own Claude Code install (`claude -p`), so answers
/// come from their subscription. Sidekick never reads Claude Code's
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

/// What one line of `--output-format stream-json` means for us.
#[derive(Debug, PartialEq)]
enum Line {
    Delta(String),
    /// A whole assistant message (sent when partial messages are off).
    Message(String),
    Done(String),
    Error(String),
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

#[async_trait]
impl AiProvider for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude_code"
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
            .ok_or_else(|| AiError::Failed("Claude Code (claude) is not installed".into()))?;
        let _ = std::fs::create_dir_all(&self.workdir);
        let mut cmd = tokio::process::Command::new(&exe);
        // The prompt goes in on stdin, never as an argument, so no text from
        // the user or the screen ever reaches a command line.
        cmd.args(self.args())
            .current_dir(&self.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        hide_console(&mut cmd);
        let mut child = cmd
            .spawn()
            .map_err(|e| AiError::Failed(format!("could not start Claude Code: {e}")))?;

        let mut prompt = transcript(req);
        if let Some(png) = &req.image {
            // Claude Code reads images from files; it runs in this folder.
            std::fs::write(self.workdir.join(SCREENSHOT), png)
                .map_err(|e| AiError::Failed(format!("could not save the screenshot: {e}")))?;
            prompt = format!(
                "A screenshot of the user's screen is saved as {SCREENSHOT} in the current folder. Read it first.\n\n{prompt}"
            );
        }
        let mut stdin = child.stdin.take().expect("piped stdin");
        stdin
            .write_all(prompt.as_bytes())
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
                    sink.send(&text);
                    full.push_str(&text);
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
                    result = Some(Ok(full.clone()));
                }
                Line::Error(msg) => result = Some(Err(AiError::Failed(msg))),
                Line::Message(_) | Line::Other => {}
            }
        }
        let status = child.wait().await.ok();
        let stderr = err_task.await.unwrap_or_default();
        match result {
            Some(r) => r,
            None if !full.is_empty() => Ok(full),
            None => {
                let detail = stderr
                    .lines()
                    .last()
                    .unwrap_or("no output")
                    .trim()
                    .to_owned();
                let code = status
                    .and_then(|s| s.code())
                    .map(|c| format!(" (exit {c})"))
                    .unwrap_or_default();
                Err(AiError::Failed(format!(
                    "Claude Code failed{code}: {detail}"
                )))
            }
        }
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
    fn model_names_are_checked() {
        assert!(valid_model("opus"));
        assert!(valid_model("claude-opus-5-5"));
        assert!(!valid_model("opus; rm -rf"));
        assert!(!valid_model(""));
    }
}
