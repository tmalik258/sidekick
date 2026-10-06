//! Setup checklist: what is installed, connected and configured, and the
//! exact command or Settings tab for each item that is not. Status checks are
//! read-only. "Run" opens a visible PowerShell window with an allowlisted
//! command from this file, except starting Ollama, which opens the Ollama
//! app like its Start menu shortcut, and paste-only steps (API keys), which
//! copy the command and open a blank PowerShell for the user. "Add for me"
//! (hooks/MCP) and the browser helper live in other modules and run only
//! when the user presses the button.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, executor, lock};

/// Where the AI, connections and tools sections start in the checklist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Group {
    Ai,
    Connect,
    Tools,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupItem {
    pub id: &'static str,
    pub group: Group,
    pub title: &'static str,
    /// What it unlocks, in a few words.
    pub why: &'static str,
    pub done: bool,
    /// Short state, for example "Ready" or "Ollama is not running".
    pub status: String,
    /// What to type, ready to copy. None when it is set in Settings.
    pub command: Option<String>,
    /// The command can be run from Sidekick in a new terminal window.
    pub runnable: bool,
    /// Button label for running it: "Install", "Download", "Sign in", "Run".
    pub action: &'static str,
    /// "Run" opens the installed app instead of a terminal window.
    pub opens_app: bool,
    /// Opens a terminal and copies the command; the user pastes and edits it.
    pub opens_terminal: bool,
    /// Settings tab where this is set up.
    pub tab: Option<&'static str>,
    /// Part of the recommended setup; the rest is optional.
    pub recommended: bool,
}

impl SetupItem {
    fn new(id: &'static str, group: Group, title: &'static str, why: &'static str) -> Self {
        Self {
            id,
            group,
            title,
            why,
            done: false,
            status: String::new(),
            command: None,
            runnable: false,
            action: "Install",
            opens_app: false,
            opens_terminal: false,
            tab: None,
            recommended: false,
        }
    }
    fn done(mut self, done: bool, ready: &str, missing: &str) -> Self {
        self.done = done;
        self.status = if done { ready } else { missing }.to_owned();
        self
    }
    fn run(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self.runnable = true;
        self
    }
    fn action(mut self, action: &'static str) -> Self {
        self.action = action;
        self
    }
    /// "Run" opens the app through [`open_app`]; there is no command.
    fn opens_app(mut self) -> Self {
        self.command = None;
        self.runnable = true;
        self.opens_app = true;
        self.action("Run")
    }
    fn copy(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self.runnable = false;
        self
    }
    /// Copies the command and opens PowerShell so the user can paste and edit.
    fn terminal_paste(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self.runnable = true;
        self.opens_terminal = true;
        self.action = "Open terminal";
        self
    }
    fn tab(mut self, tab: &'static str) -> Self {
        self.tab = Some(tab);
        self
    }
    fn recommended(mut self) -> Self {
        self.recommended = true;
        self
    }
}

/// Whether the welcome **On this PC** batch may offer this step. Ollama and
/// GitHub CLI need the checklist (Run, Sign in, model downloads); winget or
/// opening the app alone does not finish them.
pub fn welcome_bulk_install(item: &SetupItem) -> bool {
    if item.opens_app || item.opens_terminal {
        return false;
    }
    !matches!(
        item.id,
        "ollama" | "ollama_chat" | "ollama_embed" | "ollama_vision" | "gh"
    )
}

pub const CLAUDE_HOOK_URL: &str = "http://127.0.0.1:47821/claude-code";
/// Where Codex's notify script sends each finished turn.
pub const CODEX_HOOK_URL: &str = "http://127.0.0.1:47821/codex";

fn winget(id: &str) -> String {
    format!("winget install -e --id {id} --accept-source-agreements")
}

