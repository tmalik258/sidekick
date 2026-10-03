//! The browser bridge (FR-SEN-10). The Sidekick extension reports page
//! events (a login form, a long article, an Upwork job, many tabs) and picks
//! up commands (fill this login, close duplicate tabs) over a localhost-only
//! endpoint.
//!
//! Only the extension gets in: every request must carry the pairing token,
//! and any request with a web page's origin is refused outright (extensions
//! send `chrome-extension://` or `moz-extension://`, or no origin at all on
//! simple GETs).
//!
//! Pairing needs no code: the extension asks (`POST /browser/pair`, only
//! from an extension origin), the island shows Allow or Deny, and on Allow
//! the extension gets the token.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sidekick_core::{Event, EventBus, Sensitivity};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::{Approvals, Sensor, SensorGate, http};

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
    // The domain in the active tab (never the page), for routines and time.
    ("site", "browser.site"),
];

/// How long a pairing request waits for Allow on the island.
pub const PAIR_WAIT: Duration = Duration::from_secs(90);
pub const PAIR_REQUEST: &str = "browser.pair_request";

/// Commands for the extension, queued until it polls, and when each browser
/// last checked in.
#[derive(Clone, Default)]
pub struct BrowserBridge {
    queue: Arc<Mutex<VecDeque<serde_json::Value>>>,
    ready: Arc<Notify>,
    seen: Arc<Mutex<HashMap<String, i64>>>,
    /// Requests waiting for the extension's answer, by id.
    pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<serde_json::Value>>>>,
}

