use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use suja_llm::grammar::{SamplingOptions, plain_sampler, tool_call_sampler};
use suja_llm::{
    CancellationToken, Delta, Engine, EngineOptions, FinishReason, LlmError, Session,
    SessionOptions,
};
use tokio::sync::{mpsc, oneshot};

use crate::error::AgentError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerateMode {
    WithTools,
    Plain,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ContextStats {
    pub tokens_in_context: u32,
    pub remaining_tokens: u32,
}

#[derive(Clone, Debug)]
pub struct WorkerOptions {
    pub engine: EngineOptions,
    pub session: SessionOptions,
    pub sampling: SamplingOptions,
    pub prefix: String,
    pub tool_grammar: Option<String>,
}

enum Command {
    Append {
        text: String,
        reply: oneshot::Sender<Result<(), LlmError>>,
    },
    Generate {
        mode: GenerateMode,
        cancel: CancellationToken,
        deltas: mpsc::UnboundedSender<Delta>,
        done: oneshot::Sender<Result<FinishReason, LlmError>>,
    },
    ResetConversation {
        reply: oneshot::Sender<()>,
    },
    Stats {
        reply: oneshot::Sender<ContextStats>,
    },
    Shutdown,
}

pub type DeltaStream = mpsc::UnboundedReceiver<Delta>;
pub type GenerationDone = oneshot::Receiver<Result<FinishReason, LlmError>>;

#[derive(Clone)]
pub struct WorkerHandle {
    commands: std_mpsc::Sender<Command>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl WorkerHandle {
    pub async fn spawn(options: WorkerOptions) -> Result<Self, AgentError> {
        let (commands, inbox) = std_mpsc::channel();
        let (ready_tx, ready_rx) = oneshot::channel();
        let thread = thread::Builder::new()
            .name("suja-llm".into())
            .spawn(move || run(options, inbox, ready_tx))
            .map_err(|e| AgentError::Llm(LlmError::Runtime(e.to_string())))?;
        ready_rx.await.map_err(|_| AgentError::WorkerGone)??;
        Ok(Self {
            commands,
            thread: Arc::new(Mutex::new(Some(thread))),
        })
    }

    pub fn shutdown(&self) {
        let _ = self.commands.send(Command::Shutdown);
        let thread = self.thread.lock().ok().and_then(|mut slot| slot.take());
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }

    pub async fn append(&self, text: String) -> Result<(), AgentError> {
        let (reply, response) = oneshot::channel();
        self.send(Command::Append { text, reply })?;
        response
            .await
            .map_err(|_| AgentError::WorkerGone)?
            .map_err(Into::into)
    }

    pub fn generate(
        &self,
        mode: GenerateMode,
        cancel: CancellationToken,
    ) -> Result<(DeltaStream, GenerationDone), AgentError> {
        let (deltas, delta_rx) = mpsc::unbounded_channel();
        let (done, done_rx) = oneshot::channel();
        self.send(Command::Generate {
            mode,
            cancel,
            deltas,
            done,
        })?;
        Ok((delta_rx, done_rx))
    }

    pub async fn reset_conversation(&self) -> Result<(), AgentError> {
        let (reply, response) = oneshot::channel();
        self.send(Command::ResetConversation { reply })?;
        response.await.map_err(|_| AgentError::WorkerGone)
    }

    pub async fn stats(&self) -> Result<ContextStats, AgentError> {
        let (reply, response) = oneshot::channel();
        self.send(Command::Stats { reply })?;
        response.await.map_err(|_| AgentError::WorkerGone)
    }

    fn send(&self, command: Command) -> Result<(), AgentError> {
        self.commands
            .send(command)
            .map_err(|_| AgentError::WorkerGone)
    }
}

fn run(
    options: WorkerOptions,
    inbox: std_mpsc::Receiver<Command>,
    ready: oneshot::Sender<Result<(), AgentError>>,
) {
    let engine = match Engine::load(&options.engine) {
        Ok(engine) => engine,
        Err(error) => {
            let _ = ready.send(Err(error.into()));
            return;
        }
    };
    let mut session = match Session::new(&engine, options.session)
        .and_then(|mut s| s.prime(&options.prefix).map(|()| s))
    {
        Ok(session) => session,
        Err(error) => {
            let _ = ready.send(Err(error.into()));
            return;
        }
    };
    tracing::info!(
        prefix_tokens = session.tokens_in_context(),
        "language model ready"
    );
    let _ = ready.send(Ok(()));
    while let Ok(command) = inbox.recv() {
        match command {
            Command::Append { text, reply } => {
                let _ = reply.send(session.append_text(&text, &CancellationToken::new()));
            }
            Command::Generate {
                mode,
                cancel,
                deltas,
                done,
            } => {
                let result = generate(&engine, &mut session, &options, mode, &cancel, &deltas);
                let _ = done.send(result);
            }
            Command::ResetConversation { reply } => {
                session.reset_to_prefix();
                let _ = reply.send(());
            }
            Command::Stats { reply } => {
                let _ = reply.send(ContextStats {
                    tokens_in_context: session.tokens_in_context(),
                    remaining_tokens: session.remaining_tokens(),
                });
            }
            Command::Shutdown => break,
        }
    }
    drop(session);
    drop(engine);
}

fn generate(
    engine: &Engine,
    session: &mut Session<'_>,
    options: &WorkerOptions,
    mode: GenerateMode,
    cancel: &CancellationToken,
    deltas: &mpsc::UnboundedSender<Delta>,
) -> Result<FinishReason, LlmError> {
    let mut sampler = match (mode, &options.tool_grammar) {
        (GenerateMode::WithTools, Some(grammar)) => {
            tool_call_sampler(engine.model(), grammar, options.sampling)?
        }
        _ => plain_sampler(options.sampling),
    };
    session.generate(&mut sampler, cancel, |delta| {
        let _ = deltas.send(delta);
    })
}
