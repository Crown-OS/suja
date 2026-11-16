pub mod adjust_setting;
pub mod confirm;
pub mod error;
pub mod media_control;
pub mod outcome;
pub mod read_file;
pub mod registry;
pub mod run_command;
pub mod save_note;
pub mod search_files;
pub mod set_timer;
pub mod util;
pub mod web;

use async_trait::async_trait;
use serde_json::Value;

pub use error::ToolError;
pub use outcome::ToolOutcome;
pub use registry::{Registry, ToolSettings};

#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn call(&self, arguments: Value) -> Result<ToolOutcome, ToolError>;
    async fn call_confirmed(&self, arguments: Value) -> Result<ToolOutcome, ToolError> {
        self.call(arguments).await
    }
}

pub fn argument_str<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing string field `{key}`")))
}

pub fn optional_argument_str<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments.get(key).and_then(Value::as_str)
}
