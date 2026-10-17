//! Speech out: Supertonic turns each sentence into audio while the previous
//! one plays, so an answer starts sounding before it is fully written.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use sherpa_onnx::{
    GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsModelConfig,
    OfflineTtsSupertonicModelConfig,
};

use crate::audio::Output;
use crate::models::VOICE;
use crate::text::{Sentences, speakable};
use crate::{Result, VoiceError};

/// Supertonic 3's built-in voices: id, label, speaker number.
pub const VOICES: &[(&str, &str, i32)] = &[
    ("f1", "Female 1 (clear)", 0),
    ("f2", "Female 2 (lively)", 1),
    ("f3", "Female 3", 2),
    ("f4", "Female 4 (bright)", 3),
    ("f5", "Female 5 (warm)", 4),
    ("m1", "Male 1", 5),
    ("m2", "Male 2 (deep, lively)", 6),
    ("m3", "Male 3", 7),
    ("m4", "Male 4 (calm)", 8),
    ("m5", "Male 5 (deep)", 9),
];

pub const DEFAULT_VOICE: &str = "f4";

/// A short line for the settings Test button. Each voice gets its own so
/// switching voices is obvious, not the same clip on loop.
pub fn test_line(voice: &str) -> &'static str {
    match voice {
        "f1" => "This is Female 1: clear, steady, and easy to follow.",
        "f2" => "Hey! Female 2 here, lively, bright, and ready to jump in.",
        "f3" => "Hello from Female 3. A calm voice for everyday questions.",
        "f4" => "Hi, Female 4, bright and upbeat when you need a quick answer.",
        "f5" => "Hi, I'm Duskie. Female 5 is warm. Say hey Duskie anytime.",
        "m1" => "Hello. Male 1 speaking, straightforward and clear.",
        "m2" => "Hey there. Male 2: deep, lively, and easy on the ears.",
        "m3" => "Hi. Male 3 here, a solid everyday voice for Duskie.",
        "m4" => "Hello. Male 4: calm and measured when things get busy.",
        "m5" => "Hi. Male 5: deep and steady for longer answers.",
        _ => "Hi, I'm Duskie. Say hey Duskie whenever you need me.",
    }
}

/// Denoising steps per sentence: more is cleaner and slower. 10 was about
/// real-time on a laptop (queue underruns between pieces); 5 stays ahead.
const STEPS: i32 = 5;

/// The speaker number for `voice`; unknown ids (older voices) fall back to
/// the default.
pub fn speaker_id(voice: &str) -> i32 {
    let find = |v: &str| {
        VOICES
            .iter()
            .find(|(id, _, _)| *id == v)
            .map(|(_, _, sid)| *sid)
    };
    find(voice).or_else(|| find(DEFAULT_VOICE)).unwrap_or(3)
}

