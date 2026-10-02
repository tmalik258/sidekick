//! Coding agents: Claude Code and OpenAI's Codex CLI. Either one (or both)
//! can finish what the local model cannot, get Sidekick's tools, and tell
//! the island when a turn is done. Settings > AI picks which one gets the
//! handoff; "auto" takes Claude Code when it is installed, else Codex.

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
}

impl Agent {
    pub fn name(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "Claude Code",
            Agent::Codex => "Codex",
        }
    }

    fn command(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude",
            Agent::Codex => "codex",
        }
    }

    fn set_path(self, s: &Settings) -> String {
        match self {
            Agent::ClaudeCode => s.ai.claude_code.path.trim().to_owned(),
            Agent::Codex => s.ai.codex.path.trim().to_owned(),
        }
    }

    /// The program, from Settings or PATH.
    pub fn resolve(self, s: &Settings) -> Option<PathBuf> {
        Some(PathBuf::from(self.set_path(s)))
            .filter(|p| p.is_file())
            .or_else(|| which::which(self.command()).ok())
    }
}

/// The agent that gets handoffs, given what is installed.
pub fn pick(choice: &str, claude: bool, codex: bool) -> Option<Agent> {
    match (choice, claude, codex) {
        ("codex", _, true) => Some(Agent::Codex),
        ("claude_code", true, _) => Some(Agent::ClaudeCode),
        (_, true, _) => Some(Agent::ClaudeCode),
        (_, _, true) => Some(Agent::Codex),
        _ => None,
    }
}

pub fn chosen(s: &Settings) -> Option<Agent> {
    pick(
        &s.ai.coding_agent,
        Agent::ClaudeCode.resolve(s).is_some(),
        Agent::Codex.resolve(s).is_some(),
    )
}

/// What the UI needs: which agents are installed and which gets handoffs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agents {
    pub claude_code: bool,
    pub codex: bool,
    /// Display name of the agent that gets handoffs, if any.
    pub handoff: Option<String>,
}

pub fn status(s: &Settings) -> Agents {
    let claude = Agent::ClaudeCode.resolve(s).is_some();
    let codex = Agent::Codex.resolve(s).is_some();
    Agents {
        claude_code: claude,
        codex,
        handoff: pick(&s.ai.coding_agent, claude, codex).map(|a| a.name().to_owned()),
    }
}

const HANDOFF_PROMPT: &str =
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
        .ok_or("Install Claude Code or Codex first (Settings > Home > Setup), then try again.")?;
    let exe = agent
        .resolve(&settings)
        .ok_or_else(|| format!("{} is not installed", agent.name()))?;
    let dir = state.ai_workdir.join("handoff");
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not make the handoff folder: {e}"))?;
    let context = crate::ai::context(app);
    std::fs::write(
        dir.join("conversation.md"),
        crate::composio::handoff_markdown(messages, reason, &context),
    )
    .map_err(|e| format!("could not save the conversation: {e}"))?;
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
    }
    args.push(HANDOFF_PROMPT.into());
    launch(&dir, &exe, &args, &env)?;
    Ok(agent.name().to_owned())
}

#[cfg(windows)]
fn launch(dir: &Path, exe: &Path, args: &[String], env: &[(String, String)]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let mut cmd = format!(
        "Set-Location -LiteralPath {}; & {}",
        q(&dir.to_string_lossy()),
        q(&exe.to_string_lossy())
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
        assert_eq!(pick("auto", true, true), Some(Agent::ClaudeCode));
        assert_eq!(pick("auto", false, true), Some(Agent::Codex));
        assert_eq!(pick("codex", true, true), Some(Agent::Codex));
        // The chosen one is missing: use the other rather than fail.
        assert_eq!(pick("codex", true, false), Some(Agent::ClaudeCode));
        assert_eq!(pick("claude_code", false, true), Some(Agent::Codex));
        assert_eq!(pick("auto", false, false), None);
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
