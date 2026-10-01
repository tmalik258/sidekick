use std::sync::Arc;

use serde::Serialize;

use crate::{AiError, AiProvider, CancellationToken, ChatRequest, Sink};

/// Which provider answered, and the full text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Answer {
    pub provider: String,
    pub text: String,
}

/// Tries providers in order (FR-AI-02). A provider that is not reachable, or
/// fails before sending any text, hands over to the next one. Once text has
/// reached the user, a failure is reported instead of starting over.
#[derive(Default, Clone)]
pub struct Router {
    providers: Vec<Arc<dyn AiProvider>>,
}

impl Router {
    pub fn new(providers: Vec<Arc<dyn AiProvider>>) -> Self {
        Self { providers }
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.providers.iter().map(|p| p.id()).collect()
    }

    /// `local_only` keeps the request on this machine (for anything that
    /// might be private).
    pub async fn chat(
        &self,
        req: &ChatRequest,
        sink: &Sink,
        cancel: &CancellationToken,
        local_only: bool,
    ) -> Result<Answer, AiError> {
        let mut last_err = AiError::NoProvider;
        for p in &self.providers {
            if local_only && !p.is_local() {
                continue;
            }
            if cancel.is_cancelled() {
                return Err(AiError::Cancelled);
            }
            if !p.available().await {
                continue;
            }
            match p.chat(req, sink, cancel).await {
                Ok(text) => {
                    return Ok(Answer {
                        provider: p.id().to_owned(),
                        text,
                    });
                }
                Err(AiError::Cancelled) => return Err(AiError::Cancelled),
                Err(err) if sink.has_sent() => return Err(err),
                Err(err) => {
                    log::warn!("AI provider {} failed, trying the next: {err}", p.id());
                    last_err = err;
                }
            }
        }
        Err(last_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use tokio::sync::mpsc;

    struct Fake {
        id: &'static str,
        local: bool,
        up: bool,
        reply: Result<&'static str, &'static str>,
        partial: bool,
    }

    #[async_trait]
    impl AiProvider for Fake {
        fn id(&self) -> &'static str {
            self.id
        }
        fn is_local(&self) -> bool {
            self.local
        }
        async fn available(&self) -> bool {
            self.up
        }
        async fn chat(
            &self,
            _req: &ChatRequest,
            sink: &Sink,
            _cancel: &CancellationToken,
        ) -> Result<String, AiError> {
            if self.partial {
                sink.send("half");
            }
            match self.reply {
                Ok(t) => {
                    sink.send(t);
                    Ok(t.to_owned())
                }
                Err(e) => Err(AiError::Failed(e.into())),
            }
        }
    }

    fn fake(
        id: &'static str,
        up: bool,
        reply: Result<&'static str, &'static str>,
    ) -> Arc<dyn AiProvider> {
        Arc::new(Fake {
            id,
            local: id == "local",
            up,
            reply,
            partial: false,
        })
    }

    async fn ask(router: &Router, local_only: bool) -> Result<Answer, AiError> {
        let (tx, _rx) = mpsc::unbounded_channel();
        router
            .chat(
                &ChatRequest::default(),
                &Sink::new(tx),
                &CancellationToken::new(),
                local_only,
            )
            .await
    }

    #[tokio::test]
    async fn skips_unreachable_and_failing_providers() {
        let router = Router::new(vec![
            fake("claude_code", false, Ok("never")),
            fake("anthropic", true, Err("boom")),
            fake("local", true, Ok("hi")),
        ]);
        let a = ask(&router, false).await.unwrap();
        assert_eq!(a.provider, "local");
        assert_eq!(a.text, "hi");
    }

    #[tokio::test]
    async fn local_only_never_uses_cloud() {
        let router = Router::new(vec![fake("anthropic", true, Ok("cloud"))]);
        assert!(matches!(ask(&router, true).await, Err(AiError::NoProvider)));
    }

    #[tokio::test]
    async fn no_fallback_after_partial_text() {
        let router = Router::new(vec![
            Arc::new(Fake {
                id: "claude_code",
                local: false,
                up: true,
                reply: Err("cut off"),
                partial: true,
            }),
            fake("local", true, Ok("hi")),
        ]);
        let err = ask(&router, false).await.unwrap_err();
        assert_eq!(err.to_string(), "cut off");
    }
}
