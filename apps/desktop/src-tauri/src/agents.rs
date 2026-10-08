//! Coding agents: Claude Code and OpenAI's Codex CLI. Either one (or both)
//! can finish what the local model cannot, get Sidekick's tools, and tell
//! the island when a turn is done. Settings > AI picks which one gets the
//! handoff; "auto" prefers whichever of Claude Code / Codex appears higher
//! in the AI provider list and is installed.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use sidekick_core::Settings;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    ClaudeCode,
    Codex,
    /// GitHub Copilot CLI (`copilot`).
    Copilot,
    /// Cursor's agent CLI (`cursor-agent`).
    Cursor,
    /// A coding model on this PC through Ollama, run by Sidekick itself.
    Local,
}

impl Agent {
    pub fn name(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "Claude Code",
            Agent::Codex => "Codex",
            Agent::Copilot => "GitHub Copilot",
            Agent::Cursor => "Cursor",
            Agent::Local => "Local",
        }
    }

    /// The id used in settings and saved sessions.
    pub fn id(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude_code",
            Agent::Codex => "codex",
            Agent::Copilot => "copilot",
            Agent::Cursor => "cursor",
            Agent::Local => "local",
        }
    }

    pub fn from_id(id: &str) -> Option<Agent> {
        [
            Agent::ClaudeCode,
            Agent::Codex,
            Agent::Copilot,
            Agent::Cursor,
            Agent::Local,
        ]
        .into_iter()
        .find(|a| a.id() == id)
    }

    fn command(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude",
            Agent::Codex => "codex",
            Agent::Copilot => "copilot",
            Agent::Cursor => "cursor-agent",
            Agent::Local => "ollama",
        }
    }

    fn set_path(self, s: &Settings) -> String {
        match self {
            Agent::ClaudeCode => s.ai.claude_code.path.trim().to_owned(),
            Agent::Codex => s.ai.codex.path.trim().to_owned(),
            Agent::Copilot | Agent::Cursor | Agent::Local => String::new(),
        }
    }

    /// The program, from Settings or PATH.
    pub fn resolve(self, s: &Settings) -> Option<PathBuf> {
        Some(PathBuf::from(self.set_path(s)))
            .filter(|p| p.is_file())
            .or_else(|| which::which(self.command()).ok())
    }
}

/// The agent that gets handoffs, given what is installed and (for auto) order.
pub fn pick(choice: &str, claude: bool, codex: bool, order: &[String]) -> Option<Agent> {
    match choice {
        "codex" if codex => Some(Agent::Codex),
        "claude_code" if claude => Some(Agent::ClaudeCode),
        // Preferred missing: use the other rather than fail.
        "codex" if claude => Some(Agent::ClaudeCode),
        "claude_code" if codex => Some(Agent::Codex),
        _ => {
            for id in order {
                match id.as_str() {
                    "codex" if codex => return Some(Agent::Codex),
                    "claude_code" if claude => return Some(Agent::ClaudeCode),
                    _ => {}
                }
            }
            if claude {
                Some(Agent::ClaudeCode)
            } else if codex {
                Some(Agent::Codex)
            } else {
                None
            }
        }
    }
}

pub fn chosen(s: &Settings) -> Option<Agent> {
    // Copilot and Cursor when picked by name; also when nothing else is there.
    let other = |a: Agent| a.resolve(s).is_some().then_some(a);
    if let Some(a) =
        Agent::from_id(&s.ai.coding_agent).filter(|a| matches!(a, Agent::Copilot | Agent::Cursor))
        && let Some(a) = other(a)
    {
        return Some(a);
    }
    let codex = Agent::Codex.resolve(s).is_some();
    pick(
        &s.ai.coding_agent,
        usable(Agent::ClaudeCode.resolve(s).is_some(), codex),
        codex,
        &s.ai.order,
    )
    .or_else(|| other(Agent::Copilot))
    .or_else(|| other(Agent::Cursor))
}

