use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::ModelError;
use crate::event::ModelEvent;
use crate::tool::ToolSpec;
use crate::transcript::Entry;

#[derive(Clone, Debug)]
pub struct TurnRequest {
    pub system: Arc<str>,
    pub tools: Arc<[ToolSpec]>,
    pub transcript: Vec<Entry>,
    pub max_reply_tokens: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub context_tokens: u32,
    pub parallel_tool_calls: bool,
    pub runs_locally: bool,
}

/// A stronger model that the base model can hand a request to.
#[derive(Clone)]
pub struct Expert {
    pub name: String,
    /// When to use it, in words the base model reads.
    pub use_for: String,
    pub model: Arc<dyn ChatModel>,
}

/// Ends after one [`ModelEvent::Finished`] or one error.
pub type ModelStream = mpsc::UnboundedReceiver<Result<ModelEvent, ModelError>>;

pub trait ChatModel: Send + Sync {
    fn capabilities(&self) -> ModelCapabilities;

    /// Starts one assistant reply to the end of `request.transcript`.
    fn stream(&self, request: TurnRequest, cancel: CancellationToken) -> ModelStream;

    /// Prepares the fixed part of later requests, for example a prompt cache. Does not wait.
    fn warm_up(&self, _system: &Arc<str>, _tools: &Arc<[ToolSpec]>) {}
}

/// A stream that holds only `error`.
pub fn failed_stream(error: ModelError) -> ModelStream {
    let (events, stream) = mpsc::unbounded_channel();
    let _ = events.send(Err(error));
    stream
}
