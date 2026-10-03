//! Password credentials and deadlines stay in Rust, identified by their prompt id.
use crate::state::{AppState, lock};
use crate::{browser, suggestions};
use sidekick_actions::passwords::{self, LoginMatch, Store, WriteResult, WriteStatus};
use sidekick_skills::{Proposal, ProposedOption, Trust};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const SKILL_ID: &str = "browser.password_save";
pub const SAVED_EVENT: &str = "password://saved";
pub const MIRROR_EVENT: &str = "password://mirror";
const COUNTDOWN: Duration = Duration::from_secs(5);
const EDIT_TTL: Duration = Duration::from_secs(600);
const SAVED_TTL: Duration = Duration::from_secs(25);
const QUEUE_TTL: Duration = Duration::from_secs(90);
const MAX_PENDING: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Queued,
    Countdown(Instant),
    Editing,
    Retry,
    Writing,
    Saved,
}
#[derive(Clone)]
struct Lifecycle {
    phase: Phase,
    expires: Instant,
}
impl Lifecycle {
    fn new(now: Instant) -> Self {
        Self {
            phase: Phase::Queued,
            expires: now + QUEUE_TTL,
        }
    }
    fn ready(&mut self, now: Instant) {
        if self.phase == Phase::Queued {
            self.phase = Phase::Countdown(now + COUNTDOWN);
            self.expires = now + EDIT_TTL;
        }
    }
    fn edit(&mut self, now: Instant) -> Result<(), String> {
        if now >= self.expires
            || matches!(self.phase, Phase::Queued | Phase::Writing)
            || matches!(self.phase, Phase::Countdown(at) if now >= at)
        {
            return Err("that save prompt is gone or busy".into());
        }
        if self.phase != Phase::Editing {
            self.expires = now + EDIT_TTL;
        }
        self.phase = Phase::Editing;
        Ok(())
    }
    fn claim(
        &mut self,
        now: Instant,
        automatic: bool,
        override_existing: bool,
    ) -> Result<(), String> {
        if now >= self.expires {
            return Err("that save prompt expired".into());
        }
        match self.phase {
            Phase::Countdown(deadline) if automatic && now >= deadline && !override_existing => {}
            Phase::Countdown(deadline) if !automatic && now < deadline => {}
            Phase::Editing | Phase::Retry if !automatic => {}
            _ => return Err("that save prompt expired or is already being saved".into()),
        }
        self.phase = Phase::Writing;
        Ok(())
    }
}

#[derive(Clone)]
struct Target {
    store: Store,
    original_username: String,
}
#[derive(Clone)]
struct Candidate {
    browser: String,
    password: String,
}
#[derive(Clone)]
pub struct PendingPassword {
    id: String,
    realm: String,
    domain: String,
    username: String,
    password: String,
    targets: Vec<Target>,
    unavailable: Vec<WriteResult>,
    candidates: Vec<Candidate>,
    lifecycle: Lifecycle,
    cancelled: Arc<AtomicBool>,
    mirror: Option<String>,
    generation: u64,
}

#[derive(Default)]
pub struct PasswordBook {
    entries: HashMap<String, PendingPassword>,
    mirror: Option<MirrorSession>,
    last_mirror_message: String,
    generation: u64,
}
#[derive(Default)]
struct SaveRequest {
    username: Option<String>,
    password: Option<String>,
    override_existing: bool,
    source: Option<String>,
    automatic: bool,
}

impl PasswordBook {
    fn reserve(
        &mut self,
        id: &str,
        request: SaveRequest,
        now: Instant,
    ) -> Result<PendingPassword, String> {
        let SaveRequest {
            username,
            password,
            override_existing,
            source,
            automatic,
        } = request;
        let p = self.entries.get_mut(id).ok_or("that save prompt is gone")?;
        if username.is_some() || password.is_some() {
            if !matches!(p.lifecycle.phase, Phase::Editing | Phase::Retry) {
                return Err("open Edit before changing a credential".into());
            }
            if password.as_ref().is_some_and(String::is_empty) {
                return Err("password is empty".into());
            }
        }
        let candidate = if p.mirror.is_some() && override_existing {
            Some(
                p.candidates
                    .iter()
                    .find(|c| Some(&c.browser) == source.as_ref())
                    .ok_or("choose a source browser")?
                    .password
                    .clone(),
            )
        } else {
            None
        };
        p.lifecycle.claim(now, automatic, override_existing)?;
        if let Some(u) = username {
            p.username = u;
        }
        if let Some(pass) = password.or(candidate) {
            p.password = pass;
        }
        Ok(p.clone())
    }

