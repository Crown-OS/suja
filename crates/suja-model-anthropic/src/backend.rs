use std::ops::ControlFlow;
use std::sync::Arc;

use suja_model::{
    BackendData, CancellationToken, ChatModel, Finish, FinishReason, ModelCapabilities, ModelError,
    ModelEvent, ModelStream, TurnRequest, Usage,
};
use suja_model_http::{CredentialError, HttpCredentials, for_each_event, send_request};
use tokio::sync::mpsc;

use crate::options::AnthropicOptions;
use crate::reply::ReplyDecoder;
use crate::request::{BACKEND_NAME, request_body};

const API_VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

type EventSender = mpsc::UnboundedSender<Result<ModelEvent, ModelError>>;

/// Claude through the Anthropic Messages API.
pub struct AnthropicBackend {
    shared: Arc<Shared>,
    capabilities: ModelCapabilities,
}

struct Shared {
    options: AnthropicOptions,
    endpoint: String,
    credentials: HttpCredentials,
}

impl AnthropicBackend {
    pub fn new(options: AnthropicOptions) -> Result<Self, CredentialError> {
        let capabilities = ModelCapabilities {
            context_tokens: options.context_tokens,
            parallel_tool_calls: true,
            runs_locally: false,
        };
        let endpoint = format!("{}/v1/messages", options.base_url.trim_end_matches('/'));
        let credentials = HttpCredentials::new(options.credential.clone())?;
        Ok(Self {
            shared: Arc::new(Shared {
                options,
                endpoint,
                credentials,
            }),
            capabilities,
        })
    }
}

impl ChatModel for AnthropicBackend {
    fn capabilities(&self) -> ModelCapabilities {
        self.capabilities
    }

    fn stream(&self, request: TurnRequest, cancel: CancellationToken) -> ModelStream {
        let (events, stream) = mpsc::unbounded_channel();
        let shared = self.shared.clone();
        tokio::spawn(async move {
            let event = shared
                .reply(&request, &cancel, &events)
                .await
                .map(ModelEvent::Finished);
            let _ = events.send(event);
        });
        stream
    }
}

impl Shared {
    async fn reply(
        &self,
        request: &TurnRequest,
        cancel: &CancellationToken,
        events: &EventSender,
    ) -> Result<Finish, ModelError> {
        let authorized = self.credentials.authorized().await?;
        let mut builder = authorized
            .client
            .post(&self.endpoint)
            .header("anthropic-version", API_VERSION)
            .json(&request_body(&self.options, request));
        if let Some(key) = &authorized.api_key {
            builder = builder.header("x-api-key", &**key);
        }
        if self.options.fallbacks {
            builder = builder.header("anthropic-beta", FALLBACK_BETA);
        }
        let Some(response) = send_request(builder, cancel).await? else {
            return Ok(cancelled());
        };
        let mut decoder = ReplyDecoder::default();
        for_each_event(response, cancel, |event| {
            if let Some(text) = decoder.decode(&event.data)? {
                let _ = events.send(Ok(ModelEvent::Text(text)));
            }
            Ok(ControlFlow::Continue(()))
        })
        .await?;
        if cancel.is_cancelled() {
            return Ok(cancelled());
        }
        let reply = decoder.finish();
        for call in reply.calls {
            let _ = events.send(Ok(ModelEvent::ToolCall(call)));
        }
        Ok(Finish {
            reason: reply.reason,
            usage: reply.usage,
            backend_data: Some(BackendData {
                owner: BACKEND_NAME,
                content: reply.blocks,
            }),
        })
    }
}

fn cancelled() -> Finish {
    Finish {
        reason: FinishReason::Cancelled,
        usage: Usage::default(),
        backend_data: None,
    }
}
