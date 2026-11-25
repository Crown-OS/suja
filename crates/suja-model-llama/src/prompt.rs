use std::fmt::Write;

use suja_model::{AssistantReply, Entry, ToolResult, ToolSpec};

use crate::grammar::{TOOL_CALL_CLOSE, TOOL_CALL_OPEN};

pub const TURN_END: &str = "<|im_end|>\n";
pub const ASSISTANT_OPEN: &str = "<|im_start|>assistant\n";

pub fn render_system(instructions: &str, tools: &[ToolSpec]) -> String {
    let mut text = String::with_capacity(instructions.len() + tools.len() * 256);
    text.push_str("<|im_start|>system\n");
    text.push_str(instructions);
    if !tools.is_empty() {
        text.push_str(
            "\n\n# Tools\n\nYou may call one or more functions to assist with the user query.\n\n\
             You are provided with function signatures within <tools></tools> XML tags:\n<tools>",
        );
        for tool in tools {
            let function = serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                }
            });
            let _ = write!(text, "\n{function}");
        }
        text.push_str(
            "\n</tools>\n\nFor each function call, return a json object with function name and arguments \
             within <tool_call></tool_call> XML tags:\n<tool_call>\n{\"name\": <function-name>, \
             \"arguments\": <args-json-object>}\n</tool_call>",
        );
    }
    text.push_str(TURN_END);
    text
}

pub fn render_entry(entry: &Entry, text: &mut String) {
    match entry {
        Entry::User(message) => {
            let _ = write!(text, "<|im_start|>user\n{message}{TURN_END}");
        }
        Entry::Assistant(reply) => render_assistant(reply, text),
        Entry::ToolResults(results) => render_tool_results(results, text),
    }
}

fn render_assistant(reply: &AssistantReply, text: &mut String) {
    text.push_str(ASSISTANT_OPEN);
    text.push_str(&reply.text);
    for call in &reply.tool_calls {
        let body = serde_json::json!({"name": call.name, "arguments": call.arguments});
        let _ = write!(text, "{TOOL_CALL_OPEN}\n{body}\n{TOOL_CALL_CLOSE}");
    }
    text.push_str(TURN_END);
}

fn render_tool_results(results: &[ToolResult], text: &mut String) {
    text.push_str("<|im_start|>user");
    for result in results {
        let _ = write!(
            text,
            "\n<tool_response>\n{}\n</tool_response>",
            result.content
        );
    }
    text.push_str(TURN_END);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_lists_tools_in_qwen_format() {
        let tools = [ToolSpec {
            name: "clock".into(),
            description: "Tell the time".into(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }];
        let text = render_system("Be brief.", &tools);
        assert!(text.starts_with("<|im_start|>system\nBe brief."));
        assert!(text.contains("<tools>\n{\"type\":\"function\",\"function\":{\"name\":\"clock\""));
        assert!(text.ends_with("</tool_call><|im_end|>\n"));
    }

    #[test]
    fn transcript_entries_render_in_qwen_format() {
        let mut text = String::new();
        render_entry(&Entry::User("hi".into()), &mut text);
        render_entry(
            &Entry::Assistant(AssistantReply {
                text: "Checking.".into(),
                tool_calls: vec![suja_model::ToolCall {
                    id: "call_1".into(),
                    name: "clock".into(),
                    arguments: serde_json::json!({}),
                }],
                backend_data: None,
            }),
            &mut text,
        );
        render_entry(
            &Entry::ToolResults(vec![ToolResult {
                call_id: "call_1".into(),
                name: "clock".into(),
                content: "12:00".into(),
                is_error: false,
            }]),
            &mut text,
        );
        assert_eq!(
            text,
            "<|im_start|>user\nhi<|im_end|>\n\
             <|im_start|>assistant\nChecking.<tool_call>\n{\"name\":\"clock\",\"arguments\":{}}\n</tool_call><|im_end|>\n\
             <|im_start|>user\n<tool_response>\n12:00\n</tool_response><|im_end|>\n"
        );
    }
}
