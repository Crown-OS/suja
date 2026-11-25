use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use suja_model::{
    AssistantReply, CancellationToken, ChatModel, Entry, Expert, FinishReason, ModelError,
    ModelEvent, ToolCall, ToolResult, ToolSpec, TurnRequest,
};
use suja_tools::{Registry, ToolOutcome};
use tokio::sync::Mutex;

use crate::conversation::Conversation;
use crate::error::AgentError;
use crate::handoff::{HANDOFF_TOOL, addressed_expert, expert_view, handoff_target, handoff_tool};
use crate::system_prompt::{
    EXPERT_INSTRUCTIONS, HANDOFF_INSTRUCTIONS, INSTRUCTIONS, user_turn_prefix,
};

const MAX_TOOL_HOPS: usize = 3;
const CONTEXT_RESERVE_TOKENS: u32 = 1536;
const BASE_REPLY_TOKENS: u32 = 512;
const EXPERT_REPLY_TOKENS: u32 = 4096;
/// The longest expert answer kept in the conversation of the base model.
const KEPT_ANSWER_CHARS: usize = 1500;

#[derive(Clone, Debug, PartialEq)]
pub enum TurnEvent {
    Text(String),
    ToolCall(ToolCall),
    ToolResult { name: String, outcome: ToolOutcome },
    NeedsConfirmation(ToolCall),
    Handoff { expert: String, task: String },
    Finished(FinishReason),
}

#[derive(Clone, Debug, Default)]
pub struct TurnReport {
    pub spoken: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish: Option<FinishReason>,
}

#[derive(Clone)]
struct Prompt {
    system: Arc<str>,
    tools: Arc<[ToolSpec]>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Base,
    Expert(usize),
}

enum LoopEnd {
    Finished(FinishReason),
    Handoff {
        call: ToolCall,
        other_results: Vec<ToolResult>,
    },
}

/// What one turn needs besides the conversation.
struct TurnContext<'turn, OnEvent> {
    cancel: &'turn CancellationToken,
    report: &'turn mut TurnReport,
    on_event: &'turn mut OnEvent,
}

pub struct Agent {
    base: Arc<dyn ChatModel>,
    experts: Vec<Expert>,
    registry: Arc<Registry>,
    base_prompt: Prompt,
    expert_prompt: Prompt,
    conversation: Mutex<Conversation>,
    context_tokens: AtomicU32,
}

impl Agent {
    pub fn new(base: Arc<dyn ChatModel>, experts: Vec<Expert>, registry: Arc<Registry>) -> Self {
        let tools = tool_specs(&registry);
        let expert_prompt = Prompt {
            system: Arc::from(EXPERT_INSTRUCTIONS),
            tools: tools.iter().cloned().collect(),
        };
        let base_prompt = if experts.is_empty() {
            Prompt {
                system: Arc::from(INSTRUCTIONS),
                tools: tools.into(),
            }
        } else {
            let mut base_tools = tools;
            base_tools.push(handoff_tool(&experts));
            Prompt {
                system: Arc::from(format!("{INSTRUCTIONS}{HANDOFF_INSTRUCTIONS}")),
                tools: base_tools.into(),
            }
        };
        base.warm_up(&base_prompt.system, &base_prompt.tools);
        Self {
            base,
            experts,
            registry,
            base_prompt,
            expert_prompt,
            conversation: Mutex::new(Conversation::default()),
            context_tokens: AtomicU32::new(0),
        }
    }

    /// Tokens in the base model's context after its last reply.
    pub fn context_tokens(&self) -> u32 {
        self.context_tokens.load(Ordering::Relaxed)
    }

    pub fn expert_names(&self) -> impl Iterator<Item = &str> {
        self.experts.iter().map(|expert| expert.name.as_str())
    }

    pub async fn reset_conversation(&self) {
        self.conversation.lock().await.clear();
    }

