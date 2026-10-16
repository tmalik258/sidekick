//! Mail over IMAP, for accounts without Google sign-in (Advanced). One short
//! connection per check: log in, look at what arrived since the last check,
//! read only the headers, log out. The password lives in the keychain. The
//! connection is TLS with certificate checks and fixed timeouts; there is no
//! plaintext fallback.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::mail::{Mail, MailError, MailSource, parse_message};

/// Credential Manager name for the app password.
pub const SECRET: &str = "imap.password";
const TIMEOUT: Duration = Duration::from_secs(20);
/// Newest messages looked at per check.
const MAX_PER_SYNC: usize = 25;
/// A reply line or literal longer than this is refused.
const MAX_LINE: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapAccount {
    pub host: String,
    pub port: u16,
    pub user: String,
}

pub struct Imap {
    account: ImapAccount,
    password: String,
    /// Where to look next. None until the first check, which only notes
    /// where the mailbox is, so old mail is never replayed.
    next_uid: Option<u32>,
}

impl Imap {
    pub fn new(account: ImapAccount, password: String) -> Self {
        Self {
            account,
            password,
            next_uid: None,
        }
    }
}

fn offline(what: &str, e: impl std::fmt::Display) -> MailError {
    MailError::Offline(format!("{what}: {e}"))
}

