//! Voice in the app: "Hey Sidekick" opens Ask mode and shows what you say
//! as you say it; when you stop, the question goes to the AI and the answer
//! is read aloud with Supertonic. Speech models are prefetched at build/dev into
//! resources, copied into app data on launch, and only downloaded from GitHub
//! when still missing (the voice first). The assistant stays on "Preparing voice"
//! until the voice can speak; then welcome opens and greets. The microphone stops
//! whenever voice is off or Sidekick is paused.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Utc;
use serde::Serialize;
use sidekick_core::MascotEvent;
use sidekick_core::settings::VoiceSettings;
use sidekick_voice::text::strip_wake;
use sidekick_voice::{Control, Heard, Listener, ListenerConfig, Speaker, SpeechEvent, models};
use tauri::{AppHandle, Emitter, Manager};

use crate::ask;
use crate::mascot;
use crate::state::{AppState, lock};

pub const STATE_EVENT: &str = "voice://state";
pub const HEARD_EVENT: &str = "voice://heard";
pub const DOWNLOAD_EVENT: &str = "voice://download";

pub const WELCOME_EVENT: &str = "voice://welcome";

/// What Sidekick says on each welcome step (Welcome, Your AI, Connect,
/// Tools, Extras), and what the step shows, word by word as it is heard.
/// Punctuation is the direction: the voice lifts on "!" and "?" and
/// breathes at commas.
pub const WELCOME_LINES: [&str; 5] = [
    "Hey — I'm Sidekick. I live up here with you on this PC. I notice things, I help when you want, and I stay put: nothing leaves this machine, and I wait for your okay.",
    "First, how I think. If you want everything to stay on this PC, you can run a local model — only if your machine is up for it. Or use Claude Code or Codex with the plan you already have. You can use any of them, or all three, and set the order I try.",
    "Now, your world. Connect your calendar, your mail and the tools you use, and I'll start noticing what matters.",
    "A few small helpers make me sharper. Install the ones you want, and I'll wait while they finish.",
    "Welcome aboard. Say Hey Sidekick whenever you need me. Do Not Disturb is on so Windows stays quiet and alerts show once up here. Launch on login is already on, so I'm here when you sit down.",
];

