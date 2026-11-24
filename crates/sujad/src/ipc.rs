use anyhow::{Context, Result};
use crownos_ipc::adapter::tokio::serve_with_outbox;
use crownos_ipc::{RemoteError, Server, ServiceBuilder};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use suja_proto::protocol::{self, Event, Handler};
use suja_proto::{AssistantState, Status};

use tokio::sync::{mpsc, watch};

#[derive(Debug)]
pub enum Request {
    TalkStart,
    TalkStop,
    TalkToggle,
    Cancel,
    Say(String),
    Ask(String),
}

pub struct Shared {
    pub state: watch::Receiver<AssistantState>,
    pub model: String,
    pub tts_backend: String,
    pub context_tokens: Arc<AtomicU32>,
}

struct Dispatcher {
    requests: mpsc::UnboundedSender<Request>,
    shared: Shared,
}

pub fn resident_memory_mib() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| {
            s.split_whitespace()
                .nth(1)
                .and_then(|pages| pages.parse::<u64>().ok())
        })
        .map_or(0, |pages| pages * 4096 / (1024 * 1024))
}

impl Handler for Dispatcher {
    fn talk_start(&mut self) {
        let _ = self.requests.send(Request::TalkStart);
    }

    fn talk_stop(&mut self) {
        let _ = self.requests.send(Request::TalkStop);
    }

    fn talk_toggle(&mut self) {
        let _ = self.requests.send(Request::TalkToggle);
    }

    fn cancel(&mut self) {
        let _ = self.requests.send(Request::Cancel);
    }

    fn say(&mut self, text: String) {
        let _ = self.requests.send(Request::Say(text));
    }

    fn ask(&mut self, text: String) {
        let _ = self.requests.send(Request::Ask(text));
    }

    fn status(&mut self) -> Result<Status, RemoteError> {
        Ok(Status {
            state: self.shared.state.borrow().clone(),
            model: self.shared.model.clone(),
            tts_backend: self.shared.tts_backend.clone(),
            turns_in_context: self
                .shared
                .context_tokens
                .load(std::sync::atomic::Ordering::Relaxed),
            resident_memory_mib: resident_memory_mib(),
        })
    }
}

pub struct IpcServer {
    server: Server,
    dispatcher: Dispatcher,
    events: mpsc::UnboundedReceiver<Event>,
}

impl IpcServer {
    pub fn bind(
        shared: Shared,
        requests: mpsc::UnboundedSender<Request>,
        events: mpsc::UnboundedReceiver<Event>,
    ) -> Result<Self> {
        let server = ServiceBuilder::new(protocol::SERVICE)
            .build()
            .context("bind suja socket")?;
        Ok(Self {
            server,
            dispatcher: Dispatcher { requests, shared },
            events,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        let dispatcher = &mut self.dispatcher;
        let events = &mut self.events;
        serve_with_outbox(
            &mut self.server,
            |server, peer, message| {
                protocol::dispatch(dispatcher, server, peer, message).map(|()| true)
            },
            |cx| events.poll_recv(cx),
            emit,
        )
        .await
        .context("ipc server")
    }
}

fn emit(server: &mut Server, event: Event) {
    let result = match &event {
        Event::StateChanged(e) => server.emit(e),
        Event::MicLevel(e) => server.emit(e),
        Event::PartialTranscript(e) => server.emit(e),
        Event::FinalTranscript(e) => server.emit(e),
        Event::AssistantText(e) => server.emit(e),
        Event::ToolCall(e) => server.emit(e),
        Event::ToolResult(e) => server.emit(e),
        Event::TurnFinished(e) => server.emit(e),
        Event::Failure(e) => server.emit(e),
        Event::HotkeyTapped(e) => server.emit(e),
    };
    if let Err(error) = result {
        tracing::debug!(%error, "event not delivered");
    }
}