    fn remove(&mut self, id: &str) -> Option<String> {
        self.entries.remove(id).and_then(|p| {
            p.cancelled.store(true, Ordering::SeqCst);
            p.mirror
        })
    }
}

struct MirrorSession {
    id: String,
    cancelled: Arc<AtomicBool>,
    remaining: VecDeque<PendingPassword>,
    results: Vec<WriteResult>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordSaved {
    pub id: String,
    pub domain: String,
    pub expires_at: i64,
    pub message: String,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordEditDraft {
    pub id: String,
    pub domain: String,
    pub username: String,
    pub password: String,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordPrompt {
    pub id: String,
    pub domain: String,
    pub username: String,
    pub seconds: u64,
    pub phase: &'static str,
    pub missing: Vec<String>,
    pub conflicts: Vec<String>,
    pub existing: Vec<String>,
    pub unavailable: Vec<WriteResult>,
    pub sources: Vec<String>,
    pub mirror: bool,
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorStatus {
    pub running: bool,
    pub message: String,
}

fn allowed(app: &AppHandle, browser: &str) -> bool {
    let state = app.state::<AppState>();
    let settings = lock(&state.settings);
    !settings.pause.is_active(chrono::Utc::now())
        && settings.sensor_enabled("browser")
        && settings
            .password_browsers
            .as_ref()
            .is_none_or(|ids| ids.iter().any(|id| id == browser))
}
fn enabled(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let settings = lock(&state.settings);
    !settings.pause.is_active(chrono::Utc::now()) && settings.sensor_enabled("browser")
}

fn snapshots(ids: Vec<String>) -> (Vec<Store>, Vec<WriteResult>) {
    let mut stores = Vec::new();
    let mut errors = Vec::new();
    for id in ids {
        match Store::open(&id) {
            Ok(s) => stores.push(s),
            Err(e) => errors.push(passwords::failure(&id, &e.to_string())),
        }
    }
    (stores, errors)
}

pub async fn on_submitted(app: &AppHandle, event: &sidekick_core::Event) {
    let generation = lock(&app.state::<AppState>().pending_password).generation;
    let id = event.id.to_string();
    let Some(login) = app.state::<AppState>().browser.take_pending_login(&id) else {
        return;
    };
    if !enabled(app) {
        return;
    }
    let realm = passwords::realm_of(event.payload["url"].as_str().unwrap_or_default());
    let domain = event.payload["domain"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if realm.is_empty() || domain.is_empty() || login.password.is_empty() {
        return;
    }
    let ids = browser::password_target_ids(app);
    if ids.is_empty() {
        return;
    }
    let Ok((stores, unavailable)) =
        tauri::async_runtime::spawn_blocking(move || snapshots(ids)).await
    else {
        return;
    };
    if !enabled(app) {
        return;
    }
    if unavailable.is_empty()
        && stores
            .iter()
            .all(|s| s.classify(&realm, &login.username, &login.password) == LoginMatch::Exact)
    {
        return;
    }
    let pending = PendingPassword {
        id,
        realm,
        domain,
        username: login.username.clone(),
        password: login.password,
        targets: stores
            .into_iter()
            .map(|store| Target {
                store,
                original_username: login.username.clone(),
            })
            .collect(),
        unavailable,
        candidates: vec![],
        lifecycle: Lifecycle::new(Instant::now()),
        cancelled: Arc::new(AtomicBool::new(false)),
        mirror: None,
        generation,
    };
    insert_and_offer(app, pending);
}

fn insert_and_offer(app: &AppHandle, pending: PendingPassword) {
    let id = pending.id.clone();
    let domain = pending.domain.clone();
    let username = pending.username.clone();
    let mirror_id = pending.mirror.clone();
    let mirror = mirror_id.is_some();
    {
        let state = app.state::<AppState>();
        let mut book = lock(&state.pending_password);
        if book.entries.len() >= MAX_PENDING
            || pending.cancelled.load(Ordering::SeqCst)
            || pending.generation != book.generation
            || !enabled(app)
        {
            pending.cancelled.store(true, Ordering::SeqCst);
            drop(book);
            if let Some(session) = mirror_id {
                cancel_mirror_for(app, Some(&session));
            }
            return;
        }
        book.entries.insert(id.clone(), pending);
    }
    suggestions::offer(
        app,
        Proposal {
            skill_id: SKILL_ID.into(),
            skill_ids: vec![SKILL_ID.into()],
            title: if mirror {
                "Review password override"
            } else {
                "Save browser password"
            }
            .into(),
            detail: format!("{domain} · {username}"),
            options: vec![ProposedOption {
                label: if mirror { "Cancel mirroring" } else { "Cancel" }.into(),
                action: "password_save_cancel".into(),
                args: serde_json::json!({"id":id}),
                skill_id: SKILL_ID.into(),
            }],
            trust: Trust::Suggest,
            remember: None,
            priority: 94,
        },
    );
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let (expired, due) = {
                let state = app.state::<AppState>();
                let book = lock(&state.pending_password);
                let Some(p) = book.entries.get(&id) else {
                    break;
                };
                let now = Instant::now();
                (
                    now >= p.lifecycle.expires,
                    matches!(p.lifecycle.phase, Phase::Countdown(at) if now >= at),
                )
            };
            if !enabled(&app) || expired {
                let _ = cancel(&app, &id);
                break;
            }
            if due {
                let _ = commit(&app, &id, None, None, false, None, true).await;
            }
        }
    });
}

pub fn prompt_id(proposal: &Proposal) -> Option<&str> {
    (proposal.skill_id == SKILL_ID)
        .then(|| proposal.options.first()?.args["id"].as_str())
        .flatten()
}
pub fn exists(app: &AppHandle, id: &str) -> bool {
    let state = app.state::<AppState>();
    lock(&state.pending_password).entries.contains_key(id)
}
/// Called whenever the suggestion queue evicts or dismisses this exact proposal.
pub fn abandon(app: &AppHandle, id: &str) {
    let mirror = {
        let state = app.state::<AppState>();
        let mut book = lock(&state.pending_password);
        book.remove(id)
    };
    if let Some(session) = mirror {
        cancel_mirror_for(app, Some(&session));
    }
}
fn clear_prompt(app: &AppHandle, id: &str) {
    if suggestions::current(app).is_some_and(|s| s.id == id && s.skill_id == SKILL_ID) {
        let _ = suggestions::dismiss(app, id, "saved");
    }
    lock(&app.state::<AppState>().queue).retain(|q| prompt_id(&q.proposal) != Some(id));
}

pub fn cancel(app: &AppHandle, id: &str) -> Result<(), String> {
    abandon(app, id);
    clear_prompt(app, id);
    Ok(())
}
pub fn cancel_all(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut book = lock(&state.pending_password);
        book.generation = book.generation.wrapping_add(1);
        for p in book.entries.values() {
            p.cancelled.store(true, Ordering::SeqCst);
        }
        book.entries.clear();
    }
    app.state::<AppState>().browser.clear_pending_logins();
    cancel_mirror(app);
    if let Some(s) = suggestions::current(app)
        && s.skill_id == SKILL_ID
    {
        clear_prompt(app, &s.id);
    }
    lock(&app.state::<AppState>().queue).retain(|q| q.proposal.skill_id != SKILL_ID);
}

pub fn status(app: &AppHandle, id: &str, displayed: bool) -> Result<PasswordPrompt, String> {
    let state = app.state::<AppState>();
    let mut book = lock(&state.pending_password);
    let p = book.entries.get_mut(id).ok_or("that save prompt is gone")?;
    let now = Instant::now();
    if now >= p.lifecycle.expires {
        return Err("that save prompt expired".into());
    }
    if displayed {
        p.lifecycle.ready(now);
    }
    let mut missing = Vec::new();
    let mut conflicts = Vec::new();
    let mut existing = Vec::new();
    let mut unavailable = p.unavailable.clone();
    for t in &p.targets {
        match t
            .store
            .classify(&p.realm, &t.original_username, &p.password)
        {
            LoginMatch::Missing => missing.push(t.store.browser.clone()),
            LoginMatch::Different => {
                conflicts.push(t.store.browser.clone());
                existing.push(t.store.browser.clone());
            }
            LoginMatch::Exact => existing.push(t.store.browser.clone()),
            LoginMatch::Protected => unavailable.push(WriteResult {
                browser: t.store.browser.clone(),
                status: WriteStatus::Unsupported,
                message: "protected or undecryptable entry".into(),
            }),
        }
    }
    let (phase, seconds) = match p.lifecycle.phase {
        Phase::Queued => ("queued", 5),
        Phase::Countdown(at) => (
            "countdown",
            at.saturating_duration_since(now).as_millis().div_ceil(1000) as u64,
        ),
        Phase::Editing => ("editing", 0),
        Phase::Retry => ("retry", 0),
        Phase::Writing => ("writing", 0),
        Phase::Saved => ("saved", 0),
    };
    Ok(PasswordPrompt {
        id: p.id.clone(),
        domain: p.domain.clone(),
        username: p.username.clone(),
        seconds,
        phase,
        missing,
        conflicts,
        existing,
        unavailable,
        sources: p.candidates.iter().map(|c| c.browser.clone()).collect(),
        mirror: p.mirror.is_some(),
    })
}

pub fn draft(app: &AppHandle, id: &str) -> Result<PasswordEditDraft, String> {
    let state = app.state::<AppState>();
    let mut book = lock(&state.pending_password);
    let p = book.entries.get_mut(id).ok_or("that save prompt is gone")?;
    if p.mirror.is_some() {
        return Err("choose a source browser for this mirror conflict".into());
    }
    p.lifecycle.edit(Instant::now())?;
    Ok(PasswordEditDraft {
        id: p.id.clone(),
        domain: p.domain.clone(),
        username: p.username.clone(),
        password: p.password.clone(),
    })
}

pub async fn commit(
    app: &AppHandle,
    id: &str,
    username: Option<String>,
    password: Option<String>,
    override_existing: bool,
    source: Option<String>,
    automatic: bool,
) -> Result<String, String> {
    if !enabled(app) {
        cancel_all(app);
        return Err("browser passwords are paused".into());
    }
    let p = {
        let state = app.state::<AppState>();
        lock(&state.pending_password).reserve(
            id,
            SaveRequest {
                username,
                password,
                override_existing,
                source,
                automatic,
            },
            Instant::now(),
        )?
    };
    let app_worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut p = p;
        let mut results = p.unavailable.clone();
        // Timeout of a bulk conflict skips the entire account, even missing targets.
        if p.mirror.is_some() && !override_existing {
            results.extend(p.targets.iter().map(|t| WriteResult {
                browser: t.store.browser.clone(),
                status: WriteStatus::Conflict,
                message: "override timed out; account skipped".into(),
            }));
        } else {
            for t in &mut p.targets {
                if p.cancelled.load(Ordering::SeqCst) {
                    break;
                }
                if !allowed(&app_worker, &t.store.browser) {
                    results.push(WriteResult {
                        browser: t.store.browser.clone(),
                        status: WriteStatus::Disabled,
                        message: "target disabled".into(),
                    });
                    continue;
                }
                let r = t.store.write(
                    &p.realm,
                    &t.original_username,
                    &p.username,
                    &p.password,
                    override_existing,
                    || {
                        !p.cancelled.load(Ordering::SeqCst)
                            && allowed(&app_worker, &t.store.browser)
                    },
                );
                if matches!(r.status, WriteStatus::Saved | WriteStatus::Unchanged) {
                    t.original_username = p.username.clone();
                    if let Ok(store) = t.store.refresh() {
                        t.store = store;
                    }
                }
                results.push(r);
            }
        }
        (p, results)
    })
    .await;
    let (mut p, results) = match result {
        Ok(v) => v,
        Err(_) => {
            let state = app.state::<AppState>();
            if let Some(p) = lock(&state.pending_password).entries.get_mut(id) {
                p.lifecycle.phase = Phase::Retry;
            }
            return Err("password worker failed; retry manually".into());
        }
    };
    if p.cancelled.load(Ordering::SeqCst) {
        return Err("save cancelled".into());
    }
    let message = summarize(&results);
    let success = results.iter().any(|r| r.status == WriteStatus::Saved);
    let retry = !success
        && results
            .iter()
            .any(|r| matches!(r.status, WriteStatus::Locked | WriteStatus::Failed));
    if retry && p.mirror.is_none() {
        p.lifecycle.phase = Phase::Retry;
        p.lifecycle.expires = Instant::now() + EDIT_TTL;
        let failed: Vec<_> = p.unavailable.iter().map(|r| r.browser.clone()).collect();
        let restored = tauri::async_runtime::spawn_blocking(move || snapshots(failed))
            .await
            .map_err(|_| "password worker failed")?;
        p.targets.extend(restored.0.into_iter().map(|store| Target {
            store,
            original_username: p.username.clone(),
        }));
        p.unavailable = restored.1;
        let state = app.state::<AppState>();
        let mut book = lock(&state.pending_password);
        if book.entries.contains_key(id) {
            book.entries.insert(id.to_owned(), p);
        }
        return Err(message);
    }
    {
        let state = app.state::<AppState>();
        let mut book = lock(&state.pending_password);
        if !book.entries.contains_key(id) {
            return Err("save cancelled".into());
        }
        if success && p.mirror.is_none() {
            p.lifecycle.phase = Phase::Saved;
            p.lifecycle.expires = Instant::now() + SAVED_TTL;
            book.entries.insert(id.to_owned(), p.clone());
        } else {
            book.entries.remove(id);
        }
    }
    clear_prompt(app, id);
    emit_result(
        app,
        success || results.iter().all(|r| r.status == WriteStatus::Unchanged),
        &message,
        automatic,
    );
    if success && p.mirror.is_none() {
        let _ = app.emit(
            SAVED_EVENT,
            PasswordSaved {
                id: id.to_owned(),
                domain: p.domain,
                expires_at: chrono::Utc::now().timestamp_millis() + SAVED_TTL.as_millis() as i64,
                message: message.clone(),
            },
        );
    }
    if let Some(session) = p.mirror {
        finish_mirror_step(app, &session, results);
    }
    Ok(message)
}

