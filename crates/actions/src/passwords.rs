//! Local Chromium password access. No credentials enter logs, history or AI.
//! Direct local writes do not promise browser cloud synchronization.

use crate::ActionError;
use crate::capabilities::Capabilities;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Deliberately no Debug on any type containing credentials or encrypted blobs.
#[derive(Clone)]
pub struct Login {
    pub username: String,
    pub password: String,
    pub browser: String,
}

pub fn url_matches(url: &str, domain: &str) -> bool {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned));
    let domain = domain.trim_start_matches("www.").to_ascii_lowercase();
    host.is_some_and(|h| {
        let h = h.trim_start_matches("www.");
        !domain.is_empty() && (h == domain || h.ends_with(&format!(".{domain}")))
    })
}

pub fn origin_of(value: &str) -> String {
    url::Url::parse(value)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_default()
}

pub fn realm_of(value: &str) -> String {
    let origin = origin_of(value);
    if origin.is_empty() {
        String::new()
    } else {
        format!("{origin}/")
    }
}

pub fn user_data_dir(id: &str) -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    let candidates: &[&str] = match id {
        "chrome" => &["Google/Chrome/User Data"],
        "edge" => &["Microsoft/Edge/User Data"],
        "brave" => &["BraveSoftware/Brave-Browser/User Data"],
        "samsung" => &[
            "SamsungInternet/User Data",
            "Samsung/SamsungInternet/User Data",
            "Samsung/Internet/User Data",
        ],
        // Firefox/Zen use NSS, which is deliberately outside this feature.
        _ => return None,
    };
    candidates.iter().map(|p| base.join(p)).find(|p| p.is_dir())
}

/// Only a direct child within user data can be selected, including symlink checks.
pub fn selected_profile(data: &Path) -> Option<PathBuf> {
    let root = data.canonicalize().ok()?;
    let state: serde_json::Value = std::fs::read_to_string(root.join("Local State"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let valid = |name: &str| {
        if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
            return None;
        }
        let p = root.join(name).canonicalize().ok()?;
        (p.parent() == Some(root.as_path()) && p.is_dir() && p.join("Login Data").is_file())
            .then_some(p)
    };
    state["profile"]["last_used"]
        .as_str()
        .and_then(valid)
        .or_else(|| valid("Default"))
}

#[derive(Clone)]
pub struct StoredLogin {
    pub id: i64,
    pub realm: String,
    pub username: String,
    pub password: Option<String>,
    blob: Vec<u8>,
    blocked: bool,
}

/// One profile resolved once. Its in-memory rows are a consistent SQLite snapshot.
#[derive(Clone)]
pub struct Store {
    pub browser: String,
    db: PathBuf,
    key: Vec<u8>,
    pub rows: Arc<Vec<StoredLogin>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginMatch {
    Exact,
    Different,
    Missing,
    Protected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteStatus {
    Saved,
    Unchanged,
    Locked,
    Conflict,
    Unsupported,
    Failed,
    Disabled,
    Cancelled,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    pub browser: String,
    pub status: WriteStatus,
    pub message: String,
}

fn db_error(e: rusqlite::Error) -> ActionError {
    // SQL errors may contain values or trigger text. Never return them verbatim.
    let message = match e.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            "password database is locked"
        }
        Some(
            rusqlite::ErrorCode::CannotOpen
            | rusqlite::ErrorCode::ReadOnly
            | rusqlite::ErrorCode::PermissionDenied,
        ) => "password database is unavailable",
        _ => "password database operation failed",
    };
    ActionError::Failed(message.into())
}

fn read_rows(conn: &Connection, key: &[u8]) -> Result<Vec<StoredLogin>, ActionError> {
    let mut stmt = conn.prepare("SELECT id, signon_realm, username_value, password_value, blacklisted_by_user, scheme FROM logins ORDER BY id").map_err(db_error)?;
    let raw = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })
        .map_err(db_error)?;
    let mut rows = Vec::new();
    for row in raw {
        let (id, realm, username, blob, blocked, scheme) = row.map_err(db_error)?;
        // Non-HTML / federated credentials are not imported or overwritten.
        let canonical = realm_of(&realm);
        if canonical.is_empty() || scheme != 0 {
            continue;
        }
        let blocked = blocked != 0;
        let password = (!blocked)
            .then(|| decrypt_password(key, &blob).ok())
            .flatten();
        rows.push(StoredLogin {
            id,
            realm: canonical,
            username,
            password,
            blob,
            blocked,
        });
    }
    Ok(rows)
}