/// The hooks block for `~/.claude/settings.json`, the same as Settings > AI.
pub fn hook_snippet() -> String {
    let hook = |timeout: u32| serde_json::json!([{ "hooks": [{ "type": "http", "url": CLAUDE_HOOK_URL, "timeout": timeout }] }]);
    let v = serde_json::json!({
        "hooks": {
            "Stop": hook(5),
            "Notification": hook(5),
            "PermissionRequest": hook(30),
        }
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// How far the user's Claude Code settings already point at Sidekick.
#[derive(Debug, PartialEq, Eq)]
pub enum HookState {
    Missing,
    /// Hooked up, but permission requests still only ask in the terminal.
    NoPermissions,
    Complete,
}

pub fn hook_state(settings_json: &str) -> HookState {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(settings_json) else {
        return HookState::Missing;
    };
    let points_here = |event: &str| {
        v["hooks"][event].as_array().is_some_and(|groups| {
            groups
                .iter()
                .any(|g| g.to_string().contains(CLAUDE_HOOK_URL))
        })
    };
    if !points_here("Stop") && !points_here("Notification") && !points_here("PermissionRequest") {
        HookState::Missing
    } else if points_here("PermissionRequest") {
        HookState::Complete
    } else {
        HookState::NoPermissions
    }
}

/// Claude Code keeps user-scope MCP servers in `~/.claude.json`.
pub fn mcp_added(claude_json: &str, url: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(claude_json).is_ok_and(|v| {
        v["mcpServers"]
            .as_object()
            .is_some_and(|servers| servers.values().any(|s| s["url"].as_str() == Some(url)))
    })
}

/// The vision model suggested when none is set: small enough for a 4 GB GPU.
pub const VISION_MODEL: &str = "moondream";

/// Ollama names models `name:tag`; `nomic-embed-text` matches
/// `nomic-embed-text:latest`.
pub fn has_model(models: &[String], wanted: &str) -> bool {
    resolve_model_tag(models, wanted).is_some()
}

/// The actual Ollama tag for `wanted` (e.g. `moondream` → `moondream:latest`).
pub fn resolve_model_tag(models: &[String], wanted: &str) -> Option<String> {
    let base = |m: &str| m.split(':').next().unwrap_or(m).to_ascii_lowercase();
    let wanted_has_tag = wanted.contains(':');
    models.iter().find_map(|m| {
        let matched = if wanted_has_tag {
            m.eq_ignore_ascii_case(wanted)
        } else {
            base(m) == base(wanted)
        };
        matched.then(|| m.clone())
    })
}

/// When vision is unset (empty) and the suggested model is installed, save its tag.
/// Leaves an explicit Off (`"off"`) or any other non-empty choice alone.
pub fn maybe_select_vision(app: &AppHandle, models: &[String]) {
    let mut settings = lock(&app.state::<AppState>().settings).clone();
    if !settings.ai.local.vision_model.trim().is_empty() {
        return;
    }
    let Some(tag) = resolve_model_tag(models, VISION_MODEL) else {
        return;
    };
    settings.ai.local.vision_model = tag;
    let _ = crate::commands::apply_settings(app, settings);
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

pub async fn ollama_models(base_url: &str) -> Option<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(1500))
        .build()
        .ok()?;
    let resp = client
        .get(format!("{}/models", base_url.trim_end_matches('/')))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    Some(
        v["data"]
            .as_array()?
            .iter()
            .filter_map(|m| m["id"].as_str().map(str::to_owned))
            .collect(),
    )
}

fn found(name: &str) -> bool {
    which::which(name).is_ok()
}

/// `gh auth status` exits 0 when signed in.
async fn gh_signed_in() -> bool {
    let mut cmd = tokio::process::Command::new("gh");
    cmd.args(["auth", "status"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    matches!(
        tokio::time::timeout(Duration::from_secs(5), cmd.status()).await,
        Ok(Ok(s)) if s.success()
    )
}

/// How recently the extension must have checked in to count as connected.
const BROWSER_RECENT_SECS: i64 = 7 * 24 * 3600;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStatus {
    pub id: String,
    pub name: String,
    /// The extension checked in from this kind of browser recently.
    pub connected: bool,
}

/// Installed browsers and whether the extension is connected in each.
pub fn browsers(state: &AppState) -> Vec<BrowserStatus> {
    let seen = state.browser.seen();
    let now = chrono::Utc::now().timestamp();
    let recent = |name: &str| {
        seen.iter()
            .any(|(n, t)| n.eq_ignore_ascii_case(name) && now - t < BROWSER_RECENT_SECS)
    };
    executor(state)
        .capabilities()
        .browsers
        .iter()
        .map(|b| {
            let name = b.label().to_owned();
            // Firefox and Zen report as Firefox.
            let reported = if b.id == "zen" {
                "Firefox"
            } else {
                name.as_str()
            };
            BrowserStatus {
                connected: recent(reported),
                id: b.id.clone(),
                name,
            }
        })
        .collect()
}

pub async fn status(app: &AppHandle) -> Vec<SetupItem> {
    refresh_path();
    // setx writes the user env; pick the key up without asking for a restart.
    refresh_user_env("ANTHROPIC_API_KEY");
    let state = app.state::<AppState>();
    let mut settings = lock(&state.settings).clone();
    let base_url = settings.ai.local.base_url.clone();

    // Independent probes run together so wall time ≈ the slowest one.
    let caps_fut = tauri::async_runtime::spawn_blocking(sidekick_actions::Capabilities::detect);
    let models_fut = ollama_models(&base_url);
    let gh_fut = gh_signed_in();
    let (caps_res, models, gh_ok) = tokio::join!(caps_fut, models_fut, gh_fut);

    let caps = caps_res.unwrap_or_else(|_| executor(&state).capabilities().clone());
    *state
        .executor
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        std::sync::Arc::new(sidekick_actions::Executor::new(caps.clone()));
    let home = dirs::home_dir().unwrap_or_default();
    let mut items = Vec::new();

    // AI
    let claude_path = settings.ai.claude_code.path.trim();
    let claude = if claude_path.is_empty() {
        found("claude")
    } else {
        Path::new(claude_path).is_file()
    };
    items.push(
        SetupItem::new(
            "claude_code",
            Group::Ai,
            "Claude Code",
            "Uses your Claude plan. Run claude once to sign in.",
        )
        .done(claude, "Installed", "Not installed")
        .run("irm https://claude.ai/install.ps1 | iex")
        .recommended(),
    );
    let codex_path = settings.ai.codex.path.trim();
    let codex = if codex_path.is_empty() {
        found("codex")
    } else {
        Path::new(codex_path).is_file()
    };
    items.push(
        SetupItem::new(
            "codex",
            Group::Ai,
            "Codex",
            "Uses your ChatGPT plan. Run codex once to sign in.",
        )
        .done(codex, "Installed", "Not installed")
        .run("npm install -g @openai/codex"),
    );

    let ollama_installed = found("ollama");
    let ollama = SetupItem::new(
        "ollama",
        Group::Ai,
        "Ollama",
        "Free local AI on this PC, and search by meaning.",
    );
    items.push(
        match (&models, ollama_installed) {
            (Some(_), _) => ollama.done(true, "Running", ""),
            (None, true) => ollama.done(false, "", "Installed, not running").opens_app(),
            (None, false) => ollama
                .done(false, "", "Not installed")
                .run(winget("Ollama.Ollama")),
        }
        .recommended(),
    );

    let chat_model = if settings.ai.local.model.trim().is_empty() {
        "qwen3:4b".to_owned()
    } else {
        settings.ai.local.model.trim().to_owned()
    };
    let has_chat = models.as_ref().is_some_and(|m| {
        if settings.ai.local.model.trim().is_empty() {
            m.iter().any(|x| sidekick_ai::is_chat_model(x))
        } else {
            has_model(m, &chat_model)
        }
    });
    let mut chat = SetupItem::new(
        "ollama_chat",
        Group::Ai,
        "Local chat model",
        "Private answers on this PC. About 2.5 GB.",
    )
    .done(
        has_chat,
        "Downloaded",
        if models.is_some() {
            "Not downloaded"
        } else {
            "Needs Ollama running"
        },
    );
    if models.is_some() {
        chat = chat
            .run(format!("ollama pull {chat_model}"))
            .action("Download");
    }
    items.push(chat);

    let embed_model = settings.semantic_search.model.trim().to_owned();
    let has_embed = models.as_ref().is_some_and(|m| has_model(m, &embed_model));
    let mut embed = SetupItem::new(
        "ollama_embed",
        Group::Ai,
        "Search model",
        "Search by meaning. About 270 MB.",
    )
    .done(
        has_embed,
        "Downloaded",
        if models.is_some() {
            "Not downloaded"
        } else {
            "Needs Ollama running"
        },
    )
    .recommended();
    if models.is_some() {
        embed = embed
            .run(format!("ollama pull {embed_model}"))
            .action("Download");
    }
    items.push(embed);

    // Optional: a small model that sees pictures, for screen questions
    // without text. Empty means unset (auto-pick when installed); "off" is
    // an explicit OCR-only choice. Pull still targets moondream when unset/off.
    if let Some(list) = models.as_ref() {
        maybe_select_vision(app, list);
        // Re-read after a possible auto-pick.
        settings = lock(&app.state::<AppState>().settings).clone();
    }
    let vision = settings.ai.local.vision_model.trim();
    let vision_model = if vision.is_empty() || vision.eq_ignore_ascii_case("off") {
        VISION_MODEL
    } else {
        vision
    };
    let has_vision = models.as_ref().is_some_and(|m| has_model(m, vision_model));
    let mut vision_item = SetupItem::new(
        "ollama_vision",
        Group::Ai,
        "Vision model",
        "Reads pictures without text. About 1.7 GB.",
    )
    .done(
        has_vision,
        "Downloaded",
        if models.is_some() {
            "Not downloaded"
        } else {
            "Needs Ollama running"
        },
    );
    if models.is_some() {
        vision_item = vision_item
            .run(format!("ollama pull {vision_model}"))
            .action("Download");
    }
    items.push(vision_item);

    let api_key = std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.trim().is_empty());
    items.push(
        SetupItem::new(
            "anthropic",
            Group::Ai,
            "Anthropic API key",
            "Pay as you go. Opens PowerShell with the command ready to paste.",
        )
        .done(api_key, "Set", "")
        .terminal_paste("setx ANTHROPIC_API_KEY \"your-key\""),
    );

    // Connections
    let hooks = hook_state(&read(&home.join(".claude").join("settings.json")));
    let mut hook_item = SetupItem::new(
        "claude_hooks",
        Group::Connect,
        "Claude Code hooks",
        "See when it finishes, allow or deny from the island.",
    )
    .copy(hook_snippet())
    .tab("ai");
    hook_item = match hooks {
        HookState::Complete => hook_item.done(true, "Added", ""),
        HookState::NoPermissions => hook_item.done(false, "", "Added, without permission requests"),
        HookState::Missing => hook_item.done(false, "", "Not added"),
    };
    if claude {
        hook_item = hook_item.recommended();
    }
    items.push(hook_item);

    let mcp_url = format!("http://127.0.0.1:{}/mcp", crate::mcp::PORT);
    let mut mcp = SetupItem::new(
        "claude_mcp",
        Group::Connect,
        "Sidekick tools in Claude Code",
        "Lets Claude Code use Sidekick's tools.",
    )
    .done(
        mcp_added(&read(&home.join(".claude.json")), &mcp_url),
        "Added",
        "Not added",
    )
    .tab("ai");
    let mcp_cmd = format!(
        "claude mcp add --scope user --transport http sidekick {mcp_url} --header \"Authorization: Bearer {}\"",
        state.mcp_token
    );
    mcp = if claude {
        mcp.run(mcp_cmd)
    } else {
        mcp.copy(mcp_cmd)
    };
    items.push(mcp);

    let (codex_notify, codex_mcp) = crate::codex_config::status();
    let mut notify_item = SetupItem::new(
        "codex_notify",
        Group::Connect,
        "Codex notifications",
        "See when a Codex turn is done.",
    )
    .done(codex_notify, "Added", "Not added")
    .tab("ai");
    if codex && !claude {
        notify_item = notify_item.recommended();
    }
    items.push(notify_item);
    items.push(
        SetupItem::new(
            "codex_mcp",
            Group::Connect,
            "Sidekick tools in Codex",
            "Lets Codex use Sidekick's tools.",
        )
        .done(codex_mcp, "Added", "Not added")
        .tab("ai"),
    );

    // Match Connections: only count authenticated check-ins, not pair_request
    // events stored before the user clicks Allow on the island. Done when any
    // browser is paired; the list under Set up shows the rest.
    let browsers = browsers(&state);
    let any_paired = browsers.iter().any(|b| b.connected);
    items.push(
        SetupItem::new(
            "browser",
            Group::Connect,
            "Browser extension",
            "Page help, tabs and sessions on the island.",
        )
        .done(any_paired, "Paired", "Not paired")
        .tab("connections")
        .recommended(),
    );

    items.push(
        SetupItem::new(
            "code_folders",
            Group::Connect,
            "Code folders",
            "Project status, the project launcher and the end-of-day check.",
        )
        .done(!settings.code_folders.is_empty(), "Set", "")
        .tab("home")
        .recommended(),
    );

    let composio_on = crate::composio::is_set_up(&settings.composio);
    items.push(
        SetupItem::new(
            "composio",
            Group::Connect,
            "Composio",
            "One sign-in for your calendar, mail, Slack and more.",
        )
        .done(composio_on, "Connected", "Not connected")
        .tab("connections")
        .recommended(),
    );
    items.push(
        SetupItem::new(
            "search_folders",
            Group::Connect,
            "Folders to search",
            "Search inside your documents and notes from Ask mode.",
        )
        .done(!settings.index_folders.is_empty(), "Set", "None yet")
        .tab("privacy"),
    );

    let voice = crate::voice::status(app);
    let voice_ready = voice.models.iter().all(|m| m.installed);
    items.push(
        SetupItem::new(
            "voice",
            Group::Connect,
            "Voice",
            "Say \"Hey Sidekick\" and hear answers. About 205 MB, all on this PC.",
        )
        .done(
            voice_ready && settings.voice.enabled,
            "On",
            if voice.downloading {
                "Downloading"
            } else if voice_ready {
                "Downloaded, off"
            } else {
                "Not downloaded"
            },
        )
        .tab("ai"),
    );

    // Tools
    let gh = found("gh");
    let mut gh_item = SetupItem::new(
        "gh",
        Group::Tools,
        "GitHub CLI",
        "Open PRs in the morning brief.",
    )
    .recommended();
    gh_item = if !gh {
        gh_item
            .done(false, "", "Not installed")
            .run(winget("GitHub.cli"))
    } else if gh_ok {
        gh_item.done(true, "Signed in", "")
    } else {
        gh_item
            .done(false, "", "Not signed in")
            .run("gh auth login")
            .action("Sign in")
    };
    items.push(gh_item);

    // Any VS Code style editor will do (Cursor, Windsurf...); only offer
    // VS Code when none is installed.
    let editor = SetupItem::new(
        "vscode",
        Group::Tools,
        "Code editor",
        "Opens your projects and files. VS Code, Cursor or Windsurf.",
    )
    .recommended();
    items.push(match caps.code_name.as_deref() {
        Some(name) => editor.done(true, name, ""),
        None => editor
            .done(false, "", "Not installed")
            .run(winget("Microsoft.VisualStudioCode")),
    });

    let tools: [(&str, &str, &str, bool, &str, bool); 8] = [
        (
            "git",
            "Git",
            "Repo status and unsaved work.",
            found("git"),
            "Git.Git",
            true,
        ),
        (
            "tesseract",
            "Tesseract",
            "Copy text out of screenshots.",
            caps.tesseract.is_some(),
            "UB-Mannheim.TesseractOCR",
            true,
        ),
        (
            "poppler",
            "Poppler",
            "Summarize PDFs.",
            caps.pdftotext.is_some(),
            "oschwartz10612.Poppler",
            false,
        ),
        (
            "pandoc",
            "Pandoc",
            "Summarize Word files and convert documents.",
            caps.pandoc.is_some(),
            "JohnMacFarlane.Pandoc",
            false,
        ),
        (
            "ffmpeg",
            "FFmpeg",
            "Convert videos and audio.",
            caps.ffmpeg.is_some(),
            "Gyan.FFmpeg",
            false,
        ),
        (
            "magick",
            "ImageMagick",
            "Convert and resize images.",
            caps.magick.is_some(),
            "ImageMagick.ImageMagick",
            false,
        ),
        (
            "docker",
            "Docker Desktop",
            "Start Docker when a project needs it.",
            found("docker"),
            "Docker.DockerDesktop",
            false,
        ),
        (
            "libreoffice",
            "LibreOffice",
            "Convert Office files to PDF.",
            caps.soffice.is_some(),
            "TheDocumentFoundation.LibreOffice",
            false,
        ),
    ];
    for (id, title, why, ok, package, recommended) in tools {
        let mut item = SetupItem::new(id, Group::Tools, title, why)
            .done(ok, "Installed", "Not installed")
            .run(winget(package));
        if recommended {
            item = item.recommended();
        }
        items.push(item);
    }

    items
}

/// One command that installs every missing recommended tool, or None when
/// they are all there.
pub fn install_all(items: &[SetupItem]) -> Option<String> {
    let cmds: Vec<&str> = items
        .iter()
        .filter(|i| i.group == Group::Tools && i.recommended && !i.done && i.runnable)
        .filter(|i| {
            i.command
                .as_deref()
                .is_some_and(|c| c.starts_with("winget "))
        })
        .filter_map(|i| i.command.as_deref())
        .collect();
    (!cmds.is_empty()).then(|| cmds.join("; "))
}

/// Opens a PowerShell window running the item's command, so the user sees
/// everything it does. Only commands built above can run.
pub async fn run(app: &AppHandle, id: &str) -> Result<(), String> {
    let items = status(app).await;
    let command = if id == "all" {
        install_all(&items).ok_or("Everything recommended is already installed")?
    } else {
        let item = items
            .iter()
            .find(|i| i.id == id)
            .ok_or("Unknown setup step")?;
        if item.done {
            return Err(format!("{} is already set up", item.title));
        }
        if !item.runnable {
            return Err("Copy this one instead".into());
        }
        if item.opens_app {
            return open_app(item.id);
        }
        if item.opens_terminal {
            let command = item.command.clone().ok_or("Nothing to paste")?;
            return open_terminal_paste(&command);
        }
        item.command.clone().ok_or("Nothing to run")?
    };
    open_terminal(&command)
}

/// Opens an installed app the way its Start menu shortcut does.
fn open_app(id: &str) -> Result<(), String> {
    match id {
        "ollama" => open_ollama(),
        _ => Err(format!("{id} has no app to open")),
    }
}

/// The Ollama app (`ollama app.exe`, next to `ollama.exe`) starts the server
/// and keeps it running from the tray.
#[cfg(windows)]
fn open_ollama() -> Result<(), String> {
    use std::process::Stdio;
    let exe = which::which("ollama").map_err(|e| format!("Ollama was not found on PATH: {e}"))?;
    let app = exe.with_file_name("ollama app.exe");
    if !app.is_file() {
        return Err(format!(
            "The Ollama app was not found next to {}. Open Ollama from the Start menu.",
            exe.display()
        ));
    }
    std::process::Command::new(&app)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open {}: {e}", app.display()))
}

#[cfg(not(windows))]
fn open_ollama() -> Result<(), String> {
    Err(
        "Opening Ollama from Sidekick is only available on Windows; open the Ollama app instead"
            .into(),
    )
}

/// After an installer runs, PowerShell still has the old PATH; read it again
/// so the next step (say, `ollama pull` after installing Ollama) is found.
const REFRESH_PATH: &str = "$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')";

/// Joins several steps into one script, in the given order.
pub fn chain(commands: &[String]) -> String {
    commands.join(&format!("; {REFRESH_PATH}; "))
}

/// Opens apps first, then runs the rest one after another in one
/// PowerShell window: installs first, then model downloads and sign-ins.
/// Returns what it did, for the welcome screen.
pub async fn run_many(app: &AppHandle, ids: &[String]) -> Result<Vec<String>, String> {
    let items = status(app).await;
    let chosen: Vec<&SetupItem> = items
        .iter()
        .filter(|i| ids.contains(&i.id.to_owned()) && i.runnable && !i.done)
        .collect();
    let (apps, rest): (Vec<&SetupItem>, Vec<&SetupItem>) =
        chosen.into_iter().partition(|i| i.opens_app);
    let (pastes, mut steps): (Vec<&SetupItem>, Vec<&SetupItem>) =
        rest.into_iter().partition(|i| i.opens_terminal);
    let mut done = Vec::new();
    for item in apps {
        open_app(item.id)?;
        done.push(format!("Opened {}", item.title));
    }
    for item in pastes {
        let command = item.command.as_deref().ok_or("Nothing to paste")?;
        open_terminal_paste(command)?;
        done.push(format!("Opened terminal for {}", item.title));
    }
    // Installers before anything that needs what they install.
    steps.sort_by_key(|i| {
        !i.command
            .as_deref()
            .is_some_and(|c| c.starts_with("winget "))
    });
    let commands: Vec<String> = steps.iter().filter_map(|i| i.command.clone()).collect();
    if !commands.is_empty() {
        open_terminal(&chain(&commands))?;
        done.push("Installing in PowerShell".to_owned());
    }
    Ok(done)
}

/// Copies `command` and opens PowerShell with paste instructions. Does not
/// run the command — the user must put their own secret in first.
fn open_terminal_paste(command: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(command.to_owned()))
        .map_err(|e| format!("could not copy the command: {e}"))?;
    open_terminal_window(&format!(
        "Write-Host 'Command copied to the clipboard:' -ForegroundColor Cyan; Write-Host '  {}' -ForegroundColor Yellow; Write-Host ''; Write-Host 'Replace your-key with your real key, paste (Ctrl+V), press Enter, then come back to Sidekick.' -ForegroundColor White",
        command.replace('\'', "''")
    ))
}

#[cfg(windows)]
fn open_terminal(command: &str) -> Result<(), String> {
    open_terminal_window(&format!(
        "Write-Host 'Sidekick setup: {}' -ForegroundColor Cyan; {command}; Write-Host ''; Write-Host 'Done. Close this window and press Check again in Sidekick.' -ForegroundColor Green",
        command.replace('\'', "''")
    ))
}

#[cfg(windows)]
fn open_terminal_window(script: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    // Sidekick is a GUI process with no console. If we spawn PowerShell with
    // inherited stdio, it gets invalid handles and tools like ollama print
    // "failed to get console mode for stderr". `cmd /c start` opens a real
    // console window; CREATE_NO_WINDOW only hides the brief cmd trampoline.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Don't inherit the app's cwd (e.g. src-tauri in dev); open in the
    // user profile so the prompt looks like a normal terminal.
    let home = dirs::home_dir().unwrap_or_else(|| Path::new("C:\\").to_path_buf());
    // `start "title" prog` — the quoted title is required so `start` does not
    // treat the first quoted arg as the window title and drop the program.
    std::process::Command::new("cmd.exe")
        .current_dir(&home)
        .args([
            "/c",
            "start",
            "Sidekick setup",
            "powershell.exe",
            "-NoExit",
            "-NoLogo",
            "-NoProfile",
            "-Command",
            script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open PowerShell: {e}"))
}

#[cfg(not(windows))]
fn open_terminal(_command: &str) -> Result<(), String> {
    Err("Running setup commands is only available on Windows; copy the command instead".into())
}

#[cfg(not(windows))]
fn open_terminal_window(_script: &str) -> Result<(), String> {
    Err("Opening a terminal is only available on Windows; copy the command instead".into())
}

/// Installers add to PATH in the registry, which this already running
/// process does not see. Reads it again so new tools are found without a
/// restart.
#[cfg(windows)]
pub fn refresh_path() {
    let Some(fresh) = user_and_machine_env("Path") else {
        return;
    };
    if fresh.len() < 3 {
        return;
    }
    let current = std::env::var("PATH").unwrap_or_default();
    let merged = merge_path(&fresh, &current);
    if merged != current {
        // SAFETY: on Windows, set_var calls SetEnvironmentVariableW, which
        // the OS serializes; no other thread holds a pointer into the block.
        unsafe { std::env::set_var("PATH", merged) };
    }
}

#[cfg(not(windows))]
pub fn refresh_path() {}

/// `setx` and Settings write user env vars the running process does not see.
/// Pull one name from the user registry into this process so setup / AI work
/// without a restart mid-onboarding.
#[cfg(windows)]
pub fn refresh_user_env(name: &str) {
    let Some(value) = user_env(name) else {
        return;
    };
    if value.trim().is_empty() {
        return;
    }
    let current = std::env::var(name).unwrap_or_default();
    if current == value {
        return;
    }
    // SAFETY: see refresh_path.
    unsafe { std::env::set_var(name, value) };
}

#[cfg(not(windows))]
pub fn refresh_user_env(_name: &str) {}

#[cfg(windows)]
fn user_env(name: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("powershell.exe");
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "[Environment]::GetEnvironmentVariable('{}','User')",
                name.replace('\'', "''")
            ),
        ])
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

#[cfg(windows)]
fn user_and_machine_env(name: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("powershell.exe");
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "[Environment]::GetEnvironmentVariable('{n}','Machine') + ';' + [Environment]::GetEnvironmentVariable('{n}','User')",
                n = name.replace('\'', "''")
            ),
        ])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
