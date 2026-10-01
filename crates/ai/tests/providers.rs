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
async fn claude_code_streams_and_reads_the_prompt_from_stdin() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("sidekick-ai-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("claude");
    // Echoes stdin back as two deltas, then a result line.
    std::fs::write(
        &script,
        r#"#!/bin/sh
p=$(cat)
printf '%s\n' '{"type":"system","subtype":"init"}'
printf '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"you said: "}}}\n'
printf '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"%s"}}}\n' "$p"
printf '{"type":"result","subtype":"success","is_error":false,"result":"ignored"}\n'
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
    assert_eq!(r.unwrap(), "you said: hello; $(whoami)");
    assert_eq!(streamed, "you said: hello; $(whoami)");
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
                let (ctype, body) = if req.starts_with("GET /v1/models") {
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
