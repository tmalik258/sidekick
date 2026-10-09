//! Shows proposals from the skill engine on the island, one at a time, and
//! runs the option the user picks (or the first safe one, for Auto skills).

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Serialize;
use sidekick_actions::is_safe;
use sidekick_core::{ActionRecord, MascotEvent, MascotState};
use sidekick_skills::{Proposal, ProposedOption, Trust};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::mascot;
use crate::state::{Active, AppState, Queued, Suggestion, executor, lock};

pub const SUGGESTION_NEW: &str = "suggestion://new";
pub const SUGGESTION_CLEAR: &str = "suggestion://clear";
pub const ACTION_RESULT: &str = "action://result";

/// Queued proposals older than this are dropped: the moment has passed.
const STALE_AFTER: Duration = Duration::from_secs(90);
const MAX_QUEUE: usize = 5;
/// Pause between one suggestion ending and the next appearing.
const NEXT_AFTER_DISMISS: Duration = Duration::from_millis(450);
const NEXT_AFTER_ACTION: Duration = Duration::from_millis(1900);
const EXPIRY_TICK: Duration = Duration::from_millis(250);
/// Give the user a moment after they return before showing anything.
const WELCOME_BACK: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub ok: bool,
    pub message: String,
    pub path: Option<String>,
    pub auto: bool,
    /// Set when the action can be undone (it created a file).
    pub undo_id: Option<i64>,
}

const DIGITS: [Code; 10] = [
    Code::Digit0,
    Code::Digit1,
    Code::Digit2,
    Code::Digit3,
    Code::Digit4,
    Code::Digit5,
    Code::Digit6,
    Code::Digit7,
    Code::Digit8,
    Code::Digit9,
];

/// Alt+1..9 pick an option and Alt+0 is "Not now". They are global, because
/// the island never takes focus, and exist only while a suggestion is showing
/// so they never steal keys otherwise.
fn bind_keys(app: &AppHandle, options: usize) {
    // The guide card's keys step aside while a suggestion shows.
    release_digits(app);
    let gs = app.global_shortcut();
    for (n, code) in DIGITS.iter().enumerate().take(options.min(9) + 1) {
        let shortcut = Shortcut::new(Some(Modifiers::ALT), *code);
        let result = gs.on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                // Off the handler's thread: picking unregisters these very
                // shortcuts, which must not happen from inside their callback.
                let app = app.clone();
                tauri::async_runtime::spawn(async move { pick(&app, n) });
            }
        });
        if let Err(err) = result {
            log::warn!("could not register Alt+{n} (another app may use it): {err}");
        }
    }
}

fn release_digits(app: &AppHandle) {
    let gs = app.global_shortcut();
    for digit in DIGITS {
        let _ = gs.unregister(Shortcut::new(Some(Modifiers::ALT), digit));
    }
}

fn unbind_keys(app: &AppHandle) {
    release_digits(app);
    // The guide card gets its keys back once the suggestion is gone.
    bind_guide_keys(app);
}

/// Buttons on the guide card (the steps shown while Sidekick waits on
/// something you finish in another app). The island has no focus there, so
/// like a suggestion's options they are global Alt keys: Alt+1..N press a
/// button, Alt+0 cancels.
static GUIDE_KEYS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub const GUIDE_KEY_EVENT: &str = "guide://key";

pub fn set_guide_keys(app: &AppHandle, buttons: usize) {
    GUIDE_KEYS.store(buttons.min(9), Ordering::SeqCst);
    if current(app).is_none() {
        release_digits(app);
        bind_guide_keys(app);
    }
}

fn bind_guide_keys(app: &AppHandle) {
    let buttons = GUIDE_KEYS.load(Ordering::SeqCst);
    if buttons == 0 {
        return;
    }
    let gs = app.global_shortcut();
    for (n, code) in DIGITS.iter().enumerate().take(buttons + 1) {
        let shortcut = Shortcut::new(Some(Modifiers::ALT), *code);
        let result = gs.on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let _ = app.emit(GUIDE_KEY_EVENT, n);
            }
        });
        if let Err(err) = result {
            log::warn!("could not register Alt+{n} for the guide: {err}");
        }
    }
}

