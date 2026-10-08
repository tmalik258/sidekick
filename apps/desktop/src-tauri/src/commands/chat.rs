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
pub async fn editors_list(app: AppHandle) -> crate::editors::Editors {
    // Reads the registry and looks for editors on disk: off the UI thread.
    tauri::async_runtime::spawn_blocking(move || crate::editors::list(&app))
        .await
        .unwrap_or_default()
}

/// The installed agent used most in a project, from `agent:{project}`.
fn usual_agent(
    app: &AppHandle,
    key: &str,
    settings: &sidekick_core::Settings,
) -> Option<crate::agents::Agent> {
    let counts = lock(&app.state::<AppState>().storage)
        .choice_counts(key)
        .unwrap_or_default();
    let mut used: Vec<(String, u32)> = counts.into_iter().collect();
    used.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    used.iter()
        .filter_map(|(id, _)| crate::agents::Agent::from_id(id))
        .find(|a| a.resolve(settings).is_some())
}

/// The agent you usually use in this project, for the agent picker.
#[tauri::command]
pub async fn agent_usual(app: AppHandle, path: String) -> Option<String> {
    off_ui(move || {
        let project = std::path::Path::new(&path)
            .file_name()?
            .to_string_lossy()
            .into_owned();
        let settings = lock(&app.state::<AppState>().settings).clone();
        usual_agent(&app, &format!("agent:{project}"), &settings).map(|a| a.id().to_owned())
    })
    .await
    .ok()
    .flatten()
}

/// Starts Claude Code or Codex in a project, inside the island.
#[tauri::command]
pub async fn agent_start(
    app: AppHandle,
    agent: String,
    path: String,
    prompt: String,
    mode: crate::sessions::Mode,
    model: Option<String>,
    effort: Option<String>,
) -> CmdResult<crate::sessions::Started> {
    // The agent you use for this project is remembered and picked next
    // time you do not name one.
    let project = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let key = format!("agent:{project}");
    let settings = lock(&app.state::<AppState>().settings).clone();
    let agent = match crate::agents::Agent::from_id(&agent) {
        Some(a) => a,
        None => usual_agent(&app, &key, &settings)
            .or_else(|| crate::agents::chosen(&settings))
            .ok_or(
                "Install Claude Code, Codex, GitHub Copilot CLI or Cursor first (Settings > AI).",
            )?,
    };
    if !project.is_empty() && settings.learning {
        let ts = chrono::Utc::now().to_rfc3339();
        let _ = lock(&app.state::<AppState>().storage).record_choice(&key, agent.id(), &ts);
    }
    crate::sessions::start_tuned(
        &app,
        agent,
        std::path::Path::new(&path),
        &prompt,
        mode,
        model,
        effort,
    )
    .await
}

/// Continues an Ask conversation in Claude Code or Codex, inside the island.
#[tauri::command]
pub async fn agent_handoff(
    app: AppHandle,
    messages: Vec<sidekick_ai::Message>,
    reason: Option<String>,
) -> CmdResult<crate::sessions::Started> {
    let settings = lock(&app.state::<AppState>().settings).clone();
    let agent = crate::agents::chosen(&settings)
        .ok_or("Install Claude Code or Codex first (Settings > AI).")?;
    let dir = crate::agents::write_handoff(&app, &messages, reason.as_deref())?;
    crate::sessions::start(
        &app,
        agent,
        &dir,
        crate::agents::HANDOFF_PROMPT,
        crate::sessions::Mode::Ask,
    )
    .await
}

/// A follow-up, or a steer while it works.
#[tauri::command]
pub async fn agent_send(app: AppHandle, id: String, text: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || crate::sessions::send(&app, &id, &text))
        .await
        .map_err(|e| e.to_string())?
}

/// Carries on a session that ended or was cut off by a restart.
#[tauri::command]
pub async fn agent_resume(app: AppHandle, id: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || crate::sessions::resume(&app, &id))
        .await
        .map_err(|e| e.to_string())?
}

