//! Voice: "Hey Duskie" wakes a listener that transcribes what
//! you say as you say it, and answers are spoken with Supertonic. Everything runs
//! on this PC through sherpa-onnx; audio is never stored or sent anywhere,
//! only the final text goes to the AI you chose.

pub mod audio;
pub mod listen;
pub mod models;
pub mod speak;
pub mod text;

pub use listen::{Control, Heard, Listener, ListenerConfig};
pub use models::{MODELS, Model};
pub use speak::{
    DEFAULT_VOICE, Speaker, SpeechEvent, SpeechEvents, VOICES, respell, speaker_id, synthesize,
    test_line, tts_config,
};

#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("voice models are not downloaded yet")]
    MissingModels,
    #[error("no microphone found")]
    NoMicrophone,
    #[error("no speakers found")]
    NoSpeaker,
    #[error("audio device: {0}")]
    Audio(String),
    #[error("speech engine: {0}")]
    Engine(String),
    #[error("download: {0}")]
    Download(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, VoiceError>;

/// Sample rate the wake word and speech models expect.
pub const MIC_RATE: u32 = 16_000;

/// Audio is never saved: the code that hears and speaks writes no files.
#[cfg(test)]
mod no_audio_saved {
    const SOURCES: [(&str, &str); 3] = [
        ("audio.rs", include_str!("audio.rs")),
        ("listen.rs", include_str!("listen.rs")),
        ("speak.rs", include_str!("speak.rs")),
    ];
    const DESKTOP: &str = include_str!("../../../apps/desktop/src-tauri/src/voice.rs");
    const WRITES: [&str; 6] = [
        "File::create",
        "fs::write",
        "OpenOptions",
        "WavWriter",
        "hound::",
        "write_all(",
    ];

    #[test]
    fn hearing_and_speaking_write_no_files() {
        for (name, src) in SOURCES.iter().copied().chain([("voice.rs", DESKTOP)]) {
            let code = src.split("#[cfg(test)]").next().unwrap_or(src);
            for w in WRITES {
                assert!(
                    !code.contains(w),
                    "{name} uses {w}; audio must never be saved"
                );
            }
        }
    }
}
