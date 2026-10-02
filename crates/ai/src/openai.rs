use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{AiError, AiProvider, CancellationToken, ChatRequest, Sink, ToolDef, ToolRunner, sse};

pub const OLLAMA_URL: &str = "http://localhost:11434/v1";

/// T1b: a local model through any OpenAI-compatible server (Ollama, LM
/// Studio, llama.cpp server, vLLM). Nothing leaves the machine when the URL
/// is local.
pub struct OpenAiCompat {
    pub base_url: String,
    /// Empty means "the first model the server lists".
    pub model: String,
    client: reqwest::Client,
}

impl OpenAiCompat {
    pub fn new(base_url: Option<String>, model: Option<String>) -> Self {
        let base_url = base_url
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| OLLAMA_URL.into())
            .trim_end_matches('/')
            .to_owned();
        Self {
            base_url,
            model: model.unwrap_or_default().trim().to_owned(),
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Models the server offers, or None when it is not reachable.
    pub async fn models(&self) -> Option<Vec<String>> {
        let resp = self
            .client
            .get(format!("{}/models", self.base_url))
            .timeout(Duration::from_millis(1500))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let v: Value = resp.json().await.ok()?;
        Some(
            v["data"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|m| m["id"].as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        )
    }

    pub async fn pick_model(&self) -> Result<String, AiError> {
        if !self.model.is_empty() {
            return Ok(self.model.clone());
        }
        self.models()
            .await
            .and_then(|m| m.into_iter().next())
            .ok_or_else(|| {
                AiError::Failed(format!(
                    "no model found at {} (try: ollama pull qwen3:4b)",
                    self.base_url
                ))
            })
    }

    fn body(model: &str, req: &ChatRequest, stream: bool) -> Value {
        let mut messages = Vec::with_capacity(req.messages.len() + 1);
        if !req.system.is_empty() {
            messages.push(json!({"role": "system", "content": req.system}));
        }
        messages.extend(req.messages.iter().map(|m| json!(m)));
        if let (Some(data), Some(last)) = (req.image_base64(), messages.last_mut()) {
            let text = last["content"].clone();
            last["content"] = json!([
                { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{data}") } },
                { "type": "text", "text": text },
            ]);
        }
        json!({"model": model, "stream": stream, "messages": messages})
    }

    /// One non-streamed completion, for short structured answers.
    pub async fn complete(&self, req: &ChatRequest, extra: Value) -> Result<String, AiError> {
        let model = self.pick_model().await?;
        let mut body = Self::body(&model, req, false);
        if let (Some(b), Some(e)) = (body.as_object_mut(), extra.as_object()) {
            b.extend(e.clone());
        }
        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body)
            .send()
            .await?;
        let v: Value = sse::check(resp).await?.json().await?;
        Ok(v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_owned())
    }
}

/// Most tool rounds before the model must answer with what it has.
pub const MAX_TOOL_STEPS: usize = 6;
/// Longest tool result passed back to the model; small models have small
/// contexts.
const MAX_TOOL_RESULT: usize = 6_000;

/// How a chat with tools ended.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolChatEnd {
    pub text: String,
    /// The model was still calling tools when the step limit was reached.
    pub out_of_steps: bool,
    /// Tools that ran, in order.
    pub calls: Vec<String>,
}

impl OpenAiCompat {
    /// A chat where the model may call `tools` through `runner`. Each round
    /// is one non-streamed completion; the final answer goes to `sink` in
    /// one piece.
    pub async fn chat_with_tools(
        &self,
        req: &ChatRequest,
        tools: &[ToolDef],
        runner: &dyn ToolRunner,
        sink: &Sink,
        cancel: &CancellationToken,
    ) -> Result<ToolChatEnd, AiError> {
        let model = self.pick_model().await?;
        let mut messages = Self::body(&model, req, false)["messages"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let tool_json: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({ "type": "function", "function": {
                    "name": t.name, "description": t.description, "parameters": t.parameters,
                }})
            })
            .collect();
        let mut calls = Vec::new();
        for step in 0..=MAX_TOOL_STEPS {
            let last = step == MAX_TOOL_STEPS;
            let mut body = json!({ "model": model, "stream": false, "messages": messages });
            // On the last round the model has to answer, not call more tools.
            if !last && !tool_json.is_empty() {
                body["tools"] = Value::Array(tool_json.clone());
            }
            let send = self
                .client
                .post(format!("{}/chat/completions", self.base_url))
                .json(&body)
                .send();
            let resp = tokio::select! {
                _ = cancel.cancelled() => return Err(AiError::Cancelled),
                resp = send => resp?,
            };
            let v: Value = sse::check(resp).await?.json().await?;
            let msg = &v["choices"][0]["message"];
            let wanted = msg["tool_calls"].as_array().filter(|c| !c.is_empty());
            if let (Some(wanted), false) = (wanted, last) {
                messages.push(json!({
                    "role": "assistant",
                    "content": msg["content"].as_str().unwrap_or_default(),
                    "tool_calls": wanted,
                }));
                for (i, call) in wanted.iter().enumerate() {
                    let name = call["function"]["name"].as_str().unwrap_or_default();
                    let args = parse_arguments(&call["function"]["arguments"]);
                    let id = call["id"]
                        .as_str()
                        .map_or_else(|| format!("call_{step}_{i}"), str::to_owned);
                    runner.started(name);
                    calls.push(name.to_owned());
                    let result = tokio::select! {
                        _ = cancel.cancelled() => return Err(AiError::Cancelled),
                        r = runner.run(name, &args) => r,
                    };
                    let result: String = result.chars().take(MAX_TOOL_RESULT).collect();
                    messages.push(json!({
                        "role": "tool", "tool_call_id": id, "name": name, "content": result,
                    }));
                }
                continue;
            }
            let text = strip_thinking(msg["content"].as_str().unwrap_or_default());
            sink.send(&text);
            return Ok(ToolChatEnd {
                text,
                out_of_steps: last,
                calls,
            });
        }
        unreachable!("the last round always returns")
    }
}