impl BrowserBridge {
    /// Browsers whose extension checked in, with the Unix time of the last
    /// time.
    pub fn seen(&self) -> Vec<(String, i64)> {
        let mut v: Vec<(String, i64)> = self
            .seen
            .lock()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), *v)).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    fn mark(&self, browser: &str) {
        if let Ok(mut m) = self.seen.lock() {
            m.insert(browser.to_owned(), chrono::Utc::now().timestamp());
        }
    }

    pub fn send(&self, command: serde_json::Value) {
        if let Ok(mut q) = self.queue.lock() {
            if q.len() >= MAX_QUEUED {
                q.pop_front();
            }
            q.push_back(command);
        }
        self.ready.notify_waiters();
    }

    /// Sends a command that the extension answers (read a page, click),
    /// and waits for the answer.
    pub async fn request(
        &self,
        mut command: serde_json::Value,
        wait: Duration,
    ) -> Result<serde_json::Value, String> {
        let id = ulid::Ulid::new().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Ok(mut p) = self.pending.lock() {
            p.insert(id.clone(), tx);
        }
        command["id"] = id.clone().into();
        self.send(command);
        let answer = tokio::time::timeout(wait, rx).await;
        if let Ok(mut p) = self.pending.lock() {
            p.remove(&id);
        }
        match answer {
            Ok(Ok(v)) => match v["error"].as_str() {
                Some(e) if !e.is_empty() => Err(e.to_owned()),
                _ => Ok(v),
            },
            _ => Err(
                "the browser did not answer. Is the Sidekick extension installed and the browser open?"
                    .into(),
            ),
        }
    }

    /// The extension's answer to a request.
    fn resolve(&self, answer: serde_json::Value) -> bool {
        let Some(id) = answer["id"].as_str() else {
            return false;
        };
        let tx = self.pending.lock().ok().and_then(|mut p| p.remove(id));
        tx.is_some_and(|tx| tx.send(answer["result"].clone()).is_ok())
    }

    /// Whether any browser's extension checked in during the last minute.
    pub fn connected(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        self.seen().iter().any(|(_, t)| now - t < 60)
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
    /// Pairing requests waiting for Allow on the island.
    pub approvals: Approvals,
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
            let pairing = Arc::new(AtomicBool::new(false));
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    continue;
                };
                let ctx = Ctx {
                    token: token.clone(),
                    bridge: self.bridge.clone(),
                    approvals: self.approvals.clone(),
                    pairing: pairing.clone(),
                };
                let (bus, gate) = (bus.clone(), gate.clone());
                tokio::spawn(async move {
                    serve(sock, &ctx, |event| {
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

/// What each connection needs.
struct Ctx {
    token: Arc<str>,
    bridge: BrowserBridge,
    approvals: Approvals,
    /// One pairing request at a time.
    pairing: Arc<AtomicBool>,
}

/// "Chrome", "Edge", "Firefox": a short name the extension sends, cleaned.
pub fn browser_name(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
        .take(20)
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        "Browser".into()
    } else {
        clean.to_owned()
    }
}

async fn serve(mut sock: TcpStream, ctx: &Ctx, publish: impl Fn(Event)) {
    let (token, bridge) = (&*ctx.token, &ctx.bridge);
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
            "content-type, x-sidekick-token, x-sidekick-browser",
        ),
        ("access-control-allow-methods", "GET, POST"),
    ];
    if req.method == "OPTIONS" {
        http::respond(&mut sock, "204 No Content", &cors, None).await;
        return;
    }
    if (req.method.as_str(), req.path.as_str()) == ("POST", "/browser/pair") {
        // Only an extension can ask, and only one request at a time.
        if origin.is_empty() {
            http::respond(&mut sock, "403 Forbidden", &cors, None).await;
            return;
        }
        if ctx.pairing.swap(true, Ordering::SeqCst) {
            http::respond(&mut sock, "429 Too Many Requests", &cors, None).await;
            return;
        }
        let browser = browser_name(req.header("x-sidekick-browser").unwrap_or_default());
        let id = ulid::Ulid::new().to_string();
        let answer = ctx.approvals.open(&id);
        let extension: String = origin
            .split("://")
            .nth(1)
            .unwrap_or_default()
            .chars()
            .take(8)
            .collect();
        publish(Event::new(
            PAIR_REQUEST,
            BrowserSensor::ID,
            serde_json::json!({ "id": id, "browser": browser, "extension": extension }),
        ));
        let allowed = matches!(
            tokio::time::timeout(PAIR_WAIT, answer).await,
            Ok(Ok(Some(true)))
        );
        ctx.approvals.close(&id);
        ctx.pairing.store(false, Ordering::SeqCst);
        if allowed {
            bridge.mark(&browser);
            let body = serde_json::json!({ "token": token });
            http::respond(&mut sock, "200 OK", &cors, Some(&body)).await;
        } else {
            http::respond(&mut sock, "403 Forbidden", &cors, None).await;
        }
        return;
    }
    if !token_matches(req.header("x-sidekick-token").unwrap_or_default(), token) {
        http::respond(&mut sock, "401 Unauthorized", &cors, None).await;
        return;
    }
    bridge.mark(&browser_name(
        req.header("x-sidekick-browser").unwrap_or("Browser"),
    ));
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
                Some(mut event) => {
                    event.payload["browser"] =
                        browser_name(req.header("x-sidekick-browser").unwrap_or("Browser")).into();
                    publish(event);
                    http::respond(&mut sock, "204 No Content", &cors, None).await;
                }
                None => http::respond(&mut sock, "400 Bad Request", &cors, None).await,
            }
        }
        ("POST", "/browser/result") => {
            let parsed = req
                .is_json()
                .then(|| serde_json::from_slice::<serde_json::Value>(&req.body).ok())
                .flatten();
            let ok = parsed.is_some_and(|v| bridge.resolve(v));
            let status = if ok {
                "204 No Content"
            } else {
                "404 Not Found"
            };
            http::respond(&mut sock, status, &cors, None).await;
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

    #[tokio::test]
    async fn requests_get_their_answer() {
        let bridge = BrowserBridge::default();
        let b = bridge.clone();
        let waiting = tokio::spawn(async move {
            b.request(
                serde_json::json!({ "type": "read" }),
                Duration::from_secs(2),
            )
            .await
        });
        // The extension picks the command up, runs it and answers by id.
        let mut cmd = None;
        for _ in 0..50 {
            cmd = bridge.take();
            if cmd.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let cmd = cmd.expect("command queued");
        assert_eq!(cmd["type"], "read");
        assert!(
            bridge.resolve(serde_json::json!({ "id": cmd["id"], "result": { "title": "Inbox" } }))
        );
        assert_eq!(waiting.await.unwrap().unwrap()["title"], "Inbox");
        assert!(!bridge.resolve(serde_json::json!({ "id": "nope", "result": {} })));
        let err = bridge
            .request(
                serde_json::json!({ "type": "read" }),
                Duration::from_millis(20),
            )
            .await;
        assert!(err.unwrap_err().contains("did not answer"));
    }

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
            approvals: Approvals::default(),
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

    #[tokio::test]
    async fn pairs_only_after_allow_on_the_island() {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        let (_h, gate) = SensorGate::new(GateState::default());
        let bridge = BrowserBridge::default();
        let approvals = Approvals::default();
        let task = Box::new(BrowserSensor {
            port,
            token: "t0ken".into(),
            bridge: bridge.clone(),
            approvals: approvals.clone(),
        })
        .spawn(bus, gate);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let pair = |origin: &str| {
            format!(
                "POST /browser/pair HTTP/1.1\r\norigin: {origin}\r\nx-sidekick-browser: Chrome\r\ncontent-length: 0\r\n\r\n"
            )
        };
        // A web page, or a request with no origin, cannot ask.
        assert!(
            call(port, pair("https://evil.example"))
                .await
                .starts_with("HTTP/1.1 403")
        );
        assert!(
            call(
                port,
                "POST /browser/pair HTTP/1.1\r\ncontent-length: 0\r\n\r\n".into()
            )
            .await
            .starts_with("HTTP/1.1 403")
        );
        // The extension asks; the island shows the request; Allow sends the token.
        let asking = tokio::spawn(call(port, pair("chrome-extension://abcdefghij")));
        let e = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(e.kind, PAIR_REQUEST);
        assert_eq!(e.payload["browser"], "Chrome");
        assert_eq!(e.payload["extension"], "abcdefgh");
        let id = e.payload["id"].as_str().unwrap().to_owned();
        assert!(approvals.decide(&id, Some(true)));
        let resp = asking.await.unwrap();
        assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
        assert!(resp.contains("t0ken"));
        assert_eq!(bridge.seen().first().map(|s| s.0.as_str()), Some("Chrome"));

        // Deny sends nothing.
        let asking = tokio::spawn(call(port, pair("chrome-extension://abcdefghij")));
        let e = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        approvals.decide(e.payload["id"].as_str().unwrap(), Some(false));
        assert!(asking.await.unwrap().starts_with("HTTP/1.1 403"));
        task.abort();
    }
}
