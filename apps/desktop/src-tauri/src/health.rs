//! Health check: things that were working and stopped (Ollama, the Composio
//! sign-in, Claude Code hooks) get one note on the island with a Fix button,
//! instead of failing quietly. Only a change from working to broken is
//! reported, once.

use std::collections::HashMap;
use std::time::Duration;

use sidekick_core::Event;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

pub const PROBLEM: &str = "health.problem";
const EVERY: Duration = Duration::from_secs(5 * 60);
const FIRST_AFTER: Duration = Duration::from_secs(90);
/// A check that just failed is tried again after this, so a moment of
/// being busy (Ollama loading a model) is not reported as broken.
const CONFIRM_AFTER: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Check {
    Ollama,
    Composio,
    Hooks,
}

impl Check {
    pub fn id(self) -> &'static str {
        match self {
            Check::Ollama => "ollama",
            Check::Composio => "composio",
            Check::Hooks => "hooks",
        }
    }

    fn message(self) -> (&'static str, &'static str) {
        match self {
            Check::Ollama => (
                "Ollama stopped",
                "Local AI and search by meaning are off until it runs again.",
            ),
            Check::Composio => (
                "Composio needs you to sign in again",
                "Calendar, mail and app tools are paused.",
            ),
            Check::Hooks => (
                "Claude Code hooks were removed",
                "Sidekick no longer hears when Claude Code finishes or asks for permission.",
            ),
        }
    }
}

/// Which checks newly broke: working last time, broken now. `None` means
/// the check does not apply (not set up), which never counts as broken.
pub fn newly_broken(before: &HashMap<Check, bool>, now: &[(Check, Option<bool>)]) -> Vec<Check> {
    now.iter()
        .filter(|(c, ok)| *ok == Some(false) && before.get(c) == Some(&true))
        .map(|(c, _)| *c)
        .collect()
}

async fn probe(app: &AppHandle) -> Vec<(Check, Option<bool>)> {
    let settings = lock(&app.state::<AppState>().settings).clone();
    let ollama = if settings.ai.local.enabled {
        Some(
            crate::setup::ollama_models(&settings.ai.local.base_url)
                .await
                .is_some(),
        )
    } else {
        None
    };
    let composio = if settings.composio.enabled && crate::composio::signed_in() {
        match crate::composio::apps(&settings.composio).await {
            Ok(_) => Some(true),
            Err(e) if e.contains("did not accept the key") => Some(false),
            // A network blip is not a broken sign-in.
            Err(_) => None,
        }
    } else {
        None
    };
    let settings_json = dirs::home_dir()
        .map(|h| {
            std::fs::read_to_string(h.join(".claude").join("settings.json")).unwrap_or_default()
        })
        .unwrap_or_default();
    let hooks = Some(crate::setup::hook_state(&settings_json) == crate::setup::HookState::Complete);
    vec![
        (Check::Ollama, ollama),
        (Check::Composio, composio),
        (Check::Hooks, hooks),
    ]
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_AFTER).await;
        let mut before: HashMap<Check, bool> = HashMap::new();
        loop {
            let mut now = probe(&app).await;
            let mut broken = newly_broken(&before, &now);
            if !broken.is_empty() {
                tokio::time::sleep(CONFIRM_AFTER).await;
                now = probe(&app).await;
                let still = newly_broken(&before, &now);
                broken.retain(|c| still.contains(c));
            }
            for c in broken {
                let (title, detail) = c.message();
                log::warn!("health: {title}");
                app.state::<AppState>().bus.publish(Event::new(
                    PROBLEM,
                    "health",
                    serde_json::json!({ "what": c.id(), "title": title, "detail": detail }),
                ));
            }
            for (c, ok) in now {
                if let Some(ok) = ok {
                    before.insert(c, ok);
                }
            }
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// The Fix button.
pub async fn fix(app: &AppHandle, what: &str) -> Result<String, String> {
    match what {
        "ollama" => {
            // It may have come back on its own since the note appeared.
            let base_url = lock(&app.state::<AppState>().settings)
                .ai
                .local
                .base_url
                .clone();
            if crate::setup::ollama_models(&base_url).await.is_some() {
                return Ok("Ollama is running again".into());
            }
            crate::setup::run(app, "ollama")
                .await
                .map(|()| "Opening Ollama".into())
        }
        "composio" => crate::composio::sign_in(app)
            .await
            .map(|_| "Sign in to Composio in the browser".into()),
        "hooks" => crate::claude_config::add_hooks().map(|_| "Claude Code hooks added back".into()),
        _ => Err("Nothing to fix".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_only_what_broke() {
        let mut before = HashMap::new();
        before.insert(Check::Ollama, true);
        before.insert(Check::Hooks, false);
        let now = [
            (Check::Ollama, Some(false)),
            (Check::Hooks, Some(false)),
            (Check::Composio, Some(false)),
        ];
        // Ollama worked and stopped; hooks were never on; Composio is new.
        assert_eq!(newly_broken(&before, &now), vec![Check::Ollama]);
        assert!(newly_broken(&before, &[(Check::Ollama, None)]).is_empty());
    }
}
