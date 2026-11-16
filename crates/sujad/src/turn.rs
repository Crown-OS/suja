use std::time::Instant;

use suja_proto::TurnTimings;

pub struct TurnClock {
    started: Instant,
    pub timings: TurnTimings,
}

impl TurnClock {
    pub fn start_at(turn_id: u64, started: Instant) -> Self {
        Self {
            started,
            timings: TurnTimings {
                turn_id,
                ..TurnTimings::default()
            },
        }
    }

    fn elapsed_ms(&self) -> u32 {
        u32::try_from(self.started.elapsed().as_millis()).unwrap_or(u32::MAX)
    }

    pub fn mark_llm_start(&mut self) {
        self.timings.llm_start_ms = self.elapsed_ms();
    }

    pub fn mark_first_token(&mut self) {
        if self.timings.first_token_ms == 0 {
            self.timings.first_token_ms = self.elapsed_ms();
        }
    }

    pub fn mark_first_chunk(&mut self) {
        if self.timings.first_chunk_ms == 0 {
            self.timings.first_chunk_ms = self.elapsed_ms();
        }
    }

    pub fn mark_first_audio(&mut self) {
        if self.timings.first_audio_ms == 0 {
            self.timings.first_audio_ms = self.elapsed_ms();
        }
    }

    pub fn mark_llm_done(&mut self) {
        self.timings.llm_done_ms = self.elapsed_ms();
    }

    pub fn mark_playback_done(&mut self) {
        self.timings.playback_done_ms = self.elapsed_ms();
    }
}