/// Registers the keys again for the active suggestion, after something else
/// cleared every shortcut (changing the Ask shortcut does).
pub fn rebind_keys(app: &AppHandle) {
    match current(app) {
        Some(s) => bind_keys(app, s.options.len()),
        None => bind_guide_keys(app),
    }
}

/// Alt+N: option N, or "Not now" for 0.
fn pick(app: &AppHandle, n: usize) {
    log::info!("shortcut Alt+{n}");
    let Some(s) = current(app) else { return };
    let result = if n == 0 {
        dismiss(app, &s.id, "shortcut")
    } else if n <= s.options.len() {
        choose(app, &s.id, n - 1)
    } else {
        return;
    };
    if let Err(err) = result {
        log::debug!("shortcut ignored: {err}");
    }
}

pub const LATER_EVENT: &str = "suggestion://later";
/// Below this priority a suggestion never interrupts; it waits in the list.
const QUIET_BELOW: i32 = 40;
/// During a meeting, only this urgent and above interrupts.
const MEETING_FROM: i32 = 80;
const MAX_LATER: usize = 20;
/// At most this many suggestions interrupt per hour; the rest wait in the
/// list. Urgent ones (MEETING_FROM and above) still show.
const MAX_PER_HOUR: usize = 4;

/// When suggestions last interrupted, for the hourly cap.
static SHOWN: std::sync::Mutex<std::collections::VecDeque<Instant>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

/// Too many interrupted in the last hour already.
fn over_cap(now: Instant) -> bool {
    let mut shown = lock(&SHOWN);
    while shown
        .front()
        .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(3600))
    {
        shown.pop_front();
    }
    shown.len() >= MAX_PER_HOUR
}
const LATER_KEEP: chrono::Duration = chrono::Duration::hours(3);

/// How long a missed suggestion is still worth showing. Ones tied to a
/// moment (what you just copied, a screenshot, the late hour) mean nothing
/// once it has passed; messages and reminders keep for hours.
fn later_keep(skill_id: &str) -> chrono::Duration {
    const MOMENTS: &[&str] = &[
        "files.screenshot",
        "browser.many-tabs",
        "browser.long-read",
        "dev.explain-error",
        "dev.stuck",
        "system.late-night",
        "system.back",
        "system.focus",
        "system.layout",
        "system.memory-high",
    ];
    if skill_id.starts_with("clipboard.") {
        chrono::Duration::minutes(10)
    } else if MOMENTS.contains(&skill_id) {
        chrono::Duration::minutes(30)
    } else {
        LATER_KEEP
    }
}

/// The user is in a meeting now (one from the calendar, under four hours).
fn in_meeting(app: &AppHandle) -> bool {
    let now = chrono::Utc::now();
    let state = app.state::<AppState>();
    let c = state
        .calendar
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    c.meetings
        .iter()
        .any(|m| m.start <= now && now < m.end && (m.end - m.start).num_hours() < 4)
}

/// Whether a suggestion should wait quietly instead of showing now.
/// Suggestions about something the user just did (copied, opened a page)
/// are only useful right away, so they never wait.
pub fn should_wait(skill_id: &str, priority: i32, meeting: bool) -> bool {
    let just_did = skill_id.starts_with("clipboard.") || skill_id.starts_with("browser.");
    !just_did && (priority < QUIET_BELOW || (meeting && priority < MEETING_FROM))
}

