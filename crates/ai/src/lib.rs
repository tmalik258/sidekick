//! AI providers. Every AI call goes through [`AiProvider`] and the
//! [`Router`] picks the first provider that is switched on and reachable,
//! falling back down the list. Sidekick works fully without any of them.
//!
//! Decisions (pick one option from a typed list) are a separate, cheaper
//! interface, [`Decider`], served by SemIf or a local model.

mod anthropic;
mod claude_code;
mod codex;
mod decide;
mod mcp;
mod openai;
mod pool;
mod router;
mod sse;

use std::sync::atomic::{AtomicBool, Ordering};

pub use anthropic::Anthropic;
use async_trait::async_trait;
pub use claude_code::{ClaudeCode, close_sessions as close_claude_sessions};
pub use codex::{Codex, close_sessions as close_codex_sessions};
pub use decide::{Decider, Decision, DecisionOption, LocalDecider, Ranked, SemIf};
pub use mcp::{McpClient, McpTool};
pub use openai::{
    MAX_TOOL_STEPS, OpenAiCompat, ToolChatEnd, first_chat_model, is_chat_model, is_embedding_model,
    is_vision_model, strip_thinking,
};
pub use router::{Answer, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
pub use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub system: String,
    pub messages: Vec<Message>,
    /// A PNG the latest question is about (a screenshot), if any.
    pub image: Option<Vec<u8>>,
}

impl ChatRequest {
    pub fn image_base64(&self) -> Option<String> {
        use base64::Engine;
        self.image
            .as_ref()
            .map(|png| base64::engine::general_purpose::STANDARD.encode(png))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("no AI provider is set up and reachable")]
    NoProvider,
    #[error("{0}")]
    Failed(String),
    #[error("cancelled")]
    Cancelled,
}

impl From<reqwest::Error> for AiError {
    fn from(err: reqwest::Error) -> Self {
        Self::Failed(err.to_string())
    }
}

/// Where streamed text goes. Remembers whether anything was sent, so the
/// router only falls back before the user has seen a partial answer.
pub struct Sink {
    tx: UnboundedSender<String>,
    sent: AtomicBool,
}

impl Sink {
    pub fn new(tx: UnboundedSender<String>) -> Self {
        Self {
            tx,
            sent: AtomicBool::new(false),
        }
    }

    pub fn send(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.sent.store(true, Ordering::Relaxed);
        let _ = self.tx.send(text.to_owned());
    }

    pub fn has_sent(&self) -> bool {
        self.sent.load(Ordering::Relaxed)
    }
}

/// A tool a model may call, in OpenAI function form.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments.
    pub parameters: serde_json::Value,
}

/// Runs the tools a model asks for. The result (or an error) goes back to
/// the model as text.
#[async_trait]
pub trait ToolRunner: Send + Sync {
    async fn run(&self, name: &str, arguments: &serde_json::Value) -> String;
    /// Called once per tool call, before it runs, for progress text.
    fn started(&self, _name: &str, _arguments: &serde_json::Value) {}
    /// The tool only reads, so it may run at the same time as others the
    /// model asked for in the same round.
    fn parallel(&self, _name: &str) -> bool {
        false
    }
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    /// Stable id used in settings: `claude_code`, `local`, `anthropic`.
    fn id(&self) -> &'static str;
    /// Runs on this machine; nothing leaves it.
    fn is_local(&self) -> bool;
    /// Cheap check that the provider can answer right now.
    async fn available(&self) -> bool;
    /// Gets ready for a question (loads a model, starts a session) while
    /// the user is still typing. Nothing by default.
    async fn warm(&self) {}
    /// Streams the answer into `sink` and returns the full text.
    async fn chat(
        &self,
        req: &ChatRequest,
        sink: &Sink,
        cancel: &CancellationToken,
    ) -> Result<String, AiError>;
}

/// Closes warm agent sessions that have been idle too long. Call it now
/// and then (every minute or so).
pub fn sweep_sessions() {
    claude_code::sweep_sessions();
    codex::sweep_sessions();
}

/// Flattens a conversation into one prompt for providers that take a single
/// text input (Claude Code's `-p`).
pub fn transcript(req: &ChatRequest) -> String {
    let mut out = String::new();
    if !req.system.is_empty() {
        out.push_str(&req.system);
        out.push_str("\n\n");
    }
    let Some((last, earlier)) = req.messages.split_last() else {
        return out;
    };
    if !earlier.is_empty() {
        out.push_str("Conversation so far:\n");
        for m in earlier {
            let who = match m.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
            };
            out.push_str(&format!("{who}: {}\n", m.content));
        }
        out.push_str("\nReply to the user's latest message:\n");
    }
    out.push_str(&last.content);
    out
}

#[cfg(windows)]
pub(crate) fn hide_console(cmd: &mut tokio::process::Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn hide_console(_cmd: &mut tokio::process::Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_keeps_turns_in_order() {
        let req = ChatRequest {
            system: "Be brief.".into(),
            messages: vec![
                Message::user("hi"),
                Message {
                    role: Role::Assistant,
                    content: "hello".into(),
                },
                Message::user("what is 2+2?"),
            ],
            image: None,
        };
        let t = transcript(&req);
        assert!(t.starts_with("Be brief."));
        assert!(t.contains("User: hi\nAssistant: hello\n"));
        assert!(t.ends_with("what is 2+2?"));
    }

    #[test]
    fn single_message_has_no_history_header() {
        let req = ChatRequest {
            system: String::new(),
            messages: vec![Message::user("hello")],
            image: None,
        };
        assert_eq!(transcript(&req), "hello");
    }
}
