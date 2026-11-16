#[derive(Debug, thiserror::Error)]
pub enum SpeechError {
    #[error("model file missing: {0}")]
    MissingModel(String),
    #[error("speech runtime: {0}")]
    Runtime(String),
}
