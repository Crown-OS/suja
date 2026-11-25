use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use anyhow::{Result, bail};
use suja_agent::{Agent, TurnEvent};
use suja_proto::AssistantState;
use suja_proto::protocol::{self, Event};
use suja_speech::SentenceChunker;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::audio::capture::{ListenHandle, SpeechEvent};
use crate::hold::{HoldAction, HoldTracker, PressId};
use crate::hotkey::HotkeyEvent;
use crate::ipc::Request;
use crate::tts_thread::{TtsEvent, TtsHandle};
use crate::turn::TurnClock;

pub struct Inputs {
    pub requests: mpsc::UnboundedReceiver<Request>,
    pub hotkeys: mpsc::UnboundedReceiver<HotkeyEvent>,
    pub speech: mpsc::UnboundedReceiver<SpeechEvent>,
    pub tts: mpsc::UnboundedReceiver<TtsEvent>,
}

pub struct Outputs {
    pub events: mpsc::UnboundedSender<Event>,
    pub state: watch::Sender<AssistantState>,
    pub context_tokens: Arc<AtomicU32>,
}

pub struct Services {
    pub agent: Arc<Agent>,
    pub context: Arc<suja_context::Collector>,
    pub listen: ListenHandle,
    pub tts: TtsHandle,
    pub tts_events: mpsc::UnboundedSender<TtsEvent>,
    pub idle_reset: Duration,
}

struct ActiveTurn {
    id: u64,
    cancel: CancellationToken,
    clock: TurnClock,
    chunker: SentenceChunker,
    llm_done: bool,
    tts_pending: usize,
    spoke: bool,
}

pub struct Orchestrator {
    inputs: Inputs,
    outputs: Outputs,
    services: Services,
    state: AssistantState,
    next_turn: u64,
    turn: Option<ActiveTurn>,
    turn_events: mpsc::UnboundedSender<(u64, TurnEvent)>,
    turn_inbox: mpsc::UnboundedReceiver<(u64, TurnEvent)>,
    last_activity: tokio::time::Instant,
    release_at: Option<tokio::time::Instant>,
    hold: HoldTracker,
    /// Whether the microphone is open because the hotkey is held, and not
    /// because of a `talk` request.
    hold_listening: bool,
    recording_due_sender: mpsc::UnboundedSender<PressId>,
    recording_due: mpsc::UnboundedReceiver<PressId>,
}