impl Store {
    pub fn open(browser: &str) -> Result<Self, ActionError> {
        let data = user_data_dir(browser)
            .ok_or_else(|| ActionError::Failed("unsupported browser password store".into()))?;
        Self::open_in_user_data(browser, &data)
    }

    fn open_in_user_data(browser: &str, data: &Path) -> Result<Self, ActionError> {
        let profile = selected_profile(data)
            .ok_or_else(|| ActionError::Failed("no valid last-used browser profile".into()))?;
        let db = profile
            .join("Login Data")
            .canonicalize()
            .map_err(|_| ActionError::Failed("password database is unavailable".into()))?;
        if db.parent() != Some(profile.as_path()) {
            return Err(ActionError::Failed(
                "unsupported password database path".into(),
            ));
        }
        Self::from_path(browser, db, os_crypt_key(data)?)
    }

    fn from_path(browser: &str, db: PathBuf, key: Vec<u8>) -> Result<Self, ActionError> {
        let mut conn =
            Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(db_error)?;
        conn.busy_timeout(Duration::ZERO).map_err(db_error)?;
        let tx = conn.transaction().map_err(db_error)?;
        let rows = read_rows(&tx, &key)?;
        tx.commit().map_err(db_error)?;
        Ok(Self {
            browser: browser.into(),
            db,
            key,
            rows: Arc::new(rows),
        })
    }

