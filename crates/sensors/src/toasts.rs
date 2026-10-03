//! Windows notifications ("toasts"), read from the database Windows keeps
//! for Notification Center (`wpndatabase.db`). The signed-in user can read
//! it, so no special permission is needed; with Do Not Disturb on, Windows
//! still files every toast there without showing it.
//!
//! The format is undocumented, so everything here is defensive: a missing
//! column or an odd payload skips that toast instead of failing.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{DateTime, TimeZone, Utc};
use regex::Regex;
use rusqlite::{Connection, OpenFlags};

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    /// Rises with every toast, so "newer than" is `id > last`.
    pub id: i64,
    /// The sending app's id as Windows knows it (an AUMID).
    pub app_id: String,
    /// A name a person would use: "WhatsApp", "Outlook", "Slack".
    pub app: String,
    pub title: String,
    pub body: String,
    pub arrived: DateTime<Utc>,
    /// Mirrored from the phone by Phone Link; `app` is then the phone app.
    pub phone: bool,
}

pub fn db_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    Some(
        local
            .join("Microsoft")
            .join("Windows")
            .join("Notifications")
            .join("wpndatabase.db"),
    )
}

const QUERY: &str = "SELECT n.Id, h.PrimaryId, n.Payload, n.ArrivalTime \
     FROM Notification n JOIN NotificationHandler h ON h.RecordId = n.HandlerId \
     WHERE n.Type = 'toast' AND n.Id > ?1 ORDER BY n.Id LIMIT 200";

/// The newest toast id, so a fresh start does not replay old ones.
pub fn latest_id(path: &Path) -> Result<i64, String> {
    with_db(path, |c| {
        c.query_row("SELECT IFNULL(MAX(Id), 0) FROM Notification", [], |r| {
            r.get(0)
        })
    })
}

/// Toasts newer than `after`, oldest first.
pub fn read_since(path: &Path, after: i64) -> Result<Vec<Toast>, String> {
    with_db(path, |c| {
        let mut stmt = c.prepare(QUERY)?;
        let rows = stmt.query_map([after], |r| {
            let payload: Vec<u8> = match r.get_ref(2)? {
                rusqlite::types::ValueRef::Blob(b) => b.to_vec(),
                rusqlite::types::ValueRef::Text(t) => t.to_vec(),
                _ => Vec::new(),
            };
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1).unwrap_or_default(),
                payload,
                r.get::<_, i64>(3).unwrap_or_default(),
            ))
        })?;
        Ok(rows
            .flatten()
            .filter_map(|(id, app_id, payload, arrived)| {
                let (title, body) = parse_payload(&String::from_utf8_lossy(&payload));
                if title.is_empty() && body.is_empty() {
                    return None;
                }
                let (app, title, body, phone) = unwrap_phone(&app_id, title, body);
                Some(Toast {
                    id,
                    app,
                    app_id,
                    title,
                    body,
                    arrived: from_filetime(arrived),
                    phone,
                })
            })
            .collect())
    })
}

/// Opens the database read-only. Windows keeps it open, so when a direct
/// read is refused, a copy (with its write-ahead log) is read instead.
fn with_db<T>(path: &Path, f: impl Fn(&Connection) -> rusqlite::Result<T>) -> Result<T, String> {
    let direct = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .and_then(|c| {
        c.busy_timeout(std::time::Duration::from_millis(500))?;
        f(&c)
    });
    match direct {
        Ok(v) => Ok(v),
        Err(first) => {
            let dir = std::env::temp_dir().join("sidekick-toasts");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let copy = dir.join("wpndatabase.db");
            std::fs::copy(path, &copy).map_err(|e| format!("{first}; copy failed: {e}"))?;
            for ext in ["db-wal", "db-shm"] {
                let side = path.with_extension(ext);
                let _ = std::fs::remove_file(copy.with_extension(ext));
                if side.exists() {
                    let _ = std::fs::copy(&side, copy.with_extension(ext));
                }
            }
            let c = Connection::open(&copy).map_err(|e| e.to_string())?;
            f(&c).map_err(|e| e.to_string())
        }
    }
}

