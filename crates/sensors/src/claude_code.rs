use std::path::Path;
use std::time::Duration;

use sidekick_core::{Event, EventBus, Sensitivity};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate};

/// Receives Claude Code hooks (FR-SEN-09) on a local port, so Sidekick knows
/// when a session finishes or needs the user. Claude Code posts the hook
/// input here through an HTTP hook the user adds to their own
/// `~/.claude/settings.json`; Sidekick never edits that file, and never
/// reads transcripts or credentials.
pub struct ClaudeCodeSensor {
    pub port: u16,
}

impl ClaudeCodeSensor {
    pub const ID: &'static str = "claude_code";
    pub const DEFAULT_PORT: u16 = 47821;
    pub const PATH: &'static str = "/claude-code";
    pub const STOP: &'static str = "claude.stop";
    pub const NOTIFICATION: &'static str = "claude.notification";
    pub const SESSION_START: &'static str = "claude.session_start";
}

const MAX_BODY: usize = 256 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);

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
                tokio::spawn(async move {
                    if let Ok(Some(event)) = tokio::time::timeout(READ_TIMEOUT, serve(sock)).await
                        && gate.allows(Self::ID)
                    {
                        bus.publish(event);
                    }
                });
            }
        })
    }
}

/// Reads one request, always answers, and returns the event it carried.
async fn serve(mut sock: TcpStream) -> Option<Event> {
    let request = read_request(&mut sock).await;
    let (status, event) = match request {
        Some((method, path, body)) if method == "POST" && path == ClaudeCodeSensor::PATH => {
            match serde_json::from_slice::<serde_json::Value>(&body) {
                Ok(input) => ("200 OK", hook_event(&input)),
                Err(_) => ("400 Bad Request", None),
            }
        }
        Some(_) => ("404 Not Found", None),
        None => ("400 Bad Request", None),
    };
    // An empty body: Claude Code takes no decision from this hook.
    let resp = format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
    let _ = sock.write_all(resp.as_bytes()).await;
    event
}

async fn read_request(sock: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
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
        if buf.len() > 16 * 1024 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_owned();
    let path = first.next()?.split('?').next()?.to_owned();
    let length: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
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
    Some((method, path, body))
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
        let task = Box::new(ClaudeCodeSensor { port }).spawn(bus, gate);
        tokio::time::sleep(Duration::from_millis(100)).await;

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
        task.abort();
    }
}
