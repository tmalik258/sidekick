//! A small MCP client over Streamable HTTP, so a local model can use the
//! tools of a remote MCP server (Composio). Only what chat needs:
//! `initialize`, `tools/list` and `tools/call`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::AiError;

const PROTOCOL: &str = "2025-06-18";
const SESSION_HEADER: &str = "mcp-session-id";

/// One tool the server offers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments.
    pub input_schema: Value,
}

pub struct McpClient {
    url: String,
    headers: Vec<(String, String)>,
    client: reqwest::Client,
    session: Option<String>,
    next_id: AtomicU64,
}

impl McpClient {
    /// Connects and says hello. `headers` go on every request (for
    /// example an API key).
    pub async fn connect(url: &str, headers: Vec<(String, String)>) -> Result<Self, AiError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        let mut me = Self {
            url: url.trim().to_owned(),
            headers,
            client,
            session: None,
            next_id: AtomicU64::new(1),
        };
        let (result, session) = me
            .request_with_session(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL,
                    "capabilities": {},
                    "clientInfo": { "name": "sidekick", "version": env!("CARGO_PKG_VERSION") },
                }),
            )
            .await?;
        if result.get("protocolVersion").is_none() {
            return Err(AiError::Failed(
                "the MCP server did not answer initialize".into(),
            ));
        }
        me.session = session;
        me.notify("notifications/initialized").await;
        Ok(me)
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>, AiError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        // A few pages at most; chat never needs hundreds of tools.
        for _ in 0..5 {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let result = self.request("tools/list", params).await?;
            tools.extend(parse_tools(&result));
            cursor = result["nextCursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    /// Runs a tool and returns its text. A tool error comes back as text
    /// too, so the model can read it and recover.
    pub async fn call_tool(&self, name: &str, arguments: &Value) -> Result<String, AiError> {
        let result = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        let text = tool_text(&result);
        Ok(if result["isError"].as_bool() == Some(true) {
            format!("Error: {text}")
        } else {
            text
        })
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, AiError> {
        self.request_with_session(method, params)
            .await
            .map(|(r, _)| r)
    }

    async fn request_with_session(
        &self,
        method: &str,
        params: Value,
    ) -> Result<(Value, Option<String>), AiError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let resp = self.post(&body).send().await?;
        let session = resp
            .headers()
            .get(SESSION_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let resp = crate::sse::check(resp).await?;
        let is_sse = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.starts_with("text/event-stream"));
        let text = resp.text().await?;
        let msg = if is_sse {
            find_sse_response(&text, id)
        } else {
            serde_json::from_str(&text).ok()
        }
        .ok_or_else(|| AiError::Failed(format!("no answer to {method} from the MCP server")))?;
        if let Some(err) = msg.get("error") {
            let detail = err["message"].as_str().unwrap_or("unknown error");
            return Err(AiError::Failed(format!("MCP {method}: {detail}")));
        }
        Ok((msg["result"].clone(), session))
    }

    async fn notify(&self, method: &str) {
        let body = json!({ "jsonrpc": "2.0", "method": method });
        let _ = self.post(&body).send().await;
    }

    fn post(&self, body: &Value) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .post(&self.url)
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header("MCP-Protocol-Version", PROTOCOL)
            .json(body);
        if let Some(s) = &self.session {
            req = req.header(SESSION_HEADER, s);
        }
        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        req
    }
}

