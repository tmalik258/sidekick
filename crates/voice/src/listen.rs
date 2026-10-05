//! The listener: a small keyword model waits for "Hey Sidekick"; then a
//! streaming recognizer transcribes until you stop talking. Partial text
//! arrives as you speak, so the island can show it live.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use sherpa_onnx::{
    KeywordSpotter, KeywordSpotterConfig, OnlineModelConfig, OnlineRecognizer,
    OnlineRecognizerConfig, OnlineStream, OnlineTransducerModelConfig,
};

use crate::audio::Mic;
use crate::models::{SPEECH, WAKE};
use crate::text::WAKE_KEYWORDS;
use crate::{MIC_RATE, Result, VoiceError};

#[derive(Debug, Clone, PartialEq)]
pub enum Heard {
    /// The wake phrase, or a push to talk: listening starts.
    Wake,
    /// Text so far, wake phrase included.
    Partial(String),
    /// The utterance ended. Empty when nothing was said.
    Final(String),
    /// The microphone or engine failed; the listener has stopped.
    Error(String),
}

#[derive(Debug, Clone, Copy)]
pub enum Control {
    /// Start listening without the wake phrase.
    ListenNow,
    /// Stop listening and go back to waiting for the wake phrase.
    Cancel,
    /// Sidekick is speaking (true) or quiet again (false). While muted the
    /// microphone hears the speakers, so nothing it hears counts.
    Mute(bool),
}

#[derive(Debug, Clone)]
pub struct ListenerConfig {
    pub models: PathBuf,
    /// Listen for "Hey Sidekick". Off means push to talk only.
    pub wake_word: bool,
}

/// Audio kept from before the wake phrase was recognised, so the first
/// words after it are not lost.
const PRIME: usize = MIC_RATE as usize * 3 / 2;
/// After the wake phrase people pause before the question. Until this much
/// audio has passed, an utterance that is only the wake phrase does not end
/// listening; it waits for the question instead.
const QUESTION_GRACE: usize = MIC_RATE as usize * 8;
/// Longest a question may run: past this, what was heard is taken as said,
/// so steady background noise can never keep it listening.
const MAX_LISTEN: usize = MIC_RATE as usize * 20;
/// No audio this long while listening: the microphone stalled; finish.
const STALL: Duration = Duration::from_secs(3);
/// No audio this long at all: the microphone is gone; report it so the app
/// starts it again.
const DEAD: Duration = Duration::from_secs(10);

/// Owns the keyword spotter and the streaming recognizer.
pub struct Engine {
    spotter: Option<(KeywordSpotter, OnlineStream)>,
    recognizer: OnlineRecognizer,
    stream: OnlineStream,
    listening: bool,
    recent: VecDeque<f32>,
    last: String,
    /// Audio fed since listening started.
    heard: usize,
    /// While waiting, the speech model transcribes and wakes on the phrase.
    transcript_wake: bool,
    /// Sidekick's own voice is playing: audio is dropped, not heard.
    muted: bool,
}

fn model(
    dir: &Path,
    model: &crate::models::Model,
    enc: &str,
    dec: &str,
    joi: &str,
    threads: i32,
) -> OnlineModelConfig {
    OnlineModelConfig {
        transducer: OnlineTransducerModelConfig {
            encoder: Some(model.file(dir, enc)),
            decoder: Some(model.file(dir, dec)),
            joiner: Some(model.file(dir, joi)),
        },
        tokens: Some(model.file(dir, "tokens.txt")),
        num_threads: threads,
        provider: Some("cpu".into()),
        ..Default::default()
    }
}

// The engine's default features (16 kHz, 80 bins) are what the models and
// the microphone resampler use, so feat_config is left at its default.
const _: () = assert!(MIC_RATE == 16_000);

impl Engine {
    /// With the wake word on, both the keyword model and the transcript
    /// listen for it: either one wakes Sidekick.
    pub fn new(models: &Path, wake_word: bool) -> Result<Self> {
        Self::with_wake(models, wake_word, wake_word)
    }

