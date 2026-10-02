//! Composio Connect: one connection at `connect.composio.dev/mcp` (the
//! same one Claude Desktop uses) that carries every app connected in your
//! Composio account. This file holds what is known about its answers:
//! which apps are connected, a tool's result, a sign-in link. The network
//! side is in `composio.rs`.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::{Value, json};

/// Composio's MCP server for people (not developer projects).
pub const CONNECT_URL: &str = "https://connect.composio.dev/mcp";
/// The OAuth grant from Connect Composio, in Credential Manager.
pub const GRANT_NAME: &str = "composio-oauth";
/// A consumer key (`ck_...`) pasted instead of signing in.
pub const CONSUMER_KEY_NAME: &str = "composio-consumer-key";
/// The developer key from the old sign-in; only ever deleted now.
pub const LEGACY_KEY_NAME: &str = "composio-api-key";

pub const EXECUTE_TOOL: &str = "COMPOSIO_MULTI_EXECUTE_TOOL";
pub const CONNECTIONS_TOOL: &str = "COMPOSIO_MANAGE_CONNECTIONS";
pub const SEARCH_TOOL: &str = "COMPOSIO_SEARCH_TOOLS";

/// Apps Sidekick uses on its own, in the order shown, with what for.
pub const APPS: &[(&str, &str, &str)] = &[
    (
        "googlecalendar",
        "Google Calendar",
        "Meeting reminders and your day",
    ),
    ("outlook", "Outlook", "Calendar and mail from Microsoft 365"),
    ("gmail", "Gmail", "Unread client mail in the morning brief"),
    ("slack", "Slack", "Messages to you in the brief"),
    ("jira", "Jira", "Issues assigned to you"),
    ("github", "GitHub", "Pull requests and issues"),
    ("fathom", "Fathom", "Meeting notes for follow-ups"),
    ("notion", "Notion", "Pages and notes in Ask mode"),
    ("trello", "Trello", "Cards in Ask mode"),
    ("linear", "Linear", "Issues in Ask mode"),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub slug: String,
    pub name: String,
    /// What Sidekick uses it for; empty for other apps in the account.
    pub why: String,
    pub connected: bool,
}

/// A readable name for a toolkit slug.
pub fn app_name(slug: &str) -> String {
    if let Some((_, name, _)) = APPS.iter().find(|a| a.0 == slug) {
        return (*name).to_owned();
    }
    let known: &[(&str, &str)] = &[
        ("googledocs", "Google Docs"),
        ("googledrive", "Google Drive"),
        ("googlesheets", "Google Sheets"),
        ("googlemeet", "Google Meet"),
        ("one_drive", "OneDrive"),
        ("linkedin", "LinkedIn"),
        ("youtube", "YouTube"),
        ("hubspot", "HubSpot"),
        ("clickup", "ClickUp"),
    ];
    if let Some((_, n)) = known.iter().find(|k| k.0 == slug) {
        return (*n).to_owned();
    }
    let mut c = slug.replace('_', " ");
    if let Some(first) = c.get(0..1).map(str::to_uppercase) {
        c.replace_range(0..1, &first);
    }
    c
}

/// Arguments that list the connections of Sidekick's apps in one call.
pub fn list_args() -> Value {
    let toolkits: Vec<Value> = APPS
        .iter()
        .map(|(slug, _, _)| json!({ "name": slug, "action": "list" }))
        .collect();
    json!({ "toolkits": toolkits })
}

/// Arguments that start connecting one app.
pub fn add_args(slug: &str) -> Value {
    json!({ "toolkits": [{ "name": slug, "action": "add" }] })
}

/// Arguments that run one tool.
pub fn execute_args(tool: &str, arguments: Value) -> Value {
    json!({
        "tools": [{ "tool_slug": tool, "arguments": arguments }],
        "sync_response_to_workbench": false,
        "thought": "Sidekick reads this for the user.",
    })
}

/// A tool's text answer as JSON (it may be wrapped in "Error: ").
pub fn parse_text(text: &str) -> Result<Value, String> {
    let t = text.trim();
    if let Some(err) = t.strip_prefix("Error:") {
        return Err(err.trim().chars().take(300).collect());
    }
    serde_json::from_str(t).map_err(|_| t.chars().take(300).collect())
}

fn is_active(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("active"))
}

