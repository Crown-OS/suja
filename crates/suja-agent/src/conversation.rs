use suja_model::{Entry, ToolCall};

const CONFIRMATIONS: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "sure",
    "do it",
    "go ahead",
    "confirm",
    "please do",
    "ok",
    "okay",
];

#[derive(Default)]
pub struct Conversation {
    pub transcript: Vec<Entry>,
    pub pending_confirmation: Option<ToolCall>,
}

impl Conversation {
    pub fn clear(&mut self) {
        self.transcript.clear();
        self.pending_confirmation = None;
    }

    /// The call that waits for a yes, when `user_text` gives it. Any other answer drops the call.
    pub fn take_confirmed(&mut self, user_text: &str) -> Option<ToolCall> {
        let pending = self.pending_confirmation.take()?;
        is_confirmation(user_text).then_some(pending)
    }
}

fn is_confirmation(user_text: &str) -> bool {
    let normalized: String = user_text
        .chars()
        .filter(|character| !character.is_ascii_punctuation())
        .collect::<String>()
        .trim()
        .to_ascii_lowercase();
    CONFIRMATIONS
        .iter()
        .any(|yes| normalized == *yes || normalized.starts_with(&format!("{yes} ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending() -> Conversation {
        Conversation {
            transcript: Vec::new(),
            pending_confirmation: Some(ToolCall {
                id: "call_1".into(),
                name: "run_command".into(),
                arguments: serde_json::json!({"command": "rm x"}),
            }),
        }
    }

    #[test]
    fn a_yes_releases_the_pending_call() {
        let mut conversation = pending();
        assert!(conversation.take_confirmed("Yes, do it.").is_some());
        assert!(conversation.pending_confirmation.is_none());
    }

    #[test]
    fn any_other_answer_drops_the_pending_call() {
        let mut conversation = pending();
        assert!(conversation.take_confirmed("no wait").is_none());
        assert!(conversation.take_confirmed("yes").is_none());
    }
}