fn emit_result(app: &AppHandle, ok: bool, message: &str, auto: bool) {
    let _ = app.emit(
        suggestions::ACTION_RESULT,
        suggestions::ActionResult {
            ok,
            message: message.into(),
            path: None,
            auto,
            undo_id: None,
        },
    );
}
fn summarize(results: &[WriteResult]) -> String {
    if results.is_empty() {
        return "No browser password targets selected".into();
    }
    let mut parts = Vec::new();
    for (status, label) in [
        (WriteStatus::Saved, "saved"),
        (WriteStatus::Unchanged, "unchanged"),
        (WriteStatus::Conflict, "conflicts skipped"),
        (WriteStatus::Locked, "locked"),
        (WriteStatus::Unsupported, "unsupported"),
        (WriteStatus::Failed, "failed"),
        (WriteStatus::Disabled, "disabled"),
        (WriteStatus::Cancelled, "cancelled"),
    ] {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for r in results.iter().filter(|r| r.status == status) {
            *counts.entry(&r.browser).or_default() += 1;
        }
        if !counts.is_empty() {
            let total: usize = counts.values().sum();
            let names: Vec<_> = counts
                .into_iter()
                .map(|(name, count)| format!("{name}: {count}"))
                .collect();
            parts.push(format!("{label} {total} ({})", names.join(", ")));
        }
    }
    let mut details: Vec<_> = results
        .iter()
        .filter(|r| {
            matches!(
                r.status,
                WriteStatus::Failed | WriteStatus::Locked | WriteStatus::Unsupported
            )
        })
        .map(|r| r.message.as_str())
        .collect();
    details.sort_unstable();
    details.dedup();
    if !details.is_empty() {
        parts.push(details.join("; "));
    }
    parts.join("; ")
}

