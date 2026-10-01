//! The browser bridge (FR-SEN-10). The Sidekick extension reports page
//! events (a login form, a long article, an Upwork job, many tabs) and picks
//! up commands (fill this login, close duplicate tabs) over a localhost-only
//! endpoint.
//!
//! Only the extension gets in: every request must carry the pairing token
//! shown in Settings, and any request with a web page's origin is refused
//! outright (extensions send `chrome-extension://` or `moz-extension://`, or
//! no origin at all on simple GETs).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sidekick_core::{Event, EventBus, Sensitivity};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate, http};

/// How long the extension's poll for commands is held open.
const POLL_HOLD: Duration = Duration::from_secs(20);
/// Commands nobody picked up are dropped after this many are queued.
const MAX_QUEUED: usize = 16;
/// Page text sent with an event is capped here.
const MAX_TEXT: usize = 20_000;

/// Page events the extension may report, and the event kind each becomes.
const KINDS: &[(&str, &str)] = &[
    ("login_form", "browser.login_form"),
    ("long_read", "browser.long_read"),
    ("upwork_job", "browser.upwork_job"),
    ("many_tabs", "browser.many_tabs"),
];

/// Commands for the extension, queued until it polls.
#[derive(Clone, Default)]
pub struct BrowserBridge {
    queue: Arc<Mutex<VecDeque<serde_json::Value>>>,
    ready: Arc<Notify>,
}

impl BrowserBridge {
    pub fn send(&self, command: serde_json::Value) {
        if let Ok(mut q) = self.queue.lock() {
            if q.len() >= MAX_QUEUED {
                q.pop_front();
            }
            q.push_back(command);
        }
        self.ready.notify_waiters();
    }

    fn take(&self) -> Option<serde_json::Value> {
        self.queue.lock().ok()?.pop_front()
    }

    async fn next(&self) -> Option<serde_json::Value> {
        if let Some(c) = self.take() {
            return Some(c);
        }
        let notified = self.ready.notified();
        tokio::select! {
            _ = notified => self.take(),
            _ = tokio::time::sleep(POLL_HOLD) => None,
        }
    }
}

pub struct BrowserSensor {
    pub port: u16,
    pub token: String,
    pub bridge: BrowserBridge,
}

impl BrowserSensor {
    pub const ID: &'static str = "browser";
    pub const DEFAULT_PORT: u16 = 47822;
}

impl Sensor for BrowserSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        tokio::spawn(async move {
            let listener = match TcpListener::bind(("127.0.0.1", self.port)).await {
                Ok(l) => l,
                Err(err) => {
                    log::warn!("browser bridge port {} unavailable: {err}", self.port);
                    return;
                }
            };
            let token: Arc<str> = self.token.into();
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    continue;
                };
                let (bus, gate, bridge, token) = (
                    bus.clone(),
                    gate.clone(),
                    self.bridge.clone(),
                    token.clone(),
                );
                tokio::spawn(async move {
                    serve(sock, &token, &bridge, |event| {
                        if gate.allows(Self::ID) {
                            bus.publish(event);
                        }
                    })
                    .await;
                });
            }
        })
    }
}

fn is_extension_origin(origin: &str) -> bool {
    origin.starts_with("chrome-extension://") || origin.starts_with("moz-extension://")
}