/// Tool arguments arrive as a JSON string (OpenAI) or an object (some
/// servers).
fn parse_arguments(raw: &Value) -> Value {
    match raw {
        Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| json!({})),
        Value::Object(_) => raw.clone(),
        _ => json!({}),
    }
}

/// Removes a `<think>...</think>` block some models (Qwen3) put first.
pub fn strip_thinking(text: &str) -> String {
    let t = text.trim_start();
    if let Some(rest) = t.strip_prefix("<think>") {
        return match rest.find("</think>") {
            Some(end) => rest[end + "</think>".len()..].trim().to_owned(),
            None => String::new(),
        };
    }
    text.trim().to_owned()
}

fn delta(data: &str) -> Option<String> {
    let v: Value = serde_json::from_str(data).ok()?;
    v["choices"][0]["delta"]["content"]
        .as_str()
        .map(str::to_owned)
}

pub(crate) fn is_local_url(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or(url);
    if rest.starts_with("[::1]") {
        return true;
    }
    let host = rest.split(['/', ':']).next().unwrap_or_default();
    host == "localhost" || host.starts_with("127.")
}

impl OpenAiCompat {
    /// Embeds `inputs` with `model` through `/embeddings` (semantic search).
    /// Refused for servers that are not on this PC, so indexed text never
    /// leaves it.
    pub async fn embed(&self, model: &str, inputs: &[String]) -> Result<Vec<Vec<f32>>, AiError> {
        if !is_local_url(&self.base_url) {
            return Err(AiError::Failed(
                "embeddings only use a model on this PC".into(),
            ));
        }
        let resp = self
            .client
            .post(format!("{}/embeddings", self.base_url))
            .timeout(Duration::from_secs(60))
            .json(&serde_json::json!({ "model": model, "input": inputs }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(AiError::Failed(format!(
                "embedding model {model} answered {}",
                resp.status()
            )));
        }
        let body: serde_json::Value = resp.json().await?;
        let mut data: Vec<(usize, Vec<f32>)> = body["data"]
            .as_array()
            .ok_or_else(|| AiError::Failed("no embeddings in the answer".into()))?
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let index = d["index"].as_u64().map_or(i, |n| n as usize);
                let vec = d["embedding"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_f64())
                            .map(|x| x as f32)
                            .collect()
                    })
                    .unwrap_or_default();
                (index, vec)
            })
            .collect();
        data.sort_by_key(|(i, _)| *i);
        if data.len() != inputs.len() || data.iter().any(|(_, v)| v.is_empty()) {
            return Err(AiError::Failed(
                "embedding answer did not match the input".into(),
            ));
        }
        Ok(data.into_iter().map(|(_, v)| v).collect())
    }
}

