use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;

use suja_model::{
    AssistantReply, CancellationToken, Entry, Finish, FinishReason, ModelError, ModelEvent,
    ToolCall, ToolSpec, TurnRequest, Usage,
};
use tokio::sync::{mpsc, oneshot};

use crate::backend::LlamaBackendOptions;
use crate::engine::Engine;
use crate::error::LlmError;
use crate::grammar::{SamplingOptions, plain_sampler, tool_call_grammar, tool_call_sampler};
use crate::prompt::{ASSISTANT_OPEN, TURN_END, render_entry, render_system};
use crate::session::Session;
use crate::stream::Delta;

type EventSender = mpsc::UnboundedSender<Result<ModelEvent, ModelError>>;

pub enum Command {
    Prime {
        system: Arc<str>,
        tools: Arc<[ToolSpec]>,
    },
    Stream {
        request: TurnRequest,
        cancel: CancellationToken,
        events: EventSender,
    },
    Shutdown,
}

pub fn run(
    options: LlamaBackendOptions,
    inbox: std_mpsc::Receiver<Command>,
    ready: oneshot::Sender<Result<(), LlmError>>,
) {
    let engine = match Engine::load(&options.engine) {
        Ok(engine) => engine,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let session = match Session::new(&engine, options.session) {
        Ok(session) => session,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut worker = Worker {
        engine: &engine,
        session,
        sampling: options.sampling,
        prompt_signature: None,
        tool_grammar: None,
        cached: Vec::new(),
        calls_made: 0,
    };
    let _ = ready.send(Ok(()));
    while let Ok(command) = inbox.recv() {
        match command {
            Command::Prime { system, tools } => {
                if let Err(error) = worker.prime(&system, &tools) {
                    tracing::warn!(%error, "prompt prefix failed");
                }
            }
            Command::Stream {
                request,
                cancel,
                events,
            } => worker.stream(&request, &cancel, &events),
            Command::Shutdown => break,
        }
    }
}

/// Keeps the KV cache equal to the prompt prefix followed by `cached`, so a request that
/// extends the transcript only prefills its new entries.
struct Worker<'engine> {
    engine: &'engine Engine,
    session: Session<'engine>,
    sampling: SamplingOptions,
    prompt_signature: Option<u64>,
    tool_grammar: Option<String>,
    cached: Vec<Entry>,
    calls_made: u64,
}

impl Worker<'_> {
    fn prime(&mut self, system: &str, tools: &[ToolSpec]) -> Result<(), LlmError> {
        let signature = prompt_signature(system, tools);
        if self.prompt_signature == Some(signature) {
            return Ok(());
        }
        self.prompt_signature = None;
        self.cached.clear();
        self.tool_grammar = if tools.is_empty() {
            None
        } else {
            Some(tool_call_grammar(tools)?)
        };
        self.session.prime(&render_system(system, tools))?;
        self.prompt_signature = Some(signature);
        tracing::info!(
            prefix_tokens = self.session.tokens_in_context(),
            "prompt prefix ready"
        );
        Ok(())
    }

    fn stream(&mut self, request: &TurnRequest, cancel: &CancellationToken, events: &EventSender) {
        let event = match self.reply(request, cancel, events) {
            Ok(finish) => Ok(ModelEvent::Finished(finish)),
            Err(error) => {
                self.forget_conversation();
                Err(match error {
                    LlmError::ContextFull => ModelError::ContextFull,
                    other => ModelError::Runtime(other.to_string()),
                })
            }
        };
        let _ = events.send(event);
    }

    fn reply(
        &mut self,
        request: &TurnRequest,
        cancel: &CancellationToken,
        events: &EventSender,
    ) -> Result<Finish, LlmError> {
        self.prime(&request.system, &request.tools)?;
        let new_entries = self.new_entries(&request.transcript);
        let mut prompt = String::new();
        for entry in new_entries {
            render_entry(entry, &mut prompt);
        }
        prompt.push_str(ASSISTANT_OPEN);
        match self.session.append_text(&prompt, cancel) {
            Ok(()) => {}
            Err(LlmError::Cancelled) => {
                self.forget_conversation();
                return Ok(finish(FinishReason::Cancelled, Usage::default()));
            }
            Err(error) => return Err(error),
        }
        self.cached.extend_from_slice(new_entries);

        let input_tokens = self.session.tokens_in_context();
        let mut sampler = match &self.tool_grammar {
            Some(grammar) => tool_call_sampler(self.engine.model(), grammar, self.sampling)?,
            None => plain_sampler(self.sampling),
        };
        let mut reply = AssistantReply::default();
        let mut calls_made = self.calls_made;
        let reason = self.session.generate(
            &mut sampler,
            cancel,
            request.max_reply_tokens as usize,
            |delta| match delta {
                Delta::Text(text) => {
                    reply.text.push_str(&text);
                    let _ = events.send(Ok(ModelEvent::Text(text)));
                }
                Delta::ToolCall(parsed) => {
                    calls_made += 1;
                    let call = ToolCall {
                        id: format!("call_{calls_made}"),
                        name: parsed.name,
                        arguments: parsed.arguments,
                    };
                    reply.tool_calls.push(call.clone());
                    let _ = events.send(Ok(ModelEvent::ToolCall(call)));
                }
            },
        )?;
        self.calls_made = calls_made;
        let output_tokens = self.session.tokens_in_context() - input_tokens;
        if reason == FinishReason::Cancelled {
            self.forget_conversation();
        } else {
            self.session
                .append_text(TURN_END, &CancellationToken::new())?;
            self.cached.push(Entry::Assistant(reply));
        }
        Ok(finish(
            reason,
            Usage {
                input_tokens,
                output_tokens,
            },
        ))
    }

    fn new_entries<'request>(&mut self, transcript: &'request [Entry]) -> &'request [Entry] {
        if transcript.starts_with(&self.cached) {
            return &transcript[self.cached.len()..];
        }
        self.forget_conversation();
        transcript
    }

    fn forget_conversation(&mut self) {
        self.session.reset_to_prefix();
        self.cached.clear();
    }
}

fn finish(reason: FinishReason, usage: Usage) -> Finish {
    Finish {
        reason,
        usage,
        backend_data: None,
    }
}

fn prompt_signature(system: &str, tools: &[ToolSpec]) -> u64 {
    let mut hasher = DefaultHasher::new();
    system.hash(&mut hasher);
    for tool in tools {
        tool.name.hash(&mut hasher);
        tool.description.hash(&mut hasher);
        tool.parameters.to_string().hash(&mut hasher);
    }
    hasher.finish()
}
