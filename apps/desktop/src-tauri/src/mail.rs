//! New mail straight from the account, not from Windows toasts.
//!
//! A `MailSource` hands over new messages as small, content-free records
//! ([`Mail`]): who sent it, and yes or no answers (is it to me, a question,
//! a newsletter). The text of a message is read once to answer those and is
//! never stored or logged. Gmail is the first source: Google sign-in with
//! PKCE on a loopback port, `gmail.readonly`, and incremental sync with
//! `history.list`. Messages already read on another device are skipped.

use std::time::Duration;

use duskie_core::attention::Signal;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mcp_oauth::{self, Cached, Grant};

pub const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
const GOOGLE_AUTH: &str =
    "https://accounts.google.com/o/oauth2/v2/auth?access_type=offline&prompt=consent";
const GOOGLE_TOKEN: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_REVOKE: &str = "https://oauth2.googleapis.com/revoke";
const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
/// Credential Manager name for the Gmail grant.
pub const SECRET: &str = "gmail.grant";
const TIMEOUT: Duration = Duration::from_secs(20);
/// Newest messages looked at per sync, so a long absence stays cheap.
const MAX_PER_SYNC: usize = 25;

/// The Google client Duskie signs in with. Set when building, or in the
/// environment while testing with your own client.
pub fn client_id() -> Option<String> {
    std::env::var("DUSKIE_GOOGLE_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("DUSKIE_GOOGLE_CLIENT_ID").map(str::to_owned))
        .filter(|s| !s.is_empty())
}

fn client_secret() -> String {
    std::env::var("DUSKIE_GOOGLE_CLIENT_SECRET")
        .ok()
        .or_else(|| option_env!("DUSKIE_GOOGLE_CLIENT_SECRET").map(str::to_owned))
        .unwrap_or_default()
}

// ---- What a message becomes -------------------------------------------------

/// A message reduced to what scoring needs. No subject, body or snippet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mail {
    pub id: String,
    /// The sender's address, lower case. Used as the learning key only.
    pub from: String,
    pub unread: bool,
    pub to_me: bool,
    pub question: bool,
    pub deadline: bool,
    /// A newsletter, promotion or automated message.
    pub bulk: bool,
    pub age_mins: u32,
}

const DEADLINE_WORDS: &[&str] = &[
    "deadline",
    "due today",
    "due tomorrow",
    "by tomorrow",
    "by tonight",
    "asap",
    "urgent",
    "end of day",
    "eod",
    "expires",
];

fn has_deadline(text: &str) -> bool {
    let t = text.to_lowercase();
    DEADLINE_WORDS.iter().any(|w| t.contains(w))
}

