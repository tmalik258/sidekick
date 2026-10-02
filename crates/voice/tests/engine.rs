//! Runs the real models end to end: the voice says the wake phrase and a
//! question, the engine must wake and transcribe it. Needs the models, so it
//! only runs when SIDEKICK_VOICE_MODELS points at a folder holding them.

use sherpa_onnx::OfflineTts;
use sidekick_voice::audio::Resampler;
use sidekick_voice::listen::Engine;
use sidekick_voice::text::strip_wake;
use sidekick_voice::{DEFAULT_VOICE, Heard, MIC_RATE, respell, speaker_id, synthesize, tts_config};

#[test]
fn wakes_and_transcribes_spoken_audio() {
    let Ok(root) = std::env::var("SIDEKICK_VOICE_MODELS") else {
        eprintln!("skipped: set SIDEKICK_VOICE_MODELS to run");
        return;
    };
    let root = std::path::PathBuf::from(root);
    let tts = OfflineTts::create(&tts_config(&root)).expect("voice model loads");
    let mut engine = Engine::new(&root, true).unwrap();
    // The voice varies a little each run (it starts from noise), and the
    // test is about the listener, so a few takes are allowed.
    let mut heard = Vec::new();
    for _ in 0..4 {
        let text = respell("Hey Sidekick. What time is it in London right now?");
        let (said, rate) = synthesize(&tts, &text, speaker_id(DEFAULT_VOICE), 1.0).expect("speech");
        let mut audio = vec![0.0; MIC_RATE as usize];
        audio.extend(Resampler::new(rate, MIC_RATE).process(&said));
        audio.extend(vec![0.0; MIC_RATE as usize * 3]);
        heard = audio.chunks(1600).flat_map(|c| engine.feed(c)).collect();
        if heard.first() == Some(&Heard::Wake) {
            break;
        }
    }
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