    fn account<'a>(&'a self, realm: &str, username: &str) -> Vec<&'a StoredLogin> {
        self.rows
            .iter()
            .filter(|r| r.realm == realm && (r.username == username || r.blocked))
            .collect()
    }

    pub fn classify(&self, realm: &str, username: &str, password: &str) -> LoginMatch {
        classify(&self.account(realm, username), password)
    }

    /// Approval applies to the original snapshot, never to a changed database row.
    /// SQLite IMMEDIATE serializes writers; the callback makes cancellation and
    /// current settings authoritative until the transaction starts.
    pub fn write(
        &self,
        realm: &str,
        original_username: &str,
        username: &str,
        password: &str,
        override_existing: bool,
        allowed: impl Fn() -> bool,
    ) -> WriteResult {
        let serial = store_lock(&self.db);
        let _guard = serial
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = self.write_inner(
            realm,
            original_username,
            username,
            password,
            override_existing,
            allowed,
        );
        match result {
            Ok(status) => WriteResult {
                browser: self.browser.clone(),
                status,
                message: match status {
                    WriteStatus::Saved => "saved",
                    WriteStatus::Unchanged => "unchanged",
                    WriteStatus::Conflict => "different password skipped",
                    WriteStatus::Cancelled => "cancelled",
                    WriteStatus::Unsupported => "protected or undecryptable entry",
                    _ => "skipped",
                }
                .into(),
            },
            Err(e) => failure(&self.browser, &e.to_string()),
        }
    }

    fn write_inner(
        &self,
        realm: &str,
        original_username: &str,
        username: &str,
        password: &str,
        override_existing: bool,
        allowed: impl Fn() -> bool,
    ) -> Result<WriteStatus, ActionError> {
        if password.is_empty() || realm.is_empty() || realm_of(realm) != realm {
            return Err(ActionError::Invalid(
                "invalid password or sign-on realm".into(),
            ));
        }
        let expected = self.account(realm, original_username);
        let kind = classify(&expected, password);
        if kind == LoginMatch::Protected {
            return Ok(WriteStatus::Unsupported);
        }
        if !override_existing
            && (kind == LoginMatch::Different
                || (!expected.is_empty() && original_username != username))
        {
            return Ok(WriteStatus::Conflict);
        }
        if !allowed() {
            return Ok(WriteStatus::Cancelled);
        }
        let mut conn = Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(db_error)?;
        conn.busy_timeout(Duration::ZERO).map_err(db_error)?;
        if !allowed() {
            return Ok(WriteStatus::Cancelled);
        }
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let current = read_rows(&tx, &self.key)?;
        let observed: Vec<_> = current
            .iter()
            .filter(|r| r.realm == realm && (r.username == original_username || r.blocked))
            .collect();
        if !same_rows(&expected, &observed) {
            // Browser-native auto-save of this exact pair needs no replacement.
            if original_username == username && classify(&observed, password) == LoginMatch::Exact {
                return Ok(WriteStatus::Unchanged);
            }
            return Ok(WriteStatus::Conflict);
        }
        if original_username != username
            && current
                .iter()
                .any(|r| r.realm == realm && r.username == username)
        {
            return Err(ActionError::Invalid(
                "that username already has an entry for this site".into(),
            ));
        }
        if kind == LoginMatch::Exact && original_username == username {
            return Ok(WriteStatus::Unchanged);
        }
        let enc = encrypt_password(&self.key, password)?;
        let now = chrome_time_now();
        if expected.is_empty() {
            tx.execute("INSERT INTO logins (origin_url, action_url, username_element, username_value, password_element, password_value, submit_element, signon_realm, date_created, blacklisted_by_user, scheme, password_type, times_used, form_data, display_name, icon_url, federation_url, skip_zero_click, generation_upload_status, possible_username_pairs, date_last_used, date_password_modified) VALUES (?1, ?1, '', ?2, '', ?3, '', ?1, ?4, 0, 0, 0, 1, X'00000000', '', '', '', 0, 0, X'00000000', ?4, ?4)", params![realm, username, enc, now]).map_err(db_error)?;
        } else {
            for row in expected {
                tx.execute("UPDATE logins SET username_value = ?1, password_value = ?2, date_password_modified = ?3, date_last_used = ?3 WHERE id = ?4", params![username, enc, now, row.id]).map_err(db_error)?;
            }
        }
        tx.commit().map_err(db_error)?;
        Ok(WriteStatus::Saved)
    }

    /// Refresh only explicitly saved entries, keeping the same resolved profile.
    pub fn refresh(&self) -> Result<Self, ActionError> {
        Self::from_path(&self.browser, self.db.clone(), self.key.clone())
    }
}

fn store_lock(path: &Path) -> Arc<Mutex<()>> {
    type Locks = Mutex<std::collections::HashMap<PathBuf, std::sync::Weak<Mutex<()>>>>;
    static LOCKS: OnceLock<Locks> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(path).and_then(std::sync::Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(path.to_owned(), Arc::downgrade(&lock));
    lock
}

fn classify(rows: &[&StoredLogin], password: &str) -> LoginMatch {
    if rows.is_empty() {
        LoginMatch::Missing
    } else if rows.iter().any(|r| r.password.is_none() || r.blocked) {
        LoginMatch::Protected
    } else if rows.iter().all(|r| r.password.as_deref() == Some(password)) {
        LoginMatch::Exact
    } else {
        LoginMatch::Different
    }
}

fn same_rows(a: &[&StoredLogin], b: &[&StoredLogin]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.id == b.id
                && a.username == b.username
                && a.realm == b.realm
                && a.blob == b.blob
                && a.blocked == b.blocked
        })
}