fn header<'a>(msg: &'a Value, name: &str) -> Option<&'a str> {
    msg["payload"]["headers"]
        .as_array()?
        .iter()
        .find(|h| {
            h["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .and_then(|h| h["value"].as_str())
}

/// `Ali <ali@x.com>` becomes `ali@x.com`.
pub fn address_of(from: &str) -> String {
    let inner = match (from.rfind('<'), from.rfind('>')) {
        (Some(a), Some(b)) if a < b => &from[a + 1..b],
        _ => from,
    };
    inner.trim().to_lowercase()
}

/// One Gmail `messages.get?format=metadata` answer as a [`Mail`]. `me` is
/// the account's own address and `now_ms` the current time.
pub fn parse_message(msg: &Value, me: &str, now_ms: i64) -> Option<Mail> {
    let id = msg["id"].as_str()?.to_owned();
    let labels: Vec<&str> = msg["labelIds"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let from = address_of(header(msg, "From")?);
    let me = me.to_lowercase();
    let to_me = [header(msg, "To"), header(msg, "Delivered-To")]
        .into_iter()
        .flatten()
        .any(|t| t.to_lowercase().contains(&me));
    let bulk = header(msg, "List-Unsubscribe").is_some()
        || header(msg, "Auto-Submitted").is_some_and(|v| v != "no")
        || labels.iter().any(|l| {
            matches!(
                *l,
                "CATEGORY_PROMOTIONS" | "CATEGORY_UPDATES" | "CATEGORY_SOCIAL" | "CATEGORY_FORUMS"
            )
        });
    // Read once, here, for two yes or no answers. Not kept.
    let subject = header(msg, "Subject").unwrap_or_default();
    let snippet = msg["snippet"].as_str().unwrap_or_default();
    let question = subject.contains('?') || snippet.contains('?');
    let deadline = has_deadline(subject) || has_deadline(snippet);
    let sent_ms = msg["internalDate"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(now_ms);
    let age_mins = u32::try_from(((now_ms - sent_ms).max(0)) / 60_000).unwrap_or(u32::MAX);
    Some(Mail {
        id,
        from,
        unread: labels.contains(&"UNREAD"),
        to_me,
        question,
        deadline,
        bulk,
        age_mins,
    })
}

/// The scoring signal for a message. `vips` are names or addresses the user
/// always wants to hear from.
pub fn signal_of(mail: &Mail, vips: &[String]) -> Signal {
    let mut s = Signal::new(&format!("mail:{}", mail.from), "mail");
    s.sender_important = vips
        .iter()
        .any(|v| !v.is_empty() && mail.from.contains(&v.to_lowercase()));
    s.direct_to_me = mail.to_me && !mail.bulk;
    s.question = mail.question && !mail.bulk;
    s.deadline_words = mail.deadline && !mail.bulk;
    s.unanswered_mins = if s.question { mail.age_mins } else { 0 };
    s.bulk = mail.bulk;
    s
}

// ---- Polling ------------------------------------------------------------------

/// How long to wait before looking again: quick while the user is at the PC,
/// slower when idle, and backing off (up to 15 minutes) after failures.
pub fn next_poll(active: bool, failures: u32) -> Duration {
    let base = if active { 20 } else { 90 };
    let backoff = 1u64 << failures.min(6);
    Duration::from_secs((base * backoff).min(15 * 60))
}

/// Where a mail account stands, for the Settings card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailState {
    Disconnected,
    Connected,
    /// The network is down; polling continues with backoff.
    Offline,
    /// The sign-in ran out or was revoked; one click to sign in again.
    AuthExpired,
}

/// What went wrong, as far as the state goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailError {
    Offline(String),
    AuthExpired,
    Other(String),
}

impl MailError {
    pub fn state(&self) -> MailState {
        match self {
            MailError::Offline(_) => MailState::Offline,
            MailError::AuthExpired => MailState::AuthExpired,
            MailError::Other(_) => MailState::Connected,
        }
    }
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MailError::Offline(e) => write!(f, "could not reach the mail service: {e}"),
            MailError::AuthExpired => write!(f, "the sign-in ran out; connect again"),
            MailError::Other(e) => write!(f, "{e}"),
        }
    }
}

/// A place new mail comes from.
pub trait MailSource {
    /// New messages since the last call. Messages already read elsewhere
    /// are left out.
    async fn poll(&mut self) -> Result<Vec<Mail>, MailError>;
    /// Signs out and deletes what was kept.
    async fn disconnect(&mut self);
}

// ---- Gmail --------------------------------------------------------------------

pub struct Gmail {
    http: reqwest::Client,
    api: String,
    grant: Grant,
    token: Cached,
    me: String,
    history_id: Option<String>,
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(TIMEOUT)
        .https_only(false)
        .build()
        .unwrap_or_default()
}

fn offline(e: &reqwest::Error) -> MailError {
    let text = e.to_string();
    let text = match e.url() {
        Some(url) => text.replace(url.as_str(), "the mail server"),
        None => text,
    };
    if e.is_connect() || e.is_timeout() || e.is_request() {
        MailError::Offline(text)
    } else {
        MailError::Other(text)
    }
}

impl Gmail {
    pub fn new(grant: Grant) -> Self {
        Self::with_api(grant, GMAIL_API)
    }

    pub fn with_api(grant: Grant, api: &str) -> Self {
        Self {
            http: http(),
            api: api.trim_end_matches('/').to_owned(),
            grant,
            token: Cached::default(),
            me: String::new(),
            history_id: None,
        }
    }

    /// The grant to save again when the refresh token changed.
    pub fn grant(&self) -> &Grant {
        &self.grant
    }

