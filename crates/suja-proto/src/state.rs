use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssistantState {
    #[default]
    Idle,
    Listening,
    Thinking,
    Tool {
        name: String,
    },
    Confirming,
    Speaking,
}

impl AssistantState {
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub state: AssistantState,
    pub model: String,
    pub tts_backend: String,
    pub turns_in_context: u32,
    pub resident_memory_mib: u64,
}
