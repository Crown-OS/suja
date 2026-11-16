use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::outcome::ToolOutcome;
use crate::util::command::run_with_timeout;
use crate::{Tool, ToolSpec, argument_str};

pub struct AdjustSetting;

fn percent_argument(value: &str) -> Result<(String, String), ToolError> {
    let trimmed = value.trim().trim_end_matches('%');
    let number: i32 = trimmed
        .parse()
        .map_err(|_| ToolError::InvalidArguments(format!("`{value}` is not a percentage")))?;
    let clamped = number.clamp(-100, 100);
    let wpctl = match trimmed.chars().next() {
        Some('+') => format!("{}%+", clamped.abs()),
        Some('-') => format!("{}%-", clamped.abs()),
        _ => format!("{clamped}%"),
    };
    let brightness = match trimmed.chars().next() {
        Some('+') => format!("+{}%", clamped.abs()),
        Some('-') => format!("{}%-", clamped.abs()),
        _ => format!("{clamped}%"),
    };
    Ok((wpctl, brightness))
}

#[async_trait]
impl Tool for AdjustSetting {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "adjust_setting",
            description: "Set or nudge the speaker volume or screen brightness.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "setting": {"type": "string", "enum": ["volume", "brightness"]},
                    "value": {"type": "string", "description": "Percent like `40`, or relative like `+10` / `-10`"}
                },
                "required": ["setting", "value"]
            }),
        }
    }

    async fn call(&self, arguments: Value) -> Result<ToolOutcome, ToolError> {
        let setting = argument_str(&arguments, "setting")?;
        let (wpctl_value, brightness_value) = percent_argument(argument_str(&arguments, "value")?)?;
        let (program, args) = match setting {
            "volume" => (
                "wpctl",
                vec![
                    "set-volume".to_owned(),
                    "-l".to_owned(),
                    "1.0".to_owned(),
                    "@DEFAULT_AUDIO_SINK@".to_owned(),
                    wpctl_value,
                ],
            ),
            "brightness" => (
                "brightnessctl",
                vec!["-q".to_owned(), "set".to_owned(), brightness_value],
            ),
            other => {
                return Err(ToolError::InvalidArguments(format!(
                    "unknown setting `{other}`"
                )));
            }
        };
        let output = run_with_timeout(program, &args, Duration::from_secs(3)).await?;
        if output.status != 0 {
            return Ok(ToolOutcome::failure(format!(
                "{program} failed: {}",
                output.stderr.trim()
            )));
        }
        Ok(ToolOutcome::success(
            format!("{setting} adjusted"),
            format!("{setting} set via {program}"),
        ))
    }
}
