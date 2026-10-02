//! Composio's REST API: signing in from the browser (the same flow as
//! Composio's own CLI), the apps connected there, links to connect more,
//! and running a tool. The key is kept in Credential Manager.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

pub const API: &str = "https://backend.composio.dev";
const DASHBOARD: &str = "https://dashboard.composio.dev/";
pub const KEY_NAME: &str = "composio-api-key";
const TIMEOUT: Duration = Duration::from_secs(20);
/// A tool-router session is reused this long before a fresh one is made.
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);

/// Apps Sidekick offers to connect, in the order shown.
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

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(TIMEOUT)
        .build()
        .unwrap_or_default()
}

async fn check(resp: reqwest::Response) -> Result<Value, String> {
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(body);
    }
    let detail = body["error"]["message"]
        .as_str()
        .or(body["message"].as_str())
        .or(body["error"].as_str())
        .unwrap_or("no details");
    Err(match status.as_u16() {
        401 | 403 => "Composio did not accept the key. Connect Composio again.".into(),
        _ => format!("Composio answered {status}: {detail}"),
    })
}

/// A browser sign-in in progress.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Login {
    pub id: String,
    pub code: String,
    pub url: String,
}

pub async fn begin_login() -> Result<Login, String> {
    let host = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "this PC".into());
    let resp = client()
        .post(format!("{API}/api/v3.1/cli/create-session"))
        .json(&json!({ "scope": "project", "source": format!("Sidekick on {host}") }))
        .send()
        .await
        .map_err(|e| format!("Composio is not reachable: {e}"))?;
    let v = check(resp).await?;
    let id = v["id"].as_str().ok_or("Composio did not start a sign-in")?;
    Ok(Login {
        id: id.to_owned(),
        code: v["code"].as_str().unwrap_or_default().to_owned(),
        url: login_url(id),
    })
}

pub fn login_url(id: &str) -> String {
    format!("{DASHBOARD}?cliKey={id}")
}

/// The key and the account's email once the user allowed Sidekick in the
/// browser; None while it is still pending.
pub async fn poll_login(id: &str) -> Result<Option<(String, String)>, String> {
    let resp = client()
        .get(format!("{API}/api/v3.1/cli/get-session"))
        .query(&[("id", id)])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v = check(resp).await?;
    Ok(linked(&v))
}

pub fn linked(v: &Value) -> Option<(String, String)> {
    if v["status"].as_str() != Some("linked") {
        return None;
    }
    let key = v["api_key"].as_str().filter(|k| !k.is_empty())?;
    let who = v["account"]["email"]
        .as_str()
        .or(v["account"]["name"].as_str())
        .unwrap_or_default();
    Some((key.to_owned(), who.to_owned()))
}

/// The user id the apps are connected under in this project: the most
/// common one among active connections, or "default".
pub async fn guess_user_id(key: &str) -> String {
    let resp = client()
        .get(format!("{API}/api/v3.1/connected_accounts"))
        .header("x-api-key", key)
        .query(&[("statuses", "ACTIVE"), ("limit", "100")])
        .send()
        .await;
    let Ok(resp) = resp else {
        return "default".into();
    };
    let v = check(resp).await.unwrap_or(Value::Null);
    most_common_user(&v).unwrap_or_else(|| "default".into())
}

pub fn most_common_user(v: &Value) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for item in v["items"].as_array()? {
        if let Some(u) = item["user_id"].as_str().filter(|u| !u.is_empty()) {
            *counts.entry(u).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(u, _)| u.to_owned())
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub mcp_url: String,
}

static SESSION: Mutex<Option<(String, Instant, Session)>> = Mutex::new(None);

/// A tool-router session for `user_id`, made once and reused for a while.
pub async fn session(key: &str, user_id: &str) -> Result<Session, String> {
    let tag = format!("{}:{user_id}", &key[..key.len().min(12)]);
    if let Some((t, at, s)) = SESSION.lock().ok().and_then(|g| g.clone())
        && t == tag
        && at.elapsed() < SESSION_TTL
    {
        return Ok(s);
    }
    let resp = client()
        .post(format!("{API}/api/v3.1/tool_router/session"))
        .header("x-api-key", key)
        .json(&json!({ "user_id": user_id }))
        .send()
        .await
        .map_err(|e| format!("Composio is not reachable: {e}"))?;
    let v = check(resp).await?;
    let s = Session {
        id: v["session_id"]
            .as_str()
            .ok_or("Composio did not open a session")?
            .to_owned(),
        mcp_url: v["mcp"]["url"].as_str().unwrap_or_default().to_owned(),
    };
    if let Ok(mut g) = SESSION.lock() {
        *g = Some((tag, Instant::now(), s.clone()));
    }
    Ok(s)
}

