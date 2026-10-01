//! Runs the real models end to end: Kokoro says the wake phrase and a
//! question, the engine must wake and transcribe it. Needs the models, so it
//! only runs when SIDEKICK_VOICE_MODELS points at a folder holding them.

use sherpa_rs::tts::{KokoroTts, KokoroTtsConfig};
use sidekick_voice::audio::Resampler;
use sidekick_voice::listen::Engine;
use sidekick_voice::models::KOKORO;
use sidekick_voice::text::strip_wake;
use sidekick_voice::{Heard, MIC_RATE};

#[test]
fn wakes_and_transcribes_spoken_audio() {
    let Ok(root) = std::env::var("SIDEKICK_VOICE_MODELS") else {
        eprintln!("skipped: set SIDEKICK_VOICE_MODELS to run");
        return;
    };
    let root = std::path::PathBuf::from(root);
    let mut tts = KokoroTts::new(KokoroTtsConfig {
        model: KOKORO.file(&root, "model.int8.onnx"),
        voices: KOKORO.file(&root, "voices.bin"),
        tokens: KOKORO.file(&root, "tokens.txt"),
        data_dir: KOKORO.file(&root, "espeak-ng-data"),
        length_scale: 1.0,
        ..Default::default()
    });
    let said = tts
        .create("Hey Sidekick. What time is it in London right now?", 0, 1.0)
        .unwrap();
    let mut audio = vec![0.0; MIC_RATE as usize];
    audio.extend(Resampler::new(said.sample_rate, MIC_RATE).process(&said.samples));
    audio.extend(vec![0.0; MIC_RATE as usize * 3]);

    let mut engine = Engine::new(&root, true).unwrap();
    let heard: Vec<Heard> = audio.chunks(1600).flat_map(|c| engine.feed(c)).collect();
    assert_eq!(heard.first(), Some(&Heard::Wake), "{heard:?}");
    let Some(Heard::Final(text)) = heard.iter().find(|h| matches!(h, Heard::Final(_))) else {
        panic!("no final transcript: {heard:?}");
    };
    let question = strip_wake(text).to_lowercase();
    assert!(question.starts_with("what time"), "{question}");
    assert!(question.contains("london"), "{question}");
    assert!(!engine.listening());

    // Silence alone never wakes it.
    let quiet = vec![0.0f32; MIC_RATE as usize * 3];
    assert!(
        quiet
            .chunks(1600)
            .flat_map(|c| engine.feed(c))
            .next()
            .is_none()
    );
}
