use std::sync::{Arc, Mutex};

use suja_llm::grammar::tool_call_grammar;
use suja_llm::prompt::{render_assistant_end, render_system, render_tool_result, render_user};
use suja_llm::{CancellationToken, Delta, FinishReason, ToolCall, ToolSpec};
use suja_tools::{Registry, ToolOutcome};

use crate::error::AgentError;
use crate::system_prompt::{INSTRUCTIONS, user_turn_prefix};
use crate::worker::{GenerateMode, WorkerHandle, WorkerOptions};

const MAX_TOOL_HOPS: usize = 3;
const CONTEXT_RESERVE_TOKENS: u32 = 1536;
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
    worker: WorkerHandle,
    registry: Arc<Registry>,
    pending_confirmation: Mutex<Option<ToolCall>>,
}

impl Agent {
    pub fn tool_specs(registry: &Registry) -> Vec<ToolSpec> {
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

    pub fn worker_options(
        registry: &Registry,
        engine: suja_llm::EngineOptions,
        session: suja_llm::SessionOptions,
    ) -> Result<WorkerOptions, AgentError> {
        let specs = Self::tool_specs(registry);
        let tool_grammar = if specs.is_empty() {
            None
        } else {
            Some(tool_call_grammar(&specs)?)
        };
        Ok(WorkerOptions {
            engine,
            session,
            sampling: Default::default(),
            prefix: render_system(INSTRUCTIONS, &specs),
            tool_grammar,
        })
    }

    pub fn new(worker: WorkerHandle, registry: Arc<Registry>) -> Self {
        Self {
            worker,
            registry,
            pending_confirmation: Mutex::new(None),
        }
    }

    pub async fn first_action(
        &self,
        user_text: &str,
        cancel: CancellationToken,
    ) -> Result<TurnReport, AgentError> {
        self.make_room().await?;
        let prefix = user_turn_prefix(chrono::Local::now(), None);
        self.worker
            .append(render_user(&format!("{prefix}{user_text}")))
            .await?;
        let mut report = TurnReport::default();
        let finish = self
            .generate_once(&cancel, &mut report, &mut |_| {})
            .await?;
        self.worker
            .append(render_assistant_end().to_owned())
            .await?;
        report.finish = Some(finish);
        Ok(report)
    }

    async fn make_room(&self) -> Result<(), AgentError> {
        let stats = self.worker.stats().await?;
        if stats.remaining_tokens < CONTEXT_RESERVE_TOKENS {
            tracing::info!(
                remaining = stats.remaining_tokens,
                "context nearly full, starting a fresh conversation"
            );
            self.worker.reset_conversation().await?;
        }
        Ok(())
    }

    fn take_confirmed(&self, user_text: &str) -> Option<ToolCall> {
        let normalized = user_text
            .trim()
            .trim_end_matches(['.', '!'])
            .to_ascii_lowercase();
        let mut pending = self.pending_confirmation.lock().ok()?;
        if CONFIRMATIONS
            .iter()
            .any(|yes| normalized == *yes || normalized.starts_with(&format!("{yes} ")))
        {
            return pending.take();
        }
        *pending = None;
        None
    }

    pub fn worker(&self) -> &WorkerHandle {
        &self.worker
    }

    pub async fn turn(
        &self,
        user_text: &str,
        screen_context: Option<&str>,
        cancel: CancellationToken,
        mut on_event: impl FnMut(TurnEvent),
    ) -> Result<TurnReport, AgentError> {
        let prefix = user_turn_prefix(chrono::Local::now(), screen_context);
        self.worker
            .append(render_user(&format!("{prefix}{user_text}")))
            .await?;
        let mut report = TurnReport::default();
        if let Some(call) = self.take_confirmed(user_text) {
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
            self.worker
                .append(render_tool_result(&format!(
                    "The user confirmed. Tool `{}` ran: {}",
                    call.name, outcome.detail_for_model
                )))
                .await?;
            report.tool_calls.push(call);
        }
        for _hop in 0..=MAX_TOOL_HOPS {
            let finish = self
                .generate_once(&cancel, &mut report, &mut on_event)
                .await?;
            match finish {
                FinishReason::ToolCall => {
                    let Some(call) = report.tool_calls.last().cloned() else {
                        break;
                    };
                    self.worker
                        .append(render_assistant_end().to_owned())
                        .await?;
                    let outcome = self.execute(&call).await;
                    if outcome.needs_confirmation {
                        if let Ok(mut pending) = self.pending_confirmation.lock() {
                            *pending = Some(call.clone());
                        }
                        on_event(TurnEvent::NeedsConfirmation(call.clone()));
                    }
                    on_event(TurnEvent::ToolResult {
                        name: call.name.clone(),
                        outcome: outcome.clone(),
                    });
                    self.worker
                        .append(render_tool_result(&outcome.detail_for_model))
                        .await?;
                }
                other => {
                    self.worker
                        .append(render_assistant_end().to_owned())
                        .await?;
                    report.finish = Some(other);
                    on_event(TurnEvent::Finished(other));
                    return Ok(report);
                }
            }
        }
        report.finish = Some(FinishReason::LengthLimit);
        on_event(TurnEvent::Finished(FinishReason::LengthLimit));
        Ok(report)
    }

    async fn generate_once(
        &self,
        cancel: &CancellationToken,
        report: &mut TurnReport,
        on_event: &mut impl FnMut(TurnEvent),
    ) -> Result<FinishReason, AgentError> {
        let (mut deltas, done) = self
            .worker
            .generate(GenerateMode::WithTools, cancel.clone())?;
        while let Some(delta) = deltas.recv().await {
            match delta {
                Delta::Text(text) => {
                    report.spoken.push_str(&text);
                    on_event(TurnEvent::Text(text));
                }
                Delta::ToolCall(call) => {
                    report.tool_calls.push(call.clone());
                    on_event(TurnEvent::ToolCall(call));
                }
                Delta::Finished(_) => {}
            }
        }
        done.await
            .map_err(|_| AgentError::WorkerGone)?
            .map_err(Into::into)
    }

    async fn execute(&self, call: &ToolCall) -> ToolOutcome {
        match self.registry.call(&call.name, call.arguments.clone()).await {
            Ok(outcome) => outcome,
            Err(error) => ToolOutcome::failure(error.to_string()),
        }
    }
}
