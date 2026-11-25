#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    #[error("local model: {0}")]
    Llama(String),
    #[error("model file {0} not found")]
    MissingModel(String),
    #[error("this build has no llama.cpp backend")]
    NoLlama,
    #[error(transparent)]
    Credential(#[from] suja_model_http::CredentialError),
}