pub fn mirror_status(app: &AppHandle) -> MirrorStatus {
    let state = app.state::<AppState>();
    let book = lock(&state.pending_password);
    MirrorStatus {
        running: book.mirror.is_some(),
        message: book
            .mirror
            .as_ref()
            .map(|s| format!("Mirroring; {} accounts waiting", s.remaining.len()))
            .unwrap_or_else(|| book.last_mirror_message.clone()),
    }
}
pub fn cancel_mirror(app: &AppHandle) {
    cancel_mirror_for(app, None);
}
fn cancel_mirror_for(app: &AppHandle, expected: Option<&str>) {
    let (session, ids) = {
        let state = app.state::<AppState>();
        let mut book = lock(&state.pending_password);
        if expected.is_some_and(|id| book.mirror.as_ref().is_none_or(|s| s.id != id)) {
            return;
        }
        let session = book.mirror.take();
        let mut ids = Vec::new();
        if let Some(s) = &session {
            s.cancelled.store(true, Ordering::SeqCst);
            book.entries.retain(|_, p| {
                if p.mirror.as_deref() == Some(&s.id) {
                    ids.push(p.id.clone());
                    p.cancelled.store(true, Ordering::SeqCst);
                    false
                } else {
                    true
                }
            });
        }
        if let Some(s) = &session {
            book.last_mirror_message = format!("Mirroring cancelled. {}", summarize(&s.results));
        }
        (session, ids)
    };
    for id in ids {
        clear_prompt(app, &id);
    }
    if let Some(s) = session {
        let _ = app.emit(
            MIRROR_EVENT,
            MirrorStatus {
                running: false,
                message: format!("Mirroring cancelled. {}", summarize(&s.results)),
            },
        );
    }
}
fn finish_mirror_step(app: &AppHandle, session: &str, results: Vec<WriteResult>) {
    {
        let state = app.state::<AppState>();
        let mut book = lock(&state.pending_password);
        if let Some(s) = &mut book.mirror
            && s.id == session
        {
            s.results.extend(results);
        }
    }
    next_mirror(app, session);
}
fn next_mirror(app: &AppHandle, session: &str) {
    let next = {
        let state = app.state::<AppState>();
        let mut book = lock(&state.pending_password);
        let Some(s) = book.mirror.as_mut().filter(|s| s.id == session) else {
            return;
        };
        if let Some(mut p) = s.remaining.pop_front() {
            p.lifecycle = Lifecycle::new(Instant::now());
            Some(p)
        } else {
            let message = format!("Mirroring finished. {}", summarize(&s.results));
            book.mirror = None;
            book.last_mirror_message = message.clone();
            let _ = app.emit(
                MIRROR_EVENT,
                MirrorStatus {
                    running: false,
                    message,
                },
            );
            None
        }
    };
    if let Some(p) = next {
        insert_and_offer(app, p);
    }
}

