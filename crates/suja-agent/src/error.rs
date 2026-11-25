#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Model(#[from] suja_model::ModelError),
    #[error("the model stream ended without a finish")]
    StreamEnded,
    #[error("no expert model is set up")]
    NoExpert,
}
