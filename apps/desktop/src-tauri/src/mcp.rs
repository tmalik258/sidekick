//! Duskie as an MCP server for Claude Code, over
//! Streamable HTTP on 127.0.0.1. Claude Code can search your history,
//! show a note on the island, open a link, and read today's time.
//!
//! Every request needs the bearer token from Settings > AI, and requests a
//! web page makes (they carry its http(s) origin) are refused. Every tool
//! call is logged.

use duskie_sensors::http;
use duskie_skills::{Proposal, ProposedOption, Trust};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tokio::net::{TcpListener, TcpStream};

use crate::search::SHAREABLE;
use crate::state::{AppState, executor, lock};

pub const PORT: u16 = 47823;
const PATH: &str = "/mcp";
const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Config Ask-mode chats pass to Claude Code, so it can use Duskie's
/// tools too. Lives in the AI workdir, next to nothing else.
pub const CONFIG_FILE: &str = "duskie-mcp.json";

fn write_client_config(app: &AppHandle, token: &str) {
    let dir = app.state::<AppState>().ai_workdir.clone();
    let config = serde_json::json!({
        "mcpServers": {
            "duskie": {
                "type": "http",
                "url": format!("http://127.0.0.1:{PORT}/mcp"),
                "headers": { "Authorization": format!("Bearer {token}") }
            }
        }
    });
    let result = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(dir.join(CONFIG_FILE), config.to_string()));
    if let Err(err) = result {
        log::warn!("could not write the MCP config for chats: {err}");
    }
}

pub fn start(app: &AppHandle, token: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let listener = match TcpListener::bind(("127.0.0.1", PORT)).await {
            Ok(l) => l,
            Err(err) => {
                log::warn!("MCP server port {PORT} unavailable: {err}");
                let _ = std::fs::remove_file(app.state::<AppState>().ai_workdir.join(CONFIG_FILE));
                return;
            }
        };
        write_client_config(&app, &token);
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                continue;
            };
            let (app, token) = (app.clone(), token.clone());
            tauri::async_runtime::spawn(async move { serve(&app, sock, &token).await });
        }
    });
}