fn keep_for_later(app: &AppHandle, proposal: Proposal, missed: bool) {
    let state = app.state::<AppState>();
    let now = chrono::Utc::now();
    let mut later = lock(&state.later);
    // Copying something new makes the last copy's suggestion stale.
    let copied = proposal.skill_id.starts_with("clipboard.");
    later.retain(|l| {
        now - l.at < later_keep(&l.proposal.skill_id)
            && !(l.proposal.skill_id == proposal.skill_id && l.proposal.title == proposal.title)
            && !(copied && l.proposal.skill_id.starts_with("clipboard."))
    });
    if later.len() >= MAX_LATER {
        later.remove(0);
    }
    later.push(crate::state::Later {
        id: ulid::Ulid::new().to_string(),
        proposal,
        at: now,
        missed,
    });
    let count = later.len();
    drop(later);
    let _ = app.emit(LATER_EVENT, count);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaterItem {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub minutes_ago: i64,
    pub missed: bool,
}

pub fn later_list(app: &AppHandle) -> Vec<LaterItem> {
    let now = chrono::Utc::now();
    let state = app.state::<AppState>();
    let mut later = lock(&state.later);
    later.retain(|l| now - l.at < later_keep(&l.proposal.skill_id));
    later
        .iter()
        .rev()
        .map(|l| LaterItem {
            id: l.id.clone(),
            title: l.proposal.title.clone(),
            detail: l.proposal.detail.clone(),
            minutes_ago: (now - l.at).num_minutes(),
            missed: l.missed,
        })
        .collect()
}

/// Shows one waiting suggestion now (the user asked for it).
pub fn later_open(app: &AppHandle, id: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let proposal = {
        let mut later = lock(&state.later);
        let i = later
            .iter()
            .position(|l| l.id == id)
            .ok_or("That one has expired")?;
        later.remove(i).proposal
    };
    let count = lock(&state.later).len();
    let _ = app.emit(LATER_EVENT, count);
    show_or_queue(app, proposal);
    Ok(())
}

pub fn later_clear(app: &AppHandle) {
    lock(&app.state::<AppState>().later).clear();
    let _ = app.emit(LATER_EVENT, 0);
}

/// Shows a proposal now, keeps a minor one (or one during a meeting) in the
/// quiet list, or queues it while the island is busy.
pub fn offer(app: &AppHandle, mut proposal: Proposal) {
    // Cards show plain text; agents' messages often carry markdown.
    proposal.title = plain(&proposal.title);
    proposal.detail = plain(&proposal.detail);
    // Paused: nothing appears on its own. Notifications wait in "Saved for
    // later"; everything else (a moment that has passed) is dropped.
    if crate::state::is_paused(app) && !shows_while_paused(&proposal.skill_id) {
        if proposal.skill_id.starts_with("notify.") {
            keep_for_later(app, proposal, false);
        } else {
            log::debug!("paused: dropped {}", proposal.skill_id);
        }
        return;
    }
    // Focusing: everything waits, and the end-of-focus list names it.
    if crate::focus::active()
        && proposal.trust != Trust::Auto
        && !shows_while_paused(&proposal.skill_id)
    {
        crate::focus::hold(&proposal.title, &proposal.detail);
        keep_for_later(app, proposal, false);
        return;
    }
    if proposal.trust != Trust::Auto
        && (should_wait(&proposal.skill_id, proposal.priority, in_meeting(app))
            || (proposal.priority < MEETING_FROM
                && !shows_while_paused(&proposal.skill_id)
                && (crate::island::fullscreen()
                    || over_cap(Instant::now())
                    || crate::learn::quiet_here(app, &proposal.skill_id))))
    {
        keep_for_later(app, proposal, false);
        return;
    }
    show_or_queue(app, proposal);
}

/// One short line on why a suggestion showed, from its track record.
fn why_line(taken: i64, dismissed: i64) -> String {
    let total = taken + dismissed;
    if total == 0 {
        "New suggestion. Not now tells me to ask less.".into()
    } else if taken >= dismissed {
        format!("You took this {taken} of {total} times.")
    } else {
        format!("You skipped this {dismissed} of {total} times. It rests if you keep skipping.")
    }
}

/// Markdown marks removed (`**7**` reads 7), for a card's one or two lines.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let line = line.trim_start_matches(['#', '>']).trim();
        let line = line.strip_prefix("- ").unwrap_or(line);
        out.push_str(line);
    }
    let mut out = out.replace("**", "").replace("__", "").replace('`', "");
    // [label](link) keeps the label.
    while let Some(open) = out.find('[') {
        let Some(mid) = out[open..].find("](").map(|i| open + i) else {
            break;
        };
        let Some(close) = out[mid..].find(')').map(|i| mid + i) else {
            break;
        };
        let label = out[open + 1..mid].to_owned();
        out.replace_range(open..=close, &label);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the user started themselves still answers while paused: a coding
/// agent's message.
fn shows_while_paused(skill_id: &str) -> bool {
    skill_id == "mcp.notify"
}

fn show_or_queue(app: &AppHandle, proposal: Proposal) {
    let state = app.state::<AppState>();
    // While the user is away, suggestions wait instead of showing to no one.
    let busy = lock(&state.active).is_some()
        || mascot::current(app) != MascotState::Idle
        || state.away.load(std::sync::atomic::Ordering::Relaxed)
        || state.ask_open.load(std::sync::atomic::Ordering::SeqCst);
    if busy {
        let mut queue = lock(&state.queue);
        // A newer copy of the same suggestion replaces the waiting one;
        // different ones (two servers, two downloads) all stay queued.
        queue.retain(|q| {
            !(q.proposal.skill_id == proposal.skill_id && q.proposal.title == proposal.title)
        });
        if queue.len() >= MAX_QUEUE {
            queue.pop_front();
        }
        queue.push_back(Queued {
            proposal,
            at: Instant::now(),
        });
        return;
    }
    show(app, proposal);
}

fn show(app: &AppHandle, proposal: Proposal) {
    if mascot::dispatch(app, MascotEvent::SkillMatched).is_none() {
        // Raced with another state change; try again shortly.
        let app = app.clone();
        lock(&app.state::<AppState>().queue).push_front(Queued {
            proposal,
            at: Instant::now(),
        });
        schedule_next(&app, NEXT_AFTER_DISMISS);
        return;
    }
    if proposal.trust != Trust::Auto && proposal.priority < MEETING_FROM {
        lock(&SHOWN).push_back(Instant::now());
    }
    let ui = Suggestion {
        id: ulid::Ulid::new().to_string(),
        skill_id: proposal.skill_id.clone(),
        title: proposal.title.clone(),
        detail: proposal.detail.clone(),
        options: proposal.options.iter().map(|o| o.label.clone()).collect(),
        always: proposal
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| can_always(&proposal, i, &o.action))
            .collect(),
        why: if crate::learn::tracked(&proposal.skill_id) {
            let h = lock(&app.state::<AppState>().storage)
                .habit(&proposal.skill_id)
                .unwrap_or_default();
            why_line(h.accepted, h.dismissed)
        } else {
            String::new()
        },
    };
    let priority = proposal.priority;
    let auto = proposal.trust == Trust::Auto
        && proposal.options.first().is_some_and(|o| is_safe(&o.action));
    let id = ui.id.clone();
    log::info!(
        "suggesting {}: {} [{}]",
        ui.skill_id,
        ui.title,
        ui.options.join(", ")
    );
    *lock(&app.state::<AppState>().active) = Some(Active {
        ui: ui.clone(),
        proposal,
    });
    let _ = app.emit(SUGGESTION_NEW, &ui);
    mascot::dispatch(app, MascotEvent::SuggestionReady);
    if auto {
        let _ = run_choice(app, &id, 0, true);
    } else {
        bind_keys(app, ui.options.len());
        crate::voice::offer_spoken(app, &ui, priority);
        expire_when_ignored(app, id);
    }
}

