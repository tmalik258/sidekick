use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{AiError, AiProvider, CancellationToken, ChatRequest, Sink, sse};

pub const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const URL: &str = "https://api.anthropic.com/v1/messages";

/// T2 with an Anthropic API key, for people without Claude Code. The key is
/// read from `ANTHROPIC_API_KEY` at call time and never stored by Sidekick.
pub struct Anthropic {
    pub model: String,
    client: reqwest::Client,
}

impl Anthropic {
    pub fn new(model: Option<String>) -> Self {
        Self {
            model: model
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_MODEL.into()),
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    fn key() -> Option<String> {
        std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
    }

    fn body(&self, req: &ChatRequest) -> Value {
        let mut body = json!({
            "model": self.model,
            "max_tokens": 16000,
            "stream": true,
            "messages": req.messages,
        });
        if !req.system.is_empty() {
            body["system"] = json!(req.system);
        }
        // The image goes with the latest question.
        if let (Some(data), Some(last)) = (
            req.image_base64(),
            body["messages"].as_array_mut().and_then(|m| m.last_mut()),
        ) {
            let text = last["content"].clone();
            last["content"] = json!([
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": data } },
                { "type": "text", "text": text },
            ]);
        }
        // If a safety classifier declines, let the API retry on another model
        // within the same call instead of failing the answer.
        body["fallbacks"] = json!("default");
        body
    }
}

/// What one SSE payload from the Messages API means for us.
#[derive(Debug, PartialEq)]
enum Event {
    Text(String),
    Refused,
    Error(String),
    Other,
}

fn parse_event(data: &str) -> Event {
    let Ok(v) = serde_json::from_str::<Value>(data) else {
        return Event::Other;
    };
    match v["type"].as_str() {
        Some("content_block_delta") if v["delta"]["type"] == "text_delta" => {
            Event::Text(v["delta"]["text"].as_str().unwrap_or_default().to_owned())
        }
        Some("message_delta") if v["delta"]["stop_reason"] == "refusal" => Event::Refused,
        Some("error") => Event::Error(
            v["error"]["message"]
                .as_str()
                .unwrap_or("Anthropic API error")
                .to_owned(),
        ),
        _ => Event::Other,
    }
}

#[async_trait]
impl AiProvider for Anthropic {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn is_local(&self) -> bool {
        false
    }

    async fn available(&self) -> bool {
        Self::key().is_some()
    }

    async fn chat(
        &self,
        req: &ChatRequest,
        sink: &Sink,
        cancel: &CancellationToken,
    ) -> Result<String, AiError> {
        let key =
            Self::key().ok_or_else(|| AiError::Failed("ANTHROPIC_API_KEY is not set".into()))?;
        let send = self
            .client
            .post(URL)
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "server-side-fallback-2026-07-01")
            .json(&self.body(req))
            .send();
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(AiError::Cancelled),
            resp = send => resp?,
        };
        let resp = sse::check(resp).await?;
        let mut full = String::new();
        let mut failure = None;
        sse::each_data(resp, cancel, |data| {
            match parse_event(data) {
                Event::Text(t) => {
                    sink.send(&t);
                    full.push_str(&t);
                }
                Event::Refused => failure = Some("Claude declined to answer this one.".to_owned()),
                Event::Error(e) => failure = Some(e),
                Event::Other => {}
            }
            Ok(true)
        })
        .await?;
        match failure {
            Some(msg) if full.is_empty() => Err(AiError::Failed(msg)),
            _ => Ok(full),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    #[test]
    fn parses_stream_events() {
        assert_eq!(
            parse_event(
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#
            ),
            Event::Text("Hi".into())
        );
        assert_eq!(
            parse_event(r#"{"type":"message_delta","delta":{"stop_reason":"refusal"}}"#),
            Event::Refused
        );
        assert_eq!(
            parse_event(
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#
            ),
            Event::Error("Overloaded".into())
        );
        assert_eq!(parse_event(r#"{"type":"message_stop"}"#), Event::Other);
    }

    #[test]
    fn body_has_system_and_messages() {
        let a = Anthropic::new(None);
        let body = a.body(&ChatRequest {
            system: "Be brief.".into(),
            messages: vec![Message::user("hi")],
            image: None,
        });
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["system"], "Be brief.");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn images_ride_with_the_last_question() {
        let a = Anthropic::new(None);
        let body = a.body(&ChatRequest {
            system: String::new(),
            messages: vec![Message::user("what is this?")],
            image: Some(vec![1, 2, 3]),
        });
        let content = &body["messages"][0]["content"];
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["source"]["data"], "AQID");
        assert_eq!(content[1]["text"], "what is this?");
    }
}