    async fn access(&mut self) -> Result<String, MailError> {
        if let Some(t) = self.token.get() {
            return Ok(t);
        }
        match mcp_oauth::refresh(&self.grant).await {
            Ok((access, new_refresh, life)) => {
                if let Some(r) = new_refresh {
                    self.grant.refresh_token = r;
                }
                self.token.set(access.clone(), life);
                Ok(access)
            }
            Err(e) if e.contains("could not reach") => Err(MailError::Offline(e)),
            Err(_) => Err(MailError::AuthExpired),
        }
    }

    async fn get(&mut self, path: &str) -> Result<Value, MailError> {
        let token = self.access().await?;
        let resp = self
            .http
            .get(format!("{}{path}", self.api))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| offline(&e))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            // The token was revoked or expired early: renew once next time.
            self.token.clear();
            return Err(MailError::AuthExpired);
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(MailError::Other("not found".into()));
        }
        if !status.is_success() {
            return Err(MailError::Other(format!("the mail service said {status}")));
        }
        resp.json().await.map_err(|e| offline(&e))
    }

    /// First call: remember where the mailbox is now, so only mail that
    /// arrives from here on is reported.
    async fn start(&mut self) -> Result<(), MailError> {
        let p = self.get("/profile").await?;
        self.me = p["emailAddress"].as_str().unwrap_or_default().to_owned();
        self.history_id = p["historyId"].as_str().map(str::to_owned);
        Ok(())
    }
}

/// Ids of messages added, and the newest history id, from `history.list`.
pub fn added_ids(history: &Value) -> (Vec<String>, Option<String>) {
    let mut ids = Vec::new();
    for h in history["history"].as_array().into_iter().flatten() {
        for m in h["messagesAdded"].as_array().into_iter().flatten() {
            if let Some(id) = m["message"]["id"].as_str()
                && !ids.iter().any(|x| x == id)
            {
                ids.push(id.to_owned());
            }
        }
    }
    (ids, history["historyId"].as_str().map(str::to_owned))
}

impl MailSource for Gmail {
    async fn poll(&mut self) -> Result<Vec<Mail>, MailError> {
        let Some(since) = self.history_id.clone() else {
            self.start().await?;
            return Ok(Vec::new());
        };
        let page = match self
            .get(&format!(
                "/history?startHistoryId={since}&historyTypes=messageAdded&labelId=INBOX"
            ))
            .await
        {
            Ok(v) => v,
            // The saved point is too old for Google: start again from now.
            Err(MailError::Other(e)) if e == "not found" => {
                self.start().await?;
                return Ok(Vec::new());
            }
            Err(e) => return Err(e),
        };
        let (mut ids, newest) = added_ids(&page);
        if ids.len() > MAX_PER_SYNC {
            ids.drain(..ids.len() - MAX_PER_SYNC);
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut out = Vec::new();
        for id in ids {
            let msg = self
                .get(&format!(
                    "/messages/{id}?format=metadata&metadataHeaders=From&metadataHeaders=To\
                     &metadataHeaders=Delivered-To&metadataHeaders=Subject\
                     &metadataHeaders=List-Unsubscribe&metadataHeaders=Auto-Submitted"
                ))
                .await;
            let msg = match msg {
                Ok(m) => m,
                // Deleted before we looked: nothing to show.
                Err(MailError::Other(_)) => continue,
                Err(e) => return Err(e),
            };
            if let Some(mail) = parse_message(&msg, &self.me, now_ms)
                // Already read on another device: do not tell the user again.
                && mail.unread
            {
                out.push(mail);
            }
        }
        // Only move forward once everything was read, so a failure above
        // looks at the same messages again next time.
        if newest.is_some() {
            self.history_id = newest;
        }
        Ok(out)
    }

    async fn disconnect(&mut self) {
        let token = if self.grant.refresh_token.is_empty() {
            self.grant.access_token.clone()
        } else {
            self.grant.refresh_token.clone()
        };
        if !token.is_empty() {
            // Best effort: the local copy is deleted either way.
            let _ = self
                .http
                .post(GOOGLE_REVOKE)
                .form(&[("token", token.as_str())])
                .send()
                .await;
        }
        self.token.clear();
        self.grant = Grant::default();
        self.history_id = None;
        crate::secrets::delete(SECRET);
    }
}

/// Google sign-in in the browser. `open` shows the page. Returns the grant
/// to keep (saved by the caller under [`SECRET`]).
pub async fn sign_in<F, Fut>(open: F) -> Result<Grant, String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let id = client_id().ok_or("Google sign-in is not set up in this build yet.")?;
    let d = mcp_oauth::Discovery {
        resource: String::new(),
        authorization_endpoint: GOOGLE_AUTH.into(),
        token_endpoint: GOOGLE_TOKEN.into(),
        registration_endpoint: None,
        scope: Some(GMAIL_SCOPE.into()),
    };
    let (listener, redirect) = mcp_oauth::bind_loopback().await?;
    let (grant, _access, _life) =
        mcp_oauth::run_flow(&d, listener, &redirect, id, client_secret(), open).await?;
    if grant.refresh_token.is_empty() {
        return Err("Google did not give a lasting sign-in. Press Connect again.".into());
    }
    Ok(grant)
}