    /// `keyword`: the small keyword model. `transcript`: the speech model
    /// runs all the time and wakes on "hey Sidekick" in what it hears.
    pub fn with_wake(models: &Path, keyword: bool, transcript: bool) -> Result<Self> {
        let wake_word = keyword;
        if !SPEECH.installed(models) || (wake_word && !WAKE.installed(models)) {
            return Err(VoiceError::MissingModels);
        }
        let spotter = if wake_word {
            let config = KeywordSpotterConfig {
                model_config: model(
                    models,
                    &WAKE,
                    WAKE.files[0],
                    WAKE.files[1],
                    WAKE.files[2],
                    1,
                ),
                max_active_paths: 4,
                // Tuned with the wake bench (10 voices, two speeds, 100
                // everyday sentences): this boost and bar caught 92% alone
                // and 95% with the transcript, with no false wakes. Lower
                // gained nothing more.
                keywords_score: 2.5,
                keywords_threshold: 0.05,
                num_trailing_blanks: 1,
                keywords_buf: Some(WAKE_KEYWORDS.to_owned()),
                ..Default::default()
            };
            let spotter = KeywordSpotter::create(&config)
                .ok_or_else(|| VoiceError::Engine("wake word model did not load".into()))?;
            let stream = spotter.create_stream();
            Some((spotter, stream))
        } else {
            None
        };
        let config = OnlineRecognizerConfig {
            model_config: model(
                models,
                &SPEECH,
                "encoder.onnx",
                "decoder.onnx",
                "joiner.onnx",
                2,
            ),
            decoding_method: Some("greedy_search".into()),
            enable_endpoint: true,
            // Give up after 4 s of silence, end 0.9 s after speech, cap at 30 s.
            rule1_min_trailing_silence: 4.0,
            rule2_min_trailing_silence: 0.9,
            rule3_min_utterance_length: 30.0,
            ..Default::default()
        };
        let recognizer = OnlineRecognizer::create(&config)
            .ok_or_else(|| VoiceError::Engine("speech model did not load".into()))?;
        let stream = recognizer.create_stream();
        Ok(Self {
            spotter,
            recognizer,
            stream,
            listening: false,
            recent: VecDeque::with_capacity(PRIME + 4096),
            last: String::new(),
            heard: 0,
            transcript_wake: transcript,
            muted: false,
        })
    }

    /// While muted, audio is dropped: the speakers' sound must not wake
    /// Sidekick, nor be primed into the next question. Muting also ends
    /// waiting on anything half heard. A push to talk unmutes.
    pub fn set_muted(&mut self, muted: bool) {
        if muted && !self.muted {
            self.reset();
        }
        self.muted = muted;
    }

    /// Back to waiting, as if just started: nothing heard, nothing pending.
    pub fn reset(&mut self) {
        self.recognizer.reset(&self.stream);
        self.fresh_spotter();
        self.listening = false;
        self.recent.clear();
        self.last.clear();
        self.heard = 0;
    }

    pub fn listening(&self) -> bool {
        self.listening
    }

    /// Starts transcribing now, with the last moment of audio included.
    pub fn listen_now(&mut self) {
        self.muted = false;
        self.start_listening();
    }

    pub fn cancel(&mut self) {
        if self.listening {
            self.recognizer.reset(&self.stream);
        }
        self.listening = false;
        self.last.clear();
    }

    /// Ends listening now with what was heard so far (maybe nothing).
    pub fn finish(&mut self) -> Option<Heard> {
        if !self.listening {
            return None;
        }
        let text = self.current_text();
        self.recognizer.reset(&self.stream);
        self.fresh_spotter();
        self.listening = false;
        self.last.clear();
        self.recent.clear();
        self.heard = 0;
        Some(Heard::Final(text))
    }

    fn start_listening(&mut self) {
        self.fresh_spotter();
        self.listening = true;
        self.heard = 0;
        self.last.clear();
        let prime: Vec<f32> = self.recent.iter().copied().collect();
        self.recognizer.reset(&self.stream);
        self.stream.accept_waveform(MIC_RATE as i32, &prime);
    }