    /// The first reply of the base model to `user_text` in a new conversation, with no tool executed.
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
        let mut context = TurnContext {
            cancel: &cancel,
            report: &mut report,
            on_event: &mut |_| {},
        };
        let (_, finish) = self
            .generate(Role::Base, &conversation.transcript, &mut context)
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
        let mut context = TurnContext {
            cancel: &cancel,
            report: &mut report,
            on_event: &mut on_event,
        };
        let mut user_entry = format!(
            "{}{user_text}",
            user_turn_prefix(chrono::Local::now(), screen_context)
        );
        if let Some(call) = conversation.take_confirmed(user_text) {
            let outcome = self.run_confirmed(&call, &mut context).await;
            let _ = write!(
                user_entry,
                "\n[The user said yes, so `{}` already ran. Do not call it again. Its result: {}]",
                call.name, outcome.detail_for_model
            );
            context.report.tool_calls.push(call);
        }
        conversation.transcript.push(Entry::User(user_entry));

        let finish = if let Some((index, task)) = addressed_expert(&self.experts, user_text) {
            let answer = self
                .consult(index, task, &mut conversation, &mut context)
                .await?;
            conversation
                .transcript
                .push(Entry::Assistant(AssistantReply {
                    text: answer,
                    ..AssistantReply::default()
                }));
            FinishReason::EndOfTurn
        } else {
            match self.base_turn(&mut conversation, &mut context).await {
                Err(AgentError::Model(ModelError::ContextFull)) => {
                    tracing::info!("context full, keeping only this turn");
                    let turn_start = conversation
                        .transcript
                        .iter()
                        .rposition(|entry| matches!(entry, Entry::User(_)))
                        .unwrap_or(0);
                    conversation.transcript.drain(..turn_start);
                    self.context_tokens.store(0, Ordering::Relaxed);
                    self.base_turn(&mut conversation, &mut context).await?
                }
                other => other?,
            }
        };
        report.finish = Some(finish);
        on_event(TurnEvent::Finished(finish));
        Ok(report)
    }

    async fn base_turn(
        &self,
        conversation: &mut Conversation,
        context: &mut TurnContext<'_, impl FnMut(TurnEvent)>,
    ) -> Result<FinishReason, AgentError> {
        let Conversation {
            transcript,
            pending_confirmation,
        } = conversation;
        let (call, mut other_results) = match self
            .converse(Role::Base, transcript, pending_confirmation, context)
            .await?
        {
            LoopEnd::Finished(reason) => return Ok(reason),
            LoopEnd::Handoff {
                call,
                other_results,
            } => (call, other_results),
        };
        let (index, task) = handoff_target(&self.experts, &call);
        let answer = self.consult(index, task, conversation, context).await;
        other_results.push(ToolResult {
            call_id: call.id.clone(),
            name: HANDOFF_TOOL.to_owned(),
            content: match &answer {
                Ok(text) => text.clone(),
                Err(error) => format!("the expert failed: {error}"),
            },
            is_error: answer.is_err(),
        });
        conversation
            .transcript
            .push(Entry::ToolResults(other_results));
        answer.map(|_| FinishReason::EndOfTurn)
    }

    /// Runs expert `index` on `task` and gives the words it spoke.
    async fn consult(
        &self,
        index: usize,
        task: &str,
        conversation: &mut Conversation,
        context: &mut TurnContext<'_, impl FnMut(TurnEvent)>,
    ) -> Result<String, AgentError> {
        let Some(expert) = self.experts.get(index) else {
            return Err(AgentError::NoExpert);
        };
        (context.on_event)(TurnEvent::Handoff {
            expert: expert.name.clone(),
            task: task.to_owned(),
        });
        let mut view = expert_view(&conversation.transcript, task);
        let spoken_before = context.report.spoken.len();
        self.converse(
            Role::Expert(index),
            &mut view,
            &mut conversation.pending_confirmation,
            context,
        )
        .await?;
        Ok(kept_answer(context.report.spoken[spoken_before..].trim()))
    }

    /// Replies and runs tools until the model stops, hands off, or uses up its hops.
    async fn converse(
        &self,
        role: Role,
        transcript: &mut Vec<Entry>,
        pending_confirmation: &mut Option<ToolCall>,
        context: &mut TurnContext<'_, impl FnMut(TurnEvent)>,
    ) -> Result<LoopEnd, AgentError> {
        for _hop in 0..=MAX_TOOL_HOPS {
            let (reply, finish) = self.generate(role, transcript, context).await?;
            let calls = reply.tool_calls.clone();
            if !reply.is_empty() {
                transcript.push(Entry::Assistant(reply));
            }
            if finish != FinishReason::ToolCall || calls.is_empty() {
                return Ok(LoopEnd::Finished(finish));
            }
            let handoff = calls
                .iter()
                .find(|call| role == Role::Base && call.name == HANDOFF_TOOL)
                .cloned();
            let others: Vec<ToolCall> = calls
                .into_iter()
                .filter(|call| Some(&call.id) != handoff.as_ref().map(|handoff| &handoff.id))
                .collect();
            let results = self.run_calls(&others, pending_confirmation, context).await;
            if let Some(call) = handoff {
                return Ok(LoopEnd::Handoff {
                    call,
                    other_results: results,
                });
            }
            transcript.push(Entry::ToolResults(results));
        }
        Ok(LoopEnd::Finished(FinishReason::LengthLimit))
    }

    fn make_room(&self, conversation: &mut Conversation) {
        let limit = self.base.capabilities().context_tokens;
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
        role: Role,
        transcript: &[Entry],
        context: &mut TurnContext<'_, impl FnMut(TurnEvent)>,
    ) -> Result<(AssistantReply, FinishReason), AgentError> {
        let (model, prompt, max_reply_tokens) = match role {
            Role::Base => (&self.base, &self.base_prompt, BASE_REPLY_TOKENS),
            Role::Expert(index) => (
                &self.experts.get(index).ok_or(AgentError::NoExpert)?.model,
                &self.expert_prompt,
                EXPERT_REPLY_TOKENS,
            ),
        };
        let mut stream = model.stream(
            TurnRequest {
                system: prompt.system.clone(),
                tools: prompt.tools.clone(),
                transcript: transcript.to_vec(),
                max_reply_tokens,
            },
            context.cancel.clone(),
        );
        let mut reply = AssistantReply::default();
        while let Some(event) = stream.recv().await {
            match event? {
                ModelEvent::Text(text) => {
                    reply.text.push_str(&text);
                    context.report.spoken.push_str(&text);
                    (context.on_event)(TurnEvent::Text(text));
                }
                ModelEvent::ToolCall(call) => {
                    reply.tool_calls.push(call.clone());
                    context.report.tool_calls.push(call.clone());
                    (context.on_event)(TurnEvent::ToolCall(call));
                }
                ModelEvent::Finished(finish) => {
                    reply.backend_data = finish.backend_data;
                    if role == Role::Base {
                        self.context_tokens
                            .store(finish.usage.total(), Ordering::Relaxed);
                    }
                    return Ok((reply, finish.reason));
                }
            }
        }
        Err(AgentError::StreamEnded)
    }

    async fn run_calls(
        &self,
        calls: &[ToolCall],
        pending_confirmation: &mut Option<ToolCall>,
        context: &mut TurnContext<'_, impl FnMut(TurnEvent)>,
    ) -> Vec<ToolResult> {
        let mut results = Vec::with_capacity(calls.len());
        for call in calls {
            let outcome = match self.registry.call(&call.name, call.arguments.clone()).await {
                Ok(outcome) => outcome,
                Err(error) => ToolOutcome::failure(error.to_string()),
            };
            if outcome.needs_confirmation {
                *pending_confirmation = Some(call.clone());
                (context.on_event)(TurnEvent::NeedsConfirmation(call.clone()));
            }
            results.push(ToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                content: outcome.detail_for_model.clone(),
                is_error: !outcome.ok && !outcome.needs_confirmation,
            });
            (context.on_event)(TurnEvent::ToolResult {
                name: call.name.clone(),
                outcome,
            });
        }
        results
    }

    async fn run_confirmed(
        &self,
        call: &ToolCall,
        context: &mut TurnContext<'_, impl FnMut(TurnEvent)>,
    ) -> ToolOutcome {
        (context.on_event)(TurnEvent::ToolCall(call.clone()));
        let outcome = match self
            .registry
            .call_confirmed(&call.name, call.arguments.clone())
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => ToolOutcome::failure(error.to_string()),
        };
        (context.on_event)(TurnEvent::ToolResult {
            name: call.name.clone(),
            outcome: outcome.clone(),
        });
        outcome
    }
}

fn kept_answer(answer: &str) -> String {
    match answer.char_indices().nth(KEPT_ANSWER_CHARS) {
        Some((cut, _)) => format!("{}…", &answer[..cut]),
        None => answer.to_owned(),
    }
}

fn tool_specs(registry: &Registry) -> Vec<ToolSpec> {
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