/// What the UI needs: which agents are installed and which gets handoffs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agents {
    pub claude_code: bool,
    pub codex: bool,
    pub copilot: bool,
    pub cursor: bool,
    /// Display name of the agent that gets handoffs, if any.
    pub handoff: Option<String>,
    /// Every agent with whether it is ready, for the picker.
    pub list: Vec<AgentInfo>,
}

/// One agent in the picker: where it runs, whether it is ready, and the
/// one step that makes it ready when it is not.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub installed: bool,
    /// None when we cannot tell from here.
    pub signed_in: Option<bool>,
    /// Out of plan usage for now.
    pub limited: bool,
    /// The one step to fix it, when it is not ready.
    pub fix: Option<String>,
    /// Runs on this PC; nothing leaves it.
    pub local: bool,
}

impl Agent {
    fn install_hint(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "npm install -g @anthropic-ai/claude-code",
            Agent::Codex => "npm install -g @openai/codex",
            Agent::Copilot => "npm install -g @github/copilot",
            Agent::Cursor => "Install the Cursor CLI from cursor.com/cli",
            Agent::Local => "Install Ollama from ollama.com",
        }
    }

    fn login_hint(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude /login",
            Agent::Codex => "codex login",
            Agent::Copilot => "copilot, then /login",
            Agent::Cursor => "cursor-agent login",
            Agent::Local => "",
        }
    }

    /// Whether the CLI has a saved sign-in or a key, by its own files.
    fn signed_in(self) -> Option<bool> {
        let env = |k: &str| std::env::var(k).is_ok_and(|v| !v.trim().is_empty());
        let home = dirs::home_dir()?;
        let file = |p: PathBuf| p.is_file();
        Some(match self {
            Agent::ClaudeCode => {
                env("ANTHROPIC_API_KEY")
                    || file(home.join(".claude").join(".credentials.json"))
                    || std::fs::read_to_string(home.join(".claude.json"))
                        .is_ok_and(|t| t.contains("\"oauthAccount\""))
            }
            Agent::Codex => env("OPENAI_API_KEY") || file(home.join(".codex").join("auth.json")),
            Agent::Copilot => {
                if env("GH_TOKEN") || env("GITHUB_TOKEN") || env("COPILOT_GITHUB_TOKEN") {
                    true
                } else {
                    // Copilot keeps its token in the system keychain.
                    return None;
                }
            }
            // Nothing to sign in to.
            Agent::Local => true,
            Agent::Cursor => {
                if env("CURSOR_API_KEY") {
                    true
                } else {
                    return None;
                }
            }
        })
    }

    fn info(self, s: &Settings) -> AgentInfo {
        let installed = self.resolve(s).is_some();
        let signed_in = if installed { self.signed_in() } else { None };
        let limited = self == Agent::ClaudeCode && sidekick_ai::claude_code_limited();
        let fix = if !installed {
            Some(self.install_hint().to_owned())
        } else if signed_in == Some(false) {
            Some(self.login_hint().to_owned())
        } else {
            None
        };
        AgentInfo {
            id: self.id(),
            name: self.name(),
            installed,
            signed_in,
            limited,
            fix,
            local: self == Agent::Local,
        }
    }
}

pub fn status(s: &Settings) -> Agents {
    let claude = Agent::ClaudeCode.resolve(s).is_some();
    let codex = Agent::Codex.resolve(s).is_some();
    Agents {
        claude_code: claude,
        codex,
        copilot: Agent::Copilot.resolve(s).is_some(),
        cursor: Agent::Cursor.resolve(s).is_some(),
        handoff: chosen(s).map(|a| a.name().to_owned()),
        list: [
            Agent::ClaudeCode,
            Agent::Codex,
            Agent::Copilot,
            Agent::Cursor,
            Agent::Local,
        ]
        .into_iter()
        .map(|a| a.info(s))
        .collect(),
    }
}

/// Claude Code out of usage counts as missing while Codex can take over.
fn usable(claude: bool, codex: bool) -> bool {
    claude && !(codex && sidekick_ai::claude_code_limited())
}