impl Orchestrator {
    pub fn new(inputs: Inputs, outputs: Outputs, services: Services) -> Self {
        let (turn_events, turn_inbox) = mpsc::unbounded_channel();
        let (recording_due_sender, recording_due) = mpsc::unbounded_channel();
        Self {
            inputs,
            outputs,
            services,
            state: AssistantState::Idle,
            next_turn: 1,
            turn: None,
            turn_events,
            turn_inbox,
            last_activity: tokio::time::Instant::now(),
            release_at: None,
            hold: HoldTracker::default(),
            hold_listening: false,
            recording_due_sender,
            recording_due,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        let mut idle_tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                Some(request) = self.inputs.requests.recv() => self.on_request(request).await,
                Some(key) = self.inputs.hotkeys.recv() => self.on_hotkey(key)?,
                Some(press) = self.recording_due.recv() => {
                    if let Some(action) = self.hold.recording_due(press) {
                        self.on_hold_action(action);
                    }
                }
                Some(event) = self.inputs.speech.recv() => self.on_speech(event).await,
                Some(event) = self.inputs.tts.recv() => self.on_tts(event),
                Some((turn_id, event)) = self.turn_inbox.recv() => self.on_turn_event(turn_id, event),
                _ = idle_tick.tick() => self.on_idle_tick().await,
                _ = tokio::signal::ctrl_c() => return Ok(()),
            }
        }
    }

    fn set_state(&mut self, state: AssistantState) {
        if self.state == state {
            return;
        }
        self.state = state.clone();
        let _ = self.outputs.state.send(state.clone());
        self.emit(Event::StateChanged(protocol::StateChanged { state }));
    }

    fn emit(&self, event: Event) {
        let _ = self.outputs.events.send(event);
    }

    async fn on_request(&mut self, request: Request) {
        match request {
            Request::TalkStart => self.start_listening(),
            Request::TalkStop => self.stop_listening(),
            Request::TalkToggle => {
                if self.state == AssistantState::Listening {
                    self.stop_listening();
                } else {
                    self.start_listening();
                }
            }
            Request::Cancel => self.cancel_turn(),
            Request::Say(text) => self.say(text),
            Request::Ask(text) => {
                self.cancel_turn();
                self.release_at = Some(tokio::time::Instant::now());
                self.emit(Event::FinalTranscript(protocol::FinalTranscript {
                    text: text.clone(),
                }));
                self.run_turn(text);
            }
        }
    }

    fn on_hotkey(&mut self, event: HotkeyEvent) -> Result<()> {
        let now = std::time::Instant::now();
        let actions = match event {
            HotkeyEvent::Assigned(keys) => {
                tracing::info!(keys, "the compositor assigned the hotkey");
                return Ok(());
            }
            HotkeyEvent::Pressed => self.hold.pressed(now),
            HotkeyEvent::Released => self.hold.released(now).into_iter().collect(),
            HotkeyEvent::Cancelled => self.hold.cancelled().into_iter().collect(),
            HotkeyEvent::Lost(reason) => bail!("lost the connection to the compositor: {reason}"),
        };
        for action in actions {
            self.on_hold_action(action);
        }
        Ok(())
    }

    fn on_hold_action(&mut self, action: HoldAction) {
        match action {
            HoldAction::ScheduleRecording { press, at } => {
                let due = self.recording_due_sender.clone();
                tokio::spawn(async move {
                    tokio::time::sleep_until(at.into()).await;
                    let _ = due.send(press);
                });
            }
            HoldAction::Tap => self.emit(Event::HotkeyTapped(protocol::HotkeyTapped {})),
            HoldAction::StartRecording => {
                if self.state != AssistantState::Listening {
                    self.start_listening();
                    self.hold_listening = true;
                }
            }
            HoldAction::SendRecording => {
                if self.take_hold_listening() {
                    self.stop_listening();
                }
            }
            HoldAction::DropRecording => {
                if self.take_hold_listening() {
                    self.services.listen.discard();
                    self.set_state(AssistantState::Idle);
                }
            }
        }
    }

    /// Whether the open microphone belongs to the hotkey hold, which then
    /// gives it up.
    fn take_hold_listening(&mut self) -> bool {
        std::mem::take(&mut self.hold_listening) && self.state == AssistantState::Listening
    }

    fn start_listening(&mut self) {
        self.hold_listening = false;
        self.cancel_turn();
        self.last_activity = tokio::time::Instant::now();
        self.services.listen.start();
        self.set_state(AssistantState::Listening);
    }

    fn stop_listening(&mut self) {
        if self.state == AssistantState::Listening {
            self.release_at = Some(tokio::time::Instant::now());
            self.services.listen.stop();
            self.set_state(AssistantState::Thinking);
        }
    }

    fn cancel_turn(&mut self) {
        if let Some(turn) = self.turn.take() {
            turn.cancel.cancel();
            self.services.tts.set_current_turn(0);
            self.services.tts.flush_playback();
        }
        self.set_state(AssistantState::Idle);
    }

    fn say(&mut self, text: String) {
        self.cancel_turn();
        let turn_id = self.begin_turn();
        let chunks = self
            .with_turn(turn_id, |turn| {
                turn.llm_done = true;
                let mut chunks = turn.chunker.push(&text);
                chunks.extend(turn.chunker.flush());
                chunks
            })
            .unwrap_or_default();
        self.speak_chunks(turn_id, chunks);
        self.set_state(AssistantState::Speaking);
        self.finish_if_done();
    }

    fn begin_turn(&mut self) -> u64 {
        let id = self.next_turn;
        self.next_turn += 1;
        let cancel = CancellationToken::new();
        self.services.tts.set_current_turn(id);
        let started = self
            .release_at
            .take()
            .unwrap_or_else(tokio::time::Instant::now);
        self.turn = Some(ActiveTurn {
            id,
            cancel,
            clock: TurnClock::start_at(id, started.into_std()),
            chunker: SentenceChunker::default(),
            llm_done: false,
            tts_pending: 0,
            spoke: false,
        });
        id
    }

    async fn on_speech(&mut self, event: SpeechEvent) {
        match event {
            SpeechEvent::Level(_) | SpeechEvent::Partial(_)
                if self.state == AssistantState::Idle => {}
            SpeechEvent::Level(level) => self.emit(Event::MicLevel(protocol::MicLevel { level })),
            SpeechEvent::Partial(text) => {
                self.emit(Event::PartialTranscript(protocol::PartialTranscript {
                    text,
                }))
            }
            SpeechEvent::Final(text) => {
                let text = text.trim().to_lowercase();
                self.emit(Event::FinalTranscript(protocol::FinalTranscript {
                    text: text.clone(),
                }));
                if text.is_empty() {
                    self.release_at = None;
                    self.set_state(AssistantState::Idle);
                } else {
                    self.run_turn(text);
                }
            }
            SpeechEvent::Failed(message) => {
                self.emit(Event::Failure(protocol::Failure { message }));
                self.set_state(AssistantState::Idle);
            }
        }
    }

    fn run_turn(&mut self, text: String) {
        let turn_id = self.begin_turn();
        let Some(turn) = self.turn.as_mut() else {
            return;
        };
        turn.clock.mark_llm_start();
        let cancel = turn.cancel.clone();
        let agent = self.services.agent.clone();
        let context = self.services.context.clone();
        let events = self.turn_events.clone();
        self.set_state(AssistantState::Thinking);
        tokio::spawn(async move {
            let screen = context.snapshot().await.render();
            let result = agent
                .turn(&text, screen.as_deref(), cancel, |event| {
                    let _ = events.send((turn_id, event));
                })
                .await;
            if let Err(error) = result {
                tracing::error!(%error, turn_id, "turn failed");
                let _ = events.send((
                    turn_id,
                    TurnEvent::Text(format!("Sorry, that failed: {error}")),
                ));
                let _ = events.send((
                    turn_id,
                    TurnEvent::Finished(suja_model::FinishReason::Cancelled),
                ));
            }
        });
    }

    fn with_turn<R>(
        &mut self,
        turn_id: u64,
        apply: impl FnOnce(&mut ActiveTurn) -> R,
    ) -> Option<R> {
        self.turn.as_mut().filter(|t| t.id == turn_id).map(apply)
    }

    fn on_turn_event(&mut self, turn_id: u64, event: TurnEvent) {
        if self.turn.as_ref().is_none_or(|t| t.id != turn_id) {
            return;
        }
        match event {
            TurnEvent::Text(delta) => {
                let chunks = self.with_turn(turn_id, |turn| {
                    turn.clock.mark_first_token();
                    turn.chunker.push(&delta)
                });
                if delta.is_empty() {
                    return;
                }
                self.emit(Event::AssistantText(protocol::AssistantText { delta }));
                self.speak_chunks(turn_id, chunks.unwrap_or_default());
            }
            TurnEvent::ToolCall(call) => {
                let rest = self
                    .with_turn(turn_id, |turn| turn.chunker.flush())
                    .flatten();
                self.speak_chunks(turn_id, rest.into_iter().collect());
                self.set_state(AssistantState::Tool {
                    name: call.name.clone(),
                });
                self.emit(Event::ToolCall(protocol::ToolCall {
                    name: call.name,
                    arguments: call.arguments.to_string(),
                }));
            }
            TurnEvent::NeedsConfirmation(_) => self.set_state(AssistantState::Confirming),
            TurnEvent::ToolResult { name, outcome } => {
                self.emit(Event::ToolResult(protocol::ToolResult {
                    name,
                    ok: outcome.ok,
                    summary: outcome.spoken_summary,
                }));
                if self.state != AssistantState::Confirming {
                    self.set_state(AssistantState::Thinking);
                }
            }
            TurnEvent::Finished(_) => {
                self.outputs
                    .context_tokens
                    .store(self.services.agent.context_tokens(), Ordering::Relaxed);
                let rest = self
                    .with_turn(turn_id, |turn| turn.chunker.flush())
                    .flatten();
                self.speak_chunks(turn_id, rest.into_iter().collect());
                self.with_turn(turn_id, |turn| {
                    turn.llm_done = true;
                    turn.clock.mark_llm_done();
                });
                self.finish_if_done();
            }
        }
    }

    fn speak_chunks(&mut self, turn_id: u64, chunks: Vec<String>) {
        if chunks.is_empty() {
            return;
        }
        self.with_turn(turn_id, |turn| {
            turn.clock.mark_first_chunk();
            turn.tts_pending += chunks.len();
        });
        for chunk in chunks {
            self.services.tts.speak(turn_id, chunk);
        }
    }

    fn on_tts(&mut self, event: TtsEvent) {
        match event {
            TtsEvent::FirstAudio { turn_id } => {
                let live = self
                    .with_turn(turn_id, |turn| {
                        turn.clock.mark_first_audio();
                        turn.spoke = true;
                    })
                    .is_some();
                if live {
                    self.set_state(AssistantState::Speaking);
                }
            }
            TtsEvent::Finished { turn_id } => {
                self.with_turn(turn_id, |turn| {
                    turn.tts_pending = turn.tts_pending.saturating_sub(1)
                });
                self.finish_if_done();
            }
            TtsEvent::PlaybackDrained { turn_id } => {
                if self.turn.as_ref().is_some_and(|t| t.id == turn_id) {
                    self.finish_if_done();
                }
            }
            TtsEvent::Failed { turn_id, message } => {
                self.with_turn(turn_id, |turn| {
                    turn.tts_pending = turn.tts_pending.saturating_sub(1)
                });
                self.emit(Event::Failure(protocol::Failure { message }));
                self.finish_if_done();
            }
        }
    }

    fn finish_if_done(&mut self) {
        let done = self
            .turn
            .as_ref()
            .is_some_and(|t| t.llm_done && t.tts_pending == 0);
        if !done {
            return;
        }
        if !self.services.tts.playback_drained() {
            let tts = self.services.tts.clone();
            let events = self.services.tts_events.clone();
            let turn_id = self.turn.as_ref().map_or(0, |t| t.id);
            tokio::spawn(async move {
                while !tts.playback_drained() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                let _ = events.send(TtsEvent::PlaybackDrained { turn_id });
            });
            return;
        }
        let Some(mut turn) = self.turn.take() else {
            return;
        };
        turn.clock.mark_playback_done();
        self.last_activity = tokio::time::Instant::now();
        self.emit(Event::TurnFinished(protocol::TurnFinished {
            timings: turn.clock.timings,
        }));
        tracing::info!(
            turn = turn.id,
            first_token_ms = turn.clock.timings.first_token_ms,
            first_audio_ms = turn.clock.timings.first_audio_ms,
            "turn finished"
        );
        self.set_state(AssistantState::Idle);
    }

    async fn on_idle_tick(&mut self) {
        if self.state == AssistantState::Idle
            && self.last_activity.elapsed() > self.services.idle_reset
        {
            self.services.agent.reset_conversation().await;
            self.last_activity = tokio::time::Instant::now();
        }
    }
}
