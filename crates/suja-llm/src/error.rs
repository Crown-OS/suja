#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("model file missing: {0}")]
    MissingModel(String),
    #[error("llama runtime: {0}")]
    Runtime(String),
    #[error("generation cancelled")]
    Cancelled,
}
