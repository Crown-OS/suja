#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("model unavailable: {0}")]
    Unavailable(String),
    #[error("model request failed: {0}")]
    Request(String),
    #[error("model sent an answer that is not valid: {0}")]
    Protocol(String),
    #[error("model runtime: {0}")]
    Runtime(String),
}