fn parse_tools(result: &Value) -> Vec<McpTool> {
    result["tools"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    Some(McpTool {
                        name: t["name"].as_str()?.to_owned(),
                        description: t["description"].as_str().unwrap_or_default().to_owned(),
                        input_schema: t
                            .get("inputSchema")
                            .cloned()
                            .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The text parts of a tool result, joined. Other content (images,
/// resources) is noted, not sent to the model.
fn tool_text(result: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(items) = result["content"].as_array() {
        for item in items {
            match item["type"].as_str() {
                Some("text") => parts.push(item["text"].as_str().unwrap_or_default().to_owned()),
                Some(other) => parts.push(format!("[{other} content]")),
                None => {}
            }
        }
    }
    if parts.is_empty()
        && let Some(s) = result.get("structuredContent")
    {
        parts.push(s.to_string());
    }
    parts.join("\n")
}

/// The JSON-RPC message with `id` among a text/event-stream body.
fn find_sse_response(body: &str, id: u64) -> Option<Value> {
    let mut data = String::new();
    let mut found = None;
    let mut flush = |data: &mut String| {
        if !data.is_empty() {
            if let Ok(v) = serde_json::from_str::<Value>(data)
                && v["id"].as_u64() == Some(id)
            {
                found = Some(v);
            }
            data.clear();
        }
    };
    for line in body.lines() {
        if let Some(d) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(d.trim_start());
        } else if line.trim().is_empty() {
            flush(&mut data);
        }
    }
    flush(&mut data);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn reads_tools_and_results() {
        let tools = parse_tools(&json!({ "tools": [
            { "name": "JIRA_SEARCH", "description": "Find issues", "inputSchema": { "type": "object" } },
            { "name": "BARE" },
            { "description": "no name" },
        ]}));
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "JIRA_SEARCH");
        assert_eq!(tools[1].input_schema["type"], "object");
        assert_eq!(
            tool_text(&json!({ "content": [
                { "type": "text", "text": "a" },
                { "type": "image", "data": "..." },
                { "type": "text", "text": "b" },
            ]})),
            "a\n[image content]\nb"
        );
        assert_eq!(
            tool_text(&json!({ "structuredContent": { "n": 1 } })),
            r#"{"n":1}"#
        );
    }

    #[test]
    fn finds_the_answer_in_an_event_stream() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(find_sse_response(body, 7).unwrap()["result"]["ok"], true);
        assert!(find_sse_response(body, 8).is_none());
    }

    /// A fake MCP server: answers initialize with a session id (JSON),
    /// tools/list as an event stream, and tools/call only with the session.
    async fn fake_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let mut got = Vec::new();
                    // Read headers and the body (Content-Length).
                    loop {
                        let n = sock.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        got.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&got).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let len = text[..end]
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
                    let text = String::from_utf8_lossy(&got).to_string();
                    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
                    let has_session = head.to_ascii_lowercase().contains("mcp-session-id: s1");
                    let has_key = head.to_ascii_lowercase().contains("x-api-key: k");
                    let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
                    let id = req["id"].clone();
                    let (ctype, extra, payload) = match req["method"].as_str() {
                        _ if !has_key => ("application/json", "", r#"{"error":"no key"}"#.to_owned()),
                        Some("initialize") => (
                            "application/json",
                            "Mcp-Session-Id: s1\r\n",
                            json!({ "jsonrpc": "2.0", "id": id, "result": { "protocolVersion": PROTOCOL } }).to_string(),
                        ),
                        Some("notifications/initialized") => ("application/json", "", String::new()),
                        Some("tools/list") if has_session => (
                            "text/event-stream",
                            "",
                            format!(
                                "event: message\ndata: {}\n\n",
                                json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": [
                                    { "name": "GITHUB_LIST_ISSUES", "description": "List issues", "inputSchema": { "type": "object" } }
                                ]}})
                            ),
                        ),
                        Some("tools/call") if has_session => (
                            "application/json",
                            "",
                            json!({ "jsonrpc": "2.0", "id": id, "result": {
                                "content": [{ "type": "text", "text": format!("ran {}", req["params"]["name"].as_str().unwrap_or("")) }]
                            }})
                            .to_string(),
                        ),
                        _ => (
                            "application/json",
                            "",
                            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32600, "message": "no session" } }).to_string(),
                        ),
                    };
                    let status = if !has_key {
                        "401 Unauthorized"
                    } else if payload.is_empty() {
                        "202 Accepted"
                    } else {
                        "200 OK"
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                        payload.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}/mcp")
    }

    #[tokio::test]
    async fn talks_to_a_server_with_sessions() {
        let url = fake_server().await;
        let headers = vec![("x-api-key".to_owned(), "k".to_owned())];
        let client = McpClient::connect(&url, headers).await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "GITHUB_LIST_ISSUES");
        let out = client
            .call_tool("GITHUB_LIST_ISSUES", &json!({}))
            .await
            .unwrap();
        assert_eq!(out, "ran GITHUB_LIST_ISSUES");
    }

    #[tokio::test]
    async fn reports_a_refused_key() {
        let url = fake_server().await;
        let err = McpClient::connect(&url, vec![]).await.err().unwrap();
        assert!(err.to_string().contains("401"), "{err}");
    }
}
