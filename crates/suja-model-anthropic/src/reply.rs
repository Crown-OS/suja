use serde_json::{Value, json};
use suja_model::{FinishReason, ModelError, ToolCall, Usage};

enum Block {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    Other(Value),
}

pub struct FinishedReply {
    pub calls: Vec<ToolCall>,
    pub reason: FinishReason,
    pub usage: Usage,
    /// The content blocks of the reply, to send back unchanged in the next request.
    pub blocks: Value,
}

/// Collects one streamed Messages reply: text goes out at once, tool calls when it ends.
#[derive(Default)]
pub struct ReplyDecoder {
    blocks: Vec<Block>,
    stop_reason: Option<String>,
    usage: Usage,
}

impl ReplyDecoder {
    /// The new text in the event `data`, if any.
    pub fn decode(&mut self, data: &str) -> Result<Option<String>, ModelError> {
        let event: Value = serde_json::from_str(data)
            .map_err(|error| ModelError::Protocol(format!("{error}: {data}")))?;
        match event["type"].as_str().unwrap_or_default() {
            "message_start" => self.read_input_usage(&event["message"]["usage"]),
            "content_block_start" => return Ok(self.start_block(&event["content_block"])),
            "content_block_delta" => {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                return Ok(self.apply_delta(index, &event["delta"]));
            }
            "message_delta" => {
                if let Some(reason) = event["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(reason.to_owned());
                }
                if let Some(output) = event["usage"]["output_tokens"].as_u64() {
                    self.usage.output_tokens = output as u32;
                }
            }
            "error" => {
                return Err(ModelError::Request(
                    event["error"]["message"]
                        .as_str()
                        .unwrap_or("the stream reported an error")
                        .to_owned(),
                ));
            }
            _ => {}
        }
        Ok(None)
    }

    fn read_input_usage(&mut self, usage: &Value) {
        let tokens = |field: &str| usage[field].as_u64().unwrap_or(0) as u32;
        self.usage.input_tokens = tokens("input_tokens")
            + tokens("cache_read_input_tokens")
            + tokens("cache_creation_input_tokens");
    }

    fn start_block(&mut self, block: &Value) -> Option<String> {
        let text = |field: &str| block[field].as_str().unwrap_or_default().to_owned();
        let (new_block, initial_text) = match block["type"].as_str().unwrap_or_default() {
            "text" => (Block::Text(text("text")), Some(text("text"))),
            "tool_use" => (
                Block::ToolUse {
                    id: text("id"),
                    name: text("name"),
                    input_json: String::new(),
                },
                None,
            ),
            "thinking" => (
                Block::Thinking {
                    thinking: text("thinking"),
                    signature: text("signature"),
                },
                None,
            ),
            _ => (Block::Other(block.clone()), None),
        };
        self.blocks.push(new_block);
        initial_text.filter(|text| !text.is_empty())
    }

    fn apply_delta(&mut self, index: usize, delta: &Value) -> Option<String> {
        let piece = |field: &str| delta[field].as_str().unwrap_or_default();
        match (self.blocks.get_mut(index)?, delta["type"].as_str()?) {
            (Block::Text(text), "text_delta") => {
                text.push_str(piece("text"));
                Some(piece("text").to_owned()).filter(|text| !text.is_empty())
            }
            (Block::ToolUse { input_json, .. }, "input_json_delta") => {
                input_json.push_str(piece("partial_json"));
                None
            }
            (Block::Thinking { thinking, .. }, "thinking_delta") => {
                thinking.push_str(piece("thinking"));
                None
            }
            (Block::Thinking { signature, .. }, "signature_delta") => {
                signature.push_str(piece("signature"));
                None
            }
            _ => None,
        }
    }

    /// Tool calls count only when Claude stopped to use them; a reply cut at `max_tokens`
    /// keeps its text and drops its unfinished calls.
    pub fn finish(self) -> FinishedReply {
        let reason = match self.stop_reason.as_deref() {
            Some("tool_use") => FinishReason::ToolCall,
            Some("max_tokens" | "model_context_window_exceeded") => FinishReason::LengthLimit,
            Some("refusal") => FinishReason::Refused,
            _ => FinishReason::EndOfTurn,
        };
        let mut calls = Vec::new();
        let mut blocks = Vec::with_capacity(self.blocks.len());
        for block in self.blocks {
            match block {
                Block::Text(text) => blocks.push(json!({"type": "text", "text": text})),
                Block::ToolUse {
                    id,
                    name,
                    input_json,
                } => {
                    if reason != FinishReason::ToolCall {
                        continue;
                    }
                    let input = parse_input(&input_json);
                    blocks
                        .push(json!({"type": "tool_use", "id": id, "name": name, "input": input}));
                    calls.push(ToolCall {
                        id,
                        name,
                        arguments: input,
                    });
                }
                Block::Thinking {
                    thinking,
                    signature,
                } => blocks.push(
                    json!({"type": "thinking", "thinking": thinking, "signature": signature}),
                ),
                Block::Other(block) => blocks.push(block),
            }
        }
        FinishedReply {
            calls,
            reason,
            usage: self.usage,
            blocks: Value::Array(blocks),
        }
    }
}

/// The parsed tool input. Input that is not a JSON object becomes `{}`; the tool then reports
/// its missing arguments, and Claude gets that error as the tool result.
fn parse_input(input_json: &str) -> Value {
    serde_json::from_str::<Value>(input_json)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(decoder: &mut ReplyDecoder, events: &[Value]) -> String {
        events
            .iter()
            .filter_map(|event| decoder.decode(&event.to_string()).ok().flatten())
            .collect()
    }

    #[test]
    fn a_cut_reply_drops_its_unfinished_tool_call() {
        let mut decoder = ReplyDecoder::default();
        let spoken = decode_all(
            &mut decoder,
            &[
                json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
                json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Okay."}}),
                json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "t1", "name": "save_note", "input": {}}}),
                json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"te"}}),
                json!({"type": "message_delta", "delta": {"stop_reason": "max_tokens"}, "usage": {"output_tokens": 9}}),
            ],
        );
        let reply = decoder.finish();
        assert_eq!(spoken, "Okay.");
        assert_eq!(reply.reason, FinishReason::LengthLimit);
        assert!(reply.calls.is_empty());
        assert_eq!(reply.blocks, json!([{"type": "text", "text": "Okay."}]));
    }

    #[test]
    fn a_refusal_ends_the_turn_as_refused() {
        let mut decoder = ReplyDecoder::default();
        decode_all(
            &mut decoder,
            &[
                json!({"type": "message_delta", "delta": {"stop_reason": "refusal"}, "usage": {"output_tokens": 0}}),
            ],
        );
        assert_eq!(decoder.finish().reason, FinishReason::Refused);
    }

    #[test]
    fn a_stream_error_event_is_a_request_error() {
        let mut decoder = ReplyDecoder::default();
        let event = json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}});
        assert_eq!(
            decoder.decode(&event.to_string()),
            Err(ModelError::Request("Overloaded".into()))
        );
    }
}
