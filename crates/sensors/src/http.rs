//! A tiny HTTP/1.1 reader for Sidekick's localhost endpoints (Claude Code
//! hooks, the browser extension). Only what those need: one request per
//! connection, a JSON body, and checks that keep web pages out.
//!
//! A web page can send a cross-site POST to 127.0.0.1 without asking first
//! when it uses a "simple" content type. Requiring `application/json` forces
//! the browser to ask (a CORS preflight), which these endpoints never
//! approve, and any request that carries an `Origin` header (every request a
//! browser makes on a page's behalf, but not a local program's) is refused.
//! Browser extensions are told apart by their own origin and a token.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const MAX_BODY: usize = 256 * 1024;
const MAX_HEAD: usize = 16 * 1024;
pub const READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn is_json(&self) -> bool {
        self.header("content-type").is_some_and(|v| {
            v.trim()
                .to_ascii_lowercase()
                .starts_with("application/json")
        })
    }

    /// The browser origin, when a browser sent this on a page's or an
    /// extension's behalf.
    pub fn origin(&self) -> Option<&str> {
        self.header("origin").filter(|o| !o.is_empty())
    }
}

pub async fn read_request(sock: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > MAX_HEAD {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_owned();
    let path = first.next()?.split('?').next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return None;
    }
    let mut body = buf[header_end..].to_vec();
    while body.len() < length {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Some(Request {
        method,
        path,
        headers,
        body,
    })
}

/// Writes a response. `extra` headers come first (e.g. CORS for an extension).
pub async fn respond(
    sock: &mut TcpStream,
    status: &str,
    extra: &[(&str, &str)],
    body: Option<&serde_json::Value>,
) {
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let mut head = format!("HTTP/1.1 {status}\r\n");
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() {
        head.push_str("content-type: application/json\r\n");
    }
    head.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    ));
    let _ = sock.write_all(head.as_bytes()).await;
    let _ = sock.write_all(body.as_bytes()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(headers: &[(&str, &str)]) -> Request {
        Request {
            method: "POST".into(),
            path: "/x".into(),
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: vec![],
        }
    }

    #[test]
    fn json_and_origin_checks() {
        assert!(req(&[("Content-Type", "application/json; charset=utf-8")]).is_json());
        assert!(!req(&[("content-type", "text/plain")]).is_json());
        assert!(!req(&[]).is_json());
        assert_eq!(
            req(&[("Origin", "https://evil.example")]).origin(),
            Some("https://evil.example")
        );
        assert_eq!(req(&[]).origin(), None);
    }
}