pub fn failure(browser: &str, message: &str) -> WriteResult {
    let status = if message.contains("locked") {
        WriteStatus::Locked
    } else if message.contains("unsupported")
        || message.contains("encryption")
        || message.contains("key")
        || message.contains("profile")
    {
        WriteStatus::Unsupported
    } else {
        WriteStatus::Failed
    };
    WriteResult {
        browser: browser.into(),
        status,
        message: message.into(),
    }
}

pub fn lookup(
    caps: &Capabilities,
    domain: &str,
    prefer: Option<&str>,
    only: &[String],
) -> Result<Vec<Login>, ActionError> {
    let mut browsers: Vec<_> = caps
        .browsers
        .iter()
        .filter(|b| only.contains(&b.id))
        .collect();
    browsers.sort_by_key(|b| if Some(b.id.as_str()) == prefer { 0 } else { 1 });
    let mut found = Vec::new();
    let mut errors = Vec::new();
    for b in browsers {
        match Store::open(&b.id) {
            Ok(store) => {
                for row in store.rows.iter() {
                    if url_matches(&row.realm, domain)
                        && let Some(password) = row.password.clone()
                    {
                        found.push(Login {
                            username: row.username.clone(),
                            password,
                            browser: b.id.clone(),
                        });
                    }
                }
            }
            Err(e) => errors.push(e.to_string()),
        }
    }
    if found.is_empty() && !errors.is_empty() {
        return Err(ActionError::Failed(format!(
            "no readable password: {}",
            errors.join("; ")
        )));
    }
    Ok(found)
}

#[cfg(windows)]
fn dpapi_unprotect(data: &[u8]) -> Result<Vec<u8>, ActionError> {
    use std::ptr;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData};

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(ActionError::Failed(
            "could not unlock the browser password key (Windows DPAPI)".into(),
        ));
    }
    let slice = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let out = slice.to_vec();
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(out)
}

#[cfg(not(windows))]
fn dpapi_unprotect(_data: &[u8]) -> Result<Vec<u8>, ActionError> {
    Err(ActionError::Failed(
        "browser passwords are only supported on Windows".into(),
    ))
}

fn os_crypt_key(user_data: &Path) -> Result<Vec<u8>, ActionError> {
    let text = std::fs::read_to_string(user_data.join("Local State"))
        .map_err(|_| ActionError::Failed("could not read the browser Local State file".into()))?;
    let state: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| ActionError::Failed("browser Local State is not valid JSON".into()))?;
    let b64 = state["os_crypt"]["encrypted_key"]
        .as_str()
        .ok_or_else(|| ActionError::Failed("browser has no os_crypt key".into()))?;
    let raw = B64
        .decode(b64)
        .map_err(|_| ActionError::Failed("browser os_crypt key is not valid base64".into()))?;
    // "DPAPI" prefix (5 bytes), then the DPAPI blob. App-bound ("APPB") keys
    // need Chrome's elevation service; we cannot unlock those here.
    if raw.starts_with(b"APPB") {
        return Err(ActionError::Failed(
            "unsupported browser app-bound encryption".into(),
        ));
    }
    let blob = raw
        .strip_prefix(b"DPAPI")
        .ok_or_else(|| ActionError::Failed("unexpected browser key format".into()))?;
    dpapi_unprotect(blob)
}

fn decrypt_password(key: &[u8], blob: &[u8]) -> Result<String, ActionError> {
    if blob.len() < 3 + 12 + 16 {
        return Err(ActionError::Failed("password blob too short".into()));
    }
    if !blob.starts_with(b"v10") && !blob.starts_with(b"v11") {
        return Err(ActionError::Failed(
            "unsupported password encryption".into(),
        ));
    }
    let nonce = Nonce::from_slice(&blob[3..15]);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| ActionError::Failed("bad browser AES key".into()))?;
    let plain = cipher
        .decrypt(nonce, &blob[15..])
        .map_err(|_| ActionError::Failed("could not decrypt a saved password".into()))?;
    String::from_utf8(plain).map_err(|_| ActionError::Failed("password was not UTF-8".into()))
}

