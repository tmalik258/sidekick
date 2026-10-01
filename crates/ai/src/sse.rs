use futures_util::StreamExt;

use crate::{AiError, CancellationToken};

/// Reads a server-sent events body and hands each `data:` payload to `on`.
/// Stops at `[DONE]`, at the end of the body, or when `on` returns false.
pub async fn each_data(
    resp: reqwest::Response,
    cancel: &CancellationToken,
    mut on: impl FnMut(&str) -> Result<bool, AiError>,
) -> Result<(), AiError> {
    let mut body = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err(AiError::Cancelled),
            chunk = body.next() => chunk,
        };
        let Some(chunk) = chunk else {
            return Ok(());
        };
        buf.extend_from_slice(&chunk?);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let Some(data) = line.trim_end().strip_prefix("data:") else {
                continue;
            };
            let data = data.trim_start();
            if data == "[DONE]" {
                return Ok(());
            }
            if !on(data)? {
                return Ok(());
            }
        }
    }
}

/// Turns a non-2xx response into a readable error.
pub async fn check(resp: reqwest::Response) -> Result<reqwest::Response, AiError> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or(v["error"].as_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| body.chars().take(200).collect());
    Err(AiError::Failed(format!("HTTP {status}: {detail}")))
}