/// Registry entries first, then anything this process had that they lack.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn merge_path(fresh: &str, current: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    fresh
        .split(';')
        .chain(current.split(';'))
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .filter(|p| seen.insert(p.trim_end_matches(['\\', '/']).to_ascii_lowercase()))
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI paints `SetupCatalog.ts` before this module answers, so its
    /// text must match or rows change under the user when the data lands.
    #[test]
    fn ui_catalog_matches_the_items() {
        let catalog = include_str!("../../src/components/SetupCatalog.ts");
        let source = include_str!("setup.rs");
        let mut checked = 0;
        for line in catalog.lines().map(str::trim) {
            for key in ["id: ", "title: ", "why: "] {
                if let Some(literal) = line.strip_prefix(key).filter(|l| l.starts_with('"')) {
                    let literal = literal.trim_end_matches(',');
                    assert!(
                        source.contains(literal),
                        "SetupCatalog.ts has {key}{literal}, which setup.rs does not; keep them the same"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 30, "catalog not found or empty");
    }

    #[test]
    fn reads_claude_hooks() {
        assert_eq!(hook_state(""), HookState::Missing);
        assert_eq!(hook_state(r#"{"hooks":{}}"#), HookState::Missing);
        assert_eq!(hook_state(&hook_snippet()), HookState::Complete);
        let old = format!(
            r#"{{"model":"x","hooks":{{"Stop":[{{"hooks":[{{"type":"http","url":"{CLAUDE_HOOK_URL}"}}]}}]}}}}"#
        );
        assert_eq!(hook_state(&old), HookState::NoPermissions);
    }

    #[test]
    fn reads_mcp_servers() {
        let url = "http://127.0.0.1:47823/mcp";
        let added = format!(r#"{{"mcpServers":{{"sidekick":{{"type":"http","url":"{url}"}}}}}}"#);
        assert!(mcp_added(&added, url));
        assert!(!mcp_added(r#"{"mcpServers":{}}"#, url));
        assert!(!mcp_added("not json", url));
    }

    #[test]
    fn matches_ollama_models() {
        let models = vec![
            "nomic-embed-text:latest".to_owned(),
            "qwen3:4b".to_owned(),
            "moondream:latest".to_owned(),
        ];
        assert!(has_model(&models, "nomic-embed-text"));
        assert!(has_model(&models, "qwen3:4b"));
        assert!(!has_model(&models, "qwen3:8b"));
        assert!(!has_model(&models, "llama3.2"));
        assert_eq!(
            resolve_model_tag(&models, "moondream").as_deref(),
            Some("moondream:latest")
        );
        assert!(sidekick_ai::is_embedding_model("nomic-embed-text:latest"));
        assert!(!sidekick_ai::is_embedding_model("qwen3:4b"));
    }

    #[test]
    fn merges_path_without_duplicates() {
        assert_eq!(
            merge_path(r"C:\Windows;C:\Tools\", r"c:\windows;C:\Tools;D:\Own"),
            r"C:\Windows;C:\Tools\;D:\Own"
        );
    }

    #[test]
    fn chains_steps_with_a_path_refresh() {
        let script = chain(&[
            "winget install -e --id Ollama.Ollama".into(),
            "ollama pull qwen3:1.7b".into(),
        ]);
        assert!(script.starts_with("winget install"));
        assert!(script.ends_with("ollama pull qwen3:1.7b"));
        assert!(script.contains("GetEnvironmentVariable('Path','User')"));
    }

    #[test]
    fn welcome_bulk_skips_ollama_and_gh() {
        let ollama = SetupItem::new("ollama", Group::Ai, "Ollama", "w")
            .run("winget install Ollama")
            .recommended();
        let gh = SetupItem::new("gh", Group::Tools, "GitHub CLI", "w")
            .run("gh auth login")
            .recommended();
        let git = SetupItem::new("git", Group::Tools, "Git", "w")
            .run("winget install Git")
            .recommended();
        assert!(!welcome_bulk_install(&ollama));
        assert!(!welcome_bulk_install(&gh));
        assert!(welcome_bulk_install(&git));
    }

    #[test]
    fn running_ollama_opens_the_app_not_a_terminal() {
        let item = SetupItem::new("ollama", Group::Ai, "Ollama", "w").opens_app();
        assert!(item.runnable && item.opens_app);
        assert_eq!(item.command, None);
        assert_eq!(item.action, "Run");
        assert_eq!(install_all(&[item]), None);
    }

    #[test]
    fn api_key_opens_a_terminal_for_paste() {
        let item = SetupItem::new("anthropic", Group::Ai, "Anthropic API key", "w")
            .terminal_paste("setx ANTHROPIC_API_KEY \"your-key\"");
        assert!(item.runnable && item.opens_terminal && !item.opens_app);
        assert_eq!(item.action, "Open terminal");
        assert_eq!(
            item.command.as_deref(),
            Some("setx ANTHROPIC_API_KEY \"your-key\"")
        );
    }

    #[test]
    fn install_all_joins_missing_recommended_tools() {
        let tool = |id, done, recommended| {
            let mut i = SetupItem::new(id, Group::Tools, "t", "w")
                .done(done, "", "")
                .run(winget(id));
            i.recommended = recommended;
            i
        };
        let items = vec![
            tool("A", false, true),
            tool("B", true, true),
            tool("C", false, false),
        ];
        assert_eq!(
            install_all(&items).as_deref(),
            Some("winget install -e --id A --accept-source-agreements")
        );
        assert_eq!(install_all(&[tool("B", true, true)]), None);
    }
}
