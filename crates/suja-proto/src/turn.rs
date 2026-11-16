use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TurnTimings {
    pub turn_id: u64,
    pub endpoint_ms: u32,
    pub llm_start_ms: u32,
    pub first_token_ms: u32,
    pub first_chunk_ms: u32,
    pub first_audio_ms: u32,
    pub llm_done_ms: u32,
    pub playback_done_ms: u32,
}

impl TurnTimings {
    pub fn endpoint_to_first_audio_ms(&self) -> u32 {
        self.first_audio_ms.saturating_sub(self.endpoint_ms)
    }
}
