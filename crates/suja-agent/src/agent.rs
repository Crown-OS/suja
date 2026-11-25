use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use suja_model::{
    AssistantReply, CancellationToken, ChatModel, Entry, FinishReason, ModelEvent, ToolCall,
    ToolResult, ToolSpec, TurnRequest,
};
use suja_tools::{Registry, ToolOutcome};
use tokio::sync::Mutex;

use crate::conversation::Conversation;
use crate::error::AgentError;
use crate::system_prompt::{INSTRUCTIONS, user_turn_prefix};

const MAX_TOOL_HOPS: usize = 3;
const CONTEXT_RESERVE_TOKENS: u32 = 1536;
const MAX_REPLY_TOKENS: u32 = 512;

#[derive(Clone, Debug, PartialEq)]
pub enum TurnEvent {
    Text(String),
    ToolCall(ToolCall),
    ToolResult { name: String, outcome: ToolOutcome },
    NeedsConfirmation(ToolCall),
    Finished(FinishReason),
}

#[derive(Clone, Debug, Default)]
pub struct TurnReport {
    pub spoken: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish: Option<FinishReason>,
}

pub struct Agent {
    model: Arc<dyn ChatModel>,
    registry: Arc<Registry>,
    system: Arc<str>,
    tools: Arc<[ToolSpec]>,
    conversation: Mutex<Conversation>,
    context_tokens: AtomicU32,
}

impl Agent {
    pub fn new(model: Arc<dyn ChatModel>, registry: Arc<Registry>) -> Self {
        let system: Arc<str> = Arc::from(INSTRUCTIONS);
        let tools = tool_specs(&registry);
        model.warm_up(&system, &tools);
        Self {
            model,
            registry,
            system,
            tools,
            conversation: Mutex::new(Conversation::default()),
            context_tokens: AtomicU32::new(0),
        }
    }

    /// Tokens in the model's context after the last reply.
    pub fn context_tokens(&self) -> u32 {
        self.context_tokens.load(Ordering::Relaxed)
    }

    pub async fn reset_conversation(&self) {
        self.conversation.lock().await.clear();
    }

    /// The first reply to `user_text` in a new conversation, with no tool executed.
    pub async fn first_action(
        &self,
        user_text: &str,
        cancel: CancellationToken,
    ) -> Result<TurnReport, AgentError> {
        let mut conversation = self.conversation.lock().await;
        conversation.clear();
        let prefix = user_turn_prefix(chrono::Local::now(), None);
        conversation
            .transcript
            .push(Entry::User(format!("{prefix}{user_text}")));
        let mut report = TurnReport::default();
        let (_, finish) = self
            .generate(&conversation.transcript, &cancel, &mut report, &mut |_| {})
            .await?;
        conversation.clear();
        report.finish = Some(finish);
        Ok(report)
    }

    pub async fn turn(
        &self,
        user_text: &str,
        screen_context: Option<&str>,
        cancel: CancellationToken,
        mut on_event: impl FnMut(TurnEvent),
    ) -> Result<TurnReport, AgentError> {
        let mut conversation = self.conversation.lock().await;
        self.make_room(&mut conversation);
        let mut report = TurnReport::default();
        let mut user_entry = format!(
            "{}{user_text}",
            user_turn_prefix(chrono::Local::now(), screen_context)
        );
        if let Some(call) = conversation.take_confirmed(user_text) {
            let outcome = self.run_confirmed(&call, &mut on_event).await;
            let _ = write!(
                user_entry,
                "\n[confirmed: tool `{}` ran: {}]",
                call.name, outcome.detail_for_model
            );
            report.tool_calls.push(call);
        }
        conversation.transcript.push(Entry::User(user_entry));

        for _hop in 0..=MAX_TOOL_HOPS {
            let (reply, finish) = self
                .generate(
                    &conversation.transcript,
                    &cancel,
                    &mut report,
                    &mut on_event,
                )
                .await?;
            let calls = reply.tool_calls.clone();
            if !reply.is_empty() {
                conversation.transcript.push(Entry::Assistant(reply));
            }
            if finish != FinishReason::ToolCall || calls.is_empty() {
                report.finish = Some(finish);
                on_event(TurnEvent::Finished(finish));
                return Ok(report);
            }
            let results = self
                .run_calls(&calls, &mut conversation, &mut on_event)
                .await;
            conversation.transcript.push(Entry::ToolResults(results));
        }
        report.finish = Some(FinishReason::LengthLimit);
        on_event(TurnEvent::Finished(FinishReason::LengthLimit));
        Ok(report)
    }

