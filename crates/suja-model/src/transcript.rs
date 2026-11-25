use serde_json::Value;

use crate::tool::{ToolCall, ToolResult};

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    User(String),
    Assistant(AssistantReply),
    ToolResults(Vec<ToolResult>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AssistantReply {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub backend_data: Option<BackendData>,
}

impl AssistantReply {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.tool_calls.is_empty() && self.backend_data.is_none()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackendData {
    pub owner: &'static str,
    pub content: Value,
}

impl BackendData {
    /// The content when `owner` wrote it; a different backend gets `None`.
    pub fn content_for(&self, owner: &str) -> Option<&Value> {
        (self.owner == owner).then_some(&self.content)
    }
}
