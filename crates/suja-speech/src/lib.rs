pub mod chunker;
pub mod error;
pub mod models;
pub mod resample;
pub mod stt;
pub mod tts;
pub mod util;
pub mod vad;

pub use chunker::SentenceChunker;
pub use error::SpeechError;
pub use models::ModelPaths;
pub use resample::Resampler;
pub use stt::{Recognizer, RecognizerOptions, SttEvent, Transcription};
pub use tts::{Pcm, Synthesizer, TtsBackend, TtsOptions};
pub use vad::{Vad, VadEvent, VadOptions};

pub const SPEECH_SAMPLE_RATE: i32 = 16_000;
