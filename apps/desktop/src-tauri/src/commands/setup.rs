//! First-run setup, connections (Composio, Claude Code, Codex) and the browser extension.

use super::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInfo {
    token: String,
    port: u16,
}

/// The pairing code to paste into the browser extension.
#[tauri::command]
pub fn browser_info(state: State<'_, AppState>) -> BrowserInfo {
    BrowserInfo {
        token: state.browser_token.clone(),
        port: sidekick_sensors::BrowserSensor::DEFAULT_PORT,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    items: Vec<crate::setup::SetupItem>,
    /// Installs every missing recommended tool in one go.
    install_all: Option<String>,
}

/// The setup checklist, checked again each time.
#[tauri::command]
pub async fn setup_status(app: AppHandle) -> SetupStatus {
    let items = crate::setup::status(&app).await;
    let install_all = crate::setup::install_all(&items);
    SetupStatus { items, install_all }
}

/// Opens a PowerShell window running one setup step ("all" for every
/// missing recommended tool).
#[tauri::command]
pub async fn setup_run(app: AppHandle, id: String) -> CmdResult<()> {
    crate::setup::run(&app, &id).await
}

/// Opens Composio Connect in the browser to sign in.
#[tauri::command]
pub async fn composio_sign_in(app: AppHandle) -> CmdResult<String> {
    crate::composio::sign_in(&app).await
}

#[tauri::command]
pub fn composio_sign_out(app: AppHandle) -> CmdResult<()> {
    crate::composio::sign_out(&app)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposioStatus {
    signed_in: bool,
    account: String,
    apps: Vec<crate::composio_api::App>,
    error: Option<String>,
}

/// Whether Composio is connected, and which of Sidekick's apps are.
#[tauri::command]
pub async fn composio_status(app: AppHandle) -> ComposioStatus {
    let c = lock(&app.state::<AppState>().settings).composio.clone();
    if !crate::composio::signed_in() {
        return ComposioStatus {
            signed_in: false,
            account: String::new(),
            apps: Vec::new(),
            error: None,
        };
    }
    // A slow or failed check keeps the last list and says so softly.
    let (apps, error) = match crate::composio::apps(&c).await {
        Ok(a) => (a, None),
        Err(e) => (crate::composio::cached_apps(), Some(e)),
    };
    ComposioStatus {
        signed_in: true,
        account: c.account,
        apps,
        error,
    }
}

/// Connects with a Composio consumer key instead of the browser sign-in.
#[tauri::command]
pub async fn composio_use_key(app: AppHandle, key: String) -> CmdResult<String> {
    crate::composio::use_key(&app, &key).await
}

/// Opens the browser to connect one app on Composio.
#[tauri::command]
pub async fn composio_connect(app: AppHandle, slug: String) -> CmdResult<()> {
    crate::composio::connect_app(&app, &slug).await
}

/// Copies the Composio server from Claude Code's config into Settings.
#[tauri::command]
pub fn composio_import(app: AppHandle) -> CmdResult<Settings> {
    let home = dirs::home_dir().ok_or("no home folder")?;
    let text = std::fs::read_to_string(home.join(".claude.json")).unwrap_or_default();
    let (url, headers) = crate::composio::from_claude_config(&text)
        .ok_or("No Composio server in Claude Code's settings (~/.claude.json)")?;
    let mut settings = lock(&app.state::<AppState>().settings).clone();
    settings.composio.url = url;
    settings.composio.headers = headers;
    settings.composio.enabled = true;
    apply_settings(&app, settings)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    code_folders: Vec<crate::detect::Folder>,
    search_folders: Vec<crate::detect::Folder>,
    chat_models: Vec<String>,
    embed_models: Vec<String>,
    claude_installed: bool,
    claude_hooks: bool,
    claude_mcp: bool,
    composio_signed_in: bool,
    composio_in_claude: bool,
    browsers: Vec<String>,
    /// Runnable recommended steps safe for welcome **Set up** (not Ollama / gh).
    installable: Vec<crate::setup::SetupItem>,
}

/// Everything Sidekick can set up on its own, found on this PC.
#[tauri::command]
pub async fn setup_detect(app: AppHandle) -> Found {
    let state = app.state::<AppState>();
    let settings = lock(&state.settings).clone();
    let items = crate::setup::status(&app).await;
    let done = |id: &str| items.iter().any(|i| i.id == id && i.done);
    let models = crate::setup::ollama_models(&settings.ai.local.base_url)
        .await
        .unwrap_or_default();
    let mut chat_models = Vec::new();
    let mut embed_models = Vec::new();
    for m in models {
        if sidekick_ai::is_embedding_model(&m) {
            embed_models.push(m);
        } else if sidekick_ai::is_chat_model(&m) {
            chat_models.push(m);
        }
    }
    let home = dirs::home_dir().unwrap_or_default();
    let claude_json = std::fs::read_to_string(home.join(".claude.json")).unwrap_or_default();
    let (code_folders, search_folders) = tauri::async_runtime::spawn_blocking(|| {
        (crate::detect::code_roots(), crate::detect::search_folders())
    })
    .await
    .unwrap_or_default();
    Found {
        code_folders,
        search_folders,
        chat_models,
        embed_models,
        claude_installed: done("claude_code"),
        claude_hooks: done("claude_hooks"),
        claude_mcp: done("claude_mcp"),
        composio_signed_in: crate::composio::signed_in(),
        composio_in_claude: crate::composio::from_claude_config(&claude_json).is_some(),
        browsers: executor(&state)
            .capabilities()
            .browsers
            .iter()
            .map(|b| b.id.clone())
            .collect(),
        installable: items
            .into_iter()
            .filter(|i| {
                i.runnable && !i.done && i.recommended && crate::setup::welcome_bulk_install(i)
            })
            .collect(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    code_folders: Vec<String>,
    search_folders: Vec<String>,
    chat_model: Option<String>,
    claude_hooks: bool,
    claude_mcp: bool,
    /// Setup step ids to run in one PowerShell window.
    install: Vec<String>,
    voice: bool,
    launch_at_login: bool,
}

/// Applies the choices from the welcome screen. Returns what was done.
#[tauri::command]
pub async fn setup_apply(app: AppHandle, plan: Plan) -> CmdResult<Vec<String>> {
    let mut done = Vec::new();
    let mut settings = lock(&app.state::<AppState>().settings).clone();
    if !plan.code_folders.is_empty() {
        settings.code_folders = plan.code_folders;
        done.push("Code folders set".to_owned());
    }
    if !plan.search_folders.is_empty() {
        settings.index_folders = plan.search_folders;
        done.push("Search folders set".to_owned());
    }
    if let Some(m) = plan.chat_model.filter(|m| !m.trim().is_empty()) {
        settings.ai.local.model = m;
        settings.ai.local.enabled = true;
    }
    settings.voice.enabled |= plan.voice;
    settings.launch_at_login = plan.launch_at_login;
    apply_settings(&app, settings)?;
    if plan.voice {
        let _ = crate::voice::download(&app);
    }
    if plan.claude_hooks {
        crate::claude_config::add_hooks()?;
        done.push("Claude Code hooks added".to_owned());
    }
    if plan.claude_mcp {
        claude_add_mcp(app.clone()).await?;
        done.push("Sidekick tools added to Claude Code".to_owned());
    }
    if !plan.install.is_empty() {
        done.extend(crate::setup::run_many(&app, &plan.install).await?);
    }
    // Vision may already be pulled, or just finished via install above.
    let base = lock(&app.state::<AppState>().settings)
        .ai
        .local
        .base_url
        .clone();
    if let Some(models) = crate::setup::ollama_models(&base).await {
        crate::setup::maybe_select_vision(&app, &models);
    }
    crate::search::reindex_folders(&app);
    Ok(done)
}

/// Adds Sidekick's hooks to Claude Code's settings (backed up first).
#[tauri::command]
pub fn claude_add_hooks() -> CmdResult<Option<String>> {
    crate::claude_config::add_hooks().map(|b| b.map(|p| p.display().to_string()))
}

/// Adds Sidekick's MCP server to Claude Code with `claude mcp add`.
#[tauri::command]
pub async fn claude_add_mcp(app: AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let path = lock(&state.settings).ai.claude_code.path.trim().to_owned();
    let claude = if path.is_empty() {
        which::which("claude").map_err(|_| "Install Claude Code first".to_string())?
    } else {
        std::path::PathBuf::from(path)
    };
    let url = format!("http://127.0.0.1:{}/mcp", crate::mcp::PORT);
    crate::claude_config::add_mcp(&claude, &url, &state.mcp_token).await
}

/// Adds Sidekick's notify script to Codex, so the island hears when a turn
/// is done. Returns the backup of Codex's settings, if one was made.
#[tauri::command]
pub fn codex_add_notify(app: AppHandle) -> CmdResult<Option<String>> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    crate::codex_config::add_notify(&dir).map(|b| b.map(|p| p.display().to_string()))
}

/// Adds Sidekick's MCP server to Codex's settings.
#[tauri::command]
pub fn codex_add_mcp(state: State<'_, AppState>) -> CmdResult<Option<String>> {
    let url = format!("http://127.0.0.1:{}/mcp", crate::mcp::PORT);
    crate::codex_config::add_mcp(&url, &state.mcp_token).map(|b| b.map(|p| p.display().to_string()))
}

/// Installed browsers and whether the extension is connected in each.
#[tauri::command]
pub fn browsers_status(app: AppHandle) -> Vec<crate::setup::BrowserStatus> {
    crate::setup::browsers(&app.state::<AppState>())
}

/// Opens a browser's extensions page with the extension's path copied.
#[tauri::command]
pub async fn extension_install(
    app: AppHandle,
    browser: String,
) -> CmdResult<crate::extension::Guide> {
    crate::extension::install(&app, &browser).await
}
