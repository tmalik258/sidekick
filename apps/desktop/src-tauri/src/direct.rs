//! Apps connected directly over MCP, offered to Ask next to Composio. Only
//! servers whose exact command the user confirmed run. A direct connection
//! wins over Composio for the same app, and the user is told that once.
//! Every call has a timeout and a capped output; a program ends after each
//! call (dropping the client kills it).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use duskie_ai::mcp_stdio::StdioMcp;
use duskie_ai::{McpClient, McpTool};
use duskie_core::DirectServer;
use duskie_core::connectors::{self, Connection, Source};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::servers::{secret_name, spec_of};
use crate::state::{AppState, lock};

/// Between the app and the tool in the name the model sees.
const SEP: &str = "__";
const LIST_FRESH: Duration = Duration::from_secs(5 * 60);
const LIST_TIMEOUT: Duration = Duration::from_secs(15);
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest answer passed back to the model.
const OUTPUT_CAP: usize = 16_000;
pub const DUPLICATE_EVENT: &str = "connectors://duplicate";

/// One tool of one server: (name the model sees, server, tool).
pub type Offered = (String, DirectServer, McpTool);

static TOOLS: tokio::sync::Mutex<Option<(Instant, Vec<Offered>)>> =
    tokio::sync::Mutex::const_new(None);

fn clean(app: &str) -> String {
    app.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// The name the model sees for `tool` on `app`.
pub fn tool_name(app: &str, tool: &str) -> String {
    format!("{}{SEP}{tool}", clean(app))
}

fn headers(s: &DirectServer) -> Vec<(String, String)> {
    s.env_names
        .iter()
        .filter_map(|n| crate::secrets::get(&secret_name(&s.id, n)).map(|v| (n.clone(), v)))
        .collect()
}

async fn list_one(s: &DirectServer) -> Result<Vec<McpTool>, String> {
    let work = async {
        if s.kind == "stdio" {
            let mut client = StdioMcp::spawn(&spec_of(s, crate::secrets::get))
                .await
                .map_err(|e| e.to_string())?;
            client.list_tools().await.map_err(|e| e.to_string())
        } else {
            McpClient::connect(&s.url, headers(s))
                .await
                .map_err(|e| e.to_string())?
                .list_tools()
                .await
                .map_err(|e| e.to_string())
        }
    };
    tokio::time::timeout(LIST_TIMEOUT, work)
        .await
        .map_err(|_| "did not answer in time".to_owned())?
}

fn confirmed(app: &AppHandle) -> Vec<DirectServer> {
    lock(&app.state::<AppState>().settings)
        .direct_servers
        .iter()
        .filter(|s| s.confirmed)
        .cloned()
        .collect()
}

/// Every tool the confirmed servers offer, kept for a few minutes.
pub async fn tools(app: &AppHandle) -> Vec<Offered> {
    let servers = confirmed(app);
    let mut cache = TOOLS.lock().await;
    if let Some((at, list)) = cache.as_ref()
        && at.elapsed() < LIST_FRESH
        && list.iter().all(|(_, s, _)| servers.contains(s))
    {
        return list.clone();
    }
    let mut out = Vec::new();
    for s in servers {
        match list_one(&s).await {
            Ok(list) => out.extend(
                list.into_iter()
                    .map(|t| (tool_name(&s.app, &t.name), s.clone(), t)),
            ),
            Err(err) => log::warn!("direct server for {} not reachable: {err}", s.app),
        }
    }
    *cache = Some((Instant::now(), out.clone()));
    out
}

/// The tools as the model sees them, renamed so they cannot clash.
pub fn as_mcp(offered: &[Offered]) -> Vec<McpTool> {
    offered
        .iter()
        .map(|(name, s, t)| McpTool {
            name: name.clone(),
            description: format!("{} (via {} direct): {}", t.name, s.app, t.description),
            input_schema: t.input_schema.clone(),
        })
        .collect()
}

/// Apps connected directly, lower case.
pub fn apps(offered: &[Offered]) -> BTreeSet<String> {
    offered.iter().map(|(_, s, _)| s.app.clone()).collect()
}

/// A Composio tool for an app that is also connected directly: the direct
/// one is used, so this one is not offered.
pub fn shadowed(composio_tool: &str, direct: &BTreeSet<String>) -> bool {
    let upper = composio_tool.to_ascii_uppercase();
    direct
        .iter()
        .any(|app| upper.starts_with(&format!("{}_", clean(app).to_ascii_uppercase())))
}

fn told_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("duplicates_told.json"))
}