/// An option can become "Always do this" when it is safe to run alone and
/// it will be the one auto runs: the first, or any when the skill remembers
/// the last choice (which then comes first).
pub fn can_always(proposal: &Proposal, index: usize, action: &str) -> bool {
    proposal.trust == Trust::Suggest
        && crate::learn::can_automate(&proposal.skill_id)
        && sidekick_actions::is_safe(action)
        && (index == 0 || proposal.remember.is_some())
}

/// Runs the option and makes the skill automatic from now on.
pub fn always(app: &AppHandle, id: &str, index: usize) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (skill, ok) = {
        let active = lock(&state.active);
        let a = active
            .as_ref()
            .filter(|a| a.ui.id == id)
            .ok_or("That suggestion is gone")?;
        let option = a.proposal.options.get(index).ok_or("No such option")?;
        (
            a.proposal.skill_id.clone(),
            can_always(&a.proposal, index, &option.action),
        )
    };
    if !ok {
        return Err("This one always asks, because it changes things.".into());
    }
    choose(app, id, index)?;
    crate::learn::make_auto(app, &skill).map(|_| ())
}

/// Dismisses a suggestion nobody looked at. Hovering the island
/// restarts the countdown. Lives here rather than in the UI so a stalled
/// webview can never block the queue.
fn expire_when_ignored(app: &AppHandle, id: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut idle = Duration::ZERO;
        loop {
            tokio::time::sleep(EXPIRY_TICK).await;
            let state = app.state::<AppState>();
            let still_active = lock(&state.active).as_ref().is_some_and(|a| a.ui.id == id);
            if !still_active {
                return;
            }
            let limit = Duration::from_secs(u64::from(lock(&state.settings).collapse_after_secs));
            idle = if state.hovered.load(Ordering::Relaxed) {
                Duration::ZERO
            } else {
                idle + EXPIRY_TICK
            };
            if idle >= limit {
                let _ = dismiss(&app, &id, "timeout");
                return;
            }
        }
    });
}

