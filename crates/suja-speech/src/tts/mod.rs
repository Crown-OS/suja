mod sherpa;

use std::path::PathBuf;

pub use sherpa::SherpaSynthesizer;

use crate::error::SpeechError;

#[derive(Clone, Debug, PartialEq)]
pub struct Pcm {
    pub samples: Vec<f32>,
    pub sample_rate: i32,
}

#[derive(Clone, Debug)]
pub enum TtsBackend {
    Pocket { reference_voice: PathBuf },
    Kokoro { speaker_id: i32 },
}

#[derive(Clone, Debug)]
pub struct TtsOptions {
    pub model_dir: PathBuf,
    pub backend: TtsBackend,
    pub threads: i32,
    pub speed: f32,
}

pub type ChunkSink = Box<dyn FnMut(&[f32]) -> bool + Send + 'static>;

pub trait Synthesizer: Send {
    fn sample_rate(&self) -> i32;

    fn synthesize(&self, text: &str, on_chunk: ChunkSink) -> Result<Pcm, SpeechError>;
}
