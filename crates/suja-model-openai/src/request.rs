use serde_json::{Value, json};
use suja_model::{AssistantReply, Entry, ToolResult, TurnRequest};

use crate::options::OpenAiOptions;

pub fn request_body(options: &OpenAiOptions, request: &TurnRequest) -> Value {
    let mut messages = vec![json!({"role": "system", "content": &*request.system})];
    for entry in &request.transcript {
        match entry {
            Entry::User(text) => push_user(&mut messages, text),
            Entry::Assistant(reply) => messages.push(assistant_message(reply)),
            Entry::ToolResults(results) => messages.extend(results.iter().map(tool_message)),
        }
    }
    let mut body = json!({
        "model": options.model,
        "stream": true,
        "stream_options": {"include_usage": true},
        "messages": messages,
    });
    body[options.token_limit.field_name()] = json!(request.max_reply_tokens);
    if !request.tools.is_empty() {
        body["tools"] = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect();
    }
    body
}

/// Joins a user entry to a user message before it, because some servers refuse two in a row.
fn push_user(messages: &mut Vec<Value>, text: &str) {
    if let Some(last) = messages.last_mut()
        && last["role"] == "user"
        && let Some(previous) = last["content"].as_str()
    {
        last["content"] = json!(format!("{previous}\n\n{text}"));
        return;
    }
    messages.push(json!({"role": "user", "content": text}));
}

fn assistant_message(reply: &AssistantReply) -> Value {
    let mut message = json!({"role": "assistant", "content": reply.text});
    if !reply.tool_calls.is_empty() {
        if reply.text.is_empty() {
            message["content"] = Value::Null;
        }
        message["tool_calls"] = reply
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {"name": call.name, "arguments": call.arguments.to_string()},
                })
            })
            .collect();
    }
    message
}

fn tool_message(result: &ToolResult) -> Value {
    json!({"role": "tool", "tool_call_id": result.call_id, "content": result.content})
}
