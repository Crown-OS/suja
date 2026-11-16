use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::outcome::ToolOutcome;
use crate::util::command::run_with_timeout;
use crate::{Tool, ToolSpec, optional_argument_str};

pub struct SetTimer;

#[async_trait]
impl Tool for SetTimer {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "set_timer",
            description: "Start a countdown that posts a desktop notification when it ends.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "seconds": {"type": "integer", "minimum": 1, "maximum": 86400},
                    "label": {"type": "string"}
                },
                "required": ["seconds"]
            }),
        }
    }

    async fn call(&self, arguments: Value) -> Result<ToolOutcome, ToolError> {
        let seconds = arguments
            .get("seconds")
            .and_then(Value::as_u64)
            .filter(|s| (1..=86_400).contains(s))
            .ok_or_else(|| {
                ToolError::InvalidArguments("seconds must be between 1 and 86400".into())
            })?;
        let label = optional_argument_str(&arguments, "label")
            .unwrap_or("Timer")
            .to_owned();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(seconds)).await;
            let args = [
                "-a".to_owned(),
                "suja".to_owned(),
                label.clone(),
                format!("{} finished", describe(seconds)),
            ];
            if let Err(error) = run_with_timeout("notify-send", &args, Duration::from_secs(5)).await
            {
                tracing::warn!(%error, "timer notification failed");
            }
        });
        Ok(ToolOutcome::success(
            format!("timer set for {}", describe(seconds)),
            format!(
                "timer `{}` will notify in {seconds} seconds",
                optional_argument_str(&arguments, "label").unwrap_or("Timer")
            ),
        ))
    }
}

fn describe(seconds: u64) -> String {
    match seconds {
        s if s % 3600 == 0 => format!("{} hours", s / 3600),
        s if s % 60 == 0 => format!("{} minutes", s / 60),
        s => format!("{s} seconds"),
    }
}