fn token_ok(header: Option<&str>, token: &str) -> bool {
    let given = header
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or_default();
    !token.is_empty()
        && given.len() == token.len()
        && given
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

async fn serve(app: &AppHandle, mut sock: TcpStream, token: &str) {
    let Ok(Some(req)) =
        tokio::time::timeout(http::READ_TIMEOUT, http::read_request(&mut sock)).await
    else {
        return;
    };
    if req.origin().is_some_and(|o| o.starts_with("http")) {
        http::respond(&mut sock, "403 Forbidden", &[], None).await;
        return;
    }
    if !token_ok(req.header("authorization"), token) {
        http::respond(&mut sock, "401 Unauthorized", &[], None).await;
        return;
    }
    if req.path != PATH {
        http::respond(&mut sock, "404 Not Found", &[], None).await;
        return;
    }
    if req.method != "POST" {
        // No server-initiated stream; everything is request and response.
        http::respond(
            &mut sock,
            "405 Method Not Allowed",
            &[("allow", "POST")],
            None,
        )
        .await;
        return;
    }
    let Ok(msg) = serde_json::from_slice::<Value>(&req.body) else {
        let err = rpc_error(Value::Null, -32700, "parse error");
        http::respond(&mut sock, "400 Bad Request", &[], Some(&err)).await;
        return;
    };
    match handle(app, &msg).await {
        Some(resp) => http::respond(&mut sock, "200 OK", &[], Some(&resp)).await,
        // A notification: nothing to answer.
        None => http::respond(&mut sock, "202 Accepted", &[], None).await,
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn text(t: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": t.into() }] })
}

fn tool_error(t: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": t.into() }], "isError": true })
}

/// Ask mode's tools, offered to every model the same way: the local model
/// calls them directly, Claude Code and Codex through this server with a
/// `duskie_` prefix. Search and today keep their own versions below.
const SHARED: &[&str] = &[
    "find_files",
    "web_search",
    "read_page",
    "browser",
    "app_action",
    "desktop",
    "apps",
    "office",
    "recipes",
    "remember",
    "notifications",
    "pc_status",
    "storage",
    "pc_control",
    "windows",
    "open",
    "show_in_folder",
    "recent",
    "screen_text",
    "propose",
];

pub fn tools() -> Value {
    let mut list = own_tools();
    if let Some(arr) = list.as_array_mut() {
        for d in crate::ask_tools::defs()
            .into_iter()
            .filter(|d| SHARED.contains(&d.name.as_str()))
        {
            arr.push(json!({
                "name": format!("duskie_{}", d.name),
                "description": d.description,
                "inputSchema": d.parameters,
            }));
        }
    }
    list
}

fn own_tools() -> Value {
    json!([
        {
            "name": "duskie_search",
            "description": "Search the user's local history on this PC: text files in folders they chose, downloads, screenshots, actions they took, past Ask answers, and Claude Code sessions. Returns paths and matching snippets.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Words to look for" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 20 }
                },
                "required": ["query"]
            }
        },
        {
            "name": "duskie_notify",
            "description": "Show a short note to the user on the Duskie island at the top of their screen, for example when a long task finishes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "maxLength": 80 },
                    "message": { "type": "string", "maxLength": 200 }
                },
                "required": ["title"]
            }
        },
        {
            "name": "duskie_open_url",
            "description": "Open a web link (http or https) in the user's browser, optionally a specific one (chrome, edge, firefox, zen, brave) or a private window.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "browser": { "type": "string", "enum": ["chrome", "edge", "firefox", "zen", "brave"] },
                    "private": { "type": "boolean" }
                },
                "required": ["url"]
            }
        },
        {
            "name": "duskie_permission",
            "description": "Used by Claude Code itself to ask the user before a step in a Duskie session. Never call it yourself.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tool_name": { "type": "string" },
                    "input": { "type": "object" },
                    "tool_use_id": { "type": "string" }
                },
                "required": ["tool_name", "input"]
            }
        },
        {
            "name": "duskie_time_today",
            "description": "How the user's time at the computer was spent today, per app and project.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

async fn handle(app: &AppHandle, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg["method"].as_str().unwrap_or_default();
    let id = id?; // Notifications (no id) get no answer.
    Some(match method {
        "initialize" => {
            let asked = msg["params"]["protocolVersion"]
                .as_str()
                .unwrap_or_default();
            let version = VERSIONS
                .iter()
                .find(|v| **v == asked)
                .unwrap_or(&VERSIONS[0]);
            ok(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "duskie", "version": app.package_info().version.to_string() },
                    "instructions": "Duskie is the user's desktop assistant on their Windows PC. Use these tools instead of a shell: duskie_find_files to find files and folders by name, duskie_search for their history and file contents, duskie_open and duskie_show_in_folder to open things, duskie_screen_text for what is on screen, duskie_propose to offer a change as a button, duskie_notify to tell them something finished."
                }),
            )
        }
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({ "tools": tools() })),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or_default();
            let args = &msg["params"]["arguments"];
            log::info!("MCP tool call: {name}");
            ok(id, call(app, name, args).await)
        }
        _ => rpc_error(id, -32601, "method not found"),
    })
}

