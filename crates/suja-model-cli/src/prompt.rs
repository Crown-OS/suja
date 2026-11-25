use std::fmt::Write;

use suja_model::Entry;

/// The conversation as plain text for an agent CLI: earlier turns, then the last request.
pub fn render_conversation(transcript: &[Entry]) -> String {
    let (request, earlier) = match transcript.split_last() {
        Some((Entry::User(text), earlier)) => (text.as_str(), earlier),
        _ => ("", transcript),
    };
    let mut text = String::new();
    if !earlier.is_empty() {
        text.push_str("Conversation so far:\n");
        for entry in earlier {
            match entry {
                Entry::User(message) => {
                    let _ = writeln!(text, "User: {message}");
                }
                Entry::Assistant(reply) if !reply.text.is_empty() => {
                    let _ = writeln!(text, "Assistant: {}", reply.text);
                }
                Entry::Assistant(_) => {}
                Entry::ToolResults(results) => {
                    for result in results {
                        let _ =
                            writeln!(text, "(Tool {} returned: {})", result.name, result.content);
                    }
                }
            }
        }
        text.push('\n');
    }
    text.push_str(request);
    text
}

#[cfg(test)]
mod tests {
    use suja_model::AssistantReply;

    use super::*;

    #[test]
    fn earlier_turns_come_first_and_the_request_last() {
        let transcript = vec![
            Entry::User("hi".into()),
            Entry::Assistant(AssistantReply {
                text: "Hello.".into(),
                ..AssistantReply::default()
            }),
            Entry::User("plan a trip".into()),
        ];
        assert_eq!(
            render_conversation(&transcript),
            "Conversation so far:\nUser: hi\nAssistant: Hello.\n\nplan a trip"
        );
    }
}
