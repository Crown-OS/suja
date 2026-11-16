use std::fmt::Write;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

const TURN_END: &str = "<|im_end|>\n";

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

pub fn render_user(message: &str) -> String {
    format!("<|im_start|>user\n{message}{TURN_END}<|im_start|>assistant\n")
}

pub fn render_tool_result(content: &str) -> String {
    format!(
        "<|im_start|>user\n<tool_response>\n{content}\n</tool_response>{TURN_END}<|im_start|>assistant\n"
    )
}

pub fn render_assistant_end() -> &'static str {
    TURN_END
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
    fn user_turn_opens_assistant_turn() {
        assert_eq!(
            render_user("hi"),
            "<|im_start|>user\nhi<|im_end|>\n<|im_start|>assistant\n"
        );
    }
}