/// The saved grant, if the account is connected.
pub fn saved_grant() -> Option<Grant> {
    serde_json::from_str(&crate::secrets::get(SECRET)?).ok()
}

pub fn save_grant(grant: &Grant) -> Result<(), String> {
    crate::secrets::set(
        SECRET,
        &serde_json::to_string(grant).map_err(|e| e.to_string())?,
    )
}

// ---- Running it ---------------------------------------------------------------

use std::sync::Mutex;

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

/// What the Settings card shows. No message content.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: MailState,
    pub configured: bool,
    pub last_ok: Option<i64>,
    pub error: Option<String>,
    pub provider: Option<&'static str>,
}

/// State, last success (seconds), last error.
type Known = (MailState, Option<i64>, Option<String>);
static PROVIDER: Mutex<Option<&'static str>> = Mutex::new(None);
static STATUS: Mutex<Option<Known>> = Mutex::new(None);
/// Set when the user connects or disconnects, so the loop reacts at once.
static WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();

fn set_status(state: MailState, ok: bool, error: Option<String>) {
    let mut s = STATUS.lock().unwrap_or_else(|e| e.into_inner());
    let last = if ok {
        Some(chrono::Utc::now().timestamp())
    } else {
        s.as_ref().and_then(|x| x.1)
    };
    *s = Some((state, last, error));
}

pub fn status() -> Status {
    let known = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let (state, last_ok, error) = known.unwrap_or_else(|| {
        let s = if saved_grant().is_some() {
            MailState::Connected
        } else {
            MailState::Disconnected
        };
        (s, None, None)
    });
    Status {
        state,
        configured: client_id().is_some(),
        last_ok,
        error,
        provider: PROVIDER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .to_owned(),
    }
}

/// A stable number for a message, for the card's id.
fn card_id(id: &str) -> i64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut h);
    (h.finish() >> 2) as i64
}

/// Scores one message and shows it if it is worth the interruption.
fn handle(app: &AppHandle, mail: &Mail) {
    let settings = lock(&app.state::<AppState>().settings).clone();
    let n = &settings.notifications;
    if !n.enabled || settings.pause.is_active(chrono::Utc::now()) {
        return;
    }
    let signal = signal_of(mail, &n.vip);
    let scored = duskie_core::attention::score(&signal, &n.lessons, chrono::Utc::now().timestamp());
    log::debug!("mail scored {} ({:?})", scored.score, scored.level);
    use duskie_core::attention::{Context, Level, Shape};
    // Silent mail waits in the digest.
    if scored.level == Level::Silent {
        return;
    }
    let now = chrono::Utc::now().timestamp();
    let ctx = Context {
        focus: crate::focus::active(),
        ..Default::default()
    };
    let delivery = {
        let mut guard = ATTENTION.lock().unwrap_or_else(|e| e.into_inner());
        route(
            guard.get_or_insert_with(Default::default),
            &signal.key,
            scored.level,
            n.profile,
            ctx,
            now,
        )
    };
    let Some(delivery) = delivery else {
        return;
    };
    app.state::<AppState>().bus.publish(duskie_core::Event::new(
        crate::inbox::NOW_EVENT,
        "notifications",
        json!({
            "id": card_id(&mail.id),
            "app": "Gmail",
            "source": signal.key,
            "title": mail.from,
            "body": "New email",
            "why": "an email that needs you",
            "code": null,
            "compact": delivery.shape == Shape::Compact,
            "sound": delivery.sound,
        }),
    ));
}

