//! Speech out: Kokoro turns each sentence into audio while the previous one
//! plays, so an answer starts sounding before it is fully written.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use sherpa_rs::OnnxConfig;
use sherpa_rs::tts::{KokoroTts, KokoroTtsConfig};

use crate::audio::Output;
use crate::models::KOKORO;
use crate::text::{Sentences, speakable};
use crate::{Result, VoiceError};

/// Kokoro v1.0 English voices: id, label, speaker number in `voices.bin`.
/// Heart first: it is the most natural and the default.
pub const VOICES: &[(&str, &str, i32)] = &[
    ("af_heart", "Heart (American, warm)", 3),
    ("af_bella", "Bella (American, bright)", 2),
    ("af_nicole", "Nicole (American, soft)", 6),
    ("af_aoede", "Aoede (American)", 1),
    ("af_kore", "Kore (American)", 5),
    ("af_sarah", "Sarah (American)", 9),
    ("af_nova", "Nova (American)", 7),
    ("af_sky", "Sky (American)", 10),
    ("am_michael", "Michael (American)", 16),
    ("am_fenrir", "Fenrir (American)", 14),
    ("am_puck", "Puck (American)", 18),
    ("am_echo", "Echo (American)", 12),
    ("bf_emma", "Emma (British)", 21),
    ("bf_isabella", "Isabella (British)", 22),
    ("bm_george", "George (British)", 26),
    ("bm_fable", "Fable (British)", 25),
];

pub const DEFAULT_VOICE: &str = "af_heart";

/// The speaker number for `voice`; unknown ids (such as the v0.19 ones)
/// fall back to the default voice.
pub fn speaker_id(voice: &str) -> i32 {
    let find = |v: &str| {
        VOICES
            .iter()
            .find(|(id, _, _)| *id == v)
            .map(|(_, _, sid)| *sid)
    };
    find(voice).or_else(|| find(DEFAULT_VOICE)).unwrap_or(3)
}

/// British voices read best with the British dictionary.
fn british(voice: &str) -> bool {
    voice.starts_with("bf_") || voice.starts_with("bm_")
}

/// The engine settings for `voice`. Separate so a test can run the model
/// without a sound card.
pub fn tts_config(models: &Path, voice: &str) -> KokoroTtsConfig {
    let lexicon = if british(voice) {
        "lexicon-gb-en.txt"
    } else {
        "lexicon-us-en.txt"
    };
    KokoroTtsConfig {
        model: KOKORO.file(models, "model.onnx"),
        voices: KOKORO.file(models, "voices.bin"),
        tokens: KOKORO.file(models, "tokens.txt"),
        data_dir: KOKORO.file(models, "espeak-ng-data"),
        lexicon: KOKORO.file(models, lexicon),
        // Required by v1.0 even for English; without it the engine exits.
        dict_dir: KOKORO.file(models, "dict"),
        // Left empty on purpose: the engine picks it from the voice, and
        // "en-gb" makes it throw a C++ exception that aborts the app.
        lang: String::new(),
        length_scale: 1.0,
        onnx_config: OnnxConfig {
            provider: "cpu".into(),
            debug: false,
            num_threads: std::thread::available_parallelism()
                .map_or(2, |n| n.get().clamp(1, 4) as i32),
        },
        ..Default::default()
    }
}

/// A short breath after a sentence, so sentences synthesized one at a time
/// do not run into each other.
fn pause_after(text: &str) -> f32 {
    match text.trim_end().chars().last() {
        Some('.' | '!' | '?') => 0.28,
        Some(',' | ';' | ':') => 0.12,
        _ => 0.05,
    }
}

/// What is being said and when, so the UI can show words as they are heard.
#[derive(Debug, Clone)]
pub enum SpeechEvent {
    /// `text` starts sounding in `starts_in` and lasts `length`.
    Piece {
        speaker: u64,
        utterance: u64,
        text: String,
        starts_in: Duration,
        length: Duration,
    },
    /// Utterance `utterance` is fully queued and goes quiet in `ends_in`.
    End {
        speaker: u64,
        utterance: u64,
        ends_in: Duration,
    },
}

pub type SpeechEvents = Arc<dyn Fn(SpeechEvent) + Send + Sync>;

static NEXT_SPEAKER: AtomicU64 = AtomicU64::new(1);

enum Cmd {
    Text(u64, String),
    Finish(u64),
}

/// Speaks text. Each answer is an utterance; starting a new one or calling
/// [`Speaker::stop`] silences the old one at once.
pub struct Speaker {
    id: u64,
    tx: Option<Sender<Cmd>>,
    current: Arc<AtomicU64>,
    output: Arc<Output>,
    thread: Option<JoinHandle<()>>,
}

