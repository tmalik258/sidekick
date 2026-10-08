use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{AiError, AiProvider, CancellationToken, ChatRequest, Sink, ToolDef, ToolRunner, sse};

pub const OLLAMA_URL: &str = "http://127.0.0.1:11434/v1";

/// T1b: a local model through any OpenAI-compatible server (Ollama, LM
/// Studio, llama.cpp server, vLLM). Nothing leaves the machine when the URL
/// is local.
pub struct OpenAiCompat {
    pub base_url: String,
    /// Empty means "the first model the server lists".
    pub model: String,
    client: reqwest::Client,
    /// The loaded model's context in tokens, once Ollama has said (by model).
    context: std::sync::Mutex<Option<(String, usize)>>,
}

/// Ollama's own default when it does not say; smaller than most prompts with tools.
const DEFAULT_CONTEXT: usize = 4_096;

impl OpenAiCompat {
    pub fn new(base_url: Option<String>, model: Option<String>) -> Self {
        let base_url = base_url
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| OLLAMA_URL.into());
        let base_url = loopback(base_url.trim_end_matches('/'));
        let mut client = reqwest::Client::builder().connect_timeout(Duration::from_secs(3));
        // A system or VPN proxy cannot reach this PC's own 127.0.0.1, so
        // Ollama only answered once it was exposed to the network.
        if is_local_url(&base_url) {
            client = client.no_proxy();
        }
        Self {
            base_url,
            model: model.unwrap_or_default().trim().to_owned(),
            client: client.build().unwrap_or_default(),
            context: std::sync::Mutex::new(None),
        }
    }

    /// How many tokens `model` can take, from Ollama's list of loaded models;
    /// the default when the server is not Ollama or has not loaded it yet.
    async fn context_tokens(&self, model: &str) -> usize {
        if let Ok(c) = self.context.lock()
            && let Some((m, n)) = c.as_ref()
            && m == model
        {
            return *n;
        }
        let root = self.base_url.trim_end_matches("/v1");
        let found = async {
            let v: Value = self
                .client
                .get(format!("{root}/api/ps"))
                .timeout(Duration::from_millis(800))
                .send()
                .await
                .ok()?
                .json()
                .await
                .ok()?;
            v["models"]
                .as_array()?
                .iter()
                .find(|m| m["name"] == model || m["model"] == model)
                .and_then(|m| m["context_length"].as_u64())
        }
        .await;
        match found {
            Some(n) => {
                let n = usize::try_from(n).unwrap_or(DEFAULT_CONTEXT);
                if let Ok(mut c) = self.context.lock() {
                    *c = Some((model.to_owned(), n));
                }
                n
            }
            None => DEFAULT_CONTEXT,
        }
    }

    /// Characters the messages may take, after the tools and the answer.
    async fn budget(&self, model: &str, tools_chars: usize) -> usize {
        let tokens = self.context_tokens(model).await;
        (tokens.saturating_sub(crate::fit::ANSWER_TOKENS) * crate::fit::CHARS_PER_TOKEN)
            .saturating_sub(tools_chars)
            .max(2_000)
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
            .and_then(|m| first_chat_model(&m).map(str::to_owned))
            .ok_or_else(|| {
                AiError::Failed(format!(
                    "no model found at {} (try: ollama pull qwen3:4b)",
                    self.base_url
                ))
            })
    }

    fn body(model: &str, req: &ChatRequest, stream: bool) -> Value {
        let mut messages = Vec::with_capacity(req.messages.len() + 1);
        // Qwen3 thinks before every answer by default, and Ollama sends none
        // of it until it is done: 10 to 25 seconds before the first word on
        // a laptop, for questions that do not need it.
        let system = if thinks_by_default(model) && !req.think {
            format!("{}\n/no_think", req.system).trim_start().to_owned()
        } else {
            req.system.clone()
        };
        if !system.is_empty() {
            messages.push(json!({"role": "system", "content": system}));
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

/// Qwen3 models that think unless told `/no_think` (the 2507 instruct
/// builds never think; the thinking builds ignore it).
fn thinks_by_default(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("qwen3")
        && !m.starts_with("qwen3-")
        && !m.contains("2507")
        && !m.contains("instruct")
}

/// Warming the same model again within this time does nothing.
const WARM_AGAIN: Duration = Duration::from_secs(60);
static LAST_WARM: std::sync::Mutex<Option<(String, std::time::Instant)>> =
    std::sync::Mutex::new(None);

/// `localhost` as `127.0.0.1`. Windows tries `::1` first, and Ollama only
/// listens on IPv4 unless "Expose Ollama to the network" is on, so a refused
/// IPv6 connect (about 2 seconds on Windows) made a running Ollama look
/// stopped.
pub fn loopback(url: &str) -> String {
    match url.split_once("://localhost") {
        Some((scheme, rest)) if rest.is_empty() || rest.starts_with([':', '/']) => {
            format!("{scheme}://127.0.0.1{rest}")
        }
        _ => url.to_owned(),
    }
}

/// Ollama's own API root (without `/v1`), when the URL looks like Ollama.
fn ollama_root(base_url: &str) -> Option<&str> {
    let root = base_url.strip_suffix("/v1")?;
    root.contains(":11434").then_some(root)
}

/// The model to chat with when none is chosen: the first one listed (Ollama
/// lists the newest first) that can chat: not embedding, not vision-only.
pub fn first_chat_model(models: &[String]) -> Option<&str> {
    models.iter().map(String::as_str).find(|m| is_chat_model(m))
}

/// Embedding models (for search) cannot hold a conversation.
pub fn is_embedding_model(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("embed") || n.starts_with("bge") || n.contains("minilm") || n.contains("e5-")
}

/// Vision-only models (e.g. moondream) see pictures; they are not chat models.
pub fn is_vision_model(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("moondream")
        || n.contains("llava")
        || n.contains("bakllava")
        || n.contains("minicpm-v")
        || n.contains("qwen2-vl")
        || n.contains("qwen2.5-vl")
        || n.contains("vision")
}

/// A model Sidekick can use for local chat (not search, not vision-only).
pub fn is_chat_model(name: &str) -> bool {
    !is_embedding_model(name) && !is_vision_model(name)
}

/// Most tool rounds before the model must answer with what it has.
pub const MAX_TOOL_STEPS: usize = 12;
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
        let budget = self
            .budget(
                &model,
                serde_json::to_string(&tool_json).map_or(0, |s| s.len()),
            )
            .await;
        let mut calls = Vec::new();
        let mut shown = String::new();
        for step in 0..=MAX_TOOL_STEPS {
            let last = step == MAX_TOOL_STEPS;
            crate::fit::fit(&mut messages, budget);
            let mut body = json!({ "model": model, "stream": true, "messages": messages });
            // On the last round the model has to answer, not call more tools.
            if !last && !tool_json.is_empty() {
                body["tools"] = Value::Array(tool_json.clone());
            }
            // Words reach the user as they are written; a round that turns
            // out to call tools just carries on below what it said.
            let gap = if shown.is_empty() { "" } else { "\n\n" };
            let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            let round = self.stream_round(&body, &names, sink, gap, cancel).await?;
            shown.push_str(&round.text);
            let msg = json!({ "content": round.raw, "tool_calls": round.calls });
            let msg = &msg;
            let wanted = msg["tool_calls"].as_array().filter(|c| !c.is_empty());
            if let (Some(wanted), false) = (wanted, last) {
                messages.push(json!({
                    "role": "assistant",
                    "content": msg["content"].as_str().unwrap_or_default(),
                    "tool_calls": wanted,
                }));
                let parsed: Vec<(String, String, Value)> = wanted
                    .iter()
                    .enumerate()
                    .map(|(i, call)| {
                        let name = call["function"]["name"].as_str().unwrap_or_default();
                        let id = call["id"]
                            .as_str()
                            .map_or_else(|| format!("call_{step}_{i}"), str::to_owned);
                        (
                            id,
                            name.to_owned(),
                            parse_arguments(&call["function"]["arguments"]),
                        )
                    })
                    .collect();
                // Reads asked for together run together; anything that acts
                // runs one after another, in order.
                let together =
                    parsed.len() > 1 && parsed.iter().all(|(_, n, _)| runner.parallel(n));
                let results: Vec<String> = if together {
                    for (_, name, args) in &parsed {
                        runner.started(name, args);
                        calls.push(name.clone());
                    }
                    let all = futures_util::future::join_all(
                        parsed.iter().map(|(_, name, args)| runner.run(name, args)),
                    );
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(AiError::Cancelled),
                        r = all => r,
                    }
                } else {
                    let mut out = Vec::with_capacity(parsed.len());
                    for (_, name, args) in &parsed {
                        runner.started(name, args);
                        calls.push(name.clone());
                        out.push(tokio::select! {
                            _ = cancel.cancelled() => return Err(AiError::Cancelled),
                            r = runner.run(name, args) => r,
                        });
                    }
                    out
                };
                for ((id, name, _), result) in parsed.iter().zip(results) {
                    let result: String = result.chars().take(MAX_TOOL_RESULT).collect();
                    messages.push(json!({
                        "role": "tool", "tool_call_id": id, "name": name, "content": result,
                    }));
                }
                continue;
            }
            let text = shown.trim().to_owned();
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

/// Whether text so far could still turn out to be a tool call written as
/// text: a JSON object, a code fence or a `<tool_call>` tag.
fn may_be_call(t: &str) -> bool {
    let prefix_of = |marker: &str| marker.starts_with(t) || t.starts_with(marker);
    t.starts_with('{') || prefix_of("```") || prefix_of("<tool_call>")
}

/// Whether text so far could still be a tool's name on a line of its own,
/// which small models echo before answering ("pc_status\nWi-Fi is ...").
fn may_be_name(t: &str, names: &[&str]) -> bool {
    !t.contains(char::is_whitespace) && names.iter().any(|n| n.starts_with(t) || t.starts_with(n))
}

/// `text` without a leading tool name the model echoed.
fn without_name<'a>(text: &'a str, names: &[&str]) -> &'a str {
    let t = text.trim_start();
    for n in names {
        if let Some(rest) = t.strip_prefix(n)
            && rest.starts_with(char::is_whitespace)
        {
            return rest.trim_start();
        }
    }
    text
}