pub const HANDOFF_PROMPT: &str =
    "Read conversation.md in this folder. It is a conversation from Sidekick; continue it.";

/// Whether Codex already has a Composio server in its own settings.
fn codex_has_composio() -> bool {
    dirs::home_dir()
        .and_then(|h| std::fs::read_to_string(h.join(".codex").join("config.toml")).ok())
        .is_some_and(|t| t.to_lowercase().contains("composio"))
}

/// Codex settings overrides that add Composio for one run. Header values
/// go in environment variables, so no key is on a command line.
pub fn codex_composio_args(
    url: &str,
    headers: &[(String, String)],
) -> (Vec<String>, Vec<(String, String)>) {
    let mut args = vec![
        "-c".to_owned(),
        format!("mcp_servers.composio.url=\"{}\"", url.replace('"', "")),
    ];
    let mut env = Vec::new();
    let mut map = Vec::new();
    for (i, (k, v)) in headers.iter().enumerate() {
        let var = format!("SIDEKICK_COMPOSIO_HEADER_{i}");
        let key: String = k
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        map.push(format!("\"{key}\" = \"{var}\""));
        env.push((var, v.clone()));
    }
    if !map.is_empty() {
        args.push("-c".into());
        args.push(format!(
            "mcp_servers.composio.env_http_headers={{ {} }}",
            map.join(", ")
        ));
    }
    (args, env)
}

/// Saves the conversation for an agent to pick up; returns its folder.
pub fn write_handoff(
    app: &AppHandle,
    messages: &[sidekick_ai::Message],
    reason: Option<&str>,
) -> Result<PathBuf, String> {
    let dir = app.state::<AppState>().ai_workdir.join("handoff");
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not make the handoff folder: {e}"))?;
    let context = crate::ai::context(app);
    std::fs::write(
        dir.join("conversation.md"),
        crate::composio::handoff_markdown(messages, reason, &context),
    )
    .map_err(|e| format!("could not save the conversation: {e}"))?;
    Ok(dir)
}

/// Opens the chosen agent in a new terminal with the conversation so far,
/// in a folder holding only that conversation (and Composio when the agent
/// does not have it yet). The folder stays the same, so the agent asks to
/// trust it only once. Returns the agent's name.
pub async fn hand_off(
    app: &AppHandle,
    messages: &[sidekick_ai::Message],
    reason: Option<&str>,
) -> Result<String, String> {
    let state = app.state::<AppState>();
    let settings = lock(&state.settings).clone();
    let agent = chosen(&settings)
        .ok_or("Install Claude Code, Codex, GitHub Copilot CLI or Cursor first (Settings > AI), then try again.")?;
    let exe = agent
        .resolve(&settings)
        .ok_or_else(|| format!("{} is not installed", agent.name()))?;
    let dir = write_handoff(app, messages, reason)?;
    // Do not leave an old copy of a key behind.
    let _ = std::fs::remove_file(dir.join("composio-mcp.json"));

    let composio = crate::composio::server(&settings.composio).await;
    let mut args: Vec<String> = Vec::new();
    let mut env: Vec<(String, String)> = Vec::new();
    match agent {
        Agent::ClaudeCode => {
            let claude_json = dirs::home_dir()
                .map(|h| std::fs::read_to_string(h.join(".claude.json")).unwrap_or_default())
                .unwrap_or_default();
            if let Some((url, headers)) = composio
                && crate::composio::from_claude_config(&claude_json).is_none()
            {
                let headers: serde_json::Map<String, Value> = headers
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect();
                let cfg = serde_json::json!({ "mcpServers": { "composio": {
                    "type": "http", "url": url, "headers": headers,
                }}});
                let path = dir.join("composio-mcp.json");
                std::fs::write(&path, cfg.to_string()).map_err(|e| e.to_string())?;
                args.push("--mcp-config".into());
                args.push(path.to_string_lossy().into_owned());
            }
        }
        Agent::Codex => {
            if let Some((url, headers)) = composio
                && !codex_has_composio()
            {
                let (a, e) = codex_composio_args(&url, &headers);
                args.extend(a);
                env.extend(e);
            }
        }
        // Both start interactively with the first prompt already sent.
        Agent::Copilot => args.push("-i".into()),
        Agent::Cursor => {}
        // Never picked for handoffs: it cannot finish what the local model could not.
        Agent::Local => return Err("The local agent runs inside Sidekick.".into()),
    }
    args.push(HANDOFF_PROMPT.into());
    launch(&dir, &exe, &args, &env)?;
    Ok(agent.name().to_owned())
}