impl Speaker {
    pub fn start(models: &Path, voice: &str, speed: f32) -> Result<Self> {
        Self::start_with_events(models, voice, speed, None)
    }

    /// Like [`Speaker::start`], reporting each sentence as it is queued.
    pub fn start_with_events(
        models: &Path,
        voice: &str,
        speed: f32,
        events: Option<SpeechEvents>,
    ) -> Result<Self> {
        if !KOKORO.installed(models) {
            return Err(VoiceError::MissingModels);
        }
        let output = Arc::new(Output::start()?);
        let current = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel::<Cmd>();
        let config = tts_config(models, voice);
        let id = NEXT_SPEAKER.fetch_add(1, Ordering::SeqCst);
        let sid = speaker_id(voice);
        let speed = speed.clamp(0.5, 2.0);
        let (out, cur) = (output.clone(), current.clone());
        let tell = move |e: SpeechEvent| {
            if let Some(f) = &events {
                f(e);
            }
        };
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
                                (sid, speed),
                                &|text, starts_in, length| {
                                    tell(SpeechEvent::Piece {
                                        speaker: id,
                                        utterance: epoch,
                                        text,
                                        starts_in,
                                        length,
                                    })
                                },
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
                        speak_one(
                            &mut tts,
                            &out,
                            &cur,
                            epoch,
                            &text,
                            (sid, speed),
                            &|text, starts_in, length| {
                                tell(SpeechEvent::Piece {
                                    speaker: id,
                                    utterance: epoch,
                                    text,
                                    starts_in,
                                    length,
                                })
                            },
                        );
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
                            if cur.load(Ordering::SeqCst) == epoch {
                                tell(SpeechEvent::End {
                                    speaker: id,
                                    utterance: epoch,
                                    ends_in: out.queued(),
                                });
                            }
                        }
                    }
                }
            })?;
        Ok(Self {
            id,
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

    /// Speaks `text` as a new utterance and returns its id.
    pub fn say(&self, text: &str) -> u64 {
        let id = self.begin();
        self.push(id, text);
        self.finish(id);
        id
    }

    /// Tells this speaker's events apart from another's.
    pub fn id(&self) -> u64 {
        self.id
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
    (sid, speed): (i32, f32),
    queued: &dyn Fn(String, Duration, Duration),
) {
    if current.load(Ordering::SeqCst) != epoch {
        return;
    }
    match tts.create(text, sid, speed) {
        // Stopped while synthesizing: drop it.
        Ok(mut audio) if current.load(Ordering::SeqCst) == epoch => {
            let words = audio.samples.len();
            let pause = (pause_after(text) * audio.sample_rate as f32) as usize;
            audio.samples.resize(words + pause, 0.0);
            let (starts_in, _) = out.play(&audio.samples, audio.sample_rate);
            let length = Duration::from_secs_f64(words as f64 / f64::from(audio.sample_rate));
            queued(text.to_owned(), starts_in, length);
        }
        Ok(_) => {}
        Err(e) => log::warn!("kokoro: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_voices_and_dictionaries() {
        assert_eq!(speaker_id("af_heart"), 3);
        assert_eq!(speaker_id("bm_george"), 26);
        // A v0.19 id that no longer exists falls back to Heart.
        assert_eq!(speaker_id("af"), 3);
        let models = Path::new("m");
        assert!(
            tts_config(models, "bf_emma")
                .lexicon
                .ends_with("lexicon-gb-en.txt")
        );
        assert!(
            tts_config(models, "af_heart")
                .lexicon
                .ends_with("lexicon-us-en.txt")
        );
        assert!(pause_after("Hi there!") > pause_after("so,"));
    }

    /// Runs the real model when it is on disk:
    /// `SIDEKICK_KOKORO=<folder holding kokoro-multi-lang-v1_0> cargo test -p sidekick-voice -- --ignored`
    #[test]
    #[ignore = "needs the downloaded Kokoro model"]
    fn speaks_with_the_real_model() {
        let root = std::env::var("SIDEKICK_KOKORO").expect("set SIDEKICK_KOKORO");
        let models = Path::new(&root);
        for (voice, _, _) in VOICES {
            let mut tts = KokoroTts::new(tts_config(models, voice));
            let audio = tts
                .create(
                    "Hi there! I'm Sidekick. Shall we get you set up?",
                    speaker_id(voice),
                    1.0,
                )
                .expect("speech");
            let secs = audio.samples.len() as f32 / audio.sample_rate as f32;
            let peak = audio.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
            println!(
                "{voice}: {secs:.2}s at {} Hz, peak {peak:.2}",
                audio.sample_rate
            );
            assert!((1.5..10.0).contains(&secs), "{voice}: {secs}s");
            assert!(peak > 0.05, "{voice} is silent");
        }
    }
}
