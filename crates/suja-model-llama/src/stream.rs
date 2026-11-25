use serde::Deserialize;

/// A tool call as the model wrote it, before it gets an id.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ParsedToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Delta {
    Text(String),
    ToolCall(ParsedToolCall),
}
