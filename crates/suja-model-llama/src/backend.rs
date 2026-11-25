use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::thread::{self, JoinHandle};

use suja_model::{
    CancellationToken, ChatModel, ModelCapabilities, ModelError, ModelStream, ToolSpec,
    TurnRequest, failed_stream,
};
use tokio::sync::{mpsc, oneshot};

use crate::engine::EngineOptions;
use crate::error::LlmError;
use crate::grammar::SamplingOptions;
use crate::session::SessionOptions;
use crate::worker::{Command, run};

#[derive(Clone, Debug)]
pub struct LlamaBackendOptions {
    pub engine: EngineOptions,
    pub session: SessionOptions,
    pub sampling: SamplingOptions,
}

/// A GGUF model on this machine. One thread owns the model and its KV cache.
pub struct LlamaBackend {
    commands: std_mpsc::Sender<Command>,
    thread: Option<JoinHandle<()>>,
    capabilities: ModelCapabilities,
}

impl LlamaBackend {
    pub async fn load(options: LlamaBackendOptions) -> Result<Self, LlmError> {
        let capabilities = ModelCapabilities {
            context_tokens: options.session.context_tokens,
            parallel_tool_calls: false,
            runs_locally: true,
        };
        let (commands, inbox) = std_mpsc::channel();
        let (ready, loaded) = oneshot::channel();
        let thread = thread::Builder::new()
            .name("suja-llm".into())
            .spawn(move || run(options, inbox, ready))
            .map_err(|e| LlmError::Runtime(e.to_string()))?;
        loaded
            .await
            .map_err(|_| LlmError::Runtime("the language model thread stopped".into()))??;
        Ok(Self {
            commands,
            thread: Some(thread),
            capabilities,
        })
    }
}

impl ChatModel for LlamaBackend {
    fn capabilities(&self) -> ModelCapabilities {
        self.capabilities
    }

    fn stream(&self, request: TurnRequest, cancel: CancellationToken) -> ModelStream {
        let (events, stream) = mpsc::unbounded_channel();
        match self.commands.send(Command::Stream {
            request,
            cancel,
            events,
        }) {
            Ok(()) => stream,
            Err(_) => failed_stream(ModelError::Unavailable(
                "the language model thread stopped".into(),
            )),
        }
    }

    fn warm_up(&self, system: &Arc<str>, tools: &Arc<[ToolSpec]>) {
        let _ = self.commands.send(Command::Prime {
            system: system.clone(),
            tools: tools.clone(),
        });
    }
}

impl Drop for LlamaBackend {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
