//! The listener: a small keyword model waits for "Hey Sidekick"; then a
//! streaming recognizer transcribes until you stop talking. Partial text
//! arrives as you speak, so the island can show it live.

use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use sherpa_rs_sys as sys;

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

/// Owns the native keyword spotter and recognizer.
pub struct Engine {
    spotter: Option<(
        *const sys::SherpaOnnxKeywordSpotter,
        *const sys::SherpaOnnxOnlineStream,
    )>,
    recognizer: *const sys::SherpaOnnxOnlineRecognizer,
    stream: *const sys::SherpaOnnxOnlineStream,
    listening: bool,
    recent: VecDeque<f32>,
    last: String,
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

impl Engine {
    pub fn new(models: &Path, wake_word: bool) -> Result<Self> {
        if !SPEECH.installed(models) || (wake_word && !WAKE.installed(models)) {
            return Err(VoiceError::MissingModels);
        }
        let cpu = cstr("cpu");
        let spotter = if wake_word {
            let enc = cstr(&WAKE.file(models, WAKE.files[0]));
            let dec = cstr(&WAKE.file(models, WAKE.files[1]));
            let joi = cstr(&WAKE.file(models, WAKE.files[2]));
            let tok = cstr(&WAKE.file(models, "tokens.txt"));
            let keywords = cstr(WAKE_KEYWORDS);
            // SAFETY: a zeroed config is the C API's documented default; every
            // pointer set here outlives the create call.
            unsafe {
                let mut c: sys::SherpaOnnxKeywordSpotterConfig = std::mem::zeroed();
                c.feat_config.sample_rate = MIC_RATE as i32;
                c.feat_config.feature_dim = 80;
                c.model_config.transducer.encoder = enc.as_ptr();
                c.model_config.transducer.decoder = dec.as_ptr();
                c.model_config.transducer.joiner = joi.as_ptr();
                c.model_config.tokens = tok.as_ptr();
                c.model_config.num_threads = 1;
                c.model_config.provider = cpu.as_ptr();
                c.max_active_paths = 4;
                c.keywords_score = 1.0;
                c.keywords_threshold = 0.25;
                c.num_trailing_blanks = 1;
                c.keywords_buf = keywords.as_ptr();
                c.keywords_buf_size = keywords.as_bytes().len() as i32;
                let spotter = sys::SherpaOnnxCreateKeywordSpotter(&c);
                if spotter.is_null() {
                    return Err(VoiceError::Engine("wake word model did not load".into()));
                }
                let stream = sys::SherpaOnnxCreateKeywordStream(spotter);
                Some((spotter, stream))
            }
        } else {
            None
        };
        let enc = cstr(&SPEECH.file(models, "encoder.onnx"));
        let dec = cstr(&SPEECH.file(models, "decoder.onnx"));
        let joi = cstr(&SPEECH.file(models, "joiner.onnx"));
        let tok = cstr(&SPEECH.file(models, "tokens.txt"));
        let greedy = cstr("greedy_search");
        // SAFETY: as above.
        let (recognizer, stream) = unsafe {
            let mut c: sys::SherpaOnnxOnlineRecognizerConfig = std::mem::zeroed();
            c.feat_config.sample_rate = MIC_RATE as i32;
            c.feat_config.feature_dim = 80;
            c.model_config.transducer.encoder = enc.as_ptr();
            c.model_config.transducer.decoder = dec.as_ptr();
            c.model_config.transducer.joiner = joi.as_ptr();
            c.model_config.tokens = tok.as_ptr();
            c.model_config.num_threads = 2;
            c.model_config.provider = cpu.as_ptr();
            c.decoding_method = greedy.as_ptr();
            c.enable_endpoint = 1;
            // Give up after 4 s of silence, end 0.9 s after speech, cap at 30 s.
            c.rule1_min_trailing_silence = 4.0;
            c.rule2_min_trailing_silence = 0.9;
            c.rule3_min_utterance_length = 30.0;
            let recognizer = sys::SherpaOnnxCreateOnlineRecognizer(&c);
            if recognizer.is_null() {
                return Err(VoiceError::Engine("speech model did not load".into()));
            }
            (recognizer, sys::SherpaOnnxCreateOnlineStream(recognizer))
        };
        Ok(Self {
            spotter,
            recognizer,
            stream,
            listening: false,
            recent: VecDeque::with_capacity(PRIME + 4096),
            last: String::new(),
        })
    }

    pub fn listening(&self) -> bool {
        self.listening
    }

    /// Starts transcribing now, with the last moment of audio included.
    pub fn listen_now(&mut self) {
        self.start_listening();
    }

