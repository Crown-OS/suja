use std::ops::ControlFlow;
use std::sync::Arc;

use suja_model::{
    CancellationToken, ChatModel, Finish, FinishReason, ModelCapabilities, ModelError, ModelEvent,
    ModelStream, TurnRequest, Usage,
};
use suja_model_http::{CredentialError, HttpCredentials, for_each_event, send_request};
use tokio::sync::mpsc;

use crate::options::OpenAiOptions;
use crate::reply::ReplyDecoder;
use crate::request::request_body;

type EventSender = mpsc::UnboundedSender<Result<ModelEvent, ModelError>>;

/// A server with the OpenAI Chat Completions API.
pub struct OpenAiBackend {
    shared: Arc<Shared>,
    capabilities: ModelCapabilities,
}

struct Shared {
    options: OpenAiOptions,
    endpoint: String,
    credentials: HttpCredentials,
}

impl OpenAiBackend {
    pub fn new(options: OpenAiOptions) -> Result<Self, CredentialError> {
        let capabilities = ModelCapabilities {
            context_tokens: options.context_tokens,
            parallel_tool_calls: true,
            runs_locally: is_loopback(&options.base_url),
        };
        let endpoint = format!(
            "{}/chat/completions",
            options.base_url.trim_end_matches('/')
        );
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

impl ChatModel for OpenAiBackend {
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
            .json(&request_body(&self.options, request));
        if let Some(key) = &authorized.api_key {
            builder = builder.bearer_auth(key);
        }
        let Some(response) = send_request(builder, cancel).await? else {
            return Ok(cancelled());
        };
        let mut decoder = ReplyDecoder::default();
        for_each_event(response, cancel, |event| {
            if event.data == "[DONE]" {
                return Ok(ControlFlow::Break(()));
            }
            if let Some(text) = decoder.decode(&event.data)? {
                let _ = events.send(Ok(ModelEvent::Text(text)));
            }
            Ok(ControlFlow::Continue(()))
        })
        .await?;
        if cancel.is_cancelled() {
            return Ok(cancelled());
        }
        let (calls, reason, usage) = decoder.finish()?;
        for call in calls {
            let _ = events.send(Ok(ModelEvent::ToolCall(call)));
        }
        Ok(Finish {
            reason,
            usage,
            backend_data: None,
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

fn is_loopback(base_url: &str) -> bool {
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]"))
}