#[async_trait]
impl AiProvider for OpenAiCompat {
    fn id(&self) -> &'static str {
        "local"
    }

    fn is_local(&self) -> bool {
        is_local_url(&self.base_url)
    }

    async fn available(&self) -> bool {
        self.models().await.is_some_and(|m| !m.is_empty())
    }

    async fn chat(
        &self,
        req: &ChatRequest,
        sink: &Sink,
        cancel: &CancellationToken,
    ) -> Result<String, AiError> {
        let model = self.pick_model().await?;
        let send = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&Self::body(&model, req, true))
            .send();
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(AiError::Cancelled),
            resp = send => resp?,
        };
        let resp = sse::check(resp).await?;
        let mut full = String::new();
        sse::each_data(resp, cancel, |data| {
            if let Some(t) = delta(data) {
                sink.send(&t);
                full.push_str(&t);
            }
            Ok(true)
        })
        .await?;
        Ok(full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;
    use async_trait::async_trait;

    #[test]
    fn reads_stream_deltas() {
        assert_eq!(
            delta(r#"{"choices":[{"delta":{"content":"Hi"}}]}"#),
            Some("Hi".into())
        );
        assert_eq!(
            delta(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#),
            None
        );
    }

    #[test]
    fn system_prompt_goes_first() {
        let body = OpenAiCompat::body(
            "qwen3:4b",
            &ChatRequest {
                system: "Be brief.".into(),
                messages: vec![Message::user("hi")],
                image: None,
            },
            true,
        );
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "hi");
    }

    #[test]
    fn knows_local_urls() {
        assert!(is_local_url("http://localhost:11434/v1"));
        assert!(is_local_url("http://127.0.0.1:1234/v1"));
        assert!(is_local_url("http://[::1]:8080/v1"));
        assert!(!is_local_url("https://api.openai.com/v1"));
        assert!(!is_local_url("http://192.168.1.5:11434/v1"));
    }

    #[test]
    fn strips_thinking() {
        assert_eq!(strip_thinking("<think>hmm</think>\n\nHello"), "Hello");
        assert_eq!(strip_thinking("<think>never closed"), "");
        assert_eq!(strip_thinking("  plain  "), "plain");
    }

    #[test]
    fn reads_tool_arguments() {
        assert_eq!(parse_arguments(&json!(r#"{"q":"x"}"#))["q"], "x");
        assert_eq!(parse_arguments(&json!({ "q": "y" }))["q"], "y");
        assert_eq!(parse_arguments(&json!("not json")), json!({}));
    }

    /// A fake OpenAI server. With tools offered, it asks for `LIST` until a
    /// tool result is in the conversation (or forever when `stubborn`).
    async fn fake_model(stubborn: bool) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Value>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let log = log.clone();
                tokio::spawn(async move {
                    let mut got = Vec::new();
                    let mut buf = vec![0u8; 65536];
                    loop {
                        let n = sock.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        got.extend_from_slice(&buf[..n]);
                        let t = String::from_utf8_lossy(&got).to_string();
                        if let Some(end) = t.find("\r\n\r\n") {
                            let len = t[..end]
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if got.len() >= end + 4 + len {
                                break;
                            }
                        }
                    }
                    let t = String::from_utf8_lossy(&got).to_string();
                    let body: Value =
                        serde_json::from_str(t.split_once("\r\n\r\n").map_or("", |x| x.1))
                            .unwrap_or(Value::Null);
                    log.lock().unwrap().push(body.clone());
                    let has_result = body["messages"]
                        .as_array()
                        .is_some_and(|m| m.iter().any(|m| m["role"] == "tool"));
                    let message = if body.get("tools").is_some() && (stubborn || !has_result) {
                        json!({ "role": "assistant", "content": "", "tool_calls": [
                            { "id": "c1", "type": "function", "function": { "name": "LIST", "arguments": "{\"state\":\"open\"}" } }
                        ]})
                    } else {
                        json!({ "role": "assistant", "content": "<think>ok</think>You have 3 open issues." })
                    };
                    let payload = json!({ "choices": [{ "message": message }] }).to_string();
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                        payload.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}/v1"), seen)
    }

    struct Recorder(std::sync::Mutex<Vec<(String, Value)>>);

    #[async_trait]
    impl ToolRunner for Recorder {
        async fn run(&self, name: &str, arguments: &Value) -> String {
            self.0
                .lock()
                .unwrap()
                .push((name.to_owned(), arguments.clone()));
            "3 issues".into()
        }
    }

    fn list_tool() -> Vec<ToolDef> {
        vec![ToolDef {
            name: "LIST".into(),
            description: "List issues".into(),
            parameters: json!({ "type": "object" }),
        }]
    }

    #[tokio::test]
    async fn calls_tools_then_answers() {
        let (url, seen) = fake_model(false).await;
        let model = OpenAiCompat::new(Some(url), Some("qwen3:1.7b".into()));
        let runner = Recorder(Default::default());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let end = model
            .chat_with_tools(
                &ChatRequest {
                    system: "s".into(),
                    messages: vec![Message::user("my issues?")],
                    image: None,
                },
                &list_tool(),
                &runner,
                &Sink::new(tx),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(end.text, "You have 3 open issues.");
        assert!(!end.out_of_steps);
        assert_eq!(end.calls, vec!["LIST"]);
        assert_eq!(rx.recv().await.as_deref(), Some("You have 3 open issues."));
        let ran = runner.0.lock().unwrap().clone();
        assert_eq!(ran, vec![("LIST".to_owned(), json!({ "state": "open" }))]);
        let seen = seen.lock().unwrap();
        let second = &seen[1]["messages"];
        let tool_msg = second
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "tool")
            .unwrap();
        assert_eq!(tool_msg["tool_call_id"], "c1");
        assert_eq!(tool_msg["content"], "3 issues");
    }

    #[tokio::test]
    async fn stops_after_the_step_limit() {
        let (url, seen) = fake_model(true).await;
        let model = OpenAiCompat::new(Some(url), Some("m".into()));
        let runner = Recorder(Default::default());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let end = model
            .chat_with_tools(
                &ChatRequest {
                    system: String::new(),
                    messages: vec![Message::user("loop")],
                    image: None,
                },
                &list_tool(),
                &runner,
                &Sink::new(tx),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(end.out_of_steps);
        assert_eq!(end.calls.len(), MAX_TOOL_STEPS);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), MAX_TOOL_STEPS + 1);
        assert!(
            seen.last().unwrap().get("tools").is_none(),
            "the last round offers no tools"
        );
    }

    #[test]
    fn default_url_is_ollama() {
        assert_eq!(OpenAiCompat::new(None, None).base_url, OLLAMA_URL);
        assert_eq!(
            OpenAiCompat::new(Some("http://x:1/v1/".into()), None).base_url,
            "http://x:1/v1"
        );
    }
}