/// How many files rewinding to before message `index` would put back.
#[tauri::command]
pub async fn agent_rewind_preview(id: String, index: usize) -> CmdResult<usize> {
    tauri::async_runtime::spawn_blocking(move || crate::sessions::rewind_preview(&id, index))
        .await
        .map_err(|e| e.to_string())?
}

/// Puts the project back to before message `index`.
#[tauri::command]
pub async fn agent_rewind(id: String, index: usize) -> CmdResult<usize> {
    tauri::async_runtime::spawn_blocking(move || crate::sessions::rewind(&id, index))
        .await
        .map_err(|e| e.to_string())?
}

/// Files in the session's project for @ in the composer.
#[tauri::command]
pub async fn agent_files(id: String, query: String) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || crate::sessions::files(&id, &query))
        .await
        .unwrap_or_default()
}

/// Opens the session's project in the user's editor ("Open in Cursor").
#[tauri::command]
pub async fn agent_open_editor(app: AppHandle, id: String) -> CmdResult<()> {
    let path = crate::sessions::project(&id).ok_or("That session is gone.")?;
    crate::state::executor(&app.state::<AppState>())
        .run("open_in_editor", &serde_json::json!({ "path": path }))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Commands for / in the composer.
#[tauri::command]
pub fn agent_commands(id: String) -> Vec<crate::sessions::SlashCommand> {
    crate::sessions::commands(&id)
}

#[tauri::command]
pub fn agent_stop(id: String) {
    crate::sessions::stop(&id);
}

#[tauri::command]
pub fn agent_answer(question: String, answer: crate::sessions::Answer) -> CmdResult<()> {
    crate::sessions::answer(&question, answer)
}

/// What the session changed, by file and hunk.
#[tauri::command]
pub async fn agent_changes(id: String) -> CmdResult<Vec<crate::review::FileChange>> {
    tauri::async_runtime::spawn_blocking(move || crate::sessions::changes(&id))
        .await
        .map_err(|e| e.to_string())?
}

/// Undoes one hunk, one file, or everything (no path).
#[tauri::command]
pub async fn agent_undo(id: String, path: Option<String>, hunk: Option<usize>) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || crate::sessions::undo(&id, path.as_deref(), hunk))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn agent_close(id: String) {
    crate::sessions::close(&id);
}

