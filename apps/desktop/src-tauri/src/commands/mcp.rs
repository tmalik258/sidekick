//! Direct MCP: validate a stdio server before it is saved. Starting and
//! calling tools live in duskie-ai; Settings only stores what to run.

use duskie_ai::mcp_stdio::{Spec, validate};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSpecIn {
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

/// Rejects shell-looking command lines before they are saved.
#[tauri::command]
pub fn mcp_validate_stdio(spec: McpSpecIn) -> Result<(), String> {
    validate(&Spec {
        command: spec.command,
        args: spec.args,
        env: BTreeMap::new(),
    })
}