/// Shift+click: open a link option in a private window, in any browser.
pub fn make_private(app: &AppHandle, id: &str, index: usize) {
    let state = app.state::<AppState>();
    let mut current = lock(&state.active);
    if let Some(active) = current.as_mut().filter(|a| a.ui.id == id)
        && let Some(option) = active.proposal.options.get_mut(index)
        && option.action == "open_url"
        && let Some(args) = option.args.as_object_mut()
    {
        args.insert("private".into(), "true".into());
    }
}

pub fn choose(app: &AppHandle, id: &str, index: usize) -> Result<(), String> {
    run_choice(app, id, index, false)
}

fn run_choice(app: &AppHandle, id: &str, index: usize, auto: bool) -> Result<(), String> {
    let active = take(app, id)?;
    let option = active
        .proposal
        .options
        .get(index)
        .cloned()
        .ok_or("option out of range")?;
    if !auto
        && crate::learned::on(app)
        && let Some(key) = &active.proposal.remember
    {
        let ts = Utc::now().to_rfc3339();
        // A kind ("url:github.com") also counts toward all of them ("url").
        if let Some((all, _)) = key.split_once(':') {
            let _ = lock(&app.state::<AppState>().storage).record_choice(all, &option.label, &ts);
        }
        if let Err(err) =
            lock(&app.state::<AppState>().storage).record_choice(key, &option.label, &ts)
        {
            log::warn!("could not remember choice: {err}");
        }
    }
    mascot::dispatch(app, MascotEvent::Picked);
    if active.proposal.skill_id.starts_with("notify.") && option.action != "notify_level" {
        crate::inbox::lesson(format!("Acted on: {}", active.proposal.title));
    }
    let follow_up = crate::learn::on_accept(app, &active.proposal, index, auto);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = execute(&app, &option).await;
        if let Ok(sidekick_actions::Outcome {
            path: Some(path), ..
        }) = &result
        {
            remember_own_file(&app, path);
        }
        let id = log_action(&app, &active.proposal, &option, &result, auto);
        let payload = match &result {
            Ok(outcome) => ActionResult {
                ok: true,
                message: outcome.message.clone(),
                path: outcome.path.clone(),
                auto,
                undo_id: id.filter(|_| {
                    crate::undo::undo_path(&option.action, outcome.path.as_deref()).is_some()
                }),
            },
            Err(message) => ActionResult {
                ok: false,
                message: message.clone(),
                path: None,
                auto,
                undo_id: None,
            },
        };
        let _ = app.emit(ACTION_RESULT, &payload);
        app.state::<AppState>().linger.store(
            payload.ok && (payload.path.is_some() || payload.undo_id.is_some()),
            std::sync::atomic::Ordering::SeqCst,
        );
        mascot::dispatch(
            &app,
            if payload.ok {
                MascotEvent::ActionDone
            } else {
                MascotEvent::ActionFailed
            },
        );
        if let Some(offer) = follow_up {
            offer_later(&app, offer);
        }
        schedule_next(&app, NEXT_AFTER_ACTION);
    });
    Ok(())
}

