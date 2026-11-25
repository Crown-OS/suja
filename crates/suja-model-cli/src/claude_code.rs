use serde_json::Value;
use suja_model::{Finish, FinishReason, ModelError, Usage};

use crate::process::LineParser;

struct ResultLine {
    is_error: bool,
    subtype: String,
    text: String,
    usage: Usage,
}

/// Reads `claude -p --output-format stream-json --include-partial-messages`.
#[derive(Default)]
pub struct ClaudeCodeParser {
    streamed_text: bool,
    result: Option<ResultLine>,
}

impl LineParser for ClaudeCodeParser {
    fn parse(&mut self, line: &str) -> Result<Option<String>, ModelError> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Ok(None);
        };
        match value["type"].as_str() {
            Some("stream_event") if value["parent_tool_use_id"].is_null() => {
                let event = &value["event"];
                let is_text = event["type"] == "content_block_delta"
                    && event["delta"]["type"] == "text_delta";
                let text = event["delta"]["text"]
                    .as_str()
                    .filter(|text| is_text && !text.is_empty());
                self.streamed_text |= text.is_some();
                Ok(text.map(str::to_owned))
            }
            Some("result") => {
                let tokens = |field: &str| value["usage"][field].as_u64().unwrap_or(0) as u32;
                self.result = Some(ResultLine {
                    is_error: value["is_error"].as_bool().unwrap_or(false),
                    subtype: value["subtype"].as_str().unwrap_or_default().to_owned(),
                    text: value["result"].as_str().unwrap_or_default().to_owned(),
                    usage: Usage {
                        input_tokens: tokens("input_tokens")
                            + tokens("cache_read_input_tokens")
                            + tokens("cache_creation_input_tokens"),
                        output_tokens: tokens("output_tokens"),
                    },
                });
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
        let Some(result) = self.result else {
            return Err(ModelError::Request(format!(
                "claude ended without a result{}: {stderr_tail}",
                if exited_ok { "" } else { " and failed" }
            )));
        };
        if result.is_error {
            let message = if result.text.is_empty() {
                result.subtype
            } else {
                result.text
            };
            return Err(ModelError::Request(format!("claude: {message}")));
        }
        let unstreamed = (!self.streamed_text && !result.text.is_empty()).then_some(result.text);
        let reason = if result.subtype == "error_max_turns" {
            FinishReason::LengthLimit
        } else {
            FinishReason::EndOfTurn
        };
        Ok((
            unstreamed,
            Finish {
                reason,
                usage: result.usage,
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
    fn partial_text_streams_and_subagent_text_does_not() -> Result<(), ModelError> {
        let mut parser = ClaudeCodeParser::default();
        let delta = |text: &str, parent: Value| {
            json!({"type": "stream_event", "parent_tool_use_id": parent,
                   "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}})
            .to_string()
        };
        assert_eq!(parser.parse("not json")?, None);
        assert_eq!(
            parser.parse(&delta("Kyoto ", Value::Null))?,
            Some("Kyoto ".into())
        );
        assert_eq!(parser.parse(&delta("inner", json!("toolu_1")))?, None);
        parser.parse(&json!({"type": "result", "subtype": "success", "is_error": false, "result": "Kyoto is lovely.", "usage": {"input_tokens": 3, "output_tokens": 4}}).to_string())?;
        let (unstreamed, finish) = parser.finish(true, "")?;
        assert_eq!(unstreamed, None);
        assert_eq!(finish.reason, FinishReason::EndOfTurn);
        assert_eq!(finish.usage.total(), 7);
        Ok(())
    }

    #[test]
    fn an_error_result_is_a_request_error() -> Result<(), ModelError> {
        let mut parser = ClaudeCodeParser::default();
        parser.parse(&json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "result": "Not logged in"}).to_string())?;
        assert_eq!(
            parser.finish(false, "").err(),
            Some(ModelError::Request("claude: Not logged in".into()))
        );
        Ok(())
    }
}
