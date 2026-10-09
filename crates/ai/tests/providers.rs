//! Providers against stand-ins: a fake `claude` script and a tiny local
//! server that speaks the OpenAI streaming format.

use sidekick_ai::{AiProvider, CancellationToken, ChatRequest, Message, OpenAiCompat, Sink};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

async fn run(p: &dyn AiProvider, prompt: &str) -> (Result<String, String>, String) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sink = Sink::new(tx);
    let req = ChatRequest {
        system: String::new(),
        messages: vec![Message::user(prompt)],
        image: None,
    };
    let r = p
        .chat(&req, &sink, &CancellationToken::new())
        .await
        .map_err(|e| e.to_string());
    drop(sink);
    let mut streamed = String::new();
    while let Some(s) = rx.recv().await {
        streamed.push_str(&s);
    }
    (r, streamed)
}

#[cfg(unix)]
#[tokio::test]
async fn claude_code_streams_and_keeps_the_session() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("sidekick-ai-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("claude");
    // Like `claude -p --input-format stream-json`: one message per line on
    // stdin, each answered with two deltas and a result. The turn number
    // shows whether the same process answered.
    std::fs::write(
        &script,
        r#"#!/bin/sh
n=0
printf '%s\n' '{"type":"system","subtype":"init"}'
while IFS= read -r line; do
  n=$((n+1))
  t=$(printf '%s' "$line" | sed 's/.*"text":"\([^"]*\)".*/\1/')
  printf '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"%s you said: "}}}\n' "$n"
  printf '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"%s"}}}\n' "$t"
  printf '{"type":"result","subtype":"success","is_error":false,"result":"ignored"}\n'
done
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let p = sidekick_ai::ClaudeCode {
        mcp_config: None,
        path: Some(script),
        model: None,
        workdir: dir.join("work"),
    };
    assert!(p.available().await);
    let (r, streamed) = run(&p, "hello; $(whoami)").await;
    let first = r.unwrap();
    assert_eq!(first, "1 you said: hello; $(whoami)");
    assert_eq!(streamed, first);

    // The follow-up goes to the same session, and only the new message is sent.
    let (tx, _rx) = mpsc::unbounded_channel();
    let req = ChatRequest {
        system: String::new(),
        messages: vec![
            Message::user("hello; $(whoami)"),
            Message {
                role: sidekick_ai::Role::Assistant,
                content: first,
            },
            Message::user("and again"),
        ],
        image: None,
    };
    let second = p
        .chat(&req, &Sink::new(tx), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(second, "2 you said: and again");
    sidekick_ai::close_claude_sessions();
    let _ = std::fs::remove_dir_all(dir);
}

/// Serves `/models` and a streamed `/chat/completions`.
async fn fake_openai() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let (ctype, body) = if req.starts_with("POST /v1/embeddings") {
                    (
                        "application/json",
                        // Out of order on purpose: `index` decides.
                        r#"{"data":[{"index":1,"embedding":[0,1]},{"index":0,"embedding":[1,0]}]}"#
                            .to_string(),
                    )
                } else if req.starts_with("GET /v1/models") {
                    (
                        "application/json",
                        r#"{"data":[{"id":"tiny"}]}"#.to_string(),
                    )
                } else {
                    (
                        "text/event-stream",
                        [
                            r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#,
                            r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#,
                            r#"data: {"choices":[{"delta":{"content":"lo"}}]}"#,
                            "data: [DONE]",
                        ]
                        .map(|l| format!("{l}\n\n"))
                        .concat(),
                    )
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://127.0.0.1:{}/v1", addr.port())
}

#[tokio::test]
async fn local_model_streams_from_an_openai_compatible_server() {
    let url = fake_openai().await;
    let p = OpenAiCompat::new(Some(url), None);
    assert!(p.is_local());
    assert!(p.available().await);
    assert_eq!(p.pick_model().await.unwrap(), "tiny");
    let (r, streamed) = run(&p, "hi").await;
    assert_eq!(r.unwrap(), "Hello");
    assert_eq!(streamed, "Hello");
}

#[tokio::test]
async fn unreachable_local_server_is_not_available() {
    let p = OpenAiCompat::new(Some("http://127.0.0.1:9/v1".into()), None);
    assert!(!p.available().await);
}

#[tokio::test]
async fn embeds_with_a_local_server_only() {
    let url = fake_openai().await;
    let p = OpenAiCompat::new(Some(url), None);
    let v = p.embed("e", &["a".into(), "b".into()]).await.unwrap();
    assert_eq!(v, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    let remote = OpenAiCompat::new(Some("https://api.example.com/v1".into()), None);
    assert!(remote.embed("e", &["a".into()]).await.is_err());
}