/// When each sentence of the welcome line sounds, in Unix milliseconds, so
/// the UI can show the words as they are heard.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WelcomeSpeech {
    /// The step being spoken, and every step's line.
    pub step: u32,
    pub lines: [&'static str; 5],
    pub script: &'static str,
    pub pieces: Vec<SpokenPiece>,
    /// When the last word stops sounding; known once everything is queued.
    pub ends_at: Option<u64>,
    /// Nothing will be heard (no speakers, or the voice failed): the UI
    /// paces the words itself.
    pub silent: bool,
    /// Speech was asked for and is on its way (the model may still be
    /// loading): the UI waits for it instead of pacing the words itself.
    pub pending: bool,
    #[serde(skip)]
    key: Option<(u64, u64)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpokenPiece {
    pub text: String,
    pub starts_at: u64,
    pub ms: u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// What the runtime was built from; a change rebuilds it.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    wake_word: bool,
    interrupt: bool,
    voice: String,
    speed: f32,
    /// The assistant's name: "hey <name>" wakes it, so a rename rebuilds.
    name: String,
    /// The microphone opens only once onboarding is done; until then
    /// Sidekick speaks the welcome but never listens.
    listen: bool,
}

struct Runtime {
    key: Key,
    listener: Option<Listener>,
    speaker: Option<Speaker>,
}

pub struct Voice {
    pub models: PathBuf,
    runtime: Mutex<Option<Runtime>>,
    /// Speaker kept alive for welcome / say when full voice is not on yet.
    greeter: Mutex<Option<Speaker>>,
    /// A rebuild is running (loading models takes a moment).
    starting: AtomicBool,
    downloading: AtomicBool,
    cancel_download: AtomicBool,
    /// Ask mode was opened by the wake word, so it closes again if nothing
    /// was said.
    opened_by_voice: AtomicBool,
    /// Listening because the user pressed to talk, not because a wake
    /// phrase was heard; short answers are taken as said.
    pushed: AtomicBool,
    /// Spoken the first-run welcome line once this process.
    welcome_greeted: AtomicBool,
    /// Welcome opened before the voice was ready; greet after download.
    pending_welcome_greet: AtomicBool,
    /// The chat being read aloud and its utterance.
    speaking: Mutex<Option<(String, u64)>>,
    /// The first-run welcome line as it is being heard.
    welcome: Mutex<WelcomeSpeech>,
    /// A suggestion read aloud, waiting for a spoken choice: its id and
    /// option labels.
    choosing: Mutex<Option<(String, Vec<String>)>>,
    last_error: Mutex<Option<String>>,
}

impl Voice {
    pub fn new(models: PathBuf) -> Self {
        Self {
            models,
            runtime: Mutex::default(),
            greeter: Mutex::default(),
            starting: AtomicBool::new(false),
            downloading: AtomicBool::new(false),
            cancel_download: AtomicBool::new(false),
            opened_by_voice: AtomicBool::new(false),
            pushed: AtomicBool::new(false),
            welcome_greeted: AtomicBool::new(false),
            pending_welcome_greet: AtomicBool::new(false),
            speaking: Mutex::default(),
            welcome: Mutex::new(WelcomeSpeech {
                lines: WELCOME_LINES,
                script: WELCOME_LINES[0],
                ..Default::default()
            }),
            choosing: Mutex::default(),
            last_error: Mutex::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub size: u64,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    pub models: Vec<ModelStatus>,
    pub missing_bytes: u64,
    pub downloading: bool,
    /// The microphone is open and waiting for the wake word or a question.
    pub listening: bool,
    pub error: Option<String>,
    pub voices: Vec<VoiceOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VoiceOption {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HeardPayload {
    text: String,
    #[serde(rename = "final")]
    done: bool,
    /// The words have settled for a moment; an answer may start early.
    pause: bool,
    /// Ask mode was opened by the wake word.
    by_voice: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadPayload {
    label: &'static str,
    done: u64,
    total: u64,
    finished: bool,
    error: Option<String>,
}

fn voice(app: &AppHandle) -> &Voice {
    &app.state::<AppState>().inner().voice
}

pub fn status(app: &AppHandle) -> VoiceStatus {
    let v = voice(app);
    VoiceStatus {
        models: models::MODELS
            .iter()
            .map(|m| ModelStatus {
                id: m.id,
                label: m.label,
                size: m.size,
                installed: m.installed(&v.models),
            })
            .collect(),
        missing_bytes: models::missing_bytes(&v.models),
        downloading: v.downloading.load(Ordering::SeqCst),
        listening: lock(&v.runtime)
            .as_ref()
            .is_some_and(|r| r.listener.is_some()),
        error: lock(&v.last_error).clone(),
        voices: sidekick_voice::VOICES
            .iter()
            .map(|(id, label, _)| VoiceOption { id, label })
            .collect(),
    }
}

fn emit_state(app: &AppHandle) {
    let _ = app.emit(STATE_EVENT, status(app));
}

/// Starts, rebuilds or stops the voice runtime to match settings and pause.
pub fn refresh(app: &AppHandle) {
    let (settings, paused, onboarded, name) = {
        let state = app.state::<AppState>();
        let s = lock(&state.settings);
        (
            s.voice.clone(),
            s.pause.is_active(Utc::now()),
            s.onboarded,
            s.assistant_name.clone(),
        )
    };
    sidekick_voice::text::set_name(&name);
    let v = voice(app);
    // The speaker also runs for typed answers when "Speak answers" is on,
    // even with the wake word and microphone off.
    let listen = settings.enabled && models::all_installed(&v.models);
    let speak = settings.speak_answers && models::VOICE.installed(&v.models);
    let wanted = !paused && (listen || speak);
    if !wanted {
        // Taken out first: dropping it joins the listener thread, which may
        // be waiting for this lock.
        let old = lock(&v.runtime).take();
        if old.is_some() {
            drop(old);
            log::info!("voice off");
        }
        emit_state(app);
        return;
    }
    let key = Key {
        wake_word: settings.wake_word,
        interrupt: settings.interrupt,
        voice: settings.voice.clone(),
        speed: settings.speed,
        name,
        listen: onboarded && listen,
    };
    if lock(&v.runtime).as_ref().is_some_and(|r| r.key == key) {
        return;
    }
    if v.starting.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    // Only the voice or speed changed: swap the speaker and keep the
    // microphone listening. The old voice answers until the new one loads.
    let same_ears = lock(&v.runtime).as_ref().is_some_and(|r| {
        r.key.wake_word == key.wake_word
            && r.key.interrupt == key.interrupt
            && r.key.listen == key.listen
            && r.key.name == key.name
    });
    if same_ears {
        std::thread::spawn(move || {
            let v = voice(&app);
            match Speaker::start_with_events(
                &v.models,
                &settings.voice,
                settings.speed,
                Some(speech_events(&app)),
            ) {
                Ok(speaker) => {
                    let old = lock(&v.runtime).as_mut().and_then(|r| {
                        r.key = key;
                        r.speaker.replace(speaker)
                    });
                    drop(old);
                }
                Err(e) => log::warn!("voice swap: {e}"),
            }
            v.starting.store(false, Ordering::SeqCst);
            emit_state(&app);
            refresh(&app);
        });
        return;
    }
    std::thread::spawn(move || {
        // Stop the old one first so the microphone is free.
        let old = lock(&voice(&app).runtime).take();
        drop(old);
        // Full runtime owns the speakers from now on. The welcome greeter
        // goes once it is quiet, so a line being said is not cut off.
        if let Some(greeter) = lock(&voice(&app).greeter).take() {
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                while greeter.busy() && started.elapsed() < MAX_SPEECH {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                drop(greeter);
            });
        }
        let runtime = build(&app, key, &settings);
        let v = voice(&app);
        *lock(&v.runtime) = Some(runtime);
        v.starting.store(false, Ordering::SeqCst);
        emit_state(&app);
        // Settings may have changed while loading.
        refresh(&app);
    });
}

/// True while Sidekick's voice plays, false when it stops.
pub const SPEAKING_EVENT: &str = "voice://speaking";
/// How often the echo guard checks whether Sidekick is speaking.
const ECHO_CHECK: std::time::Duration = std::time::Duration::from_millis(50);
/// The room still rings a moment after the speakers go quiet.
const ECHO_TAIL: std::time::Duration = std::time::Duration::from_millis(400);

/// Keeps the microphone deaf while Sidekick's own voice plays, so a line
/// like "just say Hey Sidekick" never wakes it. Runs for the whole process.
/// Only changes are sent (and the current state to a newly built listener):
/// a push to talk right after speaking unmutes and must stay unmuted.
pub fn start_echo_guard(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("sidekick-echo-guard".into())
        .spawn(move || {
            let mut quiet_since: Option<std::time::Instant> = None;
            // The listener told last, and what it was told.
            let mut told: Option<(usize, bool)> = None;
            // What the island was told about Sidekick talking.
            let mut said_speaking = false;
            loop {
                std::thread::sleep(ECHO_CHECK);
                let v = voice(&app);
                let talking = lock(&v.greeter).as_ref().is_some_and(Speaker::busy)
                    || lock(&v.runtime)
                        .as_ref()
                        .and_then(|r| r.speaker.as_ref())
                        .is_some_and(Speaker::busy);
                if talking != said_speaking {
                    said_speaking = talking;
                    // The mascot talks along.
                    let _ = app.emit(SPEAKING_EVENT, talking);
                }
                let greeting = lock(&v.greeter).as_ref().is_some_and(Speaker::busy);
                let runtime = lock(&v.runtime);
                let Some(listener) = runtime.as_ref().and_then(|r| r.listener.as_ref()) else {
                    told = None;
                    continue;
                };
                let speaking = greeting
                    || runtime
                        .as_ref()
                        .and_then(|r| r.speaker.as_ref())
                        .is_some_and(Speaker::busy);
                let muted = if speaking {
                    quiet_since = None;
                    true
                } else {
                    let since = *quiet_since.get_or_insert_with(std::time::Instant::now);
                    since.elapsed() < ECHO_TAIL
                };
                let id = std::ptr::from_ref(listener) as usize;
                if told != Some((id, muted)) {
                    listener.send(Control::Mute(muted));
                    told = Some((id, muted));
                }
            }
        })
        .expect("could not start the echo guard thread");
}

/// True when the voice model is on disk and can speak the welcome line.
pub fn voice_ready(app: &AppHandle) -> bool {
    models::VOICE.installed(&voice(app).models)
}

/// Copies shipped voice models from the install/resources tree into app data
/// when they are missing. Fast and offline; no network.
pub fn seed_from_bundle(app: &AppHandle) {
    let dest = &voice(app).models;
    let removed = models::remove_retired(dest);
    if removed > 0 {
        log::info!("voice: removed {removed} replaced model(s)");
    }
    let Some(src) = bundled_models(app) else {
        return;
    };
    for model in models::MODELS {
        if model.installed(dest) {
            continue;
        }
        let from = src.join(model.dir);
        if !from.is_dir() {
            continue;
        }
        let to = model.path(dest);
        match copy_dir(&from, &to) {
            Ok(()) => log::info!("voice: seeded {} from bundle", model.id),
            Err(e) => log::warn!("voice: could not seed {}: {e}", model.id),
        }
    }
}

fn bundled_models(app: &AppHandle) -> Option<std::path::PathBuf> {
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|d| d.join("voice-models"));
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/voice-models");
    [bundled, Some(repo)]
        .into_iter()
        .flatten()
        .find(|p| p.is_dir())
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Downloads any missing speech models in the background (idempotent).
/// Installs the voice first so the welcome greeting can speak ASAP.
pub fn ensure_models(app: &AppHandle) {
    let v = voice(app);
    if models::all_installed(&v.models) {
        return;
    }
    if let Err(err) = download(app) {
        log::warn!("voice models: {err}");
    }
}

/// Seeds from bundle, then downloads if the voice is still missing. Opens welcome
/// once the voice can speak (used when onboarding is locked).
pub fn prepare_then_welcome(app: &AppHandle) {
    seed_from_bundle(app);
    if voice_ready(app) {
        refresh(app);
        ask::ensure_welcome(app);
        return;
    }
    // Mark greet pending so download completion opens welcome + speaks.
    voice(app)
        .pending_welcome_greet
        .store(true, Ordering::SeqCst);
    ensure_models(app);
    emit_state(app);
}

/// Speaks `text` with the live runtime speaker, or a short-lived greeter.
/// `started` gets the speaker and utterance ids before any
/// audio is made, so its events can be told apart. False if nothing will be
/// heard.
fn say_with(app: &AppHandle, text: &str, started: impl FnOnce(u64, u64)) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    let speak = |s: &Speaker| {
        let id = s.begin();
        started(s.id(), id);
        s.push(id, text);
        s.finish(id);
    };
    let v = voice(app);
    {
        let runtime = lock(&v.runtime);
        if let Some(s) = runtime.as_ref().and_then(|r| r.speaker.as_ref()) {
            speak(s);
            return true;
        }
    }
    if !models::VOICE.installed(&v.models) {
        return false;
    }
    let settings = lock(&app.state::<AppState>().settings).voice.clone();
    let mut greeter = lock(&v.greeter);
    if greeter.is_none() {
        match Speaker::start_with_events(
            &v.models,
            &settings.voice,
            settings.speed,
            Some(speech_events(app)),
        ) {
            Ok(s) => *greeter = Some(s),
            Err(e) => {
                log::warn!("voice greet: {e}");
                return false;
            }
        }
    }
    match greeter.as_ref() {
        Some(s) => {
            speak(s);
            true
        }
        None => false,
    }
}

/// Forwards what a speaker is saying to the welcome timeline, when it is
/// the welcome line.
fn speech_events(app: &AppHandle) -> sidekick_voice::SpeechEvents {
    let app = app.clone();
    std::sync::Arc::new(move |event| {
        let v = voice(&app);
        // The first sound of an answer to a spoken question: how long after
        // the question ended.
        if let SpeechEvent::Piece {
            utterance,
            starts_in,
            ..
        } = &event
            && lock(&v.speaking)
                .as_ref()
                .is_some_and(|(_, u)| u == utterance)
            && let Some(ended) = lock(&SPEECH_ENDED).take()
        {
            let ms = (ended.elapsed() + *starts_in).as_millis() as u64;
            crate::timings::record(&app, crate::timings::SPEECH_TO_FIRST_SOUND, ms);
        }
        let mut w = lock(&v.welcome);
        match event {
            SpeechEvent::Piece {
                speaker,
                utterance,
                text,
                starts_in,
                length,
            } if w.key == Some((speaker, utterance)) => w.pieces.push(SpokenPiece {
                text,
                starts_at: now_ms() + starts_in.as_millis() as u64,
                ms: length.as_millis() as u64,
            }),
            SpeechEvent::End {
                speaker,
                utterance,
                ends_in,
            } if w.key == Some((speaker, utterance)) => {
                w.ends_at = Some(now_ms() + ends_in.as_millis() as u64);
                w.pending = false;
            }
            _ => return,
        }
        let _ = app.emit(WELCOME_EVENT, w.clone());
    })
}

/// The welcome line and when its words are heard.
pub fn welcome_speech(app: &AppHandle) -> WelcomeSpeech {
    lock(&voice(app).welcome).clone()
}

/// Speaks welcome step `step`, cutting off whatever was being said.
pub fn speak_welcome_step(app: &AppHandle, step: u32) {
    let step = step.min(WELCOME_LINES.len() as u32 - 1);
    let script = WELCOME_LINES[step as usize];
    let v = voice(app);
    let heard = say_with(app, script, |speaker, utterance| {
        let mut w = lock(&v.welcome);
        *w = WelcomeSpeech {
            step,
            lines: WELCOME_LINES,
            script,
            key: Some((speaker, utterance)),
            pending: true,
            ..Default::default()
        };
        let _ = app.emit(WELCOME_EVENT, w.clone());
    });
    if !heard {
        let mut w = lock(&v.welcome);
        *w = WelcomeSpeech {
            step,
            lines: WELCOME_LINES,
            script,
            silent: true,
            ..Default::default()
        };
        let _ = app.emit(WELCOME_EVENT, w.clone());
    }
}

/// Says a short line (e.g. "Done. Composio is connected.") right away.
pub fn say_now(app: &AppHandle, text: &str) {
    say_with(app, text, |_, _| {});
}

/// Speaks the first-run welcome once models are ready. Queues until the voice
/// finishes downloading if needed.
pub fn welcome_greet(app: &AppHandle) {
    if lock(&app.state::<AppState>().settings).onboarded {
        return;
    }
    let v = voice(app);
    v.pending_welcome_greet.store(true, Ordering::SeqCst);
    if !models::VOICE.installed(&v.models) {
        ensure_models(app);
        return;
    }
    enable_voice_for_onboarding(app);
    if v.welcome_greeted.swap(true, Ordering::SeqCst) {
        v.pending_welcome_greet.store(false, Ordering::SeqCst);
        return;
    }
    v.pending_welcome_greet.store(false, Ordering::SeqCst);
    let step = lock(&app.state::<AppState>().settings).welcome_step;
    speak_welcome_step(app, step);
}

/// Turns voice on during onboarding so wake word and speaking work after setup.
fn enable_voice_for_onboarding(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut settings = lock(&state.settings).clone();
    if settings.onboarded || settings.voice.enabled {
        return;
    }
    settings.voice.enabled = true;
    if let Err(err) = crate::commands::apply_settings(app, settings) {
        log::warn!("could not enable voice for welcome: {err}");
    }
}

fn build(app: &AppHandle, key: Key, settings: &VoiceSettings) -> Runtime {
    let v = voice(app);
    let mut errors = Vec::new();
    let speaker = match Speaker::start_with_events(
        &v.models,
        &settings.voice,
        settings.speed,
        Some(speech_events(app)),
    ) {
        Ok(s) => Some(s),
        Err(e) => {
            errors.push(format!("Speech: {e}"));
            None
        }
    };
    let handle = app.clone();
    let listener = if !key.listen {
        None
    } else {
        match Listener::start(
            ListenerConfig {
                models: v.models.clone(),
                wake_word: settings.wake_word,
                interrupt: settings.interrupt,
            },
            move |heard| on_heard(&handle, heard),
        ) {
            Ok(l) => Some(l),
            Err(e) => {
                errors.push(format!("Microphone: {e}"));
                None
            }
        }
    };
    *lock(&v.last_error) = (!errors.is_empty()).then(|| errors.join(" "));
    if errors.is_empty() {
        log::info!("voice on (wake word: {})", settings.wake_word);
    } else {
        log::warn!("voice: {}", errors.join(" "));
    }
    Runtime {
        key,
        listener,
        speaker,
    }
}

fn on_heard(app: &AppHandle, heard: Heard) {
    let v = voice(app);
    match heard {
        Heard::Wake => {
            // Talking over an answer stops it.
            stop_speaking(app);
            // Load the model while the question is still being spoken.
            crate::ai::warm_up(app);
            if lock(&v.choosing).is_some() {
                // Listening for a choice, not a question: Ask stays closed.
                mascot::dispatch(app, MascotEvent::ListenStart);
                return;
            }
            // The island listens in its compact shape; Ask opens only when
            // the answer starts coming in.
            if !ask::is_open(app) {
                v.opened_by_voice.store(true, Ordering::SeqCst);
            }
            mascot::dispatch(app, MascotEvent::ListenStart);
            emit_heard(app, String::new(), false);
        }
        Heard::Partial(text) => emit_heard(app, strip_wake(&text), false),
        // A choice ("open", "not now") is taken when it is final.
        Heard::Pause(text) if lock(&v.choosing).is_none() => {
            let text = strip_wake(&text);
            if sidekick_voice::text::looks_like_request(&text) {
                emit_pause(app, text);
            }
        }
        Heard::Pause(_) => {}
        Heard::Final(text) => {
            let mut text = strip_wake(&text);
            let pushed = v.pushed.swap(false, Ordering::SeqCst);
            if let Some((id, labels)) = lock(&v.choosing).take() {
                mascot::dispatch(app, MascotEvent::Cancelled);
                // Clears what showed as heard; an empty answer asks nothing.
                emit_heard(app, String::new(), true);
                let result = match match_choice(&text, &labels) {
                    Choice::Option(i) => crate::suggestions::choose(app, &id, i),
                    Choice::Dismiss => crate::suggestions::dismiss(app, &id, "voice"),
                    Choice::Unclear => Ok(()),
                };
                if let Err(err) = result {
                    log::debug!("spoken choice ignored: {err}");
                }
                return;
            }
            // A wake on a stray sound turns noise into a word or two
            // ("byzant mixed"); that is not a question, so go back to rest.
            if !pushed && !text.is_empty() && !sidekick_voice::text::looks_like_request(&text) {
                log::debug!("voice: ignored {text:?}");
                text.clear();
            }
            mascot::dispatch(app, MascotEvent::Cancelled);
            *lock(&SPEECH_ENDED) = (!text.is_empty()).then(std::time::Instant::now);
            emit_heard(app, text, true);
            v.opened_by_voice.store(false, Ordering::SeqCst);
        }
        Heard::Error(err) => {
            *lock(&v.last_error) = Some(err);
            v.pushed.store(false, Ordering::SeqCst);
            mascot::dispatch(app, MascotEvent::Cancelled);
            // Whatever was showing as heard goes away.
            emit_heard(app, String::new(), true);
            v.opened_by_voice.store(false, Ordering::SeqCst);
            // This runs on the listener thread, which cannot join itself.
            let app = app.clone();
            std::thread::spawn(move || {
                let old = lock(&voice(&app).runtime).take();
                drop(old);
                emit_state(&app);
                // Start the microphone again (a headset unplugged, the PC
                // woke up); if it is really gone, the error stays shown.
                std::thread::sleep(std::time::Duration::from_secs(2));
                refresh(&app);
            });
        }
    }
}

fn emit_heard(app: &AppHandle, text: String, done: bool) {
    let by_voice = voice(app).opened_by_voice.load(Ordering::SeqCst);
    let _ = app.emit(
        HEARD_EVENT,
        HeardPayload {
            text,
            done,
            pause: false,
            by_voice,
        },
    );
}

/// When the last spoken question ended, until its answer is first heard.
static SPEECH_ENDED: Mutex<Option<std::time::Instant>> = Mutex::new(None);

fn emit_pause(app: &AppHandle, text: String) {
    let by_voice = voice(app).opened_by_voice.load(Ordering::SeqCst);
    let _ = app.emit(
        HEARD_EVENT,
        HeardPayload {
            text,
            done: false,
            pause: true,
            by_voice,
        },
    );
}

/// Push to talk from Ask mode: listen now, no wake word needed.
pub fn listen(app: &AppHandle) -> Result<(), String> {
    let v = voice(app);
    let runtime = lock(&v.runtime);
    let Some(listener) = runtime.as_ref().and_then(|r| r.listener.as_ref()) else {
        return Err("Voice is off. Turn it on in Settings > Voice.".into());
    };
    v.pushed.store(true, Ordering::SeqCst);
    listener.send(Control::ListenNow);
    Ok(())
}

/// Stops listening and speaking (Esc, Stop).
pub fn stop(app: &AppHandle) {
    let v = voice(app);
    v.pushed.store(false, Ordering::SeqCst);
    if let Some(l) = lock(&v.runtime).as_ref().and_then(|r| r.listener.as_ref()) {
        l.send(Control::Cancel);
    }
    stop_speaking(app);
    if mascot::current(app) == sidekick_core::MascotState::Listening {
        mascot::dispatch(app, MascotEvent::Cancelled);
    }
}

fn stop_speaking(app: &AppHandle) {
    let v = voice(app);
    lock(&v.speaking).take();
    if let Some(s) = lock(&v.runtime).as_ref().and_then(|r| r.speaker.as_ref()) {
        s.stop();
    }
    if let Some(s) = lock(&v.greeter).as_ref() {
        s.stop();
    }
}

/// Reads chat `id`'s answer aloud as it streams. False when voice is off.
pub fn begin_answer(app: &AppHandle, id: &str) -> bool {
    let v = voice(app);
    let Some(utterance) = lock(&v.runtime)
        .as_ref()
        .and_then(|r| r.speaker.as_ref())
        .map(Speaker::begin)
    else {
        return false;
    };
    *lock(&v.speaking) = Some((id.to_owned(), utterance));
    true
}

fn with_answer(app: &AppHandle, id: &str, f: impl FnOnce(&Speaker, u64)) {
    let v = voice(app);
    let Some((_, utterance)) = lock(&v.speaking).clone().filter(|(chat, _)| chat == id) else {
        return;
    };
    if let Some(s) = lock(&v.runtime).as_ref().and_then(|r| r.speaker.as_ref()) {
        f(s, utterance);
    }
}

pub fn answer_text(app: &AppHandle, id: &str, text: &str) {
    with_answer(app, id, |s, u| s.push(u, text));
}

pub fn answer_done(app: &AppHandle, id: &str, error: Option<&str>) {
    let mut spoke = false;
    with_answer(app, id, |s, u| {
        if error.is_some() {
            s.push(u, "\nSorry, that did not work. The details are on screen.");
        }
        s.finish(u);
        spoke = true;
    });
    let conversation = lock(&app.state::<AppState>().settings).voice.conversation;
    if spoke && conversation && error.is_none() {
        let (app, id) = (app.clone(), id.to_owned());
        // Listen for a reply once the answer has been read out, unless the
        // user moved on (closed Ask, or another answer started).
        listen_after_speaking(&app, move |app| {
            ask::is_open(app)
                && lock(&voice(app).speaking)
                    .as_ref()
                    .is_some_and(|(chat, _)| *chat == id)
        });
    }
}

/// Waits for the speaker to go quiet (at most `MAX_SPEECH`), then starts
/// listening if `still_wanted` holds.
fn listen_after_speaking(
    app: &AppHandle,
    still_wanted: impl Fn(&AppHandle) -> bool + Send + 'static,
) {
    let app = app.clone();
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        // Give the speaker a moment to start, then wait until it is quiet.
        std::thread::sleep(std::time::Duration::from_millis(300));
        while started.elapsed() < MAX_SPEECH {
            let busy = lock(&voice(&app).runtime)
                .as_ref()
                .and_then(|r| r.speaker.as_ref())
                .is_some_and(Speaker::busy);
            if !busy {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        if still_wanted(&app) {
            let _ = listen(&app);
        }
    });
}

const MAX_SPEECH: std::time::Duration = std::time::Duration::from_secs(90);
/// How long a spoken suggestion waits for an answer.
const CHOICE_WAIT: std::time::Duration = std::time::Duration::from_secs(6);
/// Suggestions below this priority are not read aloud.
const SPEAK_FROM_PRIORITY: i32 = 50;

fn suggestion_speech_blocked(app: &AppHandle) -> bool {
    mascot::current(app) == sidekick_core::MascotState::Listening
        || lock(&voice(app).speaking).is_some()
        || !lock(&app.state::<AppState>().chats).is_empty()
}

/// Reads a suggestion aloud ("Report.pdf downloaded. Say open, show in
/// folder, or not now.") and takes a spoken choice.
pub fn offer_spoken(app: &AppHandle, ui: &crate::state::Suggestion, priority: i32) {
    let settings = lock(&app.state::<AppState>().settings).voice.clone();
    if !settings.enabled
        || !settings.speak_suggestions
        || priority < SPEAK_FROM_PRIORITY
        || ui.options.is_empty()
        || ask::is_open(app)
        || *lock(&app.state::<AppState>().island_hidden)
        || suggestion_speech_blocked(app)
    {
        return;
    }
    let v = voice(app);
    {
        let runtime = lock(&v.runtime);
        let Some(speaker) = runtime.as_ref().and_then(|r| r.speaker.as_ref()) else {
            return;
        };
        speaker.say(&spoken_prompt(&ui.title, &ui.detail, &ui.options));
    }
    *lock(&v.speaking) = None;
    let (id, labels) = (ui.id.clone(), ui.options.clone());
    *lock(&v.choosing) = Some((id.clone(), labels));
    let wanted_id = id.clone();
    listen_after_speaking(app, move |app| {
        crate::suggestions::current(app).is_some_and(|s| s.id == wanted_id)
            && lock(&voice(app).choosing).is_some()
    });
    // Nobody answered: stop listening and leave the suggestion on screen.
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(MAX_SPEECH.min(std::time::Duration::from_secs(30)) + CHOICE_WAIT);
        let v = voice(&app);
        let stale = lock(&v.choosing)
            .as_ref()
            .is_some_and(|(cid, _)| *cid == id);
        if stale {
            lock(&v.choosing).take();
            if let Some(l) = lock(&v.runtime).as_ref().and_then(|r| r.listener.as_ref()) {
                l.send(Control::Cancel);
            }
        }
    });
}

/// "Report.pdf. Downloaded, 2 MB. Say open, show in folder, or not now."
pub fn spoken_prompt(title: &str, detail: &str, options: &[String]) -> String {
    let mut out = title.trim().trim_end_matches('.').to_owned();
    let detail = detail.replace('·', ",");
    if !detail.trim().is_empty() {
        out.push_str(". ");
        out.push_str(detail.trim().trim_end_matches('.'));
    }
    let names: Vec<String> = options.iter().take(3).map(|o| o.to_lowercase()).collect();
    out.push_str(". Say ");
    out.push_str(&names.join(", "));
    out.push_str(", or not now.");
    out
}

#[derive(Debug, PartialEq)]
pub enum Choice {
    Option(usize),
    Dismiss,
    Unclear,
}

/// What a spoken answer picks: an option by name or position, "yes" for
/// the first, or "not now".
pub fn match_choice(text: &str, labels: &[String]) -> Choice {
    let norm = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let said = norm(text);
    if said.is_empty() {
        return Choice::Unclear;
    }
    let padded = format!(" {said} ");
    // An option named in full wins, longest name first ("open in chrome"
    // before "open").
    let mut by_len: Vec<(usize, String)> = labels.iter().map(|l| norm(l)).enumerate().collect();
    by_len.sort_by_key(|(_, l)| std::cmp::Reverse(l.len()));
    for (i, label) in &by_len {
        if !label.is_empty() && padded.contains(&format!(" {label} ")) {
            return Choice::Option(*i);
        }
    }
    let has = |words: &[&str]| words.iter().any(|w| padded.contains(&format!(" {w} ")));
    if has(&[
        "not now",
        "no",
        "nope",
        "later",
        "dismiss",
        "cancel",
        "skip",
        "never mind",
        "ignore",
    ]) {
        return Choice::Dismiss;
    }
    for (i, words) in [
        &["first", "first one", "number one"][..],
        &["second", "second one", "two", "number two"][..],
        &["third", "third one", "three", "number three"][..],
    ]
    .iter()
    .enumerate()
    {
        if i < labels.len() && has(words) {
            return Choice::Option(i);
        }
    }
    // One word of an option ("chrome" for "Open in Chrome"), if only one
    // option has it.
    let hits: Vec<usize> = labels
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            norm(l)
                .split(' ')
                .filter(|w| w.len() > 3)
                .any(|w| padded.contains(&format!(" {w} ")))
        })
        .map(|(i, _)| i)
        .collect();
    if hits.len() == 1 {
        return Choice::Option(hits[0]);
    }
    if has(&[
        "yes", "yeah", "yep", "sure", "ok", "okay", "do it", "go ahead", "please",
    ]) {
        return Choice::Option(0);
    }
    Choice::Unclear
}

