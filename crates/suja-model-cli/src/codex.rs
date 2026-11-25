use serde_json::Value;
use suja_model::{Finish, FinishReason, ModelError, Usage};

use crate::process::LineParser;

/// Reads `codex exec --json`. Codex sends whole messages, not parts.
#[derive(Default)]
pub struct CodexParser {
    spoke: bool,
    usage: Usage,
    failure: Option<String>,
}

impl LineParser for CodexParser {
    fn parse(&mut self, line: &str) -> Result<Option<String>, ModelError> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Ok(None);
        };
        match value["type"].as_str() {
            Some("item.completed")
                if matches!(
                    value["item"]["type"].as_str(),
                    Some("agent_message" | "assistant_message")
                ) =>
            {
                let Some(text) = value["item"]["text"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                else {
                    return Ok(None);
                };
                let text = if self.spoke {
                    format!("\n{text}")
                } else {
                    text.to_owned()
                };
                self.spoke = true;
                Ok(Some(text))
            }
            Some("turn.completed") => {
                let tokens = |field: &str| value["usage"][field].as_u64().unwrap_or(0) as u32;
                self.usage = Usage {
                    input_tokens: tokens("input_tokens"),
                    output_tokens: tokens("output_tokens"),
                };
                Ok(None)
            }
            Some("turn.failed") => {
                self.failure = value["error"]["message"].as_str().map(str::to_owned);
                Ok(None)
            }
            Some("error") => {
                self.failure = value["message"].as_str().map(str::to_owned);
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn finish(
        self,
        exited_ok: bool,
        stderr_tail: &str,
    ) -> Result<(Option<String>, Finish), ModelError> {
        if let Some(failure) = self.failure {
            return Err(ModelError::Request(format!("codex: {failure}")));
        }
        if !exited_ok {
            return Err(ModelError::Request(format!("codex failed: {stderr_tail}")));
        }
        Ok((
            None,
            Finish {
                reason: FinishReason::EndOfTurn,
                usage: self.usage,
                backend_data: None,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn agent_messages_are_spoken_and_a_failed_turn_is_an_error() -> Result<(), ModelError> {
        let mut parser = CodexParser::default();
        let message = |text: &str| {
            json!({"type": "item.completed", "item": {"type": "agent_message", "text": text}})
                .to_string()
        };
        assert_eq!(
            parser.parse(
                &json!({"type": "item.completed", "item": {"type": "command_execution"}})
                    .to_string()
            )?,
            None
        );
        assert_eq!(parser.parse(&message("First."))?, Some("First.".into()));
        assert_eq!(parser.parse(&message("Second."))?, Some("\nSecond.".into()));
        parser.parse(
            &json!({"type": "turn.failed", "error": {"message": "usage limit"}}).to_string(),
        )?;
        assert_eq!(
            parser.finish(true, "").err(),
            Some(ModelError::Request("codex: usage limit".into()))
        );
        Ok(())
    }
}
