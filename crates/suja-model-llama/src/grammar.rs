use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;

use crate::error::LlmError;
use suja_model::ToolSpec;

pub const TOOL_CALL_OPEN: &str = "<tool_call>";
pub const TOOL_CALL_CLOSE: &str = "</tool_call>";
const TRIGGER_PATTERN: &str = r"[\s\S]*?(<tool_call>[\s\S]*)";

pub fn tool_call_grammar(tools: &[ToolSpec]) -> Result<String, LlmError> {
    let calls: Vec<serde_json::Value> = tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {"const": tool.name},
                    "arguments": tool.parameters,
                },
                "required": ["name", "arguments"],
                "additionalProperties": false,
            })
        })
        .collect();
    let schema = serde_json::json!({"oneOf": calls}).to_string();
    let generated = llama_cpp_2::json_schema_to_grammar(&schema)
        .map_err(|e| LlmError::Runtime(format!("tool grammar: {e}")))?;
    Ok(close_tool_call(&generated))
}

fn close_tool_call(generated: &str) -> String {
    let renamed = generated.replacen("root ::=", "tool-call ::=", 1);
    format!(
        "{renamed}\nroot ::= \"{TOOL_CALL_OPEN}\" space tool-call space \"{TOOL_CALL_CLOSE}\"\n"
    )
}

#[derive(Clone, Copy, Debug)]
pub struct SamplingOptions {
    pub temperature: f32,
    pub top_p: f32,
    pub seed: u32,
}

impl Default for SamplingOptions {
    fn default() -> Self {
        Self {
            temperature: 0.15,
            top_p: 0.9,
            seed: 42,
        }
    }
}

pub fn tool_call_sampler(
    model: &LlamaModel,
    grammar: &str,
    options: SamplingOptions,
) -> Result<LlamaSampler, LlmError> {
    let lazy = LlamaSampler::grammar_lazy_patterns(
        model,
        grammar,
        "root",
        &[TRIGGER_PATTERN.to_owned()],
        &[],
    )
    .map_err(|e| LlmError::Runtime(format!("lazy grammar: {e}")))?;
    Ok(LlamaSampler::chain_simple([
        lazy,
        LlamaSampler::top_p(options.top_p, 1),
        LlamaSampler::temp(options.temperature),
        LlamaSampler::dist(options.seed),
    ]))
}

pub fn plain_sampler(options: SamplingOptions) -> LlamaSampler {
    LlamaSampler::chain_simple([
        LlamaSampler::top_p(options.top_p, 1),
        LlamaSampler::temp(options.temperature),
        LlamaSampler::dist(options.seed),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_root_closes_the_tag() {
        let closed = close_tool_call("root ::= \"{\" space \"}\"\nspace ::= \" \"?");
        assert!(closed.starts_with("tool-call ::="));
        assert!(
            closed.ends_with("root ::= \"<tool_call>\" space tool-call space \"</tool_call>\"\n")
        );
    }
}