/// Whether, and how, a scored message is shown: one card per burst from a
/// sender, a cooldown after each, and the profile and context decide the shape.
fn route(
    state: &mut Attention,
    key: &str,
    level: duskie_core::attention::Level,
    profile: duskie_core::attention::Profile,
    ctx: duskie_core::attention::Context,
    now: i64,
) -> Option<duskie_core::attention::Delivery> {
    use duskie_core::attention::{Batch, Shape, deliver};
    let batch = state.batcher.admit(key, level, now);
    let delivery = deliver(level, profile, ctx);
    if batch == Batch::Show {
        state.metrics.delivered(delivery);
    }
    (batch == Batch::Show && delivery.shape != Shape::Held).then_some(delivery)
}

/// What the batcher and counters remember between messages.
#[derive(Default)]
struct Attention {
    batcher: duskie_core::attention::Batcher,
    metrics: duskie_core::attention::Metrics,
}

static ATTENTION: Mutex<Option<Attention>> = Mutex::new(None);

/// Whichever account is connected.
enum Account {
    Gmail(Gmail),
    Imap(crate::imap::Imap),
}

impl Account {
    async fn poll(&mut self) -> Result<Vec<Mail>, MailError> {
        match self {
            Account::Gmail(g) => g.poll().await,
            Account::Imap(i) => i.poll().await,
        }
    }
}

fn set_provider(p: Option<&'static str>) {
    *PROVIDER.lock().unwrap_or_else(|e| e.into_inner()) = p;
}

/// The loop that checks for new mail while the account is connected.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut source: Option<Account> = None;
        let mut failures = 0u32;
        let mut active = false;
        loop {
            if source.is_none() {
                let imap = {
                    let n = lock(&app.state::<AppState>().settings)
                        .notifications
                        .imap
                        .clone();
                    crate::imap::saved(&n)
                };
                match (saved_grant(), imap) {
                    (Some(g), _) => {
                        set_provider(Some("gmail"));
                        source = Some(Account::Gmail(Gmail::new(g)));
                    }
                    (None, Some(i)) => {
                        set_provider(Some("imap"));
                        source = Some(Account::Imap(i));
                    }
                    (None, None) => {
                        set_provider(None);
                        set_status(MailState::Disconnected, false, None);
                        // Nothing to do until the user connects.
                        WAKE.notified().await;
                        continue;
                    }
                }
            }
            let Some(account) = source.as_mut() else {
                continue;
            };
            match account.poll().await {
                Ok(new) => {
                    failures = 0;
                    active = !new.is_empty();
                    set_status(MailState::Connected, true, None);
                    for mail in &new {
                        handle(&app, mail);
                    }
                    // Tokens may have rotated: keep the newest grant.
                    if let Account::Gmail(gmail) = account
                        && gmail.grant() != &saved_grant().unwrap_or_default()
                    {
                        let _ = save_grant(gmail.grant());
                    }
                }
                Err(err) => {
                    failures += 1;
                    log::warn!("mail: {err}");
                    set_status(err.state(), false, Some(err.to_string()));
                    if err == MailError::AuthExpired {
                        // Wait for the user to connect again.
                        source = None;
                        WAKE.notified().await;
                        continue;
                    }
                }
            }
            let wait = next_poll(active, failures);
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = WAKE.notified() => { source = None; failures = 0; }
            }
        }
    });
}

/// Browser sign-in, then save and start using the account.
pub async fn connect(app: &AppHandle) -> Result<(), String> {
    let opener = app.clone();
    let grant =
        sign_in(move |url| async move { crate::composio::open_url(&opener, &url).await }).await?;
    save_grant(&grant)?;
    set_provider(Some("gmail"));
    set_status(MailState::Connected, false, None);
    WAKE.notify_one();
    Ok(())
}