/// A tool call the model wrote as text (`{"name": ..., "arguments": ...}`,
/// maybe fenced or tagged), when it names one of the `offered` tools.
fn text_call(text: &str, offered: &[&str]) -> Option<Value> {
    let mut t = text.trim();
    if let Some(inner) = t.strip_prefix("<tool_call>") {
        t = inner.trim_end().trim_end_matches("</tool_call>").trim();
    }
    if let Some(inner) = t.strip_prefix("```") {
        let inner = inner.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        t = inner.trim_end().trim_end_matches("```").trim();
    }
    let v: Value = serde_json::from_str(t).ok()?;
    let f = if v["function"].is_object() {
        &v["function"]
    } else {
        &v
    };
    let name = f["name"].as_str()?;
    if !offered.contains(&name) {
        return None;
    }
    let args = [&f["arguments"], &f["parameters"]]
        .into_iter()
        .map(parse_arguments)
        .find(|a| a.as_object().is_some_and(|o| !o.is_empty()))
        .unwrap_or_else(|| json!({}));
    Some(json!({ "id": "call_text", "type": "function",
                 "function": { "name": name, "arguments": args.to_string() } }))
}

/// What one streamed round produced.
struct Round {
    /// Everything the model wrote, thinking included (for the history).
    raw: String,
    /// What the user saw.
    text: String,
    /// Tool calls, pieced together from their streamed parts.
    calls: Vec<Value>,
}