/// Says a sample with the chosen voice.
/// Says a sample line with the chosen voice and speed. Waits (up to 20 s)
/// while a new voice loads, so the button can show it is loading.
pub fn test(app: &AppHandle) -> Result<(), String> {
    let v = voice(app);
    if !models::VOICE.installed(&v.models) {
        return Err("The voice is still downloading.".into());
    }
    let started = std::time::Instant::now();
    while v.starting.load(Ordering::SeqCst)
        && started.elapsed() < std::time::Duration::from_secs(20)
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let wanted = lock(&app.state::<AppState>().settings).voice.clone();
    let line = "Hi, I'm Sidekick. Say hey Sidekick whenever you need me.";
    {
        let runtime = lock(&v.runtime);
        if let Some(s) = runtime
            .as_ref()
            .filter(|r| r.key.voice == wanted.voice && r.key.speed == wanted.speed)
            .and_then(|r| r.speaker.as_ref())
        {
            s.say(line);
            return Ok(());
        }
    }
    // Voice off and speaking off: a one-off speaker with the chosen voice.
    let s = Speaker::start_with_events(
        &v.models,
        &wanted.voice,
        wanted.speed,
        Some(speech_events(app)),
    )
    .map_err(|e| e.to_string())?;
    s.say(line);
    let mut greeter = lock(&v.greeter);
    *greeter = Some(s);
    Ok(())
}

