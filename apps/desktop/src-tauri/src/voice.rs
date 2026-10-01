//! Voice in the app: "Hey Sidekick" opens Ask mode and shows what you say
//! as you say it; when you stop, the question goes to the AI and the answer
//! is read aloud with Kokoro. Off until turned on in Settings > Voice. The
//! microphone stops whenever voice is off or Sidekick is paused.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Utc;
use serde::Serialize;
use sidekick_core::MascotEvent;
use sidekick_core::settings::VoiceSettings;
use sidekick_voice::text::strip_wake;
use sidekick_voice::{Control, Heard, Listener, ListenerConfig, Speaker, models};
use tauri::{AppHandle, Emitter, Manager};

use crate::ask;
use crate::mascot;
use crate::state::{AppState, lock};

pub const STATE_EVENT: &str = "voice://state";
pub const HEARD_EVENT: &str = "voice://heard";
pub const DOWNLOAD_EVENT: &str = "voice://download";

/// What the runtime was built from; a change rebuilds it.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    wake_word: bool,
    voice: String,
    speed: f32,
}

struct Runtime {
    key: Key,
    listener: Option<Listener>,
    speaker: Option<Speaker>,
}

pub struct Voice {
    pub models: PathBuf,
    runtime: Mutex<Option<Runtime>>,
    /// A rebuild is running (loading models takes a moment).
    starting: AtomicBool,
    downloading: AtomicBool,
    cancel_download: AtomicBool,
    /// Ask mode was opened by the wake word, so it closes again if nothing
    /// was said.
    opened_by_voice: AtomicBool,
    /// The chat being read aloud and its utterance.
    speaking: Mutex<Option<(String, u64)>>,
    last_error: Mutex<Option<String>>,
}

impl Voice {
    pub fn new(models: PathBuf) -> Self {
        Self {
            models,
            runtime: Mutex::default(),
            starting: AtomicBool::new(false),
            downloading: AtomicBool::new(false),
            cancel_download: AtomicBool::new(false),
            opened_by_voice: AtomicBool::new(false),
            speaking: Mutex::default(),
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
    let (settings, paused) = {
        let state = app.state::<AppState>();
        let s = lock(&state.settings);
        (s.voice.clone(), s.pause.is_active(Utc::now()))
    };
    let v = voice(app);
    let wanted = settings.enabled && !paused && models::all_installed(&v.models);
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
        voice: settings.voice.clone(),
        speed: settings.speed,
    };
    if lock(&v.runtime).as_ref().is_some_and(|r| r.key == key) {
        return;
    }
    if v.starting.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        // Stop the old one first so the microphone is free.
        let old = lock(&voice(&app).runtime).take();
        drop(old);
        let runtime = build(&app, key, &settings);
        let v = voice(&app);
        *lock(&v.runtime) = Some(runtime);
        v.starting.store(false, Ordering::SeqCst);
        emit_state(&app);
        // Settings may have changed while loading.
        refresh(&app);
    });
}

fn build(app: &AppHandle, key: Key, settings: &VoiceSettings) -> Runtime {
    let v = voice(app);
    let mut errors = Vec::new();
    let speaker = match Speaker::start(&v.models, &settings.voice, settings.speed) {
        Ok(s) => Some(s),
        Err(e) => {
            errors.push(format!("Speech: {e}"));
            None
        }
    };
    let handle = app.clone();
    let listener = match Listener::start(
        ListenerConfig {
            models: v.models.clone(),
            wake_word: settings.wake_word,
        },
        move |heard| on_heard(&handle, heard),
    ) {
        Ok(l) => Some(l),
        Err(e) => {
            errors.push(format!("Microphone: {e}"));
            None
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
            if !ask::is_open(app) {
                v.opened_by_voice.store(true, Ordering::SeqCst);
                ask::open(app, ask::Open::default());
            }
            mascot::dispatch(app, MascotEvent::ListenStart);
            emit_heard(app, String::new(), false);
        }
        Heard::Partial(text) => emit_heard(app, strip_wake(&text), false),
        Heard::Final(text) => {
            let text = strip_wake(&text);
            mascot::dispatch(app, MascotEvent::Cancelled);
            emit_heard(app, text, true);
            v.opened_by_voice.store(false, Ordering::SeqCst);
        }
        Heard::Error(err) => {
            *lock(&v.last_error) = Some(err);
            mascot::dispatch(app, MascotEvent::Cancelled);
            // This runs on the listener thread, which cannot join itself.
            let app = app.clone();
            std::thread::spawn(move || {
                let old = lock(&voice(&app).runtime).take();
                drop(old);
                emit_state(&app);
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
    listener.send(Control::ListenNow);
    Ok(())
}

/// Stops listening and speaking (Esc, Stop).
pub fn stop(app: &AppHandle) {
    let v = voice(app);
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
    with_answer(app, id, |s, u| {
        if error.is_some() {
            s.push(u, "\nSorry, that did not work. The details are on screen.");
        }
        s.finish(u);
    });
}

/// Says a sample with the chosen voice.
pub fn test(app: &AppHandle) -> Result<(), String> {
    let v = voice(app);
    let runtime = lock(&v.runtime);
    let Some(s) = runtime.as_ref().and_then(|r| r.speaker.as_ref()) else {
        return Err("Turn voice on first; the voice loads in a moment.".into());
    };
    s.say("Hi, I'm Sidekick. Say hey Sidekick whenever you need me.");
    Ok(())
}

/// Downloads the missing models in the background, reporting progress.
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
        for model in models::MODELS {
            if model.installed(&v.models) {
                continue;
            }
            let result = models::install(&model, &v.models, &v.cancel_download, |done, total| {
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
        if let Some(err) = failure {
            log::warn!("voice models: {err}");
        }
        refresh(&app);
        emit_state(&app);
    });
    Ok(())
}

pub fn cancel_download(app: &AppHandle) {
    voice(app).cancel_download.store(true, Ordering::SeqCst);
}