impl OpenAiCompat {
    async fn stream_round(
        &self,
        body: &Value,
        names: &[&str],
        sink: &Sink,
        gap: &str,
        cancel: &CancellationToken,
    ) -> Result<Round, AiError> {
        let send = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(body)
            .send();
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(AiError::Cancelled),
            resp = send => resp?,
        };
        let resp = sse::check(resp).await?;
        let mut raw = String::new();
        let mut text = String::new();
        let mut think = Thinking::default();
        let mut parts: Vec<(String, String, String)> = Vec::new();
        let mut started = false;
        // Small models sometimes write a tool call as text instead of
        // calling it. A reply that starts like one is held back until the
        // round ends, so the user never sees raw JSON.
        let mut held = String::new();
        let mut holding = true;
        let mut show = |piece: &str, text: &mut String| {
            if !started {
                sink.send(gap);
                started = true;
            }
            sink.send(piece);
            text.push_str(piece);
        };
        sse::each_data(resp, cancel, |data| {
            let Ok(v) = serde_json::from_str::<Value>(data) else {
                return Ok(true);
            };
            let d = &v["choices"][0]["delta"];
            if let Some(piece) = d["content"].as_str() {
                raw.push_str(piece);
                let visible = think.push(piece);
                if !visible.is_empty() {
                    if holding {
                        held.push_str(&visible);
                        let t = held.trim_start();
                        if t.is_empty() || may_be_call(t) || may_be_name(t, names) {
                            return Ok(true);
                        }
                        holding = false;
                        let shown = without_name(&held, names).to_owned();
                        held.clear();
                        show(&shown, &mut text);
                    } else {
                        show(&visible, &mut text);
                    }
                }
            }
            for call in d["tool_calls"].as_array().into_iter().flatten() {
                let i = call["index"].as_u64().unwrap_or(parts.len() as u64) as usize;
                while parts.len() <= i {
                    parts.push(Default::default());
                }
                let part = &mut parts[i];
                if let Some(id) = call["id"].as_str() {
                    part.0 = id.to_owned();
                }
                if let Some(name) = call["function"]["name"].as_str() {
                    part.1.push_str(name);
                }
                if let Some(args) = call["function"]["arguments"].as_str() {
                    part.2.push_str(args);
                }
            }
            Ok(true)
        })
        .await?;
        let mut calls: Vec<Value> = parts
            .into_iter()
            .filter(|(_, name, _)| !name.is_empty())
            .map(|(id, name, args)| {
                json!({ "id": id, "type": "function",
                        "function": { "name": name, "arguments": if args.is_empty() { "{}".into() } else { args } } })
            })
            .collect();
        if !held.trim().is_empty() {
            let offered: Vec<&str> = body["tools"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t["function"]["name"].as_str())
                .collect();
            match text_call(&held, &offered) {
                Some(call) if calls.is_empty() => {
                    calls.push(call);
                    raw.clear();
                }
                _ => show(without_name(&held, names), &mut text),
            }
        }
        Ok(Round { raw, text, calls })
    }
}