async fn connect(
    account: &ImapAccount,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, MailError> {
    let roots = rustls_roots();
    let config = tokio_rustls::rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = tokio_rustls::rustls::pki_types::ServerName::try_from(account.host.clone())
        .map_err(|_| MailError::Other("that mail server name is not valid".into()))?;
    let tcp = tokio::time::timeout(
        TIMEOUT,
        tokio::net::TcpStream::connect((account.host.as_str(), account.port)),
    )
    .await
    .map_err(|_| offline("connecting", "timed out"))?
    .map_err(|e| offline("connecting", e))?;
    tokio::time::timeout(
        TIMEOUT,
        tokio_rustls::TlsConnector::from(std::sync::Arc::new(config)).connect(name, tcp),
    )
    .await
    .map_err(|_| offline("secure connection", "timed out"))?
    .map_err(|e| MailError::Other(format!("secure connection failed: {e}")))
}

fn rustls_roots() -> tokio_rustls::rustls::RootCertStore {
    let mut roots = tokio_rustls::rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    roots
}

impl MailSource for Imap {
    async fn poll(&mut self) -> Result<Vec<Mail>, MailError> {
        let stream = connect(&self.account).await?;
        let (found, next) = tokio::time::timeout(
            TIMEOUT * 3,
            check(stream, &self.account.user, &self.password, self.next_uid),
        )
        .await
        .map_err(|_| offline("reading mail", "timed out"))??;
        let first = self.next_uid.is_none();
        self.next_uid = Some(next);
        if first {
            return Ok(Vec::new());
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        Ok(found
            .iter()
            .filter_map(|m| parse_message(m, &self.account.user, now_ms))
            .filter(|m| m.unread)
            .collect())
    }

    async fn disconnect(&mut self) {
        self.password.clear();
        self.next_uid = None;
        crate::secrets::delete(SECRET);
    }
}

// ---- The protocol ---------------------------------------------------------------

struct Session<S> {
    io: BufReader<S>,
    tag: u32,
}

/// One reply: the untagged lines (with any literals joined in) and whether
/// the tagged line said OK.
struct Reply {
    lines: Vec<String>,
    ok: bool,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    async fn line(&mut self) -> Result<String, MailError> {
        let mut buf = String::new();
        let n = (&mut self.io)
            .take(MAX_LINE as u64)
            .read_line(&mut buf)
            .await
            .map_err(|e| offline("reading", e))?;
        if n == 0 {
            return Err(offline("reading", "the server closed the connection"));
        }
        Ok(buf)
    }

    /// Sends a command and reads until its tagged reply.
    async fn run(&mut self, command: &str) -> Result<Reply, MailError> {
        self.tag += 1;
        let tag = format!("D{}", self.tag);
        self.io
            .write_all(format!("{tag} {command}\r\n").as_bytes())
            .await
            .map_err(|e| offline("writing", e))?;
        let mut lines = Vec::new();
        loop {
            let mut line = self.line().await?;
            // A literal `{n}` follows: its n bytes belong to this line.
            while let Some(n) = literal_len(&line) {
                if n > MAX_LINE {
                    return Err(MailError::Other("the server sent too much".into()));
                }
                let mut data = vec![0u8; n];
                tokio::io::AsyncReadExt::read_exact(&mut self.io, &mut data)
                    .await
                    .map_err(|e| offline("reading", e))?;
                line.push_str(&String::from_utf8_lossy(&data));
                line.push_str(&self.line().await?);
            }
            if let Some(rest) = line.strip_prefix(&format!("{tag} ")) {
                return Ok(Reply {
                    ok: rest.trim().to_uppercase().starts_with("OK"),
                    lines,
                });
            }
            lines.push(line);
        }
    }
}

fn literal_len(line: &str) -> Option<usize> {
    let t = line.trim_end();
    let inner = t.strip_suffix('}')?;
    inner[inner.rfind('{')? + 1..].parse().ok()
}

/// A quoted IMAP string. Refuses anything that could end the command early.
fn quote(s: &str) -> Result<String, MailError> {
    if s.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
        return Err(MailError::Other("that value cannot be used".into()));
    }
    Ok(format!(
        "\"{}\"",
        s.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// Logs in, reads headers of unseen mail from `from_uid` on, logs out.
/// Returns Gmail-shaped records for `parse_message` and the next UID.
async fn check<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    user: &str,
    password: &str,
    from_uid: Option<u32>,
) -> Result<(Vec<Value>, u32), MailError> {
    let mut s = Session {
        io: BufReader::new(stream),
        tag: 0,
    };
    let greeting = s.line().await?;
    if !greeting.starts_with("* OK") && !greeting.starts_with("* PREAUTH") {
        return Err(MailError::Other("that is not an IMAP server".into()));
    }
    let login = s
        .run(&format!("LOGIN {} {}", quote(user)?, quote(password)?))
        .await?;
    if !login.ok {
        // Never echo the server's words: they can name the account.
        return Err(MailError::AuthExpired);
    }
    let select = s.run("SELECT INBOX").await?;
    if !select.ok {
        return Err(MailError::Other("could not open the inbox".into()));
    }
    let uid_next = select
        .lines
        .iter()
        .find_map(|l| {
            let at = l.find("UIDNEXT ")? + 8;
            l[at..]
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse::<u32>()
                .ok()
        })
        .ok_or_else(|| MailError::Other("the server did not say where the inbox ends".into()))?;
    let Some(start) = from_uid else {
        let _ = s.run("LOGOUT").await;
        return Ok((Vec::new(), uid_next));
    };
    let found = s.run(&format!("UID SEARCH UNSEEN UID {start}:*")).await?;
    let mut uids: Vec<u32> = found
        .lines
        .iter()
        .filter_map(|l| l.strip_prefix("* SEARCH"))
        .flat_map(|rest| rest.split_whitespace().filter_map(|n| n.parse().ok()))
        // `n:*` always includes the last message, even when it is older.
        .filter(|u| *u >= start)
        .collect();
    uids.sort_unstable();
    if uids.len() > MAX_PER_SYNC {
        uids.drain(..uids.len() - MAX_PER_SYNC);
    }
    let mut out = Vec::new();
    for uid in uids {
        let fetched = s
            .run(&format!(
                "UID FETCH {uid} (INTERNALDATE BODY.PEEK[HEADER.FIELDS (FROM TO DELIVERED-TO \
                 SUBJECT LIST-UNSUBSCRIBE AUTO-SUBMITTED)])"
            ))
            .await?;
        if fetched.ok
            && let Some(msg) = message_of(uid, &fetched.lines.join(""))
        {
            out.push(msg);
        }
    }
    let _ = s.run("LOGOUT").await;
    // Only move on after everything was read.
    Ok((out, uid_next))
}

/// A FETCH answer as the record `parse_message` reads. Messages found by an
/// UNSEEN search are unread by definition.
fn message_of(uid: u32, text: &str) -> Option<Value> {
    let mut headers: Vec<Value> = Vec::new();
    for raw in text.lines() {
        if raw.starts_with([' ', '\t']) {
            // Folded header: continues the one before.
            if let Some(last) = headers.last_mut() {
                let more = raw.trim();
                let joined = format!("{} {more}", last["value"].as_str().unwrap_or_default());
                last["value"] = Value::String(joined);
            }
        } else if let Some((name, value)) = raw.split_once(':')
            && !name.contains(' ')
            && !name.starts_with('*')
        {
            headers.push(json!({ "name": name, "value": value.trim() }));
        }
    }
    let sent_ms = internal_date_ms(text);
    Some(json!({
        "id": format!("imap:{uid}"),
        "labelIds": ["UNREAD", "INBOX"],
        "internalDate": sent_ms.map(|m| m.to_string()),
        "payload": { "headers": headers },
    }))
}

/// `INTERNALDATE "17-Jul-2026 09:30:00 +0000"` in milliseconds.
fn internal_date_ms(text: &str) -> Option<i64> {
    let at = text.find("INTERNALDATE \"")? + 14;
    let end = text[at..].find('"')? + at;
    chrono::DateTime::parse_from_str(text[at..end].trim(), "%d-%b-%Y %H:%M:%S %z")
        .ok()
        .map(|d| d.timestamp_millis())
}

/// Reads the saved account and password, if both exist.
pub fn saved(settings: &duskie_core::MailAccountSettings) -> Option<Imap> {
    if settings.host.is_empty() || settings.user.is_empty() {
        return None;
    }
    Some(Imap::new(
        ImapAccount {
            host: settings.host.clone(),
            port: settings.port,
            user: settings.user.clone(),
        },
        crate::secrets::get(SECRET)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex};

    /// A scripted server: answers each command the way a real one does.
    async fn serve(
        stream: tokio::io::DuplexStream,
        password: &'static str,
        unseen: &'static [u32],
    ) {
        let (r, mut w) = tokio::io::split(stream);
        let mut r = BufReader::new(r);
        w.write_all(b"* OK ready\r\n").await.unwrap();
        let mut line = String::new();
        loop {
            line.clear();
            if r.read_line(&mut line).await.unwrap() == 0 {
                return;
            }
            let (tag, cmd) = line.trim_end().split_once(' ').unwrap();
            let reply = if cmd.starts_with("LOGIN") {
                if cmd.contains(password) {
                    format!("{tag} OK logged in\r\n")
                } else {
                    format!("{tag} NO [AUTHENTICATIONFAILED] bad for ali@x.com\r\n")
                }
            } else if cmd.starts_with("SELECT") {
                format!("* 3 EXISTS\r\n* OK [UIDNEXT 40] next\r\n{tag} OK done\r\n")
            } else if cmd.starts_with("UID SEARCH") {
                let list: Vec<String> = unseen.iter().map(u32::to_string).collect();
                format!("* SEARCH {}\r\n{tag} OK done\r\n", list.join(" "))
            } else if cmd.starts_with("UID FETCH") {
                let uid: u32 = cmd.split_whitespace().nth(2).unwrap().parse().unwrap();
                let hdr = "From: Ali <ali@x.com>\r\nTo: me@x.com\r\nSubject: can you review\r\n this today?\r\n\r\n";
                format!(
                    "* 1 FETCH (UID {uid} INTERNALDATE \"17-Jul-2026 09:30:00 +0000\" BODY[HEADER.FIELDS (FROM TO SUBJECT)] {{{}}}\r\n{hdr})\r\n{tag} OK done\r\n",
                    hdr.len()
                )
            } else if cmd.starts_with("LOGOUT") {
                w.write_all(format!("* BYE\r\n{tag} OK bye\r\n").as_bytes())
                    .await
                    .unwrap();
                return;
            } else {
                format!("{tag} BAD unknown\r\n")
            };
            w.write_all(reply.as_bytes()).await.unwrap();
        }
    }

    async fn run(
        password: &'static str,
        unseen: &'static [u32],
        from: Option<u32>,
    ) -> Result<(Vec<Value>, u32), MailError> {
        let (client, server) = duplex(8192);
        tokio::spawn(serve(server, password, unseen));
        check(client, "me@x.com", "secret", from).await
    }

    #[tokio::test]
    async fn the_first_check_only_notes_where_the_inbox_is() {
        let (found, next) = run("secret", &[1, 2, 3], None).await.unwrap();
        assert!(found.is_empty());
        assert_eq!(next, 40);
    }

    #[tokio::test]
    async fn new_unseen_mail_is_read_by_header_only() {
        let (found, next) = run("secret", &[12, 41], Some(40)).await.unwrap();
        // 12 is older than where we left off (the `n:*` rule): skipped.
        assert_eq!(found.len(), 1);
        assert_eq!(next, 40);
        let mail = parse_message(&found[0], "me@x.com", 1_800_000_000_000).unwrap();
        assert_eq!(mail.from, "ali@x.com");
        assert!(mail.to_me && mail.unread && mail.question && !mail.bulk);
    }

    #[tokio::test]
    async fn a_wrong_password_reads_as_sign_in_needed_without_the_servers_words() {
        let err = run("other", &[], Some(1)).await.unwrap_err();
        assert_eq!(err, MailError::AuthExpired);
        assert!(!err.to_string().contains("ali@x.com"));
    }

    #[test]
    fn values_that_could_end_a_command_are_refused() {
        assert!(quote("a\r\nb").is_err());
        assert_eq!(quote("a\"b\\").unwrap(), "\"a\\\"b\\\\\"");
    }

    #[test]
    fn a_closed_connection_is_offline() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (client, server) = duplex(64);
        drop(server);
        let err = rt.block_on(check(client, "a", "b", None)).unwrap_err();
        assert!(matches!(err, MailError::Offline(_)));
    }
}
