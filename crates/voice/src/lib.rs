//! Voice (FR-VOICE): "Hey Sidekick" wakes a listener that transcribes what
//! you say as you say it, and answers are spoken with Kokoro. Everything runs
//! on this PC through sherpa-onnx; audio is never stored or sent anywhere,
//! only the final text goes to the AI you chose.

pub mod audio;
pub mod listen;
pub mod models;
pub mod speak;
pub mod text;

pub use listen::{Control, Heard, Listener, ListenerConfig};
pub use models::{MODELS, Model};
pub use speak::{DEFAULT_VOICE, Speaker, SpeechEvent, SpeechEvents, VOICES, tts_config};

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