/// Hides a leading `<think>...</think>` block while it streams in.
#[derive(Default)]
pub struct Thinking {
    state: ThinkState,
    buf: String,
}

#[derive(Default, PartialEq)]
enum ThinkState {
    #[default]
    Start,
    Inside,
    Out,
}

impl Thinking {
    /// The part of `piece` the user should see.
    pub fn push(&mut self, piece: &str) -> String {
        const OPEN: &str = "<think>";
        const CLOSE: &str = "</think>";
        match self.state {
            ThinkState::Out => piece.to_owned(),
            ThinkState::Start => {
                self.buf.push_str(piece);
                let t = self.buf.trim_start();
                if t.is_empty() || (t.len() < OPEN.len() && OPEN.starts_with(t)) {
                    return String::new();
                }
                if let Some(rest) = t.strip_prefix(OPEN) {
                    self.buf = rest.to_owned();
                    self.state = ThinkState::Inside;
                    return self.push("");
                }
                self.state = ThinkState::Out;
                std::mem::take(&mut self.buf).trim_start().to_owned()
            }
            ThinkState::Inside => {
                self.buf.push_str(piece);
                match self.buf.find(CLOSE) {
                    Some(end) => {
                        let rest = self.buf[end + CLOSE.len()..].trim_start().to_owned();
                        self.buf.clear();
                        self.state = if rest.is_empty() {
                            ThinkState::Start
                        } else {
                            ThinkState::Out
                        };
                        rest
                    }
                    None => String::new(),
                }
            }
        }
    }
}

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