pub fn forget_session() {
    if let Ok(mut g) = SESSION.lock() {
        *g = None;
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub slug: String,
    pub name: String,
    pub why: String,
    pub logo: String,
    pub connected: bool,
}

/// Sidekick's apps and whether each is connected.
pub async fn apps(key: &str, session: &Session) -> Result<Vec<App>, String> {
    let slugs: Vec<&str> = APPS.iter().map(|a| a.0).collect();
    let resp = client()
        .get(format!(
            "{API}/api/v3.1/tool_router/session/{}/toolkits",
            session.id
        ))
        .header("x-api-key", key)
        .query(&[("toolkits", slugs.join(",")), ("limit", "50".into())])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v = check(resp).await?;
    Ok(merge_apps(&v))
}

pub fn merge_apps(v: &Value) -> Vec<App> {
    let found: HashMap<&str, &Value> = v["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| Some((i["slug"].as_str()?, i)))
                .collect()
        })
        .unwrap_or_default();
    APPS.iter()
        .map(|(slug, name, why)| {
            let item = found.get(slug);
            let connected = item.is_some_and(|i| {
                i["connected_account"]["status"]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case("ACTIVE"))
            });
            App {
                slug: (*slug).into(),
                name: (*name).into(),
                why: (*why).into(),
                logo: item
                    .and_then(|i| i["meta"]["logo"].as_str())
                    .unwrap_or_default()
                    .into(),
                connected,
            }
        })
        .collect()
}

/// A page where the user signs in to `toolkit` and allows Composio.
pub async fn link(key: &str, session: &Session, toolkit: &str) -> Result<String, String> {
    let resp = client()
        .post(format!(
            "{API}/api/v3.1/tool_router/session/{}/link",
            session.id
        ))
        .header("x-api-key", key)
        .json(&json!({ "toolkit": toolkit }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v = check(resp).await?;
    v["redirect_url"]
        .as_str()
        .filter(|u| u.starts_with("https://"))
        .map(str::to_owned)
        .ok_or_else(|| "Composio did not return a sign-in page".into())
}

/// Runs one tool and returns its data.
pub async fn execute(
    key: &str,
    session: &Session,
    tool: &str,
    arguments: Value,
) -> Result<Value, String> {
    let resp = client()
        .post(format!(
            "{API}/api/v3.1/tool_router/session/{}/execute",
            session.id
        ))
        .header("x-api-key", key)
        .json(&json!({ "tool_slug": tool, "arguments": arguments }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v = check(resp).await?;
    if let Some(err) = v["error"].as_str().filter(|e| !e.is_empty()) {
        return Err(format!("{tool}: {err}"));
    }
    Ok(v["data"].clone())
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
    fn sign_in_finishes_only_when_linked_with_a_key() {
        assert!(linked(&json!({ "status": "pending", "api_key": null })).is_none());
        assert!(linked(&json!({ "status": "linked", "api_key": "" })).is_none());
        let (key, who) = linked(&json!({
            "status": "linked", "api_key": "ak_1", "account": { "email": "me@x.com", "name": "Me" }
        }))
        .unwrap();
        assert_eq!((key.as_str(), who.as_str()), ("ak_1", "me@x.com"));
        assert_eq!(
            login_url("abc"),
            "https://dashboard.composio.dev/?cliKey=abc"
        );
    }

    #[test]
    fn picks_the_user_most_apps_belong_to() {
        let v = json!({ "items": [
            { "user_id": "pg-1" }, { "user_id": "me@x.com" }, { "user_id": "me@x.com" }, { "user_id": "" }
        ]});
        assert_eq!(most_common_user(&v).as_deref(), Some("me@x.com"));
        assert_eq!(most_common_user(&json!({ "items": [] })), None);
    }

    #[test]
    fn marks_connected_apps() {
        let v = json!({ "items": [
            { "slug": "gmail", "meta": { "logo": "https://l/g.png" }, "connected_account": { "status": "ACTIVE" } },
            { "slug": "slack", "connected_account": { "status": "EXPIRED" } },
            { "slug": "jira", "connected_account": null },
        ]});
        let apps = merge_apps(&v);
        assert_eq!(apps.len(), APPS.len());
        let get = |s: &str| apps.iter().find(|a| a.slug == s).unwrap();
        assert!(get("gmail").connected);
        assert_eq!(get("gmail").logo, "https://l/g.png");
        assert!(!get("slack").connected);
        assert!(!get("jira").connected);
        assert!(!get("notion").connected);
    }

    #[test]
    fn finds_nested_lists() {
        let v = json!({ "data": { "response_data": { "items": [1, 2] } } });
        assert_eq!(find_array(&v, &["items"]).unwrap().len(), 2);
        assert!(find_array(&json!({ "x": 1 }), &["items"]).is_none());
    }
}
