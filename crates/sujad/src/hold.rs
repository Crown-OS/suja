//! What a press of the hotkey means, from how long the user holds it.
//!
//! - A release before [`RECORD_AFTER`] is a tap.
//! - At [`RECORD_AFTER`] the microphone opens.
//! - A release at [`SEND_AFTER`] or later sends the recording to the
//!   assistant. An earlier release drops it.
//! - A cancel drops the recording. The compositor cancels the press when the
//!   user presses another key or a pointer button with the hotkey held.
//!
//! The tracker only decides. The orchestrator runs the timer and the
//! microphone.

use std::time::{Duration, Instant};

pub const RECORD_AFTER: Duration = Duration::from_secs(1);
pub const SEND_AFTER: Duration = Duration::from_secs(2);

/// Tells one press from the next, so a timer of an old press does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PressId(u64);

#[derive(Debug, PartialEq, Eq)]
pub enum HoldAction {
    /// Call [`HoldTracker::recording_due`] with this press at the given time.
    ScheduleRecording {
        press: PressId,
        at: Instant,
    },
    Tap,
    StartRecording,
    SendRecording,
    DropRecording,
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Idle,
    Held { press: PressId, since: Instant },
    Recording { since: Instant },
}

#[derive(Debug)]
pub struct HoldTracker {
    phase: Phase,
    presses: u64,
}

impl Default for HoldTracker {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            presses: 0,
        }
    }
}

impl HoldTracker {
    /// The hotkey went down. A recording of an earlier press that never
    /// ended is dropped.
    pub fn pressed(&mut self, now: Instant) -> Vec<HoldAction> {
        let mut actions = self.cancelled().into_iter().collect::<Vec<_>>();
        self.presses += 1;
        let press = PressId(self.presses);
        self.phase = Phase::Held { press, since: now };
        actions.push(HoldAction::ScheduleRecording {
            press,
            at: now + RECORD_AFTER,
        });
        actions
    }

    /// The timer of `press` ran out.
    pub fn recording_due(&mut self, press: PressId) -> Option<HoldAction> {
        match self.phase {
            Phase::Held { press: held, since } if held == press => {
                self.phase = Phase::Recording { since };
                Some(HoldAction::StartRecording)
            }
            _ => None,
        }
    }

    /// The hotkey came up.
    pub fn released(&mut self, now: Instant) -> Option<HoldAction> {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => None,
            Phase::Held { since, .. } => {
                (now.duration_since(since) < RECORD_AFTER).then_some(HoldAction::Tap)
            }
            Phase::Recording { since } => Some(if now.duration_since(since) >= SEND_AFTER {
                HoldAction::SendRecording
            } else {
                HoldAction::DropRecording
            }),
        }
    }

    /// The press ended without a release.
    pub fn cancelled(&mut self) -> Option<HoldAction> {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Recording { .. } => Some(HoldAction::DropRecording),
            Phase::Idle | Phase::Held { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(tracker: &mut HoldTracker, start: Instant) -> PressId {
        match tracker.pressed(start).as_slice() {
            [HoldAction::ScheduleRecording { press, at }] => {
                assert_eq!(*at, start + RECORD_AFTER);
                *press
            }
            other => panic!("expected one schedule, got {other:?}"),
        }
    }

    fn held_for(tracker: &mut HoldTracker, hold: Duration) -> Option<HoldAction> {
        let start = Instant::now();
        let press = press(tracker, start);
        if hold >= RECORD_AFTER {
            assert_eq!(
                tracker.recording_due(press),
                Some(HoldAction::StartRecording)
            );
        }
        tracker.released(start + hold)
    }

    #[test]
    fn a_short_press_is_a_tap() {
        let mut tracker = HoldTracker::default();
        assert_eq!(
            held_for(&mut tracker, Duration::from_millis(300)),
            Some(HoldAction::Tap)
        );
    }

    #[test]
    fn a_long_hold_sends_the_recording() {
        let mut tracker = HoldTracker::default();
        assert_eq!(
            held_for(&mut tracker, Duration::from_millis(2000)),
            Some(HoldAction::SendRecording)
        );
    }

    #[test]
    fn a_hold_between_one_and_two_seconds_drops_the_recording() {
        let mut tracker = HoldTracker::default();
        assert_eq!(
            held_for(&mut tracker, Duration::from_millis(1500)),
            Some(HoldAction::DropRecording)
        );
    }

    #[test]
    fn a_cancel_drops_the_recording_and_is_not_a_tap() {
        let mut tracker = HoldTracker::default();
        let start = Instant::now();
        let first = press(&mut tracker, start);
        assert_eq!(tracker.cancelled(), None);
        assert_eq!(tracker.released(start + Duration::from_millis(100)), None);
        assert_eq!(tracker.recording_due(first), None);

        let second = press(&mut tracker, start);
        tracker.recording_due(second);
        assert_eq!(tracker.cancelled(), Some(HoldAction::DropRecording));
        assert_eq!(tracker.released(start + SEND_AFTER), None);
    }

    #[test]
    fn the_timer_of_an_old_press_does_nothing() {
        let mut tracker = HoldTracker::default();
        let start = Instant::now();
        let old = press(&mut tracker, start);
        tracker.released(start + Duration::from_millis(100));
        let _new = press(&mut tracker, start + Duration::from_millis(500));
        assert_eq!(tracker.recording_due(old), None);
    }
}