pub fn is_local_url(url: &str) -> bool {
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

    /// Ollama unloads a model after a few idle minutes; this loads it (and
    /// keeps it for 10 minutes) while the user types. Every Ask open renews it.
    async fn warm(&self) {
        let Some(root) = ollama_root(&self.base_url) else {
            return;
        };
        let Ok(model) = self.pick_model().await else {
            return;
        };
        {
            let mut last = LAST_WARM.lock().unwrap_or_else(|e| e.into_inner());
            if last
                .as_ref()
                .is_some_and(|(m, at)| *m == model && at.elapsed() < WARM_AGAIN)
            {
                return;
            }
            *last = Some((model.clone(), std::time::Instant::now()));
        }
        let sent = self
            .client
            .post(format!("{root}/api/generate"))
            .json(&json!({ "model": model, "keep_alive": "10m" }))
            .timeout(Duration::from_secs(120))
            .send()
            .await;
        if let Err(err) = sent {
            log::debug!("could not warm {model}: {err}");
        }
    }

    async fn chat(
        &self,
        req: &ChatRequest,
        sink: &Sink,
        cancel: &CancellationToken,
    ) -> Result<String, AiError> {
        let model = self.pick_model().await?;
        let mut body = Self::body(&model, req, true);
        let budget = self.budget(&model, 0).await;
        if let Some(messages) = body["messages"].as_array_mut() {
            crate::fit::fit(messages, budget);
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

    #[test]
    fn warms_only_ollama() {
        assert_eq!(
            ollama_root("http://localhost:11434/v1"),
            Some("http://localhost:11434")
        );
        assert_eq!(ollama_root("http://localhost:1234/v1"), None);
        assert_eq!(ollama_root("http://localhost:11434"), None);
    }

    #[test]
    fn talks_to_ollama_over_ipv4() {
        assert_eq!(
            loopback("http://localhost:11434/v1"),
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(loopback("http://localhost"), "http://127.0.0.1");
        assert_eq!(
            loopback("http://localhost.lan:1/v1"),
            "http://localhost.lan:1/v1"
        );
        assert_eq!(
            loopback("http://192.168.1.5:11434/v1"),
            "http://192.168.1.5:11434/v1"
        );
        assert!(
            OpenAiCompat::new(None, None)
                .base_url
                .starts_with("http://127.0.0.1:11434")
        );
    }
    use crate::Message;
    use async_trait::async_trait;

    #[test]
    fn hides_thinking_while_it_streams() {
        let mut t = Thinking::default();
        let seen: String = [
            "<thi",
            "nk>planning the",
            " answer</th",
            "ink>\n\nHello",
            " there",
        ]
        .iter()
        .map(|p| t.push(p))
        .collect();
        assert_eq!(seen, "Hello there");
        let mut plain = Thinking::default();
        let seen: String = ["He", "llo"].iter().map(|p| plain.push(p)).collect();
        assert_eq!(seen, "Hello");
    }

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
                think: false,
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
    fn never_picks_an_embedding_model_to_chat() {
        let models = vec![
            "nomic-embed-text:latest".to_owned(),
            "qwen3:1.7b".to_owned(),
            "llama3.2:3b".to_owned(),
        ];
        assert_eq!(first_chat_model(&models), Some("qwen3:1.7b"));
        assert_eq!(first_chat_model(&["bge-m3:latest".to_owned()]), None);
        assert!(is_embedding_model("mxbai-embed-large"));
        assert!(!is_embedding_model("qwen3:4b"));
    }

    #[test]
    fn never_picks_a_vision_model_to_chat() {
        assert!(is_vision_model("moondream:latest"));
        assert!(!is_chat_model("moondream"));
        assert_eq!(
            first_chat_model(&["moondream:latest".to_owned(), "qwen3:4b".to_owned(),]),
            Some("qwen3:4b")
        );
        assert_eq!(first_chat_model(&["moondream".to_owned()]), None);
    }

    #[test]
    fn strips_thinking() {
        assert_eq!(strip_thinking("<think>hmm</think>\n\nHello"), "Hello");
        assert_eq!(strip_thinking("<think>never closed"), "");
        assert_eq!(strip_thinking("  plain  "), "plain");
    }

    #[test]
    fn qwen3_answers_without_thinking() {
        assert!(thinks_by_default("qwen3:1.7b"));
        assert!(!thinks_by_default("qwen3:4b-instruct-2507-q4_K_M"));
        assert!(!thinks_by_default("qwen3-coder:30b"));
        assert!(!thinks_by_default("llama3.2:3b"));
        let req = ChatRequest {
            system: "Be brief.".into(),
            ..Default::default()
        };
        let body = OpenAiCompat::body("qwen3:1.7b", &req, true);
        assert_eq!(body["messages"][0]["content"], "Be brief.\n/no_think");
        let req = ChatRequest { think: true, ..req };
        let body = OpenAiCompat::body("qwen3:1.7b", &req, true);
        assert_eq!(body["messages"][0]["content"], "Be brief.");
    }

    #[test]
    fn an_echoed_tool_name_is_hidden() {
        let names = ["pc_status", "search"];
        assert!(may_be_name("pc_st", &names));
        assert!(!may_be_name("Wi-Fi is", &names));
        assert_eq!(
            without_name("pc_status\nWi-Fi is on", &names),
            "Wi-Fi is on"
        );
        assert_eq!(without_name("search Find apps", &names), "Find apps");
        assert_eq!(
            without_name("searching the web", &names),
            "searching the web"
        );
    }

    #[test]
    fn reads_tool_calls_written_as_text() {
        let offered = ["continue_in_claude_code", "apps"];
        let c = text_call(
            r#"{"name": "continue_in_claude_code", "arguments": {"reason": "needs more"}}"#,
            &offered,
        )
        .unwrap();
        assert_eq!(c["function"]["name"], "continue_in_claude_code");
        assert_eq!(
            parse_arguments(&c["function"]["arguments"])["reason"],
            "needs more"
        );
        let fenced = "```json\n{\"name\": \"apps\", \"parameters\": {\"action\": \"find\"}}\n```";
        assert_eq!(
            parse_arguments(&text_call(fenced, &offered).unwrap()["function"]["arguments"])["action"],
            "find"
        );
        let tagged = r#"<tool_call>{"function": {"name": "apps", "arguments": "{}"}}</tool_call>"#;
        assert!(text_call(tagged, &offered).is_some());
        // Not an offered tool, or plain JSON the user asked for: shown as is.
        assert!(text_call(r#"{"name": "rm_rf", "arguments": {}}"#, &offered).is_none());
        assert!(text_call(r#"{"a": 1}"#, &offered).is_none());
        assert!(may_be_call("{\"na"));
        assert!(may_be_call("``"));
        assert!(may_be_call("<tool"));
        assert!(!may_be_call("Sure, "));
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
                    // Not Ollama: no list of loaded models.
                    if t.starts_with("GET ") {
                        let _ = sock
                            .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
                            .await;
                        return;
                    }
                    let body: Value =
                        serde_json::from_str(t.split_once("\r\n\r\n").map_or("", |x| x.1))
                            .unwrap_or(Value::Null);
                    log.lock().unwrap().push(body.clone());
                    let has_result = body["messages"]
                        .as_array()
                        .is_some_and(|m| m.iter().any(|m| m["role"] == "tool"));
                    // Streamed like a real server, in pieces.
                    let two = body["messages"].to_string().contains("two at once");
                    let deltas: Vec<Value> = if two && !has_result {
                        vec![json!({ "tool_calls": [
                            { "index": 0, "id": "a", "type": "function", "function": { "name": "LIST", "arguments": "{\"n\":1}" } },
                            { "index": 1, "id": "b", "type": "function", "function": { "name": "LIST", "arguments": "{\"n\":2}" } },
                        ] })]
                    } else if body.get("tools").is_some() && (stubborn || !has_result) {
                        vec![
                            json!({ "tool_calls": [{ "index": 0, "id": "c1", "type": "function", "function": { "name": "LIST", "arguments": "{\"state\":" } }] }),
                            json!({ "tool_calls": [{ "index": 0, "function": { "arguments": "\"open\"}" } }] }),
                        ]
                    } else {
                        ["<think>o", "k</think>You have ", "3 open issues."]
                            .iter()
                            .map(|c| json!({ "content": c }))
                            .collect()
                    };
                    let mut events = String::new();
                    for d in deltas {
                        events.push_str(&format!(
                            "data: {}\n\n",
                            json!({ "choices": [{ "delta": d }] })
                        ));
                    }
                    events.push_str("data: [DONE]\n\n");
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{events}",
                        events.len()
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
                    think: false,
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
        // Streamed in pieces, thinking hidden.
        let mut streamed = String::new();
        while let Ok(piece) = rx.try_recv() {
            streamed.push_str(&piece);
        }
        assert_eq!(streamed, "You have 3 open issues.");
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

    /// Takes a while per call; reads may run together.
    struct Slow(bool);

    #[async_trait]
    impl ToolRunner for Slow {
        async fn run(&self, _name: &str, arguments: &Value) -> String {
            tokio::time::sleep(Duration::from_millis(300)).await;
            format!("result {}", arguments["n"])
        }
        fn parallel(&self, _name: &str) -> bool {
            self.0
        }
    }

    async fn run_two(parallel: bool) -> (Duration, Vec<Value>) {
        let (url, seen) = fake_model(false).await;
        let model = OpenAiCompat::new(Some(url), Some("m".into()));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let started = std::time::Instant::now();
        model
            .chat_with_tools(
                &ChatRequest {
                    system: String::new(),
                    messages: vec![Message::user("two at once")],
                    image: None,
                    think: false,
                },
                &list_tool(),
                &Slow(parallel),
                &Sink::new(tx),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let took = started.elapsed();
        let seen = seen.lock().unwrap();
        let tools = seen[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "tool")
            .cloned()
            .collect();
        (took, tools)
    }

    #[tokio::test]
    async fn reads_run_together_and_keep_their_order() {
        let (took, tools) = run_two(true).await;
        assert!(took < Duration::from_millis(550), "took {took:?}");
        assert_eq!(tools[0]["tool_call_id"], "a");
        assert_eq!(tools[0]["content"], "result 1");
        assert_eq!(tools[1]["tool_call_id"], "b");
        assert_eq!(tools[1]["content"], "result 2");
        let (took, _) = run_two(false).await;
        assert!(took >= Duration::from_millis(600), "actions run one by one");
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
                    think: false,
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
