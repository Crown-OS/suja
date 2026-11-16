use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::outcome::ToolOutcome;
use crate::util::command::run_with_timeout;
use crate::{Tool, ToolSpec, argument_str};

pub struct MediaControl;

#[async_trait]
impl Tool for MediaControl {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "media_control",
            description: "Control the active media player.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["play", "pause", "play-pause", "next", "previous", "stop", "status"]}
                },
                "required": ["action"]
            }),
        }
    }

    async fn call(&self, arguments: Value) -> Result<ToolOutcome, ToolError> {
        let action = argument_str(&arguments, "action")?;
        let args = match action {
            "status" => vec![
                "metadata".to_owned(),
                "--format".to_owned(),
                "{{status}}: {{artist}} - {{title}}".to_owned(),
            ],
            "play" | "pause" | "play-pause" | "next" | "previous" | "stop" => {
                vec![action.to_owned()]
            }
            other => {
                return Err(ToolError::InvalidArguments(format!(
                    "unknown action `{other}`"
                )));
            }
        };
        let output = run_with_timeout("playerctl", &args, Duration::from_secs(3)).await?;
        if output.status != 0 {
            return Ok(ToolOutcome::failure("no media player is running"));
        }
        let detail = output.stdout.trim().to_owned();
        Ok(ToolOutcome::success(
            if action == "status" {
                detail.clone()
            } else {
                format!("media {action}")
            },
            if detail.is_empty() {
                format!("playerctl {action} ok")
            } else {
                detail
            },
        ))
    }
}