/// Windows FILETIME: 100 ns steps since 1601.
fn from_filetime(ft: i64) -> DateTime<Utc> {
    const UNIX_OFFSET_SECS: i64 = 11_644_473_600;
    if ft <= 0 {
        return Utc::now();
    }
    let secs = ft / 10_000_000 - UNIX_OFFSET_SECS;
    let nanos = (ft % 10_000_000) * 100;
    Utc.timestamp_opt(secs, nanos as u32)
        .single()
        .unwrap_or_else(Utc::now)
}

static TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<text\b[^>]*?(?:/>|>(.*?)</text>)").expect("text regex"));
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<[^>]*>").expect("tag regex"));

fn decode(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Title (the first text line) and body (the rest) of a toast's XML.
pub fn parse_payload(xml: &str) -> (String, String) {
    let lines: Vec<String> = TEXT
        .captures_iter(xml)
        .filter_map(|c| c.get(1))
        .map(|m| decode(TAG.replace_all(m.as_str(), "").trim()))
        .filter(|t| !t.is_empty())
        .collect();
    match lines.split_first() {
        Some((title, rest)) => (title.clone(), rest.join("\n")),
        None => (String::new(), String::new()),
    }
}

/// Phone Link shows every phone notification as its own ("YourPhone"),
/// with the phone app's name as the first line. Puts the real app back:
/// "Gmail" / "Upwork Notification" / "New job alert..." becomes app Gmail,
/// title "Upwork Notification", on the phone.
pub fn unwrap_phone(app_id: &str, title: String, body: String) -> (String, String, String, bool) {
    let id = app_id.to_lowercase();
    let wrapped = id.contains("yourphone") || id.contains("phonelink") || id.contains("phone link");
    if !wrapped {
        return (app_name(app_id), title, body, false);
    }
    let named = title.trim();
    // The first line names the app when it is short; otherwise Phone Link
    // left it out and the title is the message's own.
    if named.is_empty() || named.split_whitespace().count() > 3 || body.trim().is_empty() {
        return ("Phone".to_owned(), title, body, true);
    }
    let app = KNOWN
        .iter()
        .find(|(k, _)| named.to_lowercase().contains(k))
        .map_or_else(|| named.to_owned(), |(_, n)| (*n).to_owned());
    let mut lines = body.splitn(2, '\n');
    let new_title = lines.next().unwrap_or_default().trim().to_owned();
    let rest = lines.next().unwrap_or_default().trim().to_owned();
    (app, new_title, rest, true)
}

/// Known senders by a piece of their id, then a best guess from the id.
const KNOWN: &[(&str, &str)] = &[
    ("whatsapp", "WhatsApp"),
    ("gmail", "Gmail"),
    ("upwork", "Upwork"),
    ("slack", "Slack"),
    ("teams", "Teams"),
    ("outlook", "Outlook"),
    ("windowscommunicationsapps", "Mail"),
    ("telegram", "Telegram"),
    ("discord", "Discord"),
    ("signal", "Signal"),
    ("messenger", "Messenger"),
    ("zoom", "Zoom"),
    ("chrome", "Chrome"),
    ("msedge", "Edge"),
    ("firefox", "Firefox"),
    ("zen", "Zen"),
    ("brave", "Brave"),
    ("cursor", "Cursor"),
    ("vscode", "VS Code"),
    ("code.exe", "VS Code"),
    ("spotify", "Spotify"),
    ("securityhealth", "Windows Security"),
    ("windows.defender", "Windows Security"),
    ("windowsupdate", "Windows Update"),
    ("calendar", "Calendar"),
    ("files", "Files"),
    ("explorer", "File Explorer"),
    ("sidekick", "Sidekick"),
];

pub fn app_name(app_id: &str) -> String {
    let lower = app_id.to_lowercase();
    if let Some((_, name)) = KNOWN.iter().find(|(k, _)| lower.contains(k)) {
        return (*name).to_owned();
    }
    // "Publisher.AppName_hash!App" or "com.vendor.app" or a path.
    let base = app_id
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(app_id)
        .split(['!', '_'])
        .next()
        .unwrap_or(app_id);
    let last = base
        .trim_end_matches(".exe")
        .rsplit('.')
        .find(|p| !p.is_empty() && !p.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or(base);
    let mut c = last.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => app_id.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_phone_link() {
        let (app, title, body, phone) = unwrap_phone(
            "Microsoft.YourPhone_8wekyb3d8bbwe!App",
            "Gmail".into(),
            "Upwork Notification\nNew job alert: Junior Coder".into(),
        );
        assert_eq!(
            (app.as_str(), title.as_str(), body.as_str(), phone),
            (
                "Gmail",
                "Upwork Notification",
                "New job alert: Junior Coder",
                true
            )
        );
        let (app, _, _, _) = unwrap_phone(
            "Microsoft.YourPhone!App",
            "WhatsApp".into(),
            "Ali\nHi".into(),
        );
        assert_eq!(app, "WhatsApp");
        let (app, title, _, phone) =
            unwrap_phone("5319275A.WhatsAppDesktop!App", "Ali".into(), "Hi".into());
        assert_eq!(
            (app.as_str(), title.as_str(), phone),
            ("WhatsApp", "Ali", false)
        );
    }

    const WHATSAPP: &str = r#"<toast launch="chat?id=1"><visual><binding template="ToastGeneric"><text hint-maxLines="1">Ali Khan</text><text>Can you send the invoice &amp; the receipt?</text><image src="x.png"/></binding></visual><actions/></toast>"#;

    #[test]
    fn reads_title_and_body() {
        let (t, b) = parse_payload(WHATSAPP);
        assert_eq!(t, "Ali Khan");
        assert_eq!(b, "Can you send the invoice & the receipt?");
        assert_eq!(parse_payload("<toast/>"), (String::new(), String::new()));
        let (t, b) = parse_payload(
            r#"<toast><visual><binding><text id="1">Only title</text><text/></binding></visual></toast>"#,
        );
        assert_eq!((t.as_str(), b.as_str()), ("Only title", ""));
    }

    #[test]
    fn names_apps() {
        assert_eq!(
            app_name("5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App"),
            "WhatsApp"
        );
        assert_eq!(app_name("com.squirrel.slack.slack"), "Slack");
        assert_eq!(app_name("Microsoft.Office.OUTLOOK.EXE.15"), "Outlook");
        assert_eq!(app_name("Contoso.Notes_abc123!App"), "Notes");
        assert_eq!(app_name(r"C:\Tools\acme.exe"), "Acme");
    }

    #[test]
    fn reads_new_toasts_from_the_database() {
        let dir = std::env::temp_dir().join(format!("sidekick-wpn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wpndatabase.db");
        let _ = std::fs::remove_file(&path);
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE NotificationHandler (RecordId INTEGER PRIMARY KEY, PrimaryId TEXT);
             CREATE TABLE Notification (Id INTEGER PRIMARY KEY, HandlerId INTEGER, Type TEXT, Payload BLOB, ArrivalTime INTEGER);
             INSERT INTO NotificationHandler VALUES (1, '5319275A.WhatsAppDesktop_x!App'), (2, 'Microsoft.Windows.Badge');",
        )
        .unwrap();
        let ft = (1_790_000_000_i64 + 11_644_473_600) * 10_000_000;
        c.execute(
            "INSERT INTO Notification VALUES (5, 1, 'toast', ?1, ?2), (6, 2, 'badge', x'00', 0), (7, 1, 'toast', ?3, ?2)",
            rusqlite::params![WHATSAPP.as_bytes(), ft, "<toast><text>Sara</text><text>Lunch?</text></toast>".as_bytes()],
        )
        .unwrap();
        drop(c);
        assert_eq!(latest_id(&path).unwrap(), 7);
        let all = read_since(&path, 0).unwrap();
        assert_eq!(all.len(), 2, "badges are not toasts");
        assert_eq!(all[0].app, "WhatsApp");
        assert_eq!(all[0].arrived.timestamp(), 1_790_000_000);
        let newer = read_since(&path, 5).unwrap();
        assert_eq!(newer.len(), 1);
        assert_eq!(newer[0].title, "Sara");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
