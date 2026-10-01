use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{AiError, AiProvider, CancellationToken, ChatRequest, Sink, sse};

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
    fn default_url_is_ollama() {
        assert_eq!(OpenAiCompat::new(None, None).base_url, OLLAMA_URL);
        assert_eq!(
            OpenAiCompat::new(Some("http://x:1/v1/".into()), None).base_url,
            "http://x:1/v1"
        );
    }
}