/// Every source is snapshotted before the first destination write.
pub async fn mirror(app: &AppHandle) -> Result<String, String> {
    if !enabled(app) {
        return Err("browser passwords are paused".into());
    }
    let ids = browser::password_target_ids(app);
    if ids.len() < 2 {
        return Err("select at least two browser password stores".into());
    }
    let session = ulid::Ulid::new().to_string();
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<AppState>();
        let mut book = lock(&state.pending_password);
        if book.mirror.is_some() {
            return Err("mirroring is already running".into());
        }
        book.mirror = Some(MirrorSession {
            id: session.clone(),
            cancelled: cancelled.clone(),
            remaining: VecDeque::new(),
            results: vec![],
        });
    }
    let _ = app.emit(
        MIRROR_EVENT,
        MirrorStatus {
            running: true,
            message: "Reading selected profiles".into(),
        },
    );
    let generation = lock(&app.state::<AppState>().pending_password).generation;
    let worker_app = app.clone();
    let worker_session = session.clone();
    let built = tauri::async_runtime::spawn_blocking(move || {
        let (stores, mut results) = snapshots(ids);
        let mut groups: BTreeMap<(String, String), Vec<Candidate>> = BTreeMap::new();
        for store in &stores {
            for row in store.rows.iter() {
                if let Some(password) = &row.password {
                    groups
                        .entry((row.realm.clone(), row.username.clone()))
                        .or_default()
                        .push(Candidate {
                            browser: store.browser.clone(),
                            password: password.clone(),
                        });
                }
            }
        }
        let mut queue = VecDeque::new();
        for ((realm, username), sources) in groups {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            let password = sources[0].password.clone();
            let conflicting = sources.iter().any(|s| s.password != password);
            if conflicting {
                // Only show a source browser if it has one unambiguous password.
                let candidates = unambiguous_sources(sources);
                if candidates.is_empty() {
                    results.extend(stores.iter().map(|s| WriteResult {
                        browser: s.browser.clone(),
                        status: WriteStatus::Conflict,
                        message: "ambiguous source skipped".into(),
                    }));
                    continue;
                }
                queue.push_back(PendingPassword {
                    id: ulid::Ulid::new().to_string(),
                    domain: passwords::origin_of(&realm)
                        .split("://")
                        .nth(1)
                        .unwrap_or_default()
                        .into(),
                    realm,
                    username: username.clone(),
                    password,
                    targets: stores
                        .iter()
                        .cloned()
                        .map(|store| Target {
                            store,
                            original_username: username.clone(),
                        })
                        .collect(),
                    unavailable: vec![],
                    candidates,
                    lifecycle: Lifecycle::new(Instant::now()),
                    cancelled: cancelled.clone(),
                    mirror: Some(worker_session.clone()),
                    generation,
                });
            } else {
                for store in &stores {
                    results.push(store.write(
                        &realm,
                        &username,
                        &username,
                        &password,
                        false,
                        || {
                            !cancelled.load(Ordering::SeqCst)
                                && allowed(&worker_app, &store.browser)
                        },
                    ));
                }
            }
        }
        (queue, results)
    })
    .await;
    let (queue, results) = match built {
        Ok(v) => v,
        Err(_) => {
            cancel_mirror(app);
            return Err("mirror worker failed".into());
        }
    };
    {
        let state = app.state::<AppState>();
        let mut book = lock(&state.pending_password);
        let Some(s) = book.mirror.as_mut().filter(|s| s.id == session) else {
            return Err("mirroring cancelled".into());
        };
        s.remaining = queue;
        s.results = results;
    }
    next_mirror(app, &session);
    let status = mirror_status(app);
    Ok(if status.running {
        "Local mirroring started; differing passwords require Override within five seconds".into()
    } else {
        status.message
    })
}
fn unambiguous_sources(sources: Vec<Candidate>) -> Vec<Candidate> {
    let mut by_browser: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in sources {
        by_browser.entry(s.browser).or_default().push(s.password);
    }
    by_browser
        .into_iter()
        .filter_map(|(browser, passwords)| {
            passwords
                .iter()
                .all(|p| p == &passwords[0])
                .then(|| Candidate {
                    browser,
                    password: passwords[0].clone(),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pending(id: &str, now: Instant) -> PendingPassword {
        PendingPassword {
            id: id.into(),
            realm: "https://example.test/".into(),
            domain: "example.test".into(),
            username: "alice".into(),
            password: "secret".into(),
            targets: vec![],
            unavailable: vec![],
            candidates: vec![],
            lifecycle: Lifecycle::new(now),
            cancelled: Arc::new(AtomicBool::new(false)),
            mirror: None,
            generation: 0,
        }
    }

    #[test]
    fn prompts_reserve_their_own_credentials_and_cancel_stops_claimed_work() {
        let now = Instant::now();
        let mut book = PasswordBook::default();
        let mut a = pending("a", now);
        a.lifecycle.ready(now);
        let mut b = pending("b", now);
        b.password = "second-site".into();
        b.lifecycle.ready(now);
        book.entries.insert("a".into(), a);
        book.entries.insert("b".into(), b);
        assert!(
            book.reserve(
                "missing",
                SaveRequest {
                    override_existing: true,
                    ..SaveRequest::default()
                },
                now
            )
            .is_err()
        );
        let claimed = book
            .reserve(
                "a",
                SaveRequest {
                    override_existing: true,
                    ..SaveRequest::default()
                },
                now,
            )
            .unwrap();
        assert_eq!(claimed.password, "secret");
        assert_eq!(book.entries["b"].password, "second-site");
        let _ = book.remove("a");
        assert!(claimed.cancelled.load(Ordering::SeqCst));
        assert!(
            book.reserve(
                "a",
                SaveRequest {
                    override_existing: true,
                    ..SaveRequest::default()
                },
                now
            )
            .is_err()
        );
        assert!(
            book.reserve(
                "b",
                SaveRequest {
                    automatic: true,
                    ..SaveRequest::default()
                },
                now + COUNTDOWN
            )
            .is_ok()
        );
    }

    #[test]
    fn mirror_needs_a_source_and_edit_validation_preserves_retry_state() {
        let now = Instant::now();
        let mut book = PasswordBook::default();
        let mut p = pending("mirror", now);
        p.mirror = Some("session".into());
        p.lifecycle.ready(now);
        p.candidates = vec![Candidate {
            browser: "chrome".into(),
            password: "selected".into(),
        }];
        book.entries.insert("mirror".into(), p);
        assert!(
            book.reserve(
                "mirror",
                SaveRequest {
                    override_existing: true,
                    ..SaveRequest::default()
                },
                now
            )
            .is_err()
        );
        assert!(matches!(
            book.entries["mirror"].lifecycle.phase,
            Phase::Countdown(_)
        ));
        let selected = book
            .reserve(
                "mirror",
                SaveRequest {
                    override_existing: true,
                    source: Some("chrome".into()),
                    ..SaveRequest::default()
                },
                now + Duration::from_secs(4),
            )
            .unwrap();
        assert_eq!(selected.password, "selected");
        let mut edit = pending("edit", now);
        edit.lifecycle.ready(now);
        edit.lifecycle.edit(now).unwrap();
        book.entries.insert("edit".into(), edit);
        assert!(
            book.reserve(
                "edit",
                SaveRequest {
                    password: Some(String::new()),
                    override_existing: true,
                    ..SaveRequest::default()
                },
                now
            )
            .is_err()
        );
        assert_eq!(book.entries["edit"].lifecycle.phase, Phase::Editing);
    }

    #[test]
    fn countdown_requires_display_and_timeout_never_overrides() {
        let now = Instant::now();
        let mut l = Lifecycle::new(now);
        assert!(l.claim(now + COUNTDOWN, true, false).is_err());
        l.ready(now);
        l.ready(now + Duration::from_secs(4));
        assert!(l.claim(now + COUNTDOWN, false, true).is_err());
        assert!(l.claim(now + COUNTDOWN, true, false).is_ok());
        assert!(l.claim(now + COUNTDOWN, true, false).is_err());
    }
    #[test]
    fn editing_pauses_and_empty_or_stale_commands_do_not_claim() {
        let now = Instant::now();
        let mut l = Lifecycle::new(now);
        l.ready(now);
        l.edit(now + Duration::from_secs(2)).unwrap();
        assert!(l.claim(now + Duration::from_secs(6), true, false).is_err());
        assert!(
            l.claim(now + EDIT_TTL + Duration::from_secs(2), false, true)
                .is_err()
        );
        assert!(l.claim(now + Duration::from_secs(6), false, true).is_ok());
    }
    #[test]
    fn sources_with_multiple_different_passwords_are_not_preselected() {
        let c = |browser: &str, password: &str| Candidate {
            browser: browser.into(),
            password: password.into(),
        };
        let sources = unambiguous_sources(vec![
            c("chrome", "old"),
            c("chrome", "new"),
            c("edge", "edge"),
        ]);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].browser, "edge");
    }
}