    fn make_room(&self, conversation: &mut Conversation) {
        let limit = self.model.capabilities().context_tokens;
        if self.context_tokens() + CONTEXT_RESERVE_TOKENS > limit {
            tracing::info!(
                used = self.context_tokens(),
                limit,
                "context nearly full, starting a fresh conversation"
            );
            conversation.clear();
            self.context_tokens.store(0, Ordering::Relaxed);
        }
    }

    async fn generate(
        &self,
        transcript: &[Entry],
        cancel: &CancellationToken,
        report: &mut TurnReport,
        on_event: &mut impl FnMut(TurnEvent),
    ) -> Result<(AssistantReply, FinishReason), AgentError> {
        let mut stream = self.model.stream(
            TurnRequest {
                system: self.system.clone(),
                tools: self.tools.clone(),
                transcript: transcript.to_vec(),
                max_reply_tokens: MAX_REPLY_TOKENS,
            },
            cancel.clone(),
        );
        let mut reply = AssistantReply::default();
        while let Some(event) = stream.recv().await {
            match event? {
                ModelEvent::Text(text) => {
                    reply.text.push_str(&text);
                    report.spoken.push_str(&text);
                    on_event(TurnEvent::Text(text));
                }
                ModelEvent::ToolCall(call) => {
                    reply.tool_calls.push(call.clone());
                    report.tool_calls.push(call.clone());
                    on_event(TurnEvent::ToolCall(call));
                }
                ModelEvent::Finished(finish) => {
                    reply.backend_data = finish.backend_data;
                    self.context_tokens
                        .store(finish.usage.total(), Ordering::Relaxed);
                    return Ok((reply, finish.reason));
                }
            }
        }
        Err(AgentError::StreamEnded)
    }

    async fn run_calls(
        &self,
        calls: &[ToolCall],
        conversation: &mut Conversation,
        on_event: &mut impl FnMut(TurnEvent),
    ) -> Vec<ToolResult> {
        let mut results = Vec::with_capacity(calls.len());
        for call in calls {
            let outcome = match self.registry.call(&call.name, call.arguments.clone()).await {
                Ok(outcome) => outcome,
                Err(error) => ToolOutcome::failure(error.to_string()),
            };
            if outcome.needs_confirmation {
                conversation.pending_confirmation = Some(call.clone());
                on_event(TurnEvent::NeedsConfirmation(call.clone()));
            }
            results.push(ToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                content: outcome.detail_for_model.clone(),
                is_error: !outcome.ok && !outcome.needs_confirmation,
            });
            on_event(TurnEvent::ToolResult {
                name: call.name.clone(),
                outcome,
            });
        }
        results
    }

    async fn run_confirmed(
        &self,
        call: &ToolCall,
        on_event: &mut impl FnMut(TurnEvent),
    ) -> ToolOutcome {
        on_event(TurnEvent::ToolCall(call.clone()));
        let outcome = match self
            .registry
            .call_confirmed(&call.name, call.arguments.clone())
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => ToolOutcome::failure(error.to_string()),
        };
        on_event(TurnEvent::ToolResult {
            name: call.name.clone(),
            outcome: outcome.clone(),
        });
        outcome
    }
}

fn tool_specs(registry: &Registry) -> Arc<[ToolSpec]> {
    registry
        .specs()
        .into_iter()
        .map(|spec| ToolSpec {
            name: spec.name.to_owned(),
            description: spec.description.to_owned(),
            parameters: spec.parameters,
        })
        .collect()
}
