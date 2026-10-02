//! Runs the real models end to end: the voice says the wake phrase and a
//! question, the engine must wake and transcribe it. Needs the models, so it
//! only runs when SIDEKICK_VOICE_MODELS points at a folder holding them.

use sherpa_onnx::OfflineTts;
use sidekick_voice::audio::Resampler;
use sidekick_voice::listen::Engine;
use sidekick_voice::text::strip_wake;
use sidekick_voice::{
    DEFAULT_VOICE, Heard, MIC_RATE, VOICES, respell, speaker_id, synthesize, tts_config,
};

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

/// People pause after "Hey Sidekick". The pause must not end listening;
/// the question that follows is what gets transcribed.
#[test]
fn waits_for_the_question_after_a_pause() {
    let Ok(root) = std::env::var("SIDEKICK_VOICE_MODELS") else {
        eprintln!("skipped: set SIDEKICK_VOICE_MODELS to run");
        return;
    };
    let root = std::path::PathBuf::from(root);
    let tts = OfflineTts::create(&tts_config(&root)).expect("voice model loads");
    let mut engine = Engine::new(&root, true).unwrap();
    let say = |text: &str| {
        let (said, rate) =
            synthesize(&tts, &respell(text), speaker_id(DEFAULT_VOICE), 1.0).expect("speech");
        Resampler::new(rate, MIC_RATE).process(&said)
    };
    // The synthetic voice varies from run to run (it starts from noise),
    // and the test is about the pause, so a few takes are allowed.
    let mut last = String::new();
    for _ in 0..3 {
        let mut audio = vec![0.0; MIC_RATE as usize];
        audio.extend(say("Hey Sidekick."));
        // A long breath before the question.
        audio.extend(vec![0.0; MIC_RATE as usize * 3 / 2]);
        audio.extend(say("What is on my calendar today?"));
        audio.extend(vec![0.0; MIC_RATE as usize * 3]);
        engine.reset();
        let heard: Vec<Heard> = audio.chunks(1600).flat_map(|c| engine.feed(c)).collect();
        let finals: Vec<&String> = heard
            .iter()
            .filter_map(|h| match h {
                Heard::Final(t) => Some(t),
                _ => None,
            })
            .collect();
        // Never two utterances: the pause must not end the first one.
        assert!(finals.len() <= 1, "{heard:?}");
        let question = finals
            .first()
            .map(|t| strip_wake(t).to_lowercase())
            .unwrap_or_default();
        if heard.first() == Some(&Heard::Wake) && question.contains("calendar") {
            return;
        }
        last = format!("{heard:?}");
    }
    panic!("never heard the question after the pause: {last}");
}

/// How often each way of waking catches "Hey Sidekick" across voices and
/// speeds, and how often everyday speech wakes it by mistake. Prints a
/// table; run with SIDEKICK_WAKE_BENCH=1 in release mode.
#[test]
fn wake_bench() {
    let (Ok(root), Ok(_)) = (
        std::env::var("SIDEKICK_VOICE_MODELS"),
        std::env::var("SIDEKICK_WAKE_BENCH"),
    ) else {
        eprintln!("skipped: set SIDEKICK_VOICE_MODELS and SIDEKICK_WAKE_BENCH to run");
        return;
    };
    let root = std::path::PathBuf::from(root);
    let tts = OfflineTts::create(&tts_config(&root)).expect("voice model loads");
    let wakes = [
        "Hey Sidekick.",
        "Hey Sidekick, what is on my calendar?",
        "Hi Sidekick, open my notes.",
        "Okay Sidekick.",
    ];
    let others = [
        "I need to finish the report before lunch.",
        "Can you send me the side project files?",
        "Hey, did you see the new kickoff deck?",
        "My sidekick app is almost done.",
        "Let's meet at the side entrance at six.",
    ];
    let mut clips: Vec<(bool, Vec<f32>)> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    for (_, _, sid) in VOICES {
        for speed in [0.9f32, 1.15] {
            for (i, text) in wakes.iter().chain(others.iter()).enumerate() {
                let Some((said, rate)) = synthesize(&tts, &respell(text), *sid, speed) else {
                    continue;
                };
                let mut audio = vec![0.0; MIC_RATE as usize / 2];
                audio.extend(Resampler::new(rate, MIC_RATE).process(&said));
                audio.extend(vec![0.0; MIC_RATE as usize * 2]);
                clips.push((i < wakes.len(), audio));
                labels.push(format!("{text} (voice {sid}, speed {speed})"));
            }
        }
    }
    eprintln!("{} clips ready", clips.len());
    for (name, keyword, transcript) in [
        ("keyword", true, false),
        ("transcript", false, true),
        ("both", true, true),
    ] {
        let mut engine = Engine::with_wake(&root, keyword, transcript).unwrap();
        let started = std::time::Instant::now();
        let (mut hit, mut wake_total, mut false_wake, mut other_total) = (0, 0, 0, 0);
        let mut seconds = 0.0;
        for (n, (is_wake, audio)) in clips.iter().enumerate() {
            seconds += audio.len() as f32 / MIC_RATE as f32;
            engine.reset();
            let heard: Vec<Heard> = audio.chunks(1600).flat_map(|c| engine.feed(c)).collect();
            let woke = heard.contains(&Heard::Wake);
            if *is_wake {
                wake_total += 1;
                hit += usize::from(woke);
            } else {
                other_total += 1;
                false_wake += usize::from(woke);
                if woke {
                    eprintln!("  {name} false wake: {} -> {heard:?}", labels[n]);
                }
            }
        }
        let rtf = started.elapsed().as_secs_f32() / seconds;
        eprintln!(
            "{name:>10}: caught {hit}/{wake_total} ({:.0}%), false wakes {false_wake}/{other_total}, real-time factor {rtf:.3}",
            100.0 * hit as f32 / wake_total as f32
        );
    }
}
