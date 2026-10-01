//! Speech out: Kokoro turns each sentence into audio while the previous one
//! plays, so an answer starts sounding before it is fully written.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use sherpa_rs::OnnxConfig;
use sherpa_rs::tts::{KokoroTts, KokoroTtsConfig};

use crate::audio::Output;
use crate::models::KOKORO;
use crate::text::{Sentences, speakable};
use crate::{Result, VoiceError};

/// Kokoro v0.19 voices: id, label, speaker number.
pub const VOICES: &[(&str, &str, i32)] = &[
    ("af", "Default (American)", 0),
    ("af_bella", "Bella (American)", 1),
    ("af_nicole", "Nicole (American)", 2),
    ("af_sarah", "Sarah (American)", 3),
    ("af_sky", "Sky (American)", 4),
    ("am_adam", "Adam (American)", 5),
    ("am_michael", "Michael (American)", 6),
    ("bf_emma", "Emma (British)", 7),
    ("bf_isabella", "Isabella (British)", 8),
    ("bm_george", "George (British)", 9),
    ("bm_lewis", "Lewis (British)", 10),
];

pub fn speaker_id(voice: &str) -> i32 {
    VOICES
        .iter()
        .find(|(id, _, _)| *id == voice)
        .map_or(0, |(_, _, sid)| *sid)
}

enum Cmd {
    Text(u64, String),
    Finish(u64),
}

/// Speaks text. Each answer is an utterance; starting a new one or calling
/// [`Speaker::stop`] silences the old one at once.
pub struct Speaker {
    tx: Option<Sender<Cmd>>,
    current: Arc<AtomicU64>,
    output: Arc<Output>,
    thread: Option<JoinHandle<()>>,
}

impl Speaker {
    pub fn start(models: &Path, voice: &str, speed: f32) -> Result<Self> {
        if !KOKORO.installed(models) {
            return Err(VoiceError::MissingModels);
        }
        let output = Arc::new(Output::start()?);
        let current = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel::<Cmd>();
        let config = KokoroTtsConfig {
            model: KOKORO.file(models, "model.int8.onnx"),
            voices: KOKORO.file(models, "voices.bin"),
            tokens: KOKORO.file(models, "tokens.txt"),
            data_dir: KOKORO.file(models, "espeak-ng-data"),
            length_scale: 1.0,
            onnx_config: OnnxConfig {
                provider: "cpu".into(),
                debug: false,
                num_threads: std::thread::available_parallelism()
                    .map_or(2, |n| n.get().clamp(1, 4) as i32),
            },
            ..Default::default()
        };
        let sid = speaker_id(voice);
        let speed = speed.clamp(0.5, 2.0);
        let (out, cur) = (output.clone(), current.clone());
        let thread = std::thread::Builder::new()
            .name("sidekick-tts".into())
            .spawn(move || {
                let mut tts = KokoroTts::new(config);
                let mut pending = Sentences::default();
                let mut pending_gen = 0u64;
                let mut in_code = false;
                let mut say = |epoch: u64, piece: &str, in_code: &mut bool| {
                    let line = piece.trim();
                    if line.starts_with("```") {
                        if !*in_code {
                            speak_one(
                                &mut tts,
                                &out,
                                &cur,
                                epoch,
                                "The code is on screen.",
                                sid,
                                speed,
                            );
                        }
                        *in_code = !*in_code;
                        return;
                    }
                    if *in_code {
                        return;
                    }
                    let text = speakable(line);
                    if !text.is_empty() {
                        speak_one(&mut tts, &out, &cur, epoch, &text, sid, speed);
                    }
                };
                for cmd in rx {
                    match cmd {
                        Cmd::Text(epoch, text) => {
                            if epoch != pending_gen {
                                pending = Sentences::default();
                                pending_gen = epoch;
                                in_code = false;
                            }
                            for piece in pending.push(&text) {
                                if cur.load(Ordering::SeqCst) != epoch {
                                    break;
                                }
                                say(epoch, &piece, &mut in_code);
                            }
                        }
                        Cmd::Finish(epoch) => {
                            if epoch == pending_gen
                                && let Some(rest) = pending.finish()
                            {
                                say(epoch, &rest, &mut in_code);
                            }
                        }
                    }
                }
            })?;
        Ok(Self {
            tx: Some(tx),
            current,
            output,
            thread: Some(thread),
        })
    }

    /// Starts a new utterance, silencing anything still playing. Returns its
    /// id for [`Speaker::push`] and [`Speaker::finish`].
    pub fn begin(&self) -> u64 {
        self.output.clear();
        self.current.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Adds streamed text to utterance `id`.
    pub fn push(&self, id: u64, text: &str) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Text(id, text.to_owned()));
        }
    }

    /// The utterance's text is complete; speak what is left.
    pub fn finish(&self, id: u64) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Finish(id));
        }
    }

    pub fn say(&self, text: &str) {
        let id = self.begin();
        self.push(id, text);
        self.finish(id);
    }

    pub fn stop(&self) {
        self.current.fetch_add(1, Ordering::SeqCst);
        self.output.clear();
    }

    pub fn busy(&self) -> bool {
        self.output.busy()
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        self.stop();
        drop(self.tx.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn speak_one(
    tts: &mut KokoroTts,
    out: &Output,
    current: &AtomicU64,
    epoch: u64,
    text: &str,
    sid: i32,
    speed: f32,
) {
    if current.load(Ordering::SeqCst) != epoch {
        return;
    }
    match tts.create(text, sid, speed) {
        // Stopped while synthesizing: drop it.
        Ok(audio) if current.load(Ordering::SeqCst) == epoch => {
            out.play(&audio.samples, audio.sample_rate)
        }
        Ok(_) => {}
        Err(e) => log::warn!("kokoro: {e}"),
    }
}