/// A tool's answer as plain text, for the local model in Ask mode.
pub async fn call_text(app: &AppHandle, name: &str, args: &Value) -> String {
    let out = call(app, name, args).await;
    out["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

async fn call(app: &AppHandle, name: &str, args: &Value) -> Value {
    // Clients set up before the rename still call the old names.
    let renamed = name
        .strip_prefix("sidekick_")
        .map(|n| format!("duskie_{n}"));
    let name = renamed.as_deref().unwrap_or(name);
    if let Some(shared) = name.strip_prefix("duskie_").filter(|n| SHARED.contains(n)) {
        let chat = crate::ask_tools::current_chat();
        // Claude Code and Codex steps show in Ask like the local model's.
        if !chat.is_empty() {
            let _ = app.emit(
                crate::composio::TOOL_EVENT,
                serde_json::json!({
                    "id": chat,
                    "name": shared,
                    "label": crate::ask_tools::step_label(shared, args),
                }),
            );
        }
        // Boxed: Ask's own tools call back into this server for today's time.
        return match Box::pin(crate::ask_tools::run(app, &chat, shared, args)).await {
            Some(out) if out.starts_with("Error") => tool_error(out),
            Some(out) => text(out),
            None => tool_error(format!("unknown tool {name}")),
        };
    }
    match name {
        "duskie_permission" => text(crate::sessions::claude_permission(app, args).await),
        "duskie_search" => {
            let query = args["query"].as_str().unwrap_or_default();
            let limit = args["limit"].as_u64().unwrap_or(8).clamp(1, 20) as u32;
            let hits = crate::search::hybrid(app, query, SHAREABLE, limit).await;
            if hits.is_empty() {
                return text(format!("Nothing found for \"{query}\"."));
            }
            let lines: Vec<String> = hits
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    format!(
                        "{}. [{}] {} ({})\n   {}",
                        i + 1,
                        h.source,
                        h.title,
                        h.reference,
                        h.snippet
                    )
                })
                .collect();
            text(lines.join("\n"))
        }
        "duskie_notify" => {
            let title: String = args["title"]
                .as_str()
                .unwrap_or("Claude Code")
                .chars()
                .take(80)
                .collect();
            let message: String = args["message"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect();
            crate::suggestions::offer(
                app,
                Proposal {
                    skill_id: "mcp.notify".into(),
                    skill_ids: vec!["mcp.notify".into()],
                    title,
                    detail: message,
                    options: vec![ProposedOption {
                        label: "Got it".into(),
                        action: "noop".into(),
                        args: json!({ "message": "OK" }),
                        skill_id: "mcp.notify".into(),
                    }],
                    trust: Trust::Suggest,
                    remember: None,
                    priority: 70,
                    compact: false,
                    silent: false,
                },
            );
            text("Shown on the island.")
        }
        "duskie_open_url" => {
            let exec = executor(&app.state::<AppState>());
            let mut a = json!({ "url": args["url"].as_str().unwrap_or_default() });
            if let Some(b) = args["browser"].as_str() {
                a["browser"] = json!(b);
            }
            if args["private"].as_bool() == Some(true) {
                a["private"] = json!("true");
            }
            match exec.run("open_url", &a).await {
                Ok(o) => text(o.message),
                Err(e) => tool_error(e.to_string()),
            }
        }
        "duskie_time_today" => {
            let day = chrono::Local::now().format("%Y-%m-%d").to_string();
            let rows = lock(&app.state::<AppState>().storage)
                .time_for_day(&day)
                .unwrap_or_default();
            if rows.is_empty() {
                return text("Nothing counted yet today.");
            }
            let lines: Vec<String> = rows
                .iter()
                .take(12)
                .map(|r| {
                    let p = if r.project.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", r.project)
                    };
                    format!("{}{}: {} min", r.app, p, r.secs / 60)
                })
                .collect();
            text(lines.join("\n"))
        }
        other => tool_error(format!("unknown tool {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_tokens_are_checked() {
        assert!(token_ok(Some("Bearer abc"), "abc"));
        assert!(!token_ok(Some("Bearer abd"), "abc"));
        assert!(!token_ok(Some("abc"), "abc"));
        assert!(!token_ok(None, "abc"));
        assert!(!token_ok(Some("Bearer "), ""));
    }

    #[test]
    fn tools_have_names_and_schemas() {
        let tools = tools();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "duskie_search",
                "duskie_notify",
                "duskie_open_url",
                "duskie_permission",
                "duskie_time_today",
                "duskie_find_files",
                "duskie_show_in_folder",
                "duskie_recent",
                "duskie_screen_text",
                "duskie_propose",
                "duskie_web_search",
                "duskie_read_page",
                "duskie_browser",
                "duskie_app_action",
                "duskie_desktop",
                "duskie_apps",
                "duskie_office",
                "duskie_recipes",
                "duskie_remember",
                "duskie_notifications",
                "duskie_pc_status",
                "duskie_storage",
                "duskie_pc_control",
                "duskie_windows",
                "duskie_open",
            ]
        );
        assert!(
            tools
                .as_array()
                .unwrap()
                .iter()
                .all(|t| t["inputSchema"]["type"] == "object")
        );
    }
}