/// Carries on in the CLI itself, in a terminal.
#[tauri::command]
pub async fn agent_terminal(app: AppHandle, id: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || crate::sessions::open_terminal(&app, &id))
        .await
        .map_err(|e| e.to_string())?
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
pub async fn agents_status(app: AppHandle) -> CmdResult<crate::agents::Agents> {
    // Looks for the CLIs on PATH: off the UI thread.
    tauri::async_runtime::spawn_blocking(move || {
        let settings = lock(&app.state::<AppState>().settings).clone();
        crate::agents::status(&settings)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Recent Ask conversations, newest first.
#[tauri::command]
pub async fn chats_list(app: AppHandle) -> CmdResult<Vec<sidekick_core::ChatSummary>> {
    tauri::async_runtime::spawn_blocking(move || {
        lock(&app.state::<AppState>().storage)
            .recent_chats(30)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A saved conversation's turns, as the UI saved them.
#[tauri::command]
pub async fn chat_get(app: AppHandle, id: String) -> CmdResult<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let text = lock(&app.state::<AppState>().storage)
            .chat_turns(&id)
            .map_err(|e| e.to_string())?
            .ok_or("That conversation is gone")?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Masks secrets in every string of a saved chat, so history never keeps them.
fn mask_strings(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::String(s) => {
            if let std::borrow::Cow::Owned(m) = sidekick_sensors::classify::mask(s) {
                *s = m;
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(mask_strings),
        serde_json::Value::Object(o) => o.values_mut().for_each(mask_strings),
        _ => {}
    }
}

#[tauri::command]
pub async fn chat_save(
    app: AppHandle,
    id: String,
    title: String,
    mut turns: serde_json::Value,
) -> CmdResult<()> {
    // Masking and writing a long chat after every answer: off the UI thread.
    tauri::async_runtime::spawn_blocking(move || {
        let title: String = sidekick_sensors::classify::mask(title.trim())
            .chars()
            .take(80)
            .collect();
        mask_strings(&mut turns);
        lock(&app.state::<AppState>().storage)
            .save_chat(&id, &title, &turns.to_string())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
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

/// Installed apps and files named like what is typed in Ask, no model.
#[tauri::command]
pub async fn instant_find(app: AppHandle, query: String) -> crate::instant::Results {
    tauri::async_runtime::spawn_blocking(move || crate::instant::find(&app, &query))
        .await
        .unwrap_or_default()
}

/// Starts an app picked from Ask's instant results.
#[tauri::command]
pub async fn app_launch(id: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || sidekick_actions::pc::launch_app_id(&id))
        .await
        .map_err(|e| e.to_string())?
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Opens a Windows Settings page picked from Ask's instant results; only the
/// named pages in `SETTINGS_PAGES` can open.
#[tauri::command]
pub async fn windows_settings_open(page: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        sidekick_actions::pc::control("open_settings", None, Some(&page))
    })
    .await
    .map_err(|e| e.to_string())?
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Switches picked from Ask's instant results, without asking a model.
const PC_SWITCHES: &[&str] = &[
    "wifi",
    "bluetooth",
    "hotspot",
    "airplane",
    "night_light",
    "dnd",
    "dark_mode",
];

/// Turns a Windows switch on or off from Ask's instant results; returns
/// what happened, in words.
#[tauri::command]
pub async fn pc_switch(name: String, on: bool) -> CmdResult<String> {
    if !PC_SWITCHES.contains(&name.as_str()) {
        return Err(format!("{name} is not a switch"));
    }
    let what = format!("{name}_{}", if on { "on" } else { "off" });
    tauri::async_runtime::spawn_blocking(move || sidekick_actions::pc::control(&what, None, None))
        .await
        .map_err(|e| e.to_string())?
        .map(|o| o.message)
        .map_err(|e| e.to_string())
}

/// Opens a file or folder picked from Ask's instant results. Programs and
/// scripts are shown in their folder instead of run.
#[tauri::command]
pub async fn file_open(app: AppHandle, path: String) -> CmdResult<()> {
    let action = if crate::ask_tools::runs_code(&path) {
        "reveal_path"
    } else {
        "open_path"
    };
    crate::state::executor(&app.state::<AppState>())
        .run(action, &serde_json::json!({ "path": path }))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Memory an agent session uses, in bytes (None once it stopped).
#[tauri::command]
pub async fn agent_memory(id: String) -> Option<u64> {
    // Walks every process on the PC: never on the UI thread.
    tauri::async_runtime::spawn_blocking(move || crate::sessions::memory(&id))
        .await
        .ok()
        .flatten()
}

/// The model and thinking for one session, from its chat box.
#[tauri::command]
pub fn agent_tune(id: String, model: Option<String>, effort: Option<String>) {
    crate::sessions::tune(&id, model, effort);
}

/// Finish a session that ran in its own worktree: merge it back.
#[tauri::command]
pub async fn agent_finish(id: String) -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(move || crate::sessions::finish(&id))
        .await
        .map_err(|e| e.to_string())?
}

/// Chats started in Cursor, for the Agents list.
#[tauri::command]
pub async fn cursor_chats() -> Vec<crate::cursor_chats::CursorChat> {
    tauri::async_runtime::spawn_blocking(|| crate::cursor_chats::list(20))
        .await
        .unwrap_or_default()
}

/// Opens a Cursor chat's project in Cursor ("Open in Cursor").
#[tauri::command]
pub async fn cursor_open(app: AppHandle, path: String) -> CmdResult<()> {
    if !std::path::Path::new(&path).is_dir() {
        return Err("That project folder is gone.".into());
    }
    if let Ok(cursor) = which::which("cursor") {
        let mut cmd = tokio::process::Command::new(cursor);
        cmd.arg(&path);
        crate::agents::hide_console(&mut cmd);
        return cmd.spawn().map(|_| ()).map_err(|e| e.to_string());
    }
    crate::state::executor(&app.state::<AppState>())
        .run(
            "open_in_editor",
            &serde_json::json!({ "path": path, "editor": "cursor" }),
        )
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
