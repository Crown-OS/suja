#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Llm(#[from] suja_llm::LlmError),
    #[error("language model worker stopped")]
    WorkerGone,
    #[error("tool `{name}` failed: {source}")]
    Tool {
        name: String,
        #[source]
        source: suja_tools::ToolError,
    },
}