/// Apps whose duplicate was not yet mentioned, from `all` connections.
pub fn untold(all: &[Connection], told: &BTreeSet<String>) -> Vec<(String, Source, Vec<Source>)> {
    connectors::duplicates(all)
        .into_iter()
        .filter(|(app, _, _)| !told.contains(app))
        .collect()
}

/// Tells the user once per app that it is connected twice and the direct
/// connection is used.
pub fn tell_duplicates(app: &AppHandle, direct: &BTreeSet<String>, composio: &[McpTool]) {
    let mut all: Vec<Connection> = Vec::new();
    for a in direct {
        all.push(Connection {
            id: a.clone(),
            app: a.clone(),
            source: Source::Direct,
            writes_allowed: Vec::new(),
        });
        if composio
            .iter()
            .any(|t| shadowed(&t.name, &[a.clone()].into()))
        {
            all.push(Connection {
                id: format!("composio:{a}"),
                app: a.clone(),
                source: Source::Composio,
                writes_allowed: Vec::new(),
            });
        }
    }
    let Some(path) = told_path(app) else { return };
    let mut told: BTreeSet<String> = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let fresh = untold(&all, &told);
    if fresh.is_empty() {
        return;
    }
    for (name, used, unused) in fresh {
        let _ = app.emit(
            DUPLICATE_EVENT,
            serde_json::json!({
                "app": name,
                "used": used.label(),
                "unused": unused.iter().map(|s| s.label()).collect::<Vec<_>>(),
            }),
        );
        told.insert(name);
    }
    if let Ok(bytes) = serde_json::to_vec(&told) {
        let _ = std::fs::write(path, bytes);
    }
}

fn cap(mut text: String) -> String {
    if text.len() > OUTPUT_CAP {
        let mut end = OUTPUT_CAP;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[cut: the answer was too long]");
    }
    text
}

/// Runs a direct tool, or None when `name` is not one. Writes run only for
/// tools the user allowed in Settings > Apps.
pub async fn call(name: &str, arguments: &Value) -> Option<String> {
    if !name.contains(SEP) {
        return None;
    }
    let (s, tool) = {
        let cache = TOOLS.lock().await;
        let (_, list) = cache.as_ref()?;
        let (_, s, t) = list.iter().find(|(n, _, _)| n == name)?;
        (s.clone(), t.name.clone())
    };
    let conn = Connection {
        id: s.id.clone(),
        app: s.app.clone(),
        source: Source::Direct,
        writes_allowed: s.writes_allowed.clone(),
    };
    if !connectors::may_call(&tool, &conn) {
        return Some(format!(
            "Error: {tool} changes things in {}, and that is off. The user can allow it in \
             Settings > Apps.",
            s.app
        ));
    }
    let work = async {
        if s.kind == "stdio" {
            let mut client = StdioMcp::spawn(&spec_of(&s, crate::secrets::get))
                .await
                .map_err(|e| e.to_string())?;
            client
                .call_tool(&tool, arguments)
                .await
                .map_err(|e| e.to_string())
        } else {
            McpClient::connect(&s.url, headers(&s))
                .await
                .map_err(|e| e.to_string())?
                .call_tool(&tool, arguments)
                .await
                .map_err(|e| e.to_string())
        }
    };
    Some(match tokio::time::timeout(CALL_TIMEOUT, work).await {
        Ok(Ok(text)) => cap(text),
        Ok(Err(err)) => format!("Error: {err}"),
        Err(_) => format!("Error: {} did not answer in time.", s.app),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_do_not_clash_and_composio_twins_are_hidden() {
        assert_eq!(tool_name("Google Drive", "list"), "google_drive__list");
        let direct: BTreeSet<String> = ["github".to_owned()].into();
        assert!(shadowed("GITHUB_LIST_ISSUES", &direct));
        assert!(!shadowed("GMAIL_FETCH_EMAILS", &direct));
    }

    #[test]
    fn tells_each_duplicate_once() {
        let c = |app: &str, source| Connection {
            id: format!("{app}{source:?}"),
            app: app.into(),
            source,
            writes_allowed: vec![],
        };
        let all = vec![
            c("github", Source::Direct),
            c("github", Source::Composio),
            c("gmail", Source::Composio),
        ];
        let fresh = untold(&all, &BTreeSet::new());
        assert_eq!(
            fresh,
            vec![("github".into(), Source::Direct, vec![Source::Composio])]
        );
        assert!(untold(&all, &["github".to_owned()].into()).is_empty());
    }

    #[test]
    fn long_answers_are_cut() {
        let out = cap("é".repeat(OUTPUT_CAP));
        assert!(out.len() < OUTPUT_CAP + 64);
        assert!(out.ends_with("too long]"));
    }
}
