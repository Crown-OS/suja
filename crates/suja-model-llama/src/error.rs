#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("model file missing: {0}")]
    MissingModel(String),
    #[error("llama runtime: {0}")]
    Runtime(String),
    #[error("generation cancelled")]
    Cancelled,
    #[error(
        "only {free_mib} MiB of VRAM free, need {needed_mib}; stop crowndictator's GPU mode or other GPU apps"
    )]
    NotEnoughVram { free_mib: u64, needed_mib: u64 },
}