/// Downloads the missing models in the background, reporting progress.
/// The voice is installed first so the welcome line can speak before wake/ASR.
pub fn download(app: &AppHandle) -> Result<(), String> {
    let v = voice(app);
    if v.downloading.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    v.cancel_download.store(false, Ordering::SeqCst);
    emit_state(app);
    let app = app.clone();
    std::thread::spawn(move || {
        let v = voice(&app);
        let mut failure = None;
        // Greeting needs the voice; wake/ASR can follow.
        let order = [&models::VOICE, &models::WAKE, &models::SPEECH];
        for model in order {
            if model.installed(&v.models) {
                continue;
            }
            let result = models::install(model, &v.models, &v.cancel_download, |done, total| {
                let _ = app.emit(
                    DOWNLOAD_EVENT,
                    DownloadPayload {
                        label: model.label,
                        done,
                        total,
                        finished: false,
                        error: None,
                    },
                );
            });
            if let Err(e) = result {
                failure = Some(e.to_string());
                break;
            }
            // Open welcome as soon as speech is usable; keep downloading the rest.
            if model.id == models::VOICE.id && failure.is_none() {
                enable_voice_for_onboarding(&app);
                emit_state(&app);
                let pending = voice(&app).pending_welcome_greet.load(Ordering::SeqCst);
                let onboarded = lock(&app.state::<AppState>().settings).onboarded;
                if pending || !onboarded {
                    ask::ensure_welcome(&app);
                }
            }
        }
        v.downloading.store(false, Ordering::SeqCst);
        let _ = app.emit(
            DOWNLOAD_EVENT,
            DownloadPayload {
                label: "",
                done: 0,
                total: 0,
                finished: true,
                error: failure.clone(),
            },
        );
        if let Some(ref err) = failure {
            log::warn!("voice models: {err}");
        } else {
            enable_voice_for_onboarding(&app);
        }
        refresh(&app);
        emit_state(&app);
        let pending = voice(&app).pending_welcome_greet.load(Ordering::SeqCst);
        let onboarded = lock(&app.state::<AppState>().settings).onboarded;
        if failure.is_none() && (pending || !onboarded) {
            ask::ensure_welcome(&app);
        }
    });
    Ok(())
}

