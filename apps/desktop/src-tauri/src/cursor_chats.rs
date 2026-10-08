//! Chats started in Cursor's own window, read from Cursor's store so they
//! show in Agents. Read-only: the database is opened immutable, so it is
//! safe while Cursor has it open, and nothing is ever written to it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::Value;

/// One Cursor chat, as Agents lists it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CursorChat {
    pub id: String,
    pub title: String,
    /// The project folder, when Cursor says which.
    pub path: Option<String>,
    pub project: String,
    pub updated_at: i64,
    /// "working" while Cursor is generating, else "idle".
    pub status: &'static str,
    pub last_reply: String,
}

fn user_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("Cursor").join("User"))
}

fn open(db: &Path) -> Option<Connection> {
    let uri = format!(
        "file:{}?immutable=1",
        db.to_string_lossy().replace('\\', "/")
    );
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .ok()
}

/// `file:///c%3A/Users/me/app` to a path.
pub fn folder_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let decoded = percent_decode(rest);
    // Windows: "/c:/Users" to "c:/Users".
    let p = match decoded.as_bytes() {
        [b'/', _, b':', ..] => decoded[1..].to_string(),
        _ => decoded,
    };
    Some(p)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Which project each chat belongs to, from each workspace's own store.
fn chat_projects(user: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(rd) = std::fs::read_dir(user.join("workspaceStorage")) else {
        return map;
    };
    for ws in rd.flatten() {
        let dir = ws.path();
        let Some(folder) = std::fs::read_to_string(dir.join("workspace.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v["folder"].as_str().and_then(folder_path))
        else {
            continue;
        };
        let Some(conn) = open(&dir.join("state.vscdb")) else {
            continue;
        };
        let data: Option<String> = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'composer.composerData'",
                [],
                |r| r.get(0),
            )
            .ok();
        let Some(v) = data.and_then(|d| serde_json::from_str::<Value>(&d).ok()) else {
            continue;
        };
        for c in v["allComposers"].as_array().into_iter().flatten() {
            if let Some(id) = c["composerId"].as_str() {
                map.insert(id.to_owned(), folder.clone());
            }
        }
    }
    map
}

/// The text of the last assistant message in a chat.
fn last_reply(conn: &Connection, id: &str, v: &Value) -> String {
    // Older chats keep the conversation inline.
    if let Some(conv) = v["conversation"].as_array()
        && let Some(t) = conv
            .iter()
            .rev()
            .find(|b| b["type"] == 2)
            .and_then(|b| b["text"].as_str())
    {
        return t.to_owned();
    }
    // Newer ones keep each message under its own key.
    let Some(bubble) = v["fullConversationHeadersOnly"]
        .as_array()
        .and_then(|h| h.iter().rev().find(|b| b["type"] == 2))
        .and_then(|b| b["bubbleId"].as_str())
    else {
        return String::new();
    };
    conn.query_row(
        "SELECT value FROM cursorDiskKV WHERE key = ?1",
        [format!("bubbleId:{id}:{bubble}")],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    .and_then(|b| b["text"].as_str().map(String::from))
    .unwrap_or_default()
}

/// One chat from its stored JSON.
pub fn parse(v: &Value, path: Option<String>, reply: String) -> Option<CursorChat> {
    let id = v["composerId"].as_str()?.to_owned();
    let updated_at = v["lastUpdatedAt"]
        .as_i64()
        .or_else(|| v["createdAt"].as_i64())
        .unwrap_or(0);
    let title = v["name"]
        .as_str()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or("Cursor chat")
        .to_owned();
    let project = path
        .as_deref()
        .and_then(|p| p.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next())
        .unwrap_or("")
        .to_owned();
    let status = if v["status"] == "generating" {
        "working"
    } else {
        "idle"
    };
    let mut last_reply = reply.trim().to_owned();
    if last_reply.chars().count() > 400 {
        last_reply = last_reply.chars().take(397).collect::<String>() + "...";
    }
    Some(CursorChat {
        id,
        title,
        path,
        project,
        updated_at,
        status,
        last_reply,
    })
}

/// Recent Cursor chats, newest first. Empty when Cursor is not installed.
pub fn list(limit: usize) -> Vec<CursorChat> {
    let Some(user) = user_dir() else {
        return Vec::new();
    };
    let Some(conn) = open(&user.join("globalStorage").join("state.vscdb")) else {
        return Vec::new();
    };
    let projects = chat_projects(&user);
    let Ok(mut stmt) =
        conn.prepare("SELECT value FROM cursorDiskKV WHERE key LIKE 'composerData:%'")
    else {
        return Vec::new();
    };
    let rows: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map(|it| it.flatten().collect())
        .unwrap_or_default();
    let mut chats: Vec<(Value, i64)> = rows
        .iter()
        .filter_map(|t| serde_json::from_str::<Value>(t).ok())
        // Chats with nothing said yet are drafts.
        .filter(|v| {
            v["fullConversationHeadersOnly"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
                || v["conversation"].as_array().is_some_and(|a| !a.is_empty())
        })
        .map(|v| {
            let at = v["lastUpdatedAt"].as_i64().unwrap_or(0);
            (v, at)
        })
        .collect();
    chats.sort_by_key(|c| std::cmp::Reverse(c.1));
    chats
        .into_iter()
        .take(limit)
        .filter_map(|(v, _)| {
            let id = v["composerId"].as_str()?.to_owned();
            let reply = last_reply(&conn, &id, &v);
            parse(&v, projects.get(&id).cloned(), reply)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_windows_folder_uris() {
        assert_eq!(
            folder_path("file:///c%3A/Users/me/my%20app").as_deref(),
            Some("c:/Users/me/my app")
        );
        assert_eq!(
            folder_path("file:///home/me/app").as_deref(),
            Some("/home/me/app")
        );
    }

    #[test]
    fn parses_a_chat() {
        let v =
            json!({"composerId":"a1","name":"Fix login","lastUpdatedAt":5,"status":"generating"});
        let c = parse(&v, Some("c:/code/shop".into()), " Done. ".into()).unwrap();
        assert_eq!(c.project, "shop");
        assert_eq!(c.status, "working");
        assert_eq!(c.last_reply, "Done.");
        let untitled = parse(&json!({"composerId":"b"}), None, String::new()).unwrap();
        assert_eq!(untitled.title, "Cursor chat");
    }

    #[test]
    fn lists_from_a_store() {
        let dir = std::env::temp_dir().join(format!("sk-cursor-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("state.vscdb");
        let _ = std::fs::remove_file(&db);
        let c = Connection::open(&db).unwrap();
        c.execute_batch("CREATE TABLE cursorDiskKV (key TEXT, value TEXT);")
            .unwrap();
        let chat = json!({"composerId":"x","name":"Tests","lastUpdatedAt":9,
            "fullConversationHeadersOnly":[{"bubbleId":"q","type":1},{"bubbleId":"r","type":2}]});
        c.execute(
            "INSERT INTO cursorDiskKV VALUES ('composerData:x', ?1)",
            [chat.to_string()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO cursorDiskKV VALUES ('bubbleId:x:r', ?1)",
            [json!({"text":"All green"}).to_string()],
        )
        .unwrap();
        drop(c);
        let conn = open(&db).unwrap();
        assert_eq!(last_reply(&conn, "x", &chat), "All green");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