async fn execute(
    app: &AppHandle,
    option: &ProposedOption,
) -> Result<sidekick_actions::Outcome, String> {
    // App-level actions that need the window system rather than the OS.
    let arg = |name: &str| option.args[name].as_str().filter(|s| !s.is_empty());
    match option.action.as_str() {
        "ask_ai" => {
            crate::ask::open(
                app,
                crate::ask::Open {
                    prompt: Some(arg("prompt").unwrap_or("Help me with this.").to_owned()),
                    ask: true,
                    clipboard: arg("clipboard") != Some("false"),
                    page: arg("page").map(str::to_owned),
                    ..Default::default()
                },
            );
            return Ok(sidekick_actions::Outcome {
                message: "Asking Sidekick".into(),
                path: None,
            });
        }
        "health_fix" => {
            let message = crate::health::fix(app, arg("what").unwrap_or_default()).await?;
            return Ok(sidekick_actions::Outcome {
                message,
                path: None,
            });
        }
        "install_update" => {
            let message = crate::updates::install(app).await?;
            return Ok(sidekick_actions::Outcome {
                message,
                path: None,
            });
        }
        "claude_always" => {
            let id = arg("id").ok_or("no request id")?;
            crate::claude_config::allow_in_project(
                arg("cwd").unwrap_or_default(),
                arg("rule").unwrap_or_default(),
            )?;
            let state = app.state::<AppState>();
            if !state.approvals.decide(id, Some(true)) {
                return Ok(sidekick_actions::Outcome {
                    message: "Saved for next time; answer this one in the terminal".into(),
                    path: None,
                });
            }
            return Ok(sidekick_actions::Outcome {
                message: format!(
                    "Allowed, and from now on in this project: {}",
                    arg("rule").unwrap_or_default()
                ),
                path: None,
            });
        }
        "browser_pair_allow" | "browser_pair_deny" => {
            let id = arg("id").ok_or("no request id")?;
            let allow = option.action == "browser_pair_allow";
            if !app.state::<AppState>().approvals.decide(id, Some(allow)) {
                return Err("The extension stopped waiting; press Connect in it again.".into());
            }
            return Ok(sidekick_actions::Outcome {
                message: if allow {
                    "Browser connected".into()
                } else {
                    "Not connected".into()
                },
                path: None,
            });
        }
        "claude_allow" | "claude_deny" | "claude_pass" => {
            let id = arg("id").ok_or("no request id")?;
            let answer = match option.action.as_str() {
                "claude_allow" => Some(true),
                "claude_deny" => Some(false),
                _ => None,
            };
            let state = app.state::<AppState>();
            if !state.approvals.decide(id, answer) {
                return Err("Claude Code already moved on; answer in the terminal.".into());
            }
            return Ok(sidekick_actions::Outcome {
                message: match answer {
                    Some(true) => "Allowed".into(),
                    Some(false) => "Denied".into(),
                    None => "Answer in the terminal".into(),
                },
                path: None,
            });
        }
        "restore_layout" => {
            return crate::layout::restore(app)
                .await
                .map(|message| sidekick_actions::Outcome {
                    message,
                    path: None,
                });
        }
        "summarize_file" => {
            return crate::files::summarize(app, arg("path").unwrap_or_default())
                .await
                .map(|message| sidekick_actions::Outcome {
                    message,
                    path: None,
                });
        }
        "fathom_followup" => {
            return crate::fathom::follow_up(
                app,
                arg("start").unwrap_or_default(),
                arg("title").unwrap_or("the meeting"),
            )
            .await
            .map(|message| sidekick_actions::Outcome {
                message,
                path: None,
            });
        }
        "open_memory" => {
            crate::ask::open(
                app,
                crate::ask::Open {
                    view: Some("settings"),
                    settings_tab: Some("memory"),
                    ..Default::default()
                },
            );
            return Ok(sidekick_actions::Outcome {
                message: "Opened Memory".into(),
                path: None,
            });
        }
        "skill_auto" => {
            let skill = arg("skill").ok_or("no skill")?;
            return crate::learn::make_auto(app, skill).map(|message| sidekick_actions::Outcome {
                message,
                path: None,
            });
        }
        "save_log" => {
            let (message, path) = crate::moments::save_log(arg("text").unwrap_or_default())?;
            return Ok(sidekick_actions::Outcome {
                message,
                path: Some(path),
            });
        }
        "routine_open_all" | "routine_open" | "routine_skip" | "routine_auto" => {
            let message = match option.action.as_str() {
                "routine_open_all" => crate::routines::open_all(app, true).await?,
                "routine_open" => {
                    let n = arg("index").and_then(|s| s.parse().ok()).unwrap_or(1);
                    crate::routines::open_one(app, n).await?
                }
                "routine_skip" => crate::routines::skip_today(app),
                _ => crate::routines::set_auto(app, arg("on") == Some("true"))?,
            };
            return Ok(sidekick_actions::Outcome {
                message,
                path: None,
            });
        }
        "recipe_run" => {
            let message = crate::recipes::run_by_id(app, arg("id").unwrap_or_default())?;
            return Ok(sidekick_actions::Outcome {
                message,
                path: None,
            });
        }
        "recipe_save" => {
            let message = crate::recipes::save(
                app,
                sidekick_core::Recipe {
                    name: arg("name").unwrap_or_default().to_owned(),
                    prompt: arg("prompt").unwrap_or_default().to_owned(),
                    ..Default::default()
                },
            )?;
            return Ok(sidekick_actions::Outcome {
                message,
                path: None,
            });
        }
        "notify_level" => {
            let level = arg("level")
                .and_then(crate::inbox::Level::parse)
                .ok_or("pick now, soon, digest or never")?;
            let message = crate::inbox::set_level(app, arg("app").unwrap_or_default(), level)?;
            return Ok(sidekick_actions::Outcome {
                message,
                path: None,
            });
        }
        "browser_close_duplicates" | "browser_save_session" => {
            return crate::browser::run(app, &option.action, &option.args).await;
        }
        _ => {}
    }
    let exec = executor(&app.state::<AppState>());
    exec.run(&option.action, &option.args)
        .await
        .map_err(|e| e.to_string())
}