/// Toolkits with an active connection, from a `list` answer.
pub fn connected_from_list(v: &Value) -> BTreeSet<String> {
    let results = v["data"]["results"]
        .as_object()
        .or(v["results"].as_object());
    results
        .map(|m| {
            m.iter()
                .filter(|(_, r)| {
                    is_active(&r["status"])
                        || r["accounts"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|x| is_active(&x["status"])))
                })
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Every app connected in the account, as the search tool's description
/// lists them ("User has manually connected the apps: gmail, slack.").
pub fn connected_from_description(description: &str) -> BTreeSet<String> {
    let Some(at) = description.find("connected the apps:") else {
        return BTreeSet::new();
    };
    let rest = &description[at + "connected the apps:".len()..];
    let list = rest.split(['.', '\n']).next().unwrap_or_default();
    list.split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .collect()
}

/// Sidekick's apps first (connected or not), then the rest of what the
/// account has connected.
pub fn merge_apps(connected: &BTreeSet<String>) -> Vec<App> {
    let mut out: Vec<App> = APPS
        .iter()
        .map(|(slug, name, why)| App {
            slug: (*slug).into(),
            name: (*name).into(),
            why: (*why).into(),
            connected: connected.contains(*slug),
        })
        .collect();
    out.extend(
        connected
            .iter()
            .filter(|s| !APPS.iter().any(|a| a.0 == s.as_str()))
            .map(|slug| App {
                slug: slug.clone(),
                name: app_name(slug),
                why: String::new(),
                connected: true,
            }),
    );
    out
}

/// The result of the first tool in a multi-execute answer.
pub fn first_result(v: &Value) -> Result<Value, String> {
    if let Some(err) = v["error"].as_str().filter(|e| !e.is_empty()) {
        return Err(err.to_owned());
    }
    let r = &v["data"]["results"][0];
    let resp = if r.get("response").is_some() {
        &r["response"]
    } else {
        r
    };
    let error = resp["error"].as_str().filter(|e| !e.is_empty());
    if resp["successful"].as_bool() == Some(false) || error.is_some() {
        return Err(error
            .unwrap_or("the tool failed")
            .chars()
            .take(300)
            .collect());
    }
    Ok(resp["data"].clone())
}

/// The first sign-in link (`redirect_url` and friends) found anywhere.
pub fn redirect_url(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            for k in ["redirect_url", "redirectUrl", "connect_url", "url"] {
                if let Some(u) = map.get(k).and_then(Value::as_str)
                    && u.starts_with("https://")
                {
                    return Some(u.to_owned());
                }
            }
            map.values().find_map(redirect_url)
        }
        Value::Array(items) => items.iter().find_map(redirect_url),
        _ => None,
    }
}

/// The first array found under any of `keys`, searched depth first. Tool
/// answers nest their lists differently (`items`, `data.items`, `value`).
pub fn find_array<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Vec<Value>> {
    match v {
        Value::Object(map) => {
            for k in keys {
                if let Some(Value::Array(a)) = map.get(*k) {
                    return Some(a);
                }
            }
            map.values().find_map(|x| find_array(x, keys))
        }
        Value::Array(items) => items.iter().find_map(|x| find_array(x, keys)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_connections_from_a_list_answer() {
        // Shape of COMPOSIO_MANAGE_CONNECTIONS with action "list".
        let v = json!({ "data": { "results": {
            "googlecalendar": { "status": "active", "accounts": [{ "status": "active" }] },
            "gmail": { "status": "something", "accounts": [{ "status": "ACTIVE" }] },
            "jira": { "status": "initiated", "accounts": [] },
        }}, "successful": true });
        let c = connected_from_list(&v);
        assert!(c.contains("googlecalendar") && c.contains("gmail"));
        assert!(!c.contains("jira"));
    }

    #[test]
    fn reads_every_connected_app_from_the_search_tool() {
        let d = "Tool Server Info: ...\n- User has manually connected the apps: figma, github, gmail, one_drive, slack. Prefer these apps when intent is unclear.";
        let c = connected_from_description(d);
        assert_eq!(c.len(), 5);
        assert!(c.contains("one_drive"));
        assert!(connected_from_description("nothing here").is_empty());
        let apps = merge_apps(&c);
        assert!(apps.iter().find(|a| a.slug == "gmail").unwrap().connected);
        assert!(!apps.iter().find(|a| a.slug == "jira").unwrap().connected);
        let figma = apps.iter().find(|a| a.slug == "figma").unwrap();
        assert!(figma.connected && figma.why.is_empty());
        assert_eq!(app_name("one_drive"), "OneDrive");
        assert_eq!(app_name("kieai"), "Kieai");
    }

    #[test]
    fn reads_a_tool_result() {
        // Shape of COMPOSIO_MULTI_EXECUTE_TOOL.
        let ok = json!({ "data": { "results": [{ "response": { "successful": true, "data": { "items": [1] } }, "tool_slug": "X", "index": 0 }] }, "error": null, "successful": true });
        assert_eq!(first_result(&ok).unwrap()["items"][0], 1);
        let failed = json!({ "data": { "results": [{ "response": { "successful": false, "error": "No connected account" } }] } });
        assert_eq!(first_result(&failed).unwrap_err(), "No connected account");
        assert!(parse_text("Error: 401").is_err());
        assert_eq!(parse_text("{\"a\":1}").unwrap()["a"], 1);
        assert_eq!(
            execute_args("GMAIL_FETCH_EMAILS", json!({}))["tools"][0]["tool_slug"],
            "GMAIL_FETCH_EMAILS"
        );
        assert_eq!(
            list_args()["toolkits"].as_array().unwrap().len(),
            APPS.len()
        );
        assert_eq!(add_args("jira")["toolkits"][0]["action"], "add");
    }

    #[test]
    fn finds_links_and_lists() {
        let v = json!({ "data": { "results": { "jira": { "redirect_url": "https://connect.composio.dev/link/x" } } } });
        assert_eq!(
            redirect_url(&v).as_deref(),
            Some("https://connect.composio.dev/link/x")
        );
        assert!(redirect_url(&json!({ "url": "http://insecure" })).is_none());
        let v = json!({ "data": { "response_data": { "items": [1, 2] } } });
        assert_eq!(find_array(&v, &["items"]).unwrap().len(), 2);
        assert!(find_array(&json!({ "x": 1 }), &["items"]).is_none());
    }
}
