//! Apps connected directly over MCP. A program on this PC is arbitrary code,
//! so nothing runs until the user confirmed the exact command; secrets go to
//! the keychain; writes are off for every tool until allowed one by one.

use std::collections::BTreeMap;

use duskie_ai::mcp_stdio::{self, Spec, StdioMcp};
use duskie_core::DirectServer;
use serde::Serialize;

use super::*;

const MAX_SERVERS: usize = 12;

fn secret_name(id: &str, var: &str) -> String {
    format!("mcp.{id}.{var}")
}

/// The program, arguments and variables to start a server with. Values come
/// from the keychain through `lookup`.
fn spec_of(s: &DirectServer, lookup: impl Fn(&str) -> Option<String>) -> Spec {
    Spec {
        command: s.command.clone(),
        args: s.args.clone(),
        env: s
            .env_names
            .iter()
            .filter_map(|n| lookup(&secret_name(&s.id, n)).map(|v| (n.clone(), v)))
            .collect(),
    }
}

/// What the user must be shown and confirm: the program and each argument.
fn command_line(s: &DirectServer) -> String {
    std::iter::once(s.command.as_str())
        .chain(s.args.iter().map(String::as_str))
        .map(|p| {
            if p.contains(' ') {
                format!("\"{p}\"")
            } else {
                p.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn check(s: &DirectServer) -> Result<(), String> {
    if s.app.trim().is_empty() {
        return Err("Name the app this server is for.".into());
    }
    match s.kind.as_str() {
        "stdio" => mcp_stdio::validate(&Spec {
            command: s.command.clone(),
            args: s.args.clone(),
            env: s
                .env_names
                .iter()
                .map(|n| (n.clone(), String::new()))
                .collect(),
        }),
        "url" if s.url.starts_with("https://") => Ok(()),
        "url" => Err("A server address must start with https://".into()),
        _ => Err("Choose a program or an address.".into()),
    }
}

fn edit(app: &AppHandle, f: impl FnOnce(&mut Vec<DirectServer>)) -> CmdResult<()> {
    let mut next = lock(&app.state::<AppState>().settings).clone();
    f(&mut next.direct_servers);
    apply_settings(app, next).map(|_| ())
}

/// Adds or replaces a server. It always starts unconfirmed; `env` values go
/// to the keychain and only their names are kept in settings.
#[tauri::command]
pub fn servers_save(
    app: AppHandle,
    mut server: DirectServer,
    env: BTreeMap<String, String>,
) -> CmdResult<String> {
    server.app = server.app.trim().to_lowercase();
    check(&server)?;
    if server.id.is_empty() {
        server.id = ulid::Ulid::new().to_string().to_lowercase();
    }
    server.env_names = env.keys().cloned().collect();
    server.confirmed = false;
    server.writes_allowed.clear();
    for (name, value) in &env {
        crate::secrets::set(&secret_name(&server.id, name), value)?;
    }
    let id = server.id.clone();
    let count = lock(&app.state::<AppState>().settings).direct_servers.len();
    edit(&app, |all| {
        all.retain(|s| s.id != id);
        if count < MAX_SERVERS {
            all.push(server);
        }
    })?;
    Ok(id)
}

/// The exact command to show before confirming.
#[tauri::command]
pub fn servers_command(app: AppHandle, id: String) -> CmdResult<String> {
    let all = lock(&app.state::<AppState>().settings)
        .direct_servers
        .clone();
    let s = all
        .iter()
        .find(|s| s.id == id)
        .ok_or("That server is gone.")?;
    Ok(if s.kind == "stdio" {
        command_line(s)
    } else {
        s.url.clone()
    })
}

/// The user confirmed running exactly what `shown` says. If the saved
/// command changed since it was shown, nothing is confirmed.
#[tauri::command]
pub fn servers_confirm(app: AppHandle, id: String, shown: String) -> CmdResult<()> {
    let now = servers_command(app.clone(), id.clone())?;
    if now != shown {
        return Err("The command changed. Look at it again before confirming.".into());
    }
    edit(&app, |all| {
        if let Some(s) = all.iter_mut().find(|s| s.id == id) {
            s.confirmed = true;
        }
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRow {
    pub name: String,
    pub description: String,
    pub writes: bool,
    pub allowed: bool,
}

/// Connects and lists the tools, so the user can see what it offers. Refuses
/// a program that was not confirmed.
#[tauri::command]
pub async fn servers_test(app: AppHandle, id: String) -> CmdResult<Vec<ToolRow>> {
    let all = lock(&app.state::<AppState>().settings)
        .direct_servers
        .clone();
    let s = all
        .into_iter()
        .find(|s| s.id == id)
        .ok_or("That server is gone.")?;
    if !s.confirmed {
        return Err("Confirm the command first.".into());
    }
    let tools = if s.kind == "stdio" {
        let spec = spec_of(&s, crate::secrets::get);
        let mut client = StdioMcp::spawn(&spec).await.map_err(|e| e.to_string())?;
        client.list_tools().await.map_err(|e| e.to_string())?
        // Dropping the client ends the program.
    } else {
        let headers = s
            .env_names
            .iter()
            .filter_map(|n| crate::secrets::get(&secret_name(&s.id, n)).map(|v| (n.clone(), v)))
            .collect();
        duskie_ai::McpClient::connect(&s.url, headers)
            .await
            .map_err(|e| e.to_string())?
            .list_tools()
            .await
            .map_err(|e| e.to_string())?
    };
    Ok(tools
        .into_iter()
        .map(|t| ToolRow {
            writes: duskie_core::connectors::is_write(&t.name),
            allowed: s.writes_allowed.contains(&t.name),
            name: t.name,
            description: t.description,
        })
        .collect())
}

/// Lets one tool change things, or takes that back.
#[tauri::command]
pub fn servers_allow_write(app: AppHandle, id: String, tool: String, allow: bool) -> CmdResult<()> {
    edit(&app, |all| {
        if let Some(s) = all.iter_mut().find(|s| s.id == id) {
            s.writes_allowed.retain(|t| t != &tool);
            if allow {
                s.writes_allowed.push(tool);
            }
        }
    })
}

/// Removes a server and its saved variables.
#[tauri::command]
pub fn servers_remove(app: AppHandle, id: String) -> CmdResult<()> {
    let all = lock(&app.state::<AppState>().settings)
        .direct_servers
        .clone();
    if let Some(s) = all.iter().find(|s| s.id == id) {
        for n in &s.env_names {
            crate::secrets::delete(&secret_name(&s.id, n));
        }
    }
    edit(&app, |all| all.retain(|s| s.id != id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> DirectServer {
        DirectServer {
            id: "x1".into(),
            app: "github".into(),
            kind: "stdio".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@scope/server".into(), "my folder".into()],
            env_names: vec!["TOKEN".into()],
            ..Default::default()
        }
    }

    #[test]
    fn the_confirm_text_is_the_exact_command() {
        assert_eq!(
            command_line(&server()),
            "npx -y @scope/server \"my folder\""
        );
    }

    #[test]
    fn the_spec_takes_variable_values_from_the_keychain_only() {
        let spec = spec_of(&server(), |k| {
            (k == "mcp.x1.TOKEN").then(|| "secret".to_owned())
        });
        assert_eq!(spec.env.get("TOKEN").map(String::as_str), Some("secret"));
        assert_eq!(spec.args.len(), 3);
        let none = spec_of(&server(), |_| None);
        assert!(none.env.is_empty());
    }

    #[test]
    fn shell_lines_and_plain_http_are_refused() {
        let mut s = server();
        assert!(check(&s).is_ok());
        s.command = "npx -y server".into();
        assert!(check(&s).is_err());
        let url = DirectServer {
            app: "x".into(),
            kind: "url".into(),
            url: "http://x".into(),
            ..Default::default()
        };
        assert!(check(&url).is_err());
        let ok = DirectServer {
            url: "https://x.example/mcp".into(),
            ..url
        };
        assert!(check(&ok).is_ok());
    }
}