fn log_action(
    app: &AppHandle,
    proposal: &Proposal,
    option: &ProposedOption,
    result: &Result<sidekick_actions::Outcome, String>,
    auto: bool,
) -> Option<i64> {
    let record = ActionRecord {
        id: 0,
        ts: Utc::now().to_rfc3339(),
        skill_id: option.skill_id.clone(),
        action: option.action.clone(),
        label: option.label.clone(),
        ok: result.is_ok(),
        message: match result {
            Ok(o) => o.message.clone(),
            Err(e) => e.clone(),
        },
        auto,
        undo_path: result
            .as_ref()
            .ok()
            .and_then(|o| crate::undo::undo_path(&option.action, o.path.as_deref())),
        undone: false,
    };
    log::info!(
        "{} -> {} ({}): {}",
        proposal.skill_id,
        record.label,
        record.action,
        record.message
    );
    crate::search::index_action(
        app,
        &record.label,
        &record.message,
        &record.skill_id,
        &record.ts,
    );
    match lock(&app.state::<AppState>().storage).log_action(&record) {
        Ok(id) => Some(id),
        Err(err) => {
            log::warn!("could not log action: {err}");
            None
        }
    }
}

pub fn dismiss(app: &AppHandle, id: &str, reason: &str) -> Result<(), String> {
    let active = take(app, id)?;
    log::info!(
        "suggestion from {} dismissed: {reason}",
        active.proposal.skill_id
    );
    crate::learn::on_dismiss(app, &active.proposal.skill_id, reason);
    if reason == "user" && active.proposal.skill_id.starts_with("notify.") {
        let from = active
            .proposal
            .options
            .iter()
            .find_map(|o| o.args["app"].as_str().or(o.args["name"].as_str()))
            .unwrap_or_default()
            .to_owned();
        crate::inbox::lesson(format!("Dismissed: {}", active.proposal.title));
        if let Some(note) = crate::inbox::on_dismiss(app, &from) {
            log::info!("notifications: {note}");
        }
    }
    // Shown while nobody was looking: keep it, so hovering the island
    // later still finds it.
    if reason == "timeout" {
        keep_for_later(app, active.proposal, true);
    }
    mascot::dispatch(app, MascotEvent::Dismissed);
    schedule_next(app, NEXT_AFTER_DISMISS);
    Ok(())
}

/// Removes the active suggestion if `id` matches, and tells the UI.
fn take(app: &AppHandle, id: &str) -> Result<Active, String> {
    let state = app.state::<AppState>();
    let taken = {
        let mut current = lock(&state.active);
        match current.as_ref() {
            Some(a) if a.ui.id == id => current.take(),
            _ => None,
        }
    };
    let active = taken.ok_or("suggestion is no longer active")?;
    unbind_keys(app);
    let _ = app.emit(SUGGESTION_CLEAR, &active.ui.id);
    Ok(active)
}