    pub fn cancel(&mut self) {
        if self.listening {
            // SAFETY: the recognizer and stream live as long as `self`.
            unsafe { sys::SherpaOnnxOnlineStreamReset(self.recognizer, self.stream) };
        }
        self.listening = false;
        self.last.clear();
    }

    fn start_listening(&mut self) {
        self.listening = true;
        self.last.clear();
        let prime: Vec<f32> = self.recent.iter().copied().collect();
        // SAFETY: as above; the samples outlive the call.
        unsafe {
            sys::SherpaOnnxOnlineStreamReset(self.recognizer, self.stream);
            sys::SherpaOnnxOnlineStreamAcceptWaveform(
                self.stream,
                MIC_RATE as i32,
                prime.as_ptr(),
                prime.len() as i32,
            );
        }
    }

    /// Feeds 16 kHz mono audio and returns what was heard.
    pub fn feed(&mut self, chunk: &[f32]) -> Vec<Heard> {
        let mut out = Vec::new();
        self.recent.extend(chunk.iter().copied());
        while self.recent.len() > PRIME {
            self.recent.pop_front();
        }
        if !self.listening {
            if let Some((spotter, kws)) = self.spotter
                && self.spot(spotter, kws, chunk)
            {
                out.push(Heard::Wake);
                self.start_listening();
            }
            return out;
        }
        // SAFETY: as above.
        unsafe {
            sys::SherpaOnnxOnlineStreamAcceptWaveform(
                self.stream,
                MIC_RATE as i32,
                chunk.as_ptr(),
                chunk.len() as i32,
            );
            while sys::SherpaOnnxIsOnlineStreamReady(self.recognizer, self.stream) == 1 {
                sys::SherpaOnnxDecodeOnlineStream(self.recognizer, self.stream);
            }
            let res = sys::SherpaOnnxGetOnlineStreamResult(self.recognizer, self.stream);
            let text = if res.is_null() || (*res).text.is_null() {
                String::new()
            } else {
                CStr::from_ptr((*res).text)
                    .to_string_lossy()
                    .trim()
                    .to_owned()
            };
            if !res.is_null() {
                sys::SherpaOnnxDestroyOnlineRecognizerResult(res);
            }
            if text != self.last {
                self.last = text.clone();
                out.push(Heard::Partial(text.clone()));
            }
            if sys::SherpaOnnxOnlineStreamIsEndpoint(self.recognizer, self.stream) == 1 {
                sys::SherpaOnnxOnlineStreamReset(self.recognizer, self.stream);
                if let Some((spotter, kws)) = self.spotter {
                    sys::SherpaOnnxResetKeywordStream(spotter, kws);
                }
                self.listening = false;
                self.last.clear();
                // The words just heard must not wake it again.
                self.recent.clear();
                out.push(Heard::Final(text));
            }
        }
        out
    }

    fn spot(
        &self,
        spotter: *const sys::SherpaOnnxKeywordSpotter,
        kws: *const sys::SherpaOnnxOnlineStream,
        chunk: &[f32],
    ) -> bool {
        // SAFETY: as above.
        unsafe {
            sys::SherpaOnnxOnlineStreamAcceptWaveform(
                kws,
                MIC_RATE as i32,
                chunk.as_ptr(),
                chunk.len() as i32,
            );
            let mut found = false;
            while sys::SherpaOnnxIsKeywordStreamReady(spotter, kws) == 1 {
                sys::SherpaOnnxDecodeKeywordStream(spotter, kws);
                let res = sys::SherpaOnnxGetKeywordResult(spotter, kws);
                if res.is_null() {
                    continue;
                }
                let hit = !(*res).keyword.is_null()
                    && !CStr::from_ptr((*res).keyword).to_bytes().is_empty();
                sys::SherpaOnnxDestroyKeywordResult(res);
                if hit {
                    sys::SherpaOnnxResetKeywordStream(spotter, kws);
                    found = true;
                }
            }
            found
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: created in `new`, destroyed once here.
        unsafe {
            sys::SherpaOnnxDestroyOnlineStream(self.stream);
            sys::SherpaOnnxDestroyOnlineRecognizer(self.recognizer);
            if let Some((spotter, kws)) = self.spotter {
                sys::SherpaOnnxDestroyOnlineStream(kws);
                sys::SherpaOnnxDestroyKeywordSpotter(spotter);
            }
        }
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
                while !stopped.load(Ordering::Relaxed) {
                    while let Ok(c) = control_rx.try_recv() {
                        match c {
                            Control::ListenNow => {
                                engine.listen_now();
                                on(Heard::Wake);
                            }
                            Control::Cancel => engine.cancel(),
                        }
                    }
                    match chunks.recv_timeout(Duration::from_millis(100)) {
                        Ok(chunk) => {
                            for heard in engine.feed(&chunk) {
                                on(heard);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
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