/// The engine settings. Separate so a test can run the model without a
/// sound card.
pub fn tts_config(models: &Path) -> OfflineTtsConfig {
    let file = |name: &str| Some(VOICE.file(models, name));
    OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            supertonic: OfflineTtsSupertonicModelConfig {
                duration_predictor: file("duration_predictor.int8.onnx"),
                text_encoder: file("text_encoder.int8.onnx"),
                vector_estimator: file("vector_estimator.int8.onnx"),
                vocoder: file("vocoder.int8.onnx"),
                tts_json: file("tts.json"),
                unicode_indexer: file("unicode_indexer.bin"),
                voice_style: file("voice.bin"),
            },
            num_threads: std::thread::available_parallelism()
                .map_or(2, |n| n.get().clamp(1, 4) as i32),
            provider: Some("cpu".into()),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Synthesizes one piece of text: (samples, sample rate).
pub fn synthesize(tts: &OfflineTts, text: &str, sid: i32, speed: f32) -> Option<(Vec<f32>, u32)> {
    let config = GenerationConfig {
        sid,
        speed,
        num_steps: STEPS,
        extra: Some(std::collections::HashMap::from([(
            "lang".to_owned(),
            serde_json::Value::from("en"),
        )])),
        ..Default::default()
    };
    let audio = tts.generate_with_config(text, &config, None::<fn(&[f32], f32) -> bool>)?;
    let rate = u32::try_from(audio.sample_rate()).ok()?;
    let samples = audio.samples().to_vec();
    (!samples.is_empty()).then_some((samples, rate))
}

/// Written how the voice should say it. Mid-line stops become commas so the
/// model breathes without saying "dot". Pieces stay short so the next synth
/// finishes before the speaker queue runs dry.
pub fn respell(text: &str) -> String {
    tts_punctuation(text)
}

/// `. ` / `! ` / `? ` → `, ` so Supertonic does not say "dot"; trailing
/// sentence stops are dropped (silence after the clip covers the end).
fn tts_punctuation(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev = i.checked_sub(1).map(|j| chars[j]);
        let next = chars.get(i + 1).copied();
        if matches!(c, '.' | '!' | '?')
            && prev.is_some_and(|p| p.is_alphabetic() || p == '\'' || p == '"')
            && next == Some(' ')
        {
            out.push(',');
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    while matches!(out.chars().last(), Some('.' | '!' | '?')) {
        out.pop();
    }
    out
}

/// Almost no injected silence between pieces, joins already reset prosody,
/// and extra quiet made every cut feel like a stall. A tiny breath only after
/// a full stop.
fn pause_after(text: &str) -> f32 {
    match text.trim_end().chars().last() {
        Some('.' | '!' | '?') => 0.04,
        _ => 0.0,
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
        if !VOICE.installed(models) {
            return Err(VoiceError::MissingModels);
        }
        let output = Arc::new(Output::start()?);
        let current = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel::<Cmd>();
        let config = tts_config(models);
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
                let Some(tts) = OfflineTts::create(&config) else {
                    log::warn!("voice: the speech model did not load");
                    return;
                };
                let mut pending = Sentences::default();
                let mut pending_gen = 0u64;
                let mut in_code = false;
                let say = |epoch: u64, piece: &str, in_code: &mut bool| {
                    let line = piece.trim();
                    if line.starts_with("```") {
                        if !*in_code {
                            speak_one(
                                &tts,
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
                            &tts,
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
    tts: &OfflineTts,
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
    // Said respelled; reported as written, so the shown words still line up.
    match synthesize(tts, &respell(text), sid, speed) {
        // Stopped while synthesizing: drop it.
        Some((mut samples, rate)) if current.load(Ordering::SeqCst) == epoch => {
            let words = samples.len();
            let pause = (pause_after(text) * rate as f32) as usize;
            samples.resize(words + pause, 0.0);
            let (starts_in, _) = out.play(&samples, rate);
            let length = Duration::from_secs_f64(words as f64 / f64::from(rate));
            queued(text.to_owned(), starts_in, length);
        }
        Some(_) => {}
        None => log::warn!("voice: no audio for {text:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_voices() {
        assert_eq!(speaker_id("f1"), 0);
        assert_eq!(speaker_id("m5"), 9);
        // Kokoro ids from before fall back to the default voice (Female 4).
        assert_eq!(speaker_id("af_heart"), 3);
        assert!(pause_after("Hi there!") > pause_after("so,"));
        assert_ne!(test_line("f1"), test_line("m2"));
        assert!(test_line("f5").contains("Female 5"));
    }

    #[test]
    fn turns_mid_stops_into_commas_so_dot_is_not_said() {
        assert_eq!(
            respell("Now, your world. Connect your calendar"),
            "Now, your world, Connect your calendar"
        );
        assert_eq!(respell("I'm Duskie."), "I'm Duskie");
        assert_eq!(respell("Hi! I'm Duskie."), "Hi, I'm Duskie");
    }

    /// Runs the real model when it is on disk:
    /// `DUSKIE_VOICE_MODELS=<folder holding the voice model> cargo test -p duskie-voice -- --ignored`
    #[test]
    #[ignore = "needs the downloaded voice model"]
    fn speaks_with_the_real_model() {
        let root = std::env::var("DUSKIE_VOICE_MODELS").expect("set DUSKIE_VOICE_MODELS");
        let models = Path::new(&root);
        let tts = OfflineTts::create(&tts_config(models)).expect("model loads");
        for (voice, _, sid) in VOICES {
            let (samples, rate) = synthesize(
                &tts,
                "Hi there! I'm Duskie. Shall we get you set up?",
                *sid,
                1.0,
            )
            .expect("speech");
            let secs = samples.len() as f32 / rate as f32;
            let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
            println!("{voice}: {secs:.2}s at {rate} Hz, peak {peak:.2}");
            assert!((1.5..10.0).contains(&secs), "{voice}: {secs}s");
            assert!(peak > 0.05, "{voice} is silent");
        }
    }
}