pub fn current(app: &AppHandle) -> Option<Suggestion> {
    lock(&app.state::<AppState>().active)
        .as_ref()
        .map(|a| a.ui.clone())
}

/// Shows the next queued proposal once the island is free again.
/// How long a file Sidekick made is ignored by the downloads sensor.
const OWN_FILE_FOR: Duration = Duration::from_secs(120);

fn remember_own_file(app: &AppHandle, path: &str) {
    let state = app.state::<AppState>();
    let mut own = lock(&state.own_files);
    own.retain(|_, at| at.elapsed() < OWN_FILE_FOR);
    own.insert(std::path::PathBuf::from(path), Instant::now());
}

/// True for a file one of Sidekick's own actions just created.
pub fn is_own_file(app: &AppHandle, path: &str) -> bool {
    lock(&app.state::<AppState>().own_files)
        .get(std::path::Path::new(path))
        .is_some_and(|at| at.elapsed() < OWN_FILE_FOR)
}

/// Queues a follow-up so it shows after the current result.
fn offer_later(app: &AppHandle, proposal: Proposal) {
    lock(&app.state::<AppState>().queue).push_back(Queued {
        proposal,
        at: Instant::now(),
    });
}

/// Shows the next queued suggestion, if any (after a held result).
pub fn resume(app: &AppHandle) {
    schedule_next(app, NEXT_AFTER_DISMISS);
}

/// The user is back: what arrived while they were away counts as fresh.
pub fn welcome_back(app: &AppHandle) {
    let now = Instant::now();
    for q in lock(&app.state::<AppState>().queue).iter_mut() {
        q.at = now;
    }
    schedule_next(app, WELCOME_BACK);
}

fn schedule_next(app: &AppHandle, after: Duration) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(after).await;
        let state = app.state::<AppState>();
        if state.away.load(std::sync::atomic::Ordering::Relaxed)
            || state.ask_open.load(std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        if lock(&state.active).is_some() || mascot::current(&app) != MascotState::Idle {
            // Still busy: the action that finishes will schedule again.
            if mascot::current(&app) == MascotState::Sleeping {
                lock(&state.queue).drain(..);
            }
            return;
        }
        let next = {
            let mut queue = lock(&state.queue);
            let mut retained = std::collections::VecDeque::new();
            for q in queue.drain(..) {
                if q.at.elapsed() < STALE_AFTER {
                    retained.push_back(q);
                }
            }
            *queue = retained;
            queue.pop_front()
        };
        if let Some(q) = next {
            show(&app, q.proposal);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn why_line_reads_the_track_record() {
        assert!(why_line(0, 0).starts_with("New"));
        assert_eq!(why_line(4, 1), "You took this 4 of 5 times.");
        assert!(why_line(1, 3).starts_with("You skipped this 3 of 4"));
    }

    #[test]
    fn cards_show_plain_text() {
        assert_eq!(
            plain("Done, Calculator is open with **7** displayed."),
            "Done, Calculator is open with 7 displayed."
        );
        assert_eq!(
            plain("## Fixed\n- see [the PR](https://x.y/1) and `cargo test`"),
            "Fixed see the PR and cargo test"
        );
    }

    #[test]
    fn moments_leave_the_missed_list_sooner() {
        assert_eq!(
            later_keep("clipboard.long-text"),
            chrono::Duration::minutes(10)
        );
        assert_eq!(
            later_keep("files.screenshot"),
            chrono::Duration::minutes(30)
        );
        assert_eq!(later_keep("notify.now"), LATER_KEEP);
        assert!(shows_while_paused("mcp.notify"));
        assert!(!shows_while_paused("system.late-night"));
    }

    #[test]
    fn minor_suggestions_wait_but_what_you_just_did_does_not() {
        assert!(should_wait("files.copy", 10, false));
        assert!(!should_wait("files.download", 50, false));
        assert!(
            should_wait("files.download", 50, true),
            "meetings hold most things"
        );
        assert!(!should_wait("dev.claude-permission", 95, true));
        assert!(!should_wait("clipboard.color", 35, true));
        assert!(!should_wait("browser.login", 50, true));
    }
}