fn encrypt_password(key: &[u8], password: &str) -> Result<Vec<u8>, ActionError> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| ActionError::Failed("bad browser AES key".into()))?;
    let mut nonce_bytes = [0u8; 12];
    getrandom_nonce(&mut nonce_bytes)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, password.as_bytes())
        .map_err(|_| ActionError::Failed("could not encrypt password".into()))?;
    let mut out = Vec::with_capacity(3 + 12 + ct.len());
    out.extend_from_slice(b"v10");
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn getrandom_nonce(buf: &mut [u8]) -> Result<(), ActionError> {
    getrandom::fill(buf)
        .map_err(|_| ActionError::Failed("no secure randomness for password encrypt".into()))
}

fn chrome_time_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
        + 11_644_473_600_000_000
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        dir: PathBuf,
        db: PathBuf,
        key: Vec<u8>,
    }
    impl Fixture {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("sidekick-password-test-{}", ulid::Ulid::new()));
            std::fs::create_dir_all(&dir).unwrap();
            let db = dir.join("Login Data");
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch("CREATE TABLE logins (id INTEGER PRIMARY KEY, origin_url TEXT, action_url TEXT, username_element TEXT, username_value TEXT, password_element TEXT, password_value BLOB, submit_element TEXT, signon_realm TEXT, date_created INTEGER, blacklisted_by_user INTEGER, scheme INTEGER, password_type INTEGER, times_used INTEGER, form_data BLOB, date_synced INTEGER, display_name TEXT, icon_url TEXT, federation_url TEXT, skip_zero_click INTEGER, generation_upload_status INTEGER, possible_username_pairs BLOB, date_last_used INTEGER, date_password_modified INTEGER);").unwrap();
            Self {
                dir,
                db,
                key: vec![7; 32],
            }
        }
        fn insert(&self, url: &str, username: &str, password: &str) {
            Connection::open(&self.db).unwrap().execute("INSERT INTO logins(origin_url,signon_realm,username_value,password_value,blacklisted_by_user,scheme) VALUES (?1,?2,?3,?4,0,0)", params![url,realm_of(url),username,encrypt_password(&self.key,password).unwrap()]).unwrap();
        }
        fn store(&self) -> Store {
            Store::from_path("fixture", self.db.clone(), self.key.clone()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn crypto_and_realms() {
        let key = [7; 32];
        assert_eq!(
            decrypt_password(&key, &encrypt_password(&key, "secret").unwrap()).unwrap(),
            "secret"
        );
        assert!(decrypt_password(&key, b"v20unsupported").is_err());
        assert_eq!(
            realm_of("https://EXAMPLE.com:443/login?a=1"),
            "https://example.com/"
        );
        assert_eq!(realm_of("file:///secret"), "");
        assert!(url_matches("https://gist.github.com/", "github.com"));
        assert!(!url_matches("https://github.com.evil.io/", "github.com"));
        assert!(!url_matches("https://com/", "example.com"));
    }

    #[test]
    fn update_preserves_url_and_other_accounts_and_requires_approval() {
        let f = Fixture::new();
        f.insert("https://example.com/login", "alice", "old");
        f.insert("https://other.test/login", "bob", "keep");
        let s = f.store();
        assert_eq!(
            s.write(
                "https://example.com/",
                "alice",
                "alice",
                "new",
                false,
                || true
            )
            .status,
            WriteStatus::Conflict
        );
        assert_eq!(
            s.write(
                "https://example.com/",
                "alice",
                "alice",
                "new",
                true,
                || true
            )
            .status,
            WriteStatus::Saved
        );
        let fresh = f.store();
        assert_eq!(fresh.rows.len(), 2);
        assert_eq!(
            fresh.classify("https://other.test/", "bob", "keep"),
            LoginMatch::Exact
        );
        let url: String = Connection::open(&f.db)
            .unwrap()
            .query_row(
                "SELECT origin_url FROM logins WHERE username_value='alice'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(url, "https://example.com/login");
        assert_eq!(
            s.write(
                "https://example.com/",
                "alice",
                "alice",
                "newer",
                true,
                || true
            )
            .status,
            WriteStatus::Conflict
        );
    }

    #[test]
    fn collision_cancel_protected_and_sql_rollback() {
        let f = Fixture::new();
        f.insert("https://example.com/login", "alice", "old");
        f.insert("https://example.com/login", "bob", "other");
        let s = f.store();
        assert_eq!(
            s.write("https://example.com/", "alice", "bob", "new", true, || true)
                .status,
            WriteStatus::Failed
        );
        assert_eq!(
            s.write(
                "https://example.com/",
                "alice",
                "alice",
                "new",
                true,
                || false
            )
            .status,
            WriteStatus::Cancelled
        );
        let c = Connection::open(&f.db).unwrap();
        c.execute_batch("CREATE TRIGGER refuse BEFORE UPDATE ON logins BEGIN SELECT RAISE(ABORT, 'private trigger text'); END;").unwrap();
        let result = s.write(
            "https://example.com/",
            "alice",
            "alice",
            "new",
            true,
            || true,
        );
        assert_eq!(result.status, WriteStatus::Failed);
        assert!(!result.message.contains("private"));
        assert_eq!(
            f.store().classify("https://example.com/", "alice", "old"),
            LoginMatch::Exact
        );

        c.execute_batch("DROP TRIGGER refuse; UPDATE logins SET password_value=X'763230' WHERE username_value='bob';").unwrap();
        assert_eq!(
            f.store()
                .write("https://example.com/", "bob", "bob", "new", true, || true)
                .status,
            WriteStatus::Unsupported
        );
    }

    #[test]
    fn wal_snapshot_locks_and_missing_entry_races() {
        let f = Fixture::new();
        let mut c = Connection::open(&f.db).unwrap();
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        f.insert("https://example.com/login", "alice", "old");
        let snapshot = f.store();
        assert_eq!(
            snapshot.classify("https://example.com/", "alice", "old"),
            LoginMatch::Exact
        );
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert_eq!(
            snapshot
                .write(
                    "https://example.com/",
                    "alice",
                    "alice",
                    "new",
                    true,
                    || true
                )
                .status,
            WriteStatus::Locked
        );
        tx.rollback().unwrap();
        f.insert("https://new.test/login", "eve", "external");
        assert_eq!(
            snapshot
                .write("https://new.test/", "eve", "eve", "new", false, || true)
                .status,
            WriteStatus::Conflict
        );
        assert_eq!(snapshot.rows.len(), 1);
        assert_eq!(
            f.store()
                .write(
                    "https://missing.test/",
                    "jane",
                    "jane",
                    "new",
                    false,
                    || true
                )
                .status,
            WriteStatus::Saved
        );
    }

    #[cfg(windows)]
    #[test]
    fn disposable_profile_uses_native_dpapi_for_local_save_fill_and_edit() {
        use std::ptr;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData};
        let f = Fixture::new();
        let data = f.dir.join("User Data");
        let profile = data.join("Default");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::copy(&f.db, profile.join("Login Data")).unwrap();
        let input = CRYPT_INTEGER_BLOB {
            cbData: f.key.len() as u32,
            pbData: f.key.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null(),
                0,
                &mut output,
            )
        };
        assert_ne!(ok, 0);
        let mut raw = b"DPAPI".to_vec();
        raw.extend_from_slice(unsafe {
            std::slice::from_raw_parts(output.pbData, output.cbData as usize)
        });
        unsafe {
            LocalFree(output.pbData as _);
        }
        std::fs::write(data.join("Local State"), serde_json::to_vec(&serde_json::json!({ "profile": { "last_used": "Default" }, "os_crypt": { "encrypted_key": B64.encode(raw) } })).unwrap()).unwrap();
        let initial = Store::open_in_user_data("fixture", &data).unwrap();
        assert_eq!(
            initial
                .write(
                    "https://example.test/",
                    "alice",
                    "alice",
                    "first",
                    false,
                    || true
                )
                .status,
            WriteStatus::Saved
        );
        let saved = Store::open_in_user_data("fixture", &data).unwrap();
        assert_eq!(
            saved.classify("https://example.test/", "alice", "first"),
            LoginMatch::Exact
        );
        assert_eq!(
            saved
                .write(
                    "https://example.test/",
                    "alice",
                    "alice",
                    "second",
                    false,
                    || true
                )
                .status,
            WriteStatus::Conflict
        );
        assert_eq!(
            saved
                .write(
                    "https://example.test/",
                    "alice",
                    "renamed",
                    "second",
                    true,
                    || true
                )
                .status,
            WriteStatus::Saved
        );
        let edited = Store::open_in_user_data("fixture", &data).unwrap();
        assert_eq!(edited.rows.len(), 1);
        assert_eq!(
            edited.classify("https://example.test/", "renamed", "second"),
            LoginMatch::Exact
        );
    }

    #[test]
    fn inserts_into_current_chromium_schema_without_date_synced() {
        let fixture = Fixture::new();
        Connection::open(&fixture.db)
            .unwrap()
            .execute_batch("ALTER TABLE logins DROP COLUMN date_synced;")
            .unwrap();
        let store = fixture.store();
        assert_eq!(
            store
                .write(
                    "https://example.test/",
                    "alice",
                    "alice",
                    "dummy",
                    false,
                    || true
                )
                .status,
            WriteStatus::Saved
        );
        assert_eq!(
            fixture
                .store()
                .classify("https://example.test/", "alice", "dummy"),
            LoginMatch::Exact
        );
    }

    /// Opt-in integration check for a browser-created, disposable profile only.
    #[cfg(windows)]
    #[test]
    #[ignore = "requires an isolated browser-created profile and browser restart harness"]
    fn browser_created_profile_save_edit_restart() {
        let data = PathBuf::from(
            std::env::var_os("SIDEKICK_PASSWORD_TEST_PROFILE").expect("profile required"),
        )
        .canonicalize()
        .unwrap();
        let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap();
        assert_eq!(data.parent(), Some(target.as_path()));
        assert!(
            data.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("password-browser-live-")
        );
        let mode = std::env::var("SIDEKICK_PASSWORD_TEST_MODE").unwrap();
        let store = Store::open_in_user_data("disposable-browser", &data).unwrap();
        let realm = std::env::var("SIDEKICK_PASSWORD_TEST_ORIGIN").unwrap();
        let origin = url::Url::parse(&realm).unwrap();
        assert_eq!(origin.host_str(), Some("127.0.0.1"));
        assert_eq!(origin.scheme(), "http");
        let realm = realm.as_str();
        let native: Vec<_> = store
            .rows
            .iter()
            .filter(|r| r.username == "native-fixture")
            .collect();
        assert_eq!(native.len(), 1, "browser-native seed must exist");
        let baseline = data.join("native-ciphertext.hex");
        let native_blob = B64.encode(&native[0].blob);
        if mode == "save" {
            std::fs::write(&baseline, &native_blob).unwrap();
            assert_eq!(
                store
                    .write(
                        realm,
                        "sidekick-fixture",
                        "sidekick-fixture",
                        "dummy-first-123!",
                        false,
                        || true
                    )
                    .status,
                WriteStatus::Saved
            );
        } else {
            assert_eq!(
                std::fs::read_to_string(&baseline).unwrap(),
                native_blob,
                "unrelated native password changed"
            );
            let expected = if mode == "final" {
                "dummy-edited-456!"
            } else {
                "dummy-first-123!"
            };
            assert_eq!(
                store.classify(realm, "sidekick-fixture", expected),
                LoginMatch::Exact
            );
            if mode == "edit" {
                assert_eq!(
                    store
                        .write(
                            realm,
                            "sidekick-fixture",
                            "sidekick-fixture",
                            "dummy-edited-456!",
                            false,
                            || true
                        )
                        .status,
                    WriteStatus::Conflict
                );
                assert_eq!(
                    store
                        .write(
                            realm,
                            "sidekick-fixture",
                            "sidekick-fixture",
                            "dummy-edited-456!",
                            true,
                            || true
                        )
                        .status,
                    WriteStatus::Saved
                );
            }
        }
        let conn =
            Connection::open_with_flags(&store.db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
    }

    #[test]
    fn mirror_snapshots_preserve_conflicting_sources_until_explicit_override() {
        let a = Fixture::new();
        let b = Fixture::new();
        let empty = Fixture::new();
        a.insert("https://example.test/login", "alice", "old");
        b.insert("https://example.test/login", "alice", "new");
        let snapshots = [a.store(), b.store(), empty.store()];
        assert_eq!(
            snapshots[1]
                .write(
                    "https://example.test/",
                    "alice",
                    "alice",
                    "old",
                    false,
                    || true
                )
                .status,
            WriteStatus::Conflict
        );
        assert_eq!(
            b.store().classify("https://example.test/", "alice", "new"),
            LoginMatch::Exact
        );
        assert_eq!(
            snapshots[1]
                .write(
                    "https://example.test/",
                    "alice",
                    "alice",
                    "old",
                    true,
                    || true
                )
                .status,
            WriteStatus::Saved
        );
        assert_eq!(
            snapshots[2]
                .write(
                    "https://example.test/",
                    "alice",
                    "alice",
                    "old",
                    true,
                    || true
                )
                .status,
            WriteStatus::Saved
        );
        assert_eq!(
            snapshots[1].classify("https://example.test/", "alice", "new"),
            LoginMatch::Exact
        );
        assert_eq!(
            b.store().classify("https://example.test/", "alice", "old"),
            LoginMatch::Exact
        );
        assert_eq!(
            empty
                .store()
                .classify("https://example.test/", "alice", "old"),
            LoginMatch::Exact
        );
    }

    #[test]
    fn account_rename_requires_explicit_override_and_keeps_original_id() {
        let f = Fixture::new();
        f.insert("https://example.test/login", "alice", "same");
        let snapshot = f.store();
        assert_eq!(
            snapshot
                .write(
                    "https://example.test/",
                    "alice",
                    "new-name",
                    "same",
                    false,
                    || true
                )
                .status,
            WriteStatus::Conflict
        );
        assert_eq!(
            snapshot
                .write(
                    "https://example.test/",
                    "alice",
                    "new-name",
                    "same",
                    true,
                    || true
                )
                .status,
            WriteStatus::Saved
        );
        let fresh = f.store();
        assert_eq!(fresh.rows.len(), 1);
        assert_eq!(fresh.rows[0].id, snapshot.rows[0].id);
        assert_eq!(fresh.rows[0].username, "new-name");
    }

    #[test]
    fn last_used_profile_cannot_escape_root() {
        let f = Fixture::new();
        let data = f.dir.join("User Data");
        std::fs::create_dir_all(data.join("Default")).unwrap();
        std::fs::create_dir_all(data.join("Profile 2")).unwrap();
        std::fs::write(data.join("Default/Login Data"), []).unwrap();
        std::fs::write(data.join("Profile 2/Login Data"), []).unwrap();
        std::fs::write(
            data.join("Local State"),
            r#"{"profile":{"last_used":"Profile 2"}}"#,
        )
        .unwrap();
        assert!(selected_profile(&data).unwrap().ends_with("Profile 2"));
        std::fs::write(
            data.join("Local State"),
            r#"{"profile":{"last_used":"../outside"}}"#,
        )
        .unwrap();
        assert!(selected_profile(&data).unwrap().ends_with("Default"));
    }
}
