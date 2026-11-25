use serde_json::{Value, json};
use suja_model::{AssistantReply, Entry, ToolResult, TurnRequest};

use crate::options::AnthropicOptions;

pub const BACKEND_NAME: &str = "anthropic";

pub fn request_body(options: &AnthropicOptions, request: &TurnRequest) -> Value {
    let mut messages: Vec<Value> = Vec::with_capacity(request.transcript.len());
    for entry in &request.transcript {
        match entry {
            Entry::User(text) => {
                push_user_blocks(&mut messages, [json!({"type": "text", "text": text})])
            }
            Entry::ToolResults(results) => {
                push_user_blocks(&mut messages, results.iter().map(tool_result_block));
            }
            Entry::Assistant(reply) => {
                messages.push(json!({"role": "assistant", "content": assistant_blocks(reply)}));
            }
        }
    }
    let mut body = json!({
        "model": options.model,
        "max_tokens": request.max_reply_tokens.max(options.max_tokens),
        "stream": true,
        "system": [{"type": "text", "text": &*request.system, "cache_control": {"type": "ephemeral"}}],
        "messages": messages,
        "cache_control": {"type": "ephemeral"},
    });
    if !request.tools.is_empty() {
        body["tools"] = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.parameters,
                    "eager_input_streaming": true,
                })
            })
            .collect();
    }
    if let Some(effort) = &options.effort {
        body["output_config"] = json!({"effort": effort});
    }
    if options.fallbacks {
        body["fallbacks"] = json!("default");
    }
    body
}

/// Adds blocks to a user message at the end, or starts one; tool results and text of one
/// user turn go in one message.
fn push_user_blocks(messages: &mut Vec<Value>, blocks: impl IntoIterator<Item = Value>) {
    if let Some(last) = messages.last_mut()
        && last["role"] == "user"
        && let Some(content) = last["content"].as_array_mut()
    {
        content.extend(blocks);
        return;
    }
    messages.push(json!({"role": "user", "content": blocks.into_iter().collect::<Vec<_>>()}));
}

fn tool_result_block(result: &ToolResult) -> Value {
    json!({
        "type": "tool_result",
        "tool_use_id": result.call_id,
        "content": result.content,
        "is_error": result.is_error,
    })
}

/// The blocks exactly as Claude sent them when this backend wrote the reply, so thinking
/// blocks go back unchanged; else blocks built from the text and the calls.
fn assistant_blocks(reply: &AssistantReply) -> Value {
    if let Some(blocks) = reply
        .backend_data
        .as_ref()
        .and_then(|data| data.content_for(BACKEND_NAME))
    {
        return blocks.clone();
    }
    let mut blocks = Vec::with_capacity(reply.tool_calls.len() + 1);
    if !reply.text.is_empty() {
        blocks.push(json!({"type": "text", "text": reply.text}));
    }
    blocks.extend(reply.tool_calls.iter().map(|call| {
        json!({"type": "tool_use", "id": call.id, "name": call.name, "input": call.arguments})
    }));
    Value::Array(blocks)
}
