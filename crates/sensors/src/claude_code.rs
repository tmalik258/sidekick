use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;

use sidekick_core::{Event, EventBus, Sensitivity};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate, http};

/// Receives Claude Code hooks (FR-SEN-09) on a local port, so Sidekick knows
/// when a session finishes or needs the user. Claude Code posts the hook
/// input here through an HTTP hook the user adds to their own
/// `~/.claude/settings.json`; Sidekick never edits that file, and never
/// reads transcripts or credentials.
pub struct ClaudeCodeSensor {
    pub port: u16,
    /// Permission requests waiting for Allow or Deny on the island.
    pub approvals: Approvals,
}

/// Open permission requests (FR-DEV-06), by id. `Some(true)` allows,
/// `Some(false)` denies, `None` hands the question back to the terminal.
#[derive(Clone, Default)]
pub struct Approvals(Arc<Mutex<HashMap<String, oneshot::Sender<Option<bool>>>>>);

impl Approvals {
    fn open(&self, id: &str) -> oneshot::Receiver<Option<bool>> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut m) = self.0.lock() {
            m.insert(id.to_owned(), tx);
        }
        rx
    }

    fn close(&self, id: &str) {
        if let Ok(mut m) = self.0.lock() {
            m.remove(id);
        }
    }

    /// Answers a waiting request. False when it already timed out.
    pub fn decide(&self, id: &str, allow: Option<bool>) -> bool {
        let tx = self.0.lock().ok().and_then(|mut m| m.remove(id));
        tx.is_some_and(|tx| tx.send(allow).is_ok())
    }
}

/// How long a permission request waits for the island before Claude Code
/// shows its own prompt.
pub const APPROVAL_WAIT: Duration = Duration::from_secs(25);

impl ClaudeCodeSensor {
    pub const ID: &'static str = "claude_code";
    pub const DEFAULT_PORT: u16 = 47821;
    pub const PATH: &'static str = "/claude-code";
    pub const STOP: &'static str = "claude.stop";
    pub const NOTIFICATION: &'static str = "claude.notification";
    pub const SESSION_START: &'static str = "claude.session_start";
    pub const PERMISSION: &'static str = "claude.permission";
}

const READ_TIMEOUT: Duration = http::READ_TIMEOUT;

impl Sensor for ClaudeCodeSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        tokio::spawn(async move {
            let listener = match TcpListener::bind(("127.0.0.1", self.port)).await {
                Ok(l) => l,
                Err(err) => {
                    log::warn!("Claude Code hook port {} unavailable: {err}", self.port);
                    return;
                }
            };
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    continue;
                };
                let bus = bus.clone();
                let gate = gate.clone();
                let approvals = self.approvals.clone();
                tokio::spawn(async move {
                    serve(sock, &bus, &gate, &approvals).await;
                });
            }
        })
    }
}

/// Reads one request, always answers, and returns the event it carried.
/// Only a local program may post here: anything a browser sends (it carries
/// an `Origin`) or that is not JSON is refused, so a web page cannot forge
/// a Claude Code event.
async fn serve(mut sock: TcpStream, bus: &EventBus, gate: &SensorGate, approvals: &Approvals) {
    let Ok(request) = tokio::time::timeout(READ_TIMEOUT, http::read_request(&mut sock)).await
    else {
        return;
    };
    let allowed = gate.allows(ClaudeCodeSensor::ID);
    // A permission request waits for Allow or Deny on the island.
    if let Some(r) = &request
        && r.origin().is_none()
        && r.method == "POST"
        && r.path == ClaudeCodeSensor::PATH
        && r.is_json()
        && let Ok(input) = serde_json::from_slice::<serde_json::Value>(&r.body)
        && let Some(event) = permission_event(&input)
    {
        let mut answer = None;
        if allowed {
            let id = event.payload["id"].as_str().unwrap_or_default().to_owned();
            let rx = approvals.open(&id);
            bus.publish(event);
            answer = tokio::time::timeout(APPROVAL_WAIT, rx)
                .await
                .ok()
                .and_then(Result::ok)
                .flatten();
            approvals.close(&id);
        }
        let body = answer.map(|allow| {
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": if allow { "allow" } else { "deny" } }
                }
            })
        });
        http::respond(&mut sock, "200 OK", &[], body.as_ref()).await;
        return;
    }
    let (status, event) = match request {
        Some(r) if r.origin().is_some() => ("403 Forbidden", None),
        Some(r) if r.method == "POST" && r.path == ClaudeCodeSensor::PATH => {
            match serde_json::from_slice::<serde_json::Value>(&r.body) {
                Ok(input) if r.is_json() => ("200 OK", hook_event(&input)),
                Ok(_) => ("415 Unsupported Media Type", None),
                Err(_) => ("400 Bad Request", None),
            }
        }
        Some(_) => ("404 Not Found", None),
        None => ("400 Bad Request", None),
    };
    // An empty body: Claude Code takes no decision from this hook.
    http::respond(&mut sock, status, &[], None).await;
    if let Some(e) = event
        && allowed
    {
        bus.publish(e);
    }
}