/// Constant-time comparison, so the token cannot be guessed byte by byte.
fn token_matches(given: &str, expected: &str) -> bool {
    !expected.is_empty()
        && given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

async fn serve(mut sock: TcpStream, token: &str, bridge: &BrowserBridge, publish: impl Fn(Event)) {
    let Ok(Some(req)) =
        tokio::time::timeout(http::READ_TIMEOUT, http::read_request(&mut sock)).await
    else {
        return;
    };
    // A web page's request always carries its own http(s) origin: refused.
    // The extension's GET requests may carry no origin at all, so the
    // pairing token, checked below, is what keeps everything else out.
    let origin = req.origin().unwrap_or_default().to_owned();
    if !origin.is_empty() && !is_extension_origin(&origin) {
        http::respond(&mut sock, "403 Forbidden", &[], None).await;
        return;
    }
    let cors = [
        (
            "access-control-allow-origin",
            if origin.is_empty() {
                "null"
            } else {
                origin.as_str()
            },
        ),
        (
            "access-control-allow-headers",
            "content-type, x-sidekick-token",
        ),
        ("access-control-allow-methods", "GET, POST"),
    ];
    if req.method == "OPTIONS" {
        http::respond(&mut sock, "204 No Content", &cors, None).await;
        return;
    }
    if !token_matches(req.header("x-sidekick-token").unwrap_or_default(), token) {
        http::respond(&mut sock, "401 Unauthorized", &cors, None).await;
        return;
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/browser/hello") => {
            http::respond(
                &mut sock,
                "200 OK",
                &cors,
                Some(&serde_json::json!({ "ok": true })),
            )
            .await;
        }
        ("POST", "/browser/event") => {
            let parsed = req
                .is_json()
                .then(|| serde_json::from_slice::<serde_json::Value>(&req.body).ok())
                .flatten();
            match parsed.as_ref().and_then(page_event) {
                Some(event) => {
                    publish(event);
                    http::respond(&mut sock, "204 No Content", &cors, None).await;
                }
                None => http::respond(&mut sock, "400 Bad Request", &cors, None).await,
            }
        }
        ("GET", "/browser/next") => match bridge.next().await {
            Some(cmd) => http::respond(&mut sock, "200 OK", &cors, Some(&cmd)).await,
            None => http::respond(&mut sock, "204 No Content", &cors, None).await,
        },
        _ => http::respond(&mut sock, "404 Not Found", &cors, None).await,
    }
}

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or_default()
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Turns an extension report into an event, keeping only known fields.
pub fn page_event(input: &serde_json::Value) -> Option<Event> {
    let kind = input["kind"].as_str()?;
    let (_, event_kind) = KINDS.iter().find(|(k, _)| *k == kind)?;
    let url = input["url"].as_str().unwrap_or_default();
    if !(url.starts_with("http://") || url.starts_with("https://")) && kind != "many_tabs" {
        return None;
    }
    let clip = |key: &str, max: usize| -> String {
        input[key]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(max)
            .collect()
    };
    let host = host_of(url);
    let payload = serde_json::json!({
        "url": url,
        "domain": host.trim_start_matches("www."),
        "title": clip("title", 200),
        "text": clip("text", MAX_TEXT),
        "words": input["words"].as_u64().unwrap_or(0),
        "count": input["count"].as_u64().unwrap_or(0),
        "duplicates": input["duplicates"].as_u64().unwrap_or(0),
        "tab": input["tab"].as_u64().unwrap_or(0),
    });
    Some(
        Event::new(*event_kind, BrowserSensor::ID, payload).with_sensitivity(Sensitivity::Personal),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GateState;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn maps_known_page_events_only() {
        let e = page_event(&serde_json::json!({
            "kind": "login_form", "url": "https://www.github.com/login", "title": "Sign in", "tab": 7,
        }))
        .unwrap();
        assert_eq!(e.kind, "browser.login_form");
        assert_eq!(e.payload["domain"], "github.com");
        assert_eq!(e.payload["tab"], 7);
        assert!(
            page_event(&serde_json::json!({ "kind": "run_this", "url": "https://x.com" }))
                .is_none()
        );
        assert!(
            page_event(&serde_json::json!({ "kind": "login_form", "url": "file:///etc/passwd" }))
                .is_none()
        );
    }

    #[test]
    fn hosts_and_tokens() {
        assert_eq!(
            host_of("https://user:pw@Accounts.Example.com:8443/a?b"),
            "accounts.example.com"
        );
        assert!(token_matches("abc123", "abc123"));
        assert!(!token_matches("abc124", "abc123"));
        assert!(!token_matches("", ""));
    }

    async fn call(port: u16, raw: String) -> String {
        let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        sock.write_all(raw.as_bytes()).await.unwrap();
        let mut resp = String::new();
        sock.read_to_string(&mut resp).await.unwrap();
        resp
    }

    #[tokio::test]
    async fn only_the_paired_extension_gets_in() {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        let (_h, gate) = SensorGate::new(GateState::default());
        let bridge = BrowserBridge::default();
        let task = Box::new(BrowserSensor {
            port,
            token: "t0ken".into(),
            bridge: bridge.clone(),
        })
        .spawn(bus, gate);
        tokio::time::sleep(Duration::from_millis(100)).await;

        let body = r#"{"kind":"login_form","url":"https://github.com/login","title":"Sign in"}"#;
        let post = |origin: &str, token: &str| {
            format!(
                "POST /browser/event HTTP/1.1\r\norigin: {origin}\r\nx-sidekick-token: {token}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            )
        };
        assert!(
            call(port, post("https://evil.example", "t0ken"))
                .await
                .starts_with("HTTP/1.1 403")
        );
        assert!(
            call(port, post("chrome-extension://abc", "wrong"))
                .await
                .starts_with("HTTP/1.1 401")
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), rx.recv())
                .await
                .is_err(),
            "refused requests publish nothing"
        );
        assert!(
            call(port, post("chrome-extension://abc", "t0ken"))
                .await
                .starts_with("HTTP/1.1 204")
        );
        let e = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(e.kind, "browser.login_form");

        // A queued command reaches the extension's poll.
        bridge.send(serde_json::json!({ "type": "close_duplicates" }));
        let resp = call(
            port,
            "GET /browser/next HTTP/1.1\r\norigin: moz-extension://xyz\r\nx-sidekick-token: t0ken\r\n\r\n".into(),
        )
        .await;
        assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
        assert!(resp.contains("close_duplicates"));
        task.abort();
    }
}
