//! Ask: chats, answers, proposals and the agents behind them.

use super::*;

#[tauri::command]
pub async fn ai_status(app: AppHandle) -> Vec<ai::ProviderStatus> {
    ai::status(&app).await
}

#[tauri::command]
pub fn ai_chat(
    app: AppHandle,
    id: String,
    messages: Vec<sidekick_ai::Message>,
    attach: ai::Attach,
    local_only: bool,
) {
    ai::chat(&app, id, messages, attach, local_only);
}

/// Installed code editors and the one projects open in.
#[tauri::command]
pub fn editors_list(app: AppHandle) -> crate::editors::Editors {
    crate::editors::list(&app)
}

#[tauri::command]
pub fn ai_cancel(app: AppHandle, id: String) {
    ai::cancel(&app, &id);
}

/// Shows an answer that was started early, at a pause in speech.
#[tauri::command]
pub fn ai_release(id: String) {
    ai::release(&id);
}

#[tauri::command]
pub fn timing_record(app: AppHandle, name: String, ms: u64) {
    crate::timings::record(&app, &name, ms);
}

#[tauri::command]
pub fn timings_recent(app: AppHandle) -> Vec<crate::timings::Timing> {
    crate::timings::recent(&app, 60)
}

/// Turns the island into Ask mode, optionally with a prompt.
#[tauri::command]
pub fn ask_open(app: AppHandle, prompt: Option<String>, ask: bool) {
    ask::open(
        &app,
        ask::Open {
            prompt,
            ask,
            ..Default::default()
        },
    );
}

/// Shows welcome when the island is ready and onboarding is not done yet.
#[tauri::command]
pub fn ask_ensure_welcome(app: AppHandle) {
    ask::ensure_welcome(&app);
}

/// Parks welcome until the user hovers the island again (does not finish onboarding).
#[tauri::command]
pub fn ask_defer_welcome(app: AppHandle) {
    ask::defer_welcome(&app);
}

/// Brings a parked welcome back, e.g. once what it was waiting for is done.
#[tauri::command]
pub fn ask_resume_welcome(app: AppHandle) {
    ask::resume_welcome(&app);
}

#[tauri::command]
pub fn ask_close(app: AppHandle) {
    ask::close(&app);
}

/// Moves what an earlier action created to the Recycle Bin.
#[tauri::command]
pub fn action_undo(app: AppHandle, id: i64) -> CmdResult<String> {
    crate::undo::undo(&app, id)
}

/// Runs an action Ask offered as a button, after the user tapped it.
#[tauri::command]
pub async fn ai_run_proposal(app: AppHandle, id: String) -> CmdResult<crate::ask_tools::Ran> {
    crate::ask_tools::run_proposal(&app, &id).await
}

/// Opens the coding agent (Claude Code or Codex) in a terminal with this
/// conversation, to finish what the local model could not. Returns its name.
#[tauri::command]
pub async fn ai_handoff(
    app: AppHandle,
    messages: Vec<sidekick_ai::Message>,
    reason: Option<String>,
) -> CmdResult<String> {
    crate::agents::hand_off(&app, &messages, reason.as_deref()).await
}

/// Opens a file, folder or web link from an answer (a clicked link).
/// Programs are never started this way.
#[tauri::command]
pub async fn ai_open_link(app: AppHandle, target: String) -> CmdResult<String> {
    crate::ask_tools::open_target(&app, &target).await
}

/// Which coding agents are installed and which one gets handoffs.
#[tauri::command]
pub fn agents_status(state: State<'_, AppState>) -> crate::agents::Agents {
    crate::agents::status(&lock(&state.settings))
}

/// Recent Ask conversations, newest first.
#[tauri::command]
pub fn chats_list(state: State<'_, AppState>) -> CmdResult<Vec<sidekick_core::ChatSummary>> {
    lock(&state.storage)
        .recent_chats(30)
        .map_err(|e| e.to_string())
}

/// A saved conversation's turns, as the UI saved them.
#[tauri::command]
pub fn chat_get(state: State<'_, AppState>, id: String) -> CmdResult<serde_json::Value> {
    let text = lock(&state.storage)
        .chat_turns(&id)
        .map_err(|e| e.to_string())?
        .ok_or("That conversation is gone")?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn chat_save(
    state: State<'_, AppState>,
    id: String,
    title: String,
    turns: serde_json::Value,
) -> CmdResult<()> {
    let title: String = title.trim().chars().take(80).collect();
    lock(&state.storage)
        .save_chat(&id, &title, &turns.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn chat_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    lock(&state.storage)
        .delete_chat(&id)
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModels {
    reachable: bool,
    chat: Vec<String>,
    vision: Vec<String>,
    embed: Vec<String>,
}

/// Models on the local AI server, split into chat, vision, and search models.
#[tauri::command]
pub async fn local_models(app: AppHandle) -> LocalModels {
    let base = lock(&app.state::<AppState>().settings)
        .ai
        .local
        .base_url
        .clone();
    let Some(models) = crate::setup::ollama_models(&base).await else {
        return LocalModels {
            reachable: false,
            chat: Vec::new(),
            vision: Vec::new(),
            embed: Vec::new(),
        };
    };
    // Auto-pick vision when the suggested model is installed but unset.
    crate::setup::maybe_select_vision(&app, &models);
    split_local_models(models, true)
}

fn split_local_models(models: Vec<String>, reachable: bool) -> LocalModels {
    let mut chat = Vec::new();
    let mut vision = Vec::new();
    let mut embed = Vec::new();
    for m in models {
        if sidekick_ai::is_embedding_model(&m) {
            embed.push(m);
        } else if sidekick_ai::is_vision_model(&m) {
            vision.push(m);
        } else if sidekick_ai::is_chat_model(&m) {
            chat.push(m);
        }
    }
    LocalModels {
        reachable,
        chat,
        vision,
        embed,
    }
}

#[cfg(test)]
mod tests {
    use super::split_local_models;

    #[test]
    fn local_models_keep_moondream_out_of_chat() {
        let models = split_local_models(
            vec![
                "moondream:latest".into(),
                "qwen3:4b".into(),
                "nomic-embed-text".into(),
            ],
            true,
        );
        assert_eq!(models.chat, ["qwen3:4b"]);
        assert_eq!(models.vision, ["moondream:latest"]);
        assert_eq!(models.embed, ["nomic-embed-text"]);
        assert!(!models.chat.iter().any(|m| m.contains("moondream")));
    }
}
