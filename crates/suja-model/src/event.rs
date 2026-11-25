use crate::tool::ToolCall;
use crate::transcript::BackendData;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishReason {
    EndOfTurn,
    ToolCall,
    Cancelled,
    LengthLimit,
    Refused,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl Usage {
    pub fn total(self) -> u32 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finish {
    pub reason: FinishReason,
    pub usage: Usage,
    /// Blocks of the reply that only this backend reads, kept in the transcript unchanged.
    pub backend_data: Option<BackendData>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModelEvent {
    Text(String),
    ToolCall(ToolCall),
    Finished(Finish),
}
