use serde_json::json;
use suja_model::{AssistantReply, Entry, Expert, ToolCall, ToolSpec};

pub const HANDOFF_TOOL: &str = "handoff";

pub fn handoff_tool(experts: &[Expert]) -> ToolSpec {
    let names: Vec<&str> = experts.iter().map(|expert| expert.name.as_str()).collect();
    let listing: Vec<String> = experts
        .iter()
        .map(|expert| format!("{}: {}", expert.name, expert.use_for))
        .collect();
    ToolSpec {
        name: HANDOFF_TOOL.to_owned(),
        description: format!(
            "Hand the request to a stronger model, which answers the user directly. Experts: {}.",
            listing.join("; ")
        ),
        parameters: json!({
            "type": "object",
            "properties": {
                "expert": {"type": "string", "enum": names},
                "task": {"type": "string", "description": "The full request, with every fact the expert needs"}
            },
            "required": ["expert", "task"]
        }),
    }
}

/// The expert and the task of a handoff call. An unknown name goes to the first expert.
pub fn handoff_target<'call>(experts: &[Expert], call: &'call ToolCall) -> (usize, &'call str) {
    let name = call.arguments["expert"].as_str().unwrap_or_default();
    let task = call.arguments["task"].as_str().unwrap_or_default();
    let index = experts
        .iter()
        .position(|expert| expert.name.eq_ignore_ascii_case(name))
        .unwrap_or(0);
    (index, task)
}

/// The expert and the rest of the request when the user says "ask <expert> ...".
pub fn addressed_expert<'text>(
    experts: &[Expert],
    user_text: &'text str,
) -> Option<(usize, &'text str)> {
    let lowered = user_text.trim_start().to_ascii_lowercase();
    let rest = lowered.strip_prefix("ask ")?;
    experts
        .iter()
        .enumerate()
        .filter_map(|(index, expert)| {
            let spoken_name = expert.name.to_ascii_lowercase().replace(['-', '_'], " ");
            let after = rest.strip_prefix(&spoken_name)?;
            let ends_at_word = after.is_empty() || after.starts_with([' ', ',', ':']);
            ends_at_word.then_some((index, spoken_name.len(), after.len()))
        })
        .max_by_key(|&(_, name_length, _)| name_length)
        .map(|(index, _, rest_length)| {
            let task = &user_text[user_text.len() - rest_length..];
            (index, task.trim_start_matches([' ', ',', ':']))
        })
}

/// The conversation as an expert sees it: no handoff calls, earlier expert answers as assistant
/// text, and the task as the last user words.
pub fn expert_view(transcript: &[Entry], task: &str) -> Vec<Entry> {
    let mut view = Vec::with_capacity(transcript.len() + 1);
    for entry in transcript {
        match entry {
            Entry::Assistant(reply) if reply.tool_calls.iter().any(is_handoff) => {
                let mut kept = reply.clone();
                kept.tool_calls.retain(|call| !is_handoff(call));
                kept.backend_data = None;
                push_assistant(&mut view, kept);
            }
            Entry::ToolResults(results) => {
                let (answers, others): (Vec<_>, Vec<_>) = results
                    .iter()
                    .cloned()
                    .partition(|result| result.name == HANDOFF_TOOL);
                if !others.is_empty() {
                    view.push(Entry::ToolResults(others));
                }
                for answer in answers {
                    push_assistant(
                        &mut view,
                        AssistantReply {
                            text: answer.content,
                            ..AssistantReply::default()
                        },
                    );
                }
            }
            other => view.push(other.clone()),
        }
    }
    if !task.trim().is_empty() {
        view.push(Entry::User(format!(
            "[The assistant handed this request to you: {task}]"
        )));
    }
    view
}

fn is_handoff(call: &ToolCall) -> bool {
    call.name == HANDOFF_TOOL
}

fn push_assistant(view: &mut Vec<Entry>, reply: AssistantReply) {
    if reply.is_empty() {
        return;
    }
    if let Some(Entry::Assistant(last)) = view.last_mut()
        && last.tool_calls.is_empty()
        && reply.tool_calls.is_empty()
    {
        if !last.text.is_empty() {
            last.text.push(' ');
        }
        last.text.push_str(&reply.text);
        last.backend_data = None;
        return;
    }
    view.push(Entry::Assistant(reply));
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use suja_model::{
        CancellationToken, ChatModel, ModelCapabilities, ModelError, ModelStream, ToolResult,
        TurnRequest, failed_stream,
    };

    use super::*;

    struct NoModel;

    impl ChatModel for NoModel {
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities {
                context_tokens: 0,
                parallel_tool_calls: false,
                runs_locally: true,
            }
        }

        fn stream(&self, _: TurnRequest, _: CancellationToken) -> ModelStream {
            failed_stream(ModelError::Unavailable("test".into()))
        }
    }

    fn experts() -> Vec<Expert> {
        ["claude", "claude-code"]
            .into_iter()
            .map(|name| Expert {
                name: name.into(),
                use_for: "hard work".into(),
                model: Arc::new(NoModel),
            })
            .collect()
    }

    #[test]
    fn ask_claude_code_picks_the_longer_name() {
        let experts = experts();
        assert_eq!(
            addressed_expert(&experts, "Ask Claude Code, fix the build"),
            Some((1, "fix the build"))
        );
        assert_eq!(
            addressed_expert(&experts, "ask claude why the sky is blue"),
            Some((0, "why the sky is blue"))
        );
        assert_eq!(addressed_expert(&experts, "ask claudette"), None);
        assert_eq!(addressed_expert(&experts, "what is the time"), None);
    }

    #[test]
    fn the_expert_sees_earlier_answers_as_text_and_no_handoff_calls() {
        let call = ToolCall {
            id: "call_1".into(),
            name: HANDOFF_TOOL.into(),
            arguments: json!({"expert": "claude", "task": "plan a trip"}),
        };
        let transcript = vec![
            Entry::User("plan a trip".into()),
            Entry::Assistant(AssistantReply {
                text: "Let me ask Claude.".into(),
                tool_calls: vec![call],
                backend_data: None,
            }),
            Entry::ToolResults(vec![ToolResult {
                call_id: "call_1".into(),
                name: HANDOFF_TOOL.into(),
                content: "Go to Kyoto.".into(),
                is_error: false,
            }]),
            Entry::User("and for how long?".into()),
        ];
        let view = expert_view(&transcript, "how long to stay in Kyoto");
        assert_eq!(
            view,
            vec![
                Entry::User("plan a trip".into()),
                Entry::Assistant(AssistantReply {
                    text: "Let me ask Claude. Go to Kyoto.".into(),
                    ..AssistantReply::default()
                }),
                Entry::User("and for how long?".into()),
                Entry::User(
                    "[The assistant handed this request to you: how long to stay in Kyoto]".into()
                ),
            ]
        );
    }
}