pub fn cancel_download(app: &AppHandle) {
    voice(app).cancel_download.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn matches_spoken_choices() {
        let opts = labels(&["Open", "Show in folder", "Convert to PNG"]);
        assert_eq!(match_choice("Open it", &opts), Choice::Option(0));
        assert_eq!(
            match_choice("show in folder please", &opts),
            Choice::Option(1)
        );
        assert_eq!(match_choice("the third one", &opts), Choice::Option(2));
        assert_eq!(
            match_choice("png", &opts),
            Choice::Unclear,
            "too short to guess"
        );
        assert_eq!(match_choice("convert it", &opts), Choice::Option(2));
        assert_eq!(match_choice("Not now.", &opts), Choice::Dismiss);
        assert_eq!(match_choice("nope", &opts), Choice::Dismiss);
        assert_eq!(match_choice("yes", &opts), Choice::Option(0));
        assert_eq!(match_choice("", &opts), Choice::Unclear);
        assert_eq!(match_choice("what is the weather", &opts), Choice::Unclear);
        let browsers = labels(&["Open", "Open in Chrome"]);
        assert_eq!(match_choice("open in chrome", &browsers), Choice::Option(1));
    }

    #[test]
    fn reads_a_suggestion_aloud() {
        assert_eq!(
            spoken_prompt(
                "report.pdf",
                "Downloaded · 2 MB",
                &labels(&["Open", "Show in folder"])
            ),
            "report.pdf. Downloaded , 2 MB. Say open, show in folder, or not now."
        );
    }
}