/// Opens `exe args` in a new terminal in `dir`.
pub fn open_terminal(dir: &Path, exe: &Path, args: &[String]) -> Result<(), String> {
    launch(dir, exe, args, &[])
}

/// Keeps a console window from flashing up for a background process.
pub fn hide_console(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[cfg(windows)]
fn launch(dir: &Path, exe: &Path, args: &[String], env: &[(String, String)]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    if !exe.is_file() {
        return Err(format!(
            "could not find {}. Install the coding agent or set its path in Settings > AI.",
            exe.display()
        ));
    }
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let exe_q = q(&exe.to_string_lossy());
    let mut cmd = format!(
        "Set-Location -LiteralPath {}; $exe = {}; if (-not (Test-Path -LiteralPath $exe)) {{ Write-Host \"Could not find $exe. Install the coding agent or set its path in Settings > AI.\"; exit 1 }}; & $exe",
        q(&dir.to_string_lossy()),
        exe_q
    );
    for a in args {
        cmd.push(' ');
        cmd.push_str(&q(a));
    }
    let mut p = std::process::Command::new("powershell.exe");
    p.args(["-NoExit", "-NoLogo", "-NoProfile", "-Command", &cmd])
        .creation_flags(CREATE_NEW_CONSOLE);
    for (k, v) in env {
        p.env(k, v);
    }
    p.spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open a terminal: {e}"))
}

#[cfg(not(windows))]
fn launch(
    dir: &Path,
    exe: &Path,
    _args: &[String],
    _env: &[(String, String)],
) -> Result<(), String> {
    Err(format!(
        "Opening {} is only available on Windows. The conversation is in {}",
        exe.display(),
        dir.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_agent_that_is_there() {
        let claude_first = ["claude_code".into(), "codex".into()];
        let codex_first = ["local".into(), "codex".into(), "claude_code".into()];
        assert_eq!(
            pick("auto", true, true, &claude_first),
            Some(Agent::ClaudeCode)
        );
        assert_eq!(pick("auto", true, true, &codex_first), Some(Agent::Codex));
        assert_eq!(pick("auto", false, true, &claude_first), Some(Agent::Codex));
        assert_eq!(pick("codex", true, true, &claude_first), Some(Agent::Codex));
        // The chosen one is missing: use the other rather than fail.
        assert_eq!(
            pick("codex", true, false, &codex_first),
            Some(Agent::ClaudeCode)
        );
        assert_eq!(
            pick("claude_code", false, true, &claude_first),
            Some(Agent::Codex)
        );
        assert_eq!(pick("auto", false, false, &codex_first), None);
        // Neither coding agent in order: Claude then Codex.
        assert_eq!(
            pick("auto", true, true, &["local".into(), "anthropic".into()]),
            Some(Agent::ClaudeCode)
        );
    }

    #[test]
    fn codex_gets_composio_without_keys_on_the_command_line() {
        let (args, env) = codex_composio_args(
            "https://connect.composio.dev/mcp",
            &[("authorization".into(), "Bearer secret".into())],
        );
        let joined = args.join(" ");
        assert!(joined.contains("mcp_servers.composio.url=\"https://connect.composio.dev/mcp\""));
        assert!(joined.contains("\"authorization\" = \"SIDEKICK_COMPOSIO_HEADER_0\""));
        assert!(!joined.contains("secret"));
        assert_eq!(
            env,
            vec![(
                "SIDEKICK_COMPOSIO_HEADER_0".to_owned(),
                "Bearer secret".to_owned()
            )]
        );
    }
}