/// Checks an IMAP login once, then keeps the account. The password goes to
/// the keychain; only the host and user are in settings.
pub async fn connect_imap(
    app: &AppHandle,
    account: crate::imap::ImapAccount,
    password: String,
) -> Result<(), String> {
    let mut probe = crate::imap::Imap::new(account.clone(), password.clone());
    probe.poll().await.map_err(|e| match e {
        MailError::AuthExpired => {
            "The server did not accept that user and password. Many providers need an app password."
                .to_owned()
        }
        other => other.to_string(),
    })?;
    crate::secrets::set(crate::imap::SECRET, &password)?;
    let mut next = lock(&app.state::<AppState>().settings).clone();
    next.notifications.imap = duskie_core::MailAccountSettings {
        host: account.host,
        port: account.port,
        user: account.user,
    };
    crate::commands::apply_settings(app, next)?;
    set_provider(Some("imap"));
    set_status(MailState::Connected, false, None);
    WAKE.notify_one();
    Ok(())
}

/// Signs out of Google, deletes the saved sign-in, and stops checking.
pub async fn disconnect_account(app: &AppHandle) {
    if let Some(g) = saved_grant() {
        Gmail::new(g).disconnect().await;
    }
    crate::secrets::delete(SECRET);
    crate::secrets::delete(crate::imap::SECRET);
    let mut next = lock(&app.state::<AppState>().settings).clone();
    next.notifications.imap = Default::default();
    let _ = crate::commands::apply_settings(app, next);
    set_provider(None);
    set_status(MailState::Disconnected, false, None);
    WAKE.notify_one();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const NOW: i64 = 1_800_000_000_000;

    fn msg(
        id: &str,
        labels: &[&str],
        headers: &[(&str, &str)],
        snippet: &str,
        age_mins: i64,
    ) -> Value {
        json!({
            "id": id,
            "labelIds": labels,
            "snippet": snippet,
            "internalDate": (NOW - age_mins * 60_000).to_string(),
            "payload": { "headers": headers.iter().map(|(n, v)| json!({"name": n, "value": v})).collect::<Vec<_>>() },
        })
    }

    #[test]
    fn reads_the_address_out_of_a_from_header() {
        assert_eq!(address_of("Ali Khan <Ali@Example.com>"), "ali@example.com");
        assert_eq!(address_of("bob@x.io"), "bob@x.io");
    }

    #[test]
    fn a_direct_question_becomes_a_scored_signal() {
        let m = parse_message(
            &msg(
                "1",
                &["INBOX", "UNREAD"],
                &[
                    ("From", "Sam <sam@client.com>"),
                    ("To", "me@mine.com"),
                    ("Subject", "Can you send the invoice by tomorrow?"),
                ],
                "",
                90,
            ),
            "me@mine.com",
            NOW,
        )
        .unwrap();
        assert!(m.unread && m.to_me && m.question && m.deadline && !m.bulk);
        assert_eq!(m.age_mins, 90);
        let s = signal_of(&m, &[]);
        assert_eq!(s.key, "mail:sam@client.com");
        assert!(s.direct_to_me && s.question && s.unanswered_mins == 90);
    }

    #[test]
    fn newsletters_are_bulk_and_never_score_as_questions() {
        let m = parse_message(
            &msg(
                "2",
                &["INBOX", "UNREAD", "CATEGORY_PROMOTIONS"],
                &[
                    ("From", "deals@shop.com"),
                    ("To", "me@mine.com"),
                    ("Subject", "Want 20% off?"),
                    ("List-Unsubscribe", "<mailto:x@y>"),
                ],
                "",
                5,
            ),
            "me@mine.com",
            NOW,
        )
        .unwrap();
        assert!(m.bulk);
        let s = signal_of(&m, &[]);
        assert!(s.bulk && !s.question && !s.direct_to_me);
    }

    #[test]
    fn vips_match_by_name_or_address() {
        let m = parse_message(
            &msg("3", &["UNREAD"], &[("From", "Boss <boss@corp.com>")], "", 1),
            "me@mine.com",
            NOW,
        )
        .unwrap();
        assert!(signal_of(&m, &["boss".into()]).sender_important);
        assert!(!signal_of(&m, &["someone".into()]).sender_important);
        assert!(!signal_of(&m, &[String::new()]).sender_important);
    }

    #[test]
    fn mail_records_hold_no_text() {
        // The type has no field for a subject, snippet or body.
        let m = parse_message(
            &msg(
                "4",
                &["UNREAD"],
                &[("From", "a@b.c"), ("Subject", "secret plan?")],
                "secret",
                1,
            ),
            "me@x.y",
            NOW,
        )
        .unwrap();
        let dump = format!("{m:?}");
        assert!(!dump.contains("secret"));
    }

    #[test]
    fn a_burst_from_one_sender_is_one_card_and_focus_holds_the_rest() {
        use duskie_core::attention::{Context, Level, Profile, Shape};
        let mut st = Attention::default();
        let ctx = Context::default();
        let first = route(&mut st, "mail:a", Level::Normal, Profile::Normal, ctx, 1000).unwrap();
        assert_eq!(first.shape, Shape::Full);
        // Same sender seconds later: folded into the first.
        assert!(route(&mut st, "mail:a", Level::Normal, Profile::Normal, ctx, 1010).is_none());
        // Another sender, but the user is focusing: held, not shown.
        let focus = Context { focus: true, ..ctx };
        assert!(
            route(
                &mut st,
                "mail:b",
                Level::Normal,
                Profile::Normal,
                focus,
                1020
            )
            .is_none()
        );
        // Urgent gets through focus.
        assert!(route(&mut st, "mail:c", Level::Urgent, Profile::Calm, focus, 1030).is_some());
        // Calm shows ordinary mail as a small pill.
        let calm = route(&mut st, "mail:d", Level::Normal, Profile::Calm, ctx, 1040).unwrap();
        assert_eq!(calm.shape, Shape::Compact);
    }

    #[test]
    fn polling_is_quick_when_active_and_backs_off() {
        assert_eq!(next_poll(true, 0), Duration::from_secs(20));
        assert_eq!(next_poll(false, 0), Duration::from_secs(90));
        assert_eq!(next_poll(true, 2), Duration::from_secs(80));
        assert_eq!(next_poll(false, 10), Duration::from_secs(15 * 60));
        assert!(next_poll(true, 0) >= Duration::from_secs(15));
        assert!(next_poll(true, 0) <= Duration::from_secs(30));
        assert!(next_poll(false, 0) >= Duration::from_secs(60));
        assert!(next_poll(false, 0) <= Duration::from_secs(120));
    }

    #[test]
    fn history_lists_new_message_ids_once() {
        let h = json!({
            "historyId": "99",
            "history": [
                { "messagesAdded": [{ "message": { "id": "a" } }, { "message": { "id": "b" } }] },
                { "messagesAdded": [{ "message": { "id": "a" } }] },
            ],
        });
        assert_eq!(
            added_ids(&h),
            (vec!["a".to_owned(), "b".to_owned()], Some("99".to_owned()))
        );
        assert_eq!(added_ids(&json!({})), (vec![], None));
    }

    /// A tiny Gmail and token server on a local port. `routes` maps a path
    /// prefix to a status and JSON body; every request line is recorded.
    async fn fake(
        routes: Vec<(&'static str, u16, Value)>,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let routes = routes.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let line = head.lines().next().unwrap_or_default().to_owned();
                    let target = line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_owned();
                    log.lock().unwrap().push(target.clone());
                    let (status, body) = routes
                        .iter()
                        .find(|(p, _, _)| target.starts_with(p))
                        .map_or((404, json!({})), |(_, s, b)| (*s, b.clone()));
                    let body = body.to_string();
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        (base, seen)
    }

    fn grant_for(base: &str) -> Grant {
        Grant {
            client_id: "id".into(),
            client_secret: String::new(),
            token_endpoint: format!("{base}/token"),
            resource: String::new(),
            refresh_token: "refresh".into(),
            access_token: String::new(),
        }
    }

    #[tokio::test]
    async fn gmail_reports_only_new_unread_mail_after_the_start_point() {
        let now = chrono::Utc::now().timestamp_millis();
        let routes = vec![
            (
                "/token",
                200,
                json!({ "access_token": "tok", "expires_in": 3600 }),
            ),
            (
                "/profile",
                200,
                json!({ "emailAddress": "me@mine.com", "historyId": "10" }),
            ),
            (
                "/history",
                200,
                json!({
                    "historyId": "12",
                    "history": [{ "messagesAdded": [
                        { "message": { "id": "unread1" } },
                        { "message": { "id": "readelsewhere" } },
                    ]}],
                }),
            ),
            (
                "/messages/unread1",
                200,
                msg(
                    "unread1",
                    &["INBOX", "UNREAD"],
                    &[
                        ("From", "Sam <sam@c.com>"),
                        ("To", "me@mine.com"),
                        ("Subject", "Quick question?"),
                    ],
                    "",
                    2,
                ),
            ),
            (
                "/messages/readelsewhere",
                200,
                msg(
                    "readelsewhere",
                    &["INBOX"],
                    &[("From", "x@y.z"), ("To", "me@mine.com")],
                    "",
                    2,
                ),
            ),
        ];
        let (base, seen) = fake(routes).await;
        let mut g = Gmail::with_api(grant_for(&base), &base);
        // First poll only learns where the mailbox is.
        assert!(g.poll().await.unwrap().is_empty());
        let got = g.poll().await.unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].id, "unread1");
        assert!(got[0].question && got[0].to_me);
        assert_eq!(g.history_id.as_deref(), Some("12"));
        // The internalDate in the fixtures is fixed; parse used a real clock.
        let _ = now;
        let paths = seen.lock().unwrap().clone();
        assert!(
            paths
                .iter()
                .any(|p| p.starts_with("/history?startHistoryId=10"))
        );
    }

    #[tokio::test]
    async fn an_old_history_point_restarts_instead_of_failing() {
        let routes = vec![
            (
                "/token",
                200,
                json!({ "access_token": "tok", "expires_in": 3600 }),
            ),
            (
                "/profile",
                200,
                json!({ "emailAddress": "me@mine.com", "historyId": "50" }),
            ),
            ("/history", 404, json!({})),
        ];
        let (base, _) = fake(routes).await;
        let mut g = Gmail::with_api(grant_for(&base), &base);
        g.history_id = Some("1".into());
        assert!(g.poll().await.unwrap().is_empty());
        assert_eq!(g.history_id.as_deref(), Some("50"));
    }

    #[tokio::test]
    async fn a_revoked_sign_in_reads_as_auth_expired() {
        let routes = vec![("/token", 400, json!({ "error": "invalid_grant" }))];
        let (base, _) = fake(routes).await;
        let mut g = Gmail::with_api(grant_for(&base), &base);
        g.history_id = Some("1".into());
        let err = g.poll().await.unwrap_err();
        assert_eq!(err, MailError::AuthExpired);
        assert_eq!(err.state(), MailState::AuthExpired);
    }

    #[tokio::test]
    async fn an_unreachable_service_reads_as_offline() {
        // Nothing listens on this port.
        let mut g = Gmail::with_api(grant_for("http://127.0.0.1:9"), "http://127.0.0.1:9");
        g.history_id = Some("1".into());
        let err = g.poll().await.unwrap_err();
        assert_eq!(err.state(), MailState::Offline, "{err:?}");
    }

    #[tokio::test]
    async fn a_failure_part_way_does_not_skip_messages() {
        let routes = vec![
            (
                "/token",
                200,
                json!({ "access_token": "tok", "expires_in": 3600 }),
            ),
            (
                "/profile",
                200,
                json!({ "emailAddress": "me@mine.com", "historyId": "10" }),
            ),
            (
                "/history",
                200,
                json!({ "historyId": "12", "history": [{ "messagesAdded": [{ "message": { "id": "m1" } }] }] }),
            ),
            ("/messages/m1", 401, json!({})),
        ];
        let (base, _) = fake(routes).await;
        let mut g = Gmail::with_api(grant_for(&base), &base);
        g.poll().await.unwrap();
        assert_eq!(g.poll().await.unwrap_err(), MailError::AuthExpired);
        // The point did not move, so the message is looked at again.
        assert_eq!(g.history_id.as_deref(), Some("10"));
    }
}
