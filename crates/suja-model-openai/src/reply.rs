use serde::Deserialize;
use suja_model::{FinishReason, ModelError, ToolCall, Usage};

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<ChunkUsage>,
}

#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    delta: ChoiceDelta,
    finish_reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct ChoiceDelta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

#[derive(Deserialize)]
struct ToolCallDelta {
    index: Option<usize>,
    id: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct ChunkUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

/// Collects one streamed reply: text goes out at once, tool calls when the reply ends.
#[derive(Default)]
pub struct ReplyDecoder {
    calls: Vec<PartialCall>,
    finish_reason: Option<String>,
    usage: Usage,
}

impl ReplyDecoder {
    /// The new text in the chunk `data`, if any.
    pub fn decode(&mut self, data: &str) -> Result<Option<String>, ModelError> {
        let chunk: Chunk = serde_json::from_str(data)
            .map_err(|error| ModelError::Protocol(format!("{error}: {data}")))?;
        if let Some(usage) = chunk.usage {
            self.usage = Usage {
                input_tokens: usage.prompt_tokens,
                output_tokens: usage.completion_tokens,
            };
        }
        let Some(choice) = chunk.choices.into_iter().next() else {
            return Ok(None);
        };
        if let Some(reason) = choice.finish_reason {
            self.finish_reason = Some(reason);
        }
        for delta in choice.delta.tool_calls {
            self.merge_call(delta);
        }
        Ok(choice.delta.content.filter(|text| !text.is_empty()))
    }

    fn merge_call(&mut self, delta: ToolCallDelta) {
        let starts_new_call = delta.id.as_ref().is_some_and(|id| {
            self.calls
                .last()
                .is_none_or(|last| !last.id.is_empty() && &last.id != id)
        });
        let index = delta.index.unwrap_or(if starts_new_call {
            self.calls.len()
        } else {
            self.calls.len().saturating_sub(1)
        });
        if self.calls.len() <= index {
            self.calls.resize_with(index + 1, PartialCall::default);
        }
        let call = &mut self.calls[index];
        if let Some(id) = delta.id {
            call.id = id;
        }
        if let Some(function) = delta.function {
            if let Some(name) = function.name {
                call.name.push_str(&name);
            }
            if let Some(arguments) = function.arguments {
                call.arguments.push_str(&arguments);
            }
        }
    }

    pub fn finish(self) -> Result<(Vec<ToolCall>, FinishReason, Usage), ModelError> {
        let calls = self
            .calls
            .into_iter()
            .enumerate()
            .filter(|(_, call)| !call.name.is_empty())
            .map(|(number, call)| {
                let arguments = if call.arguments.trim().is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::from_str(&call.arguments).map_err(|error| {
                        ModelError::Protocol(format!("arguments of `{}`: {error}", call.name))
                    })?
                };
                Ok(ToolCall {
                    id: if call.id.is_empty() {
                        format!("call_{number}")
                    } else {
                        call.id
                    },
                    name: call.name,
                    arguments,
                })
            })
            .collect::<Result<Vec<_>, ModelError>>()?;
        let reason = match self.finish_reason.as_deref() {
            _ if !calls.is_empty() => FinishReason::ToolCall,
            Some("length") => FinishReason::LengthLimit,
            Some("content_filter") => FinishReason::Refused,
            _ => FinishReason::EndOfTurn,
        };
        Ok((calls, reason, self.usage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_fragments_are_joined_by_index() -> Result<(), ModelError> {
        let mut decoder = ReplyDecoder::default();
        for data in [
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"set_timer","arguments":"{\"sec"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"onds\": 60}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#,
        ] {
            assert_eq!(decoder.decode(data), Ok(None));
        }
        let (calls, reason, usage) = decoder.finish()?;
        assert_eq!(reason, FinishReason::ToolCall);
        assert_eq!(usage.total(), 15);
        assert_eq!(calls[0].name, "set_timer");
        assert_eq!(calls[0].arguments, serde_json::json!({"seconds": 60}));
        Ok(())
    }

    #[test]
    fn calls_without_index_are_told_apart_by_id() -> Result<(), ModelError> {
        let mut decoder = ReplyDecoder::default();
        for data in [
            r#"{"choices":[{"delta":{"tool_calls":[{"id":"a","function":{"name":"one","arguments":"{}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"id":"b","function":{"name":"two","arguments":"{}"}}]}}]}"#,
        ] {
            let _ = decoder.decode(data);
        }
        let (calls, _, _) = decoder.finish()?;
        let names: Vec<_> = calls.iter().map(|call| call.name.as_str()).collect();
        assert_eq!(names, ["one", "two"]);
        Ok(())
    }
}
