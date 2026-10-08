use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{AiError, AiProvider, CancellationToken, ChatRequest, Sink, sse};

pub const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const URL: &str = "https://api.anthropic.com/v1/messages";

/// Chat with an Anthropic API key, for people without Claude Code. The key is
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

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Usage {
    input: u64,
    cache_write: u64,
    cache_read: u64,
    output: u64,
}

fn usage_of(u: &Value) -> Usage {
    let n = |k: &str| u[k].as_u64().unwrap_or(0);
    Usage {
        input: n("input_tokens"),
        cache_write: n("cache_creation_input_tokens"),
        cache_read: n("cache_read_input_tokens"),
        output: n("output_tokens"),
    }
}

/// Dollars per million input and output tokens, by model family; None for a
/// model whose price Sidekick does not know (the answer then shows no cost).
fn price(model: &str) -> Option<(f64, f64)> {
    let m = model.to_lowercase();
    if m.contains("haiku") {
        Some((1.0, 5.0))
    } else if m.contains("sonnet") {
        Some((3.0, 15.0))
    } else if m.contains("opus") {
        Some((5.0, 25.0))
    } else {
        None
    }
}

/// What an answer cost: cache writes at 1.25 times the input price, cache
/// reads at a tenth of it.
fn cost(model: &str, u: Usage) -> Option<f64> {
    let (input, output) = price(model)?;
    let tokens_in = u.input as f64 + u.cache_write as f64 * 1.25 + u.cache_read as f64 * 0.1;
    Some((tokens_in * input + u.output as f64 * output) / 1_000_000.0)
}

/// What one SSE payload from the Messages API means for us.
#[derive(Debug, PartialEq)]
enum Event {
    Text(String),
    /// Tokens so far: input (with cache writes and reads) and output.
    Usage(Usage),
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
        Some("message_start") => Event::Usage(usage_of(&v["message"]["usage"])),
        Some("message_delta") if v["usage"].is_object() => Event::Usage(usage_of(&v["usage"])),
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
        let mut used = Usage::default();
        sse::each_data(resp, cancel, |data| {
            match parse_event(data) {
                Event::Text(t) => {
                    sink.send(&t);
                    full.push_str(&t);
                }
                // message_start has the input; message_delta the running output.
                Event::Usage(u) => {
                    used = Usage {
                        input: used.input.max(u.input),
                        cache_write: used.cache_write.max(u.cache_write),
                        cache_read: used.cache_read.max(u.cache_read),
                        output: used.output.max(u.output),
                    };
                }
                Event::Refused => failure = Some("Claude declined to answer this one.".to_owned()),
                Event::Error(e) => failure = Some(e),
                Event::Other => {}
            }
            Ok(true)
        })
        .await?;
        if let Some(usd) = cost(&self.model, used) {
            sink.set_cost(usd);
        }
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
    fn prices_an_answer() {
        let e = parse_event(
            r#"{"type":"message_start","message":{"usage":{"input_tokens":2000,"cache_read_input_tokens":10000,"output_tokens":1}}}"#,
        );
        let Event::Usage(u) = e else { panic!("{e:?}") };
        let u = Usage { output: 500, ..u };
        // Haiku: 2000 in + 10000 cached * 0.1 = 3000 at $1/M, 500 out at $5/M.
        let c = cost("claude-haiku-4-5-20251001", u).unwrap();
        assert!((c - 0.0055).abs() < 1e-9, "{c}");
        assert_eq!(cost("some-other-model", u), None);
        assert!(matches!(
            parse_event(
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}"#
            ),
            Event::Usage(Usage { output: 42, .. })
        ));
    }

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
            think: false,
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
            think: false,
        });
        let content = &body["messages"][0]["content"];
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["source"]["data"], "AQID");
        assert_eq!(content[1]["text"], "what is this?");
    }
}