/// A `PermissionRequest` hook as an event: what the tool wants to do, in
/// one line. The id ties the island's answer to the waiting request.
pub fn permission_event(input: &serde_json::Value) -> Option<Event> {
    if input["hook_event_name"].as_str()? != "PermissionRequest" {
        return None;
    }
    let tool = input["tool_name"].as_str().unwrap_or("a tool");
    let ti = &input["tool_input"];
    let what = ti["command"]
        .as_str()
        .or(ti["file_path"].as_str())
        .or(ti["url"].as_str())
        .or(ti["description"].as_str())
        .unwrap_or_default();
    let summary: String = what.chars().take(160).collect();
    let cwd = input["cwd"].as_str().unwrap_or_default();
    let project = Path::new(cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Claude Code");
    let id = input["tool_use_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "{}-{}",
                input["session_id"].as_str().unwrap_or("s"),
                ulid::Ulid::new()
            )
        });
    Some(
        Event::new(
            ClaudeCodeSensor::PERMISSION,
            ClaudeCodeSensor::ID,
            serde_json::json!({
                "id": id,
                "project": project,
                "tool": tool,
                "summary": summary,
                "seconds": APPROVAL_WAIT.as_secs(),
            }),
        )
        .with_sensitivity(Sensitivity::Personal),
    )
}

/// Turns hook input into an event. Only the fields Sidekick needs are kept;
/// the transcript path is dropped on purpose.
pub fn hook_event(input: &serde_json::Value) -> Option<Event> {
    let kind = match input["hook_event_name"].as_str()? {
        "Stop" => ClaudeCodeSensor::STOP,
        "Notification" => ClaudeCodeSensor::NOTIFICATION,
        "SessionStart" => ClaudeCodeSensor::SESSION_START,
        _ => return None,
    };
    let cwd = input["cwd"].as_str().unwrap_or_default();
    let project = Path::new(cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Claude Code");
    let message: String = input["message"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect();
    Some(
        Event::new(
            kind,
            ClaudeCodeSensor::ID,
            serde_json::json!({
                "project": project,
                "cwd": cwd,
                "session": input["session_id"].as_str().unwrap_or_default(),
                "message": message,
                "type": input["notification_type"].as_str().unwrap_or_default(),
            }),
        )
        .with_sensitivity(Sensitivity::Personal),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GateState;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn maps_hook_input() {
        let e = hook_event(&serde_json::json!({
            "hook_event_name": "Notification",
            "session_id": "abc",
            "cwd": "/home/me/code/sidekick",
            "message": "Claude needs your permission to use Bash",
            "transcript_path": "/home/me/.claude/projects/x.jsonl",
        }))
        .unwrap();
        assert_eq!(e.kind, ClaudeCodeSensor::NOTIFICATION);
        assert_eq!(e.payload["project"], "sidekick");
        assert!(e.payload.get("transcript_path").is_none());
        assert!(hook_event(&serde_json::json!({"hook_event_name": "PreToolUse"})).is_none());
    }

    #[tokio::test]
    async fn receives_a_stop_hook_over_http() {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        let (_h, gate) = SensorGate::new(GateState::default());
        let approvals = Approvals::default();
        let task = Box::new(ClaudeCodeSensor {
            port,
            approvals: approvals.clone(),
        })
        .spawn(bus, gate);
        tokio::time::sleep(Duration::from_millis(100)).await;

        // A permission request waits for the island's answer.
        let perm = r#"{"hook_event_name":"PermissionRequest","session_id":"s1","cwd":"C:\\code\\api","tool_name":"Bash","tool_input":{"command":"npm test"},"tool_use_id":"t1"}"#;
        let waiting = tokio::spawn(async move {
            let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let req = format!(
                "POST /claude-code HTTP/1.1\r\nhost: x\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{perm}",
                perm.len()
            );
            sock.write_all(req.as_bytes()).await.unwrap();
            let mut resp = String::new();
            sock.read_to_string(&mut resp).await.unwrap();
            resp
        });
        let e = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(e.kind, ClaudeCodeSensor::PERMISSION);
        assert_eq!(e.payload["summary"], "npm test");
        assert!(approvals.decide("t1", Some(false)));
        let resp = waiting.await.unwrap();
        assert!(resp.contains(r#""behavior":"deny""#), "{resp}");
        assert!(!approvals.decide("t1", Some(true)), "already answered");

        let body = r#"{"hook_event_name":"Stop","session_id":"s1","cwd":"C:\\code\\api"}"#;
        let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let req = format!(
            "POST /claude-code HTTP/1.1\r\nhost: x\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        sock.write_all(req.as_bytes()).await.unwrap();
        let mut resp = String::new();
        sock.read_to_string(&mut resp).await.unwrap();
        assert!(resp.starts_with("HTTP/1.1 200"));

        let e = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(e.kind, ClaudeCodeSensor::STOP);
        assert_eq!(e.payload["cwd"], "C:\\code\\api");

        let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        sock.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
        let mut resp = String::new();
        sock.read_to_string(&mut resp).await.unwrap();
        assert!(resp.starts_with("HTTP/1.1 404"));

        // What a web page could send: a "simple" cross-site POST, or JSON
        // with the page's Origin. Both are refused and publish nothing.
        for (ctype, origin) in [
            ("text/plain", ""),
            ("application/json", "origin: https://evil.example\r\n"),
        ] {
            let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let req = format!(
                "POST /claude-code HTTP/1.1\r\nhost: x\r\n{origin}content-type: {ctype}\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(req.as_bytes()).await.unwrap();
            let mut resp = String::new();
            sock.read_to_string(&mut resp).await.unwrap();
            assert!(resp.starts_with("HTTP/1.1 4"), "{resp}");
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(300), rx.recv())
                .await
                .is_err(),
            "a refused request must not publish an event"
        );
        task.abort();
    }
}