    /// Feeds 16 kHz mono audio and returns what was heard.
    pub fn feed(&mut self, chunk: &[f32]) -> Vec<Heard> {
        if self.muted {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.recent.extend(chunk.iter().copied());
        while self.recent.len() > PRIME {
            self.recent.pop_front();
        }
        if !self.listening {
            let spotted = self.spot(chunk);
            if !self.transcript_wake {
                if spotted {
                    out.push(Heard::Wake);
                    self.start_listening();
                }
                return out;
            }
            // The transcript already holds the wake audio, so listening
            // just carries on from here.
            let text = self.decode(chunk);
            if spotted || crate::text::find_wake(&text).is_some() {
                out.push(Heard::Wake);
                // The keyword model may still hold the phrase; it must not
                // fire on it later.
                self.fresh_spotter();
                self.listening = true;
                self.heard = 0;
                self.last.clear();
            } else {
                if self.recognizer.is_endpoint(&self.stream) {
                    self.recognizer.reset(&self.stream);
                }
                return out;
            }
        } else {
            self.decode(chunk);
            self.heard += chunk.len();
            if self.heard > MAX_LISTEN {
                out.extend(self.finish());
                return out;
            }
        }
        let text = self.current_text();
        if text != self.last {
            self.last = text.clone();
            out.push(Heard::Partial(text.clone()));
        }
        if self.recognizer.is_endpoint(&self.stream) {
            self.recognizer.reset(&self.stream);
            // Only the wake phrase so far: keep listening for the question.
            if crate::text::strip_wake(&text).is_empty() && self.heard < QUESTION_GRACE {
                if !self.last.is_empty() {
                    self.last.clear();
                    out.push(Heard::Partial(String::new()));
                }
                return out;
            }
            self.fresh_spotter();
            self.listening = false;
            self.last.clear();
            // The words just heard must not wake it again.
            self.recent.clear();
            out.push(Heard::Final(text));
        }
        out
    }

    /// A new keyword stream: a reset keeps audio it has buffered, which
    /// could fire the wake word again a moment later.
    fn fresh_spotter(&mut self) {
        if let Some((spotter, kws)) = &mut self.spotter {
            *kws = spotter.create_stream();
        }
    }

    fn decode(&self, chunk: &[f32]) -> String {
        self.stream.accept_waveform(MIC_RATE as i32, chunk);
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
        self.raw_text()
    }

    fn raw_text(&self) -> String {
        self.recognizer
            .get_result(&self.stream)
            .map(|r| r.text.trim().to_owned())
            .unwrap_or_default()
    }

    /// What was said from the wake phrase on (words before it are chatter).
    fn current_text(&self) -> String {
        let text = self.raw_text();
        match crate::text::find_wake(&text) {
            Some(at) => text[at..].to_owned(),
            None => text,
        }
    }

    fn spot(&self, chunk: &[f32]) -> bool {
        let Some((spotter, kws)) = &self.spotter else {
            return false;
        };
        kws.accept_waveform(MIC_RATE as i32, chunk);
        let mut found = false;
        while spotter.is_ready(kws) {
            spotter.decode(kws);
            if spotter
                .get_result(kws)
                .is_some_and(|r| !r.keyword.is_empty())
            {
                spotter.reset(kws);
                found = true;
            }
        }
        found
    }
}

/// Runs the microphone and engine on a background thread.
pub struct Listener {
    control: Sender<Control>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Listener {
    pub fn start(
        config: ListenerConfig,
        mut on: impl FnMut(Heard) + Send + 'static,
    ) -> Result<Self> {
        let (control, control_rx) = mpsc::channel::<Control>();
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        let stopped = stop.clone();
        let thread = std::thread::Builder::new()
            .name("sidekick-listener".into())
            .spawn(move || {
                let mut engine = match Engine::new(&config.models, config.wake_word) {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let (tx, chunks) = mpsc::sync_channel::<Vec<f32>>(64);
                let mic = match Mic::start(tx) {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                let mut last_chunk = std::time::Instant::now();
                while !stopped.load(Ordering::Relaxed) {
                    while let Ok(c) = control_rx.try_recv() {
                        match c {
                            Control::ListenNow => {
                                engine.listen_now();
                                on(Heard::Wake);
                            }
                            Control::Cancel => engine.cancel(),
                            Control::Mute(muted) => engine.set_muted(muted),
                        }
                    }
                    match chunks.recv_timeout(Duration::from_millis(100)) {
                        Ok(chunk) => {
                            last_chunk = std::time::Instant::now();
                            for heard in engine.feed(&chunk) {
                                on(heard);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            let quiet = last_chunk.elapsed();
                            if quiet > STALL
                                && let Some(heard) = engine.finish()
                            {
                                on(heard);
                            }
                            if quiet > DEAD {
                                on(Heard::Error("the microphone stopped responding".into()));
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            on(Heard::Error("the microphone stopped".into()));
                            break;
                        }
                    }
                }
                drop(mic);
            })?;
        ready_rx
            .recv()
            .map_err(|_| VoiceError::Engine("listener thread ended".into()))??;
        Ok(Self {
            control,
            stop,
            thread: Some(thread),
        })
    }

    pub fn send(&self, c: Control) {
        let _ = self.control.send(c);
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
