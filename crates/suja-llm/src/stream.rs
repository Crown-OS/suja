use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishReason {
    EndOfTurn,
    ToolCall,
    Cancelled,
    LengthLimit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Delta {
    Text(String),
    ToolCall(ToolCall),
    Finished(FinishReason),
}
