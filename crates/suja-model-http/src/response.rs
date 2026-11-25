use std::ops::ControlFlow;

use reqwest::{RequestBuilder, Response};
use suja_model::{CancellationToken, ModelError};

use crate::sse::{ServerSentEvent, SseParser};

const ERROR_BODY_LIMIT: usize = 600;

/// The response to `request`, or `None` when `cancel` fires first. A failed status is a
/// [`ModelError::Request`] with the start of the body.
pub async fn send_request(
    request: RequestBuilder,
    cancel: &CancellationToken,
) -> Result<Option<Response>, ModelError> {
    let response = tokio::select! {
        () = cancel.cancelled() => return Ok(None),
        response = request.send() => response.map_err(|error| ModelError::Request(error.to_string()))?,
    };
    let status = response.status();
    if status.is_success() {
        return Ok(Some(response));
    }
    let body = response.text().await.unwrap_or_default();
    let body: String = body.chars().take(ERROR_BODY_LIMIT).collect();
    Err(ModelError::Request(format!("{status}: {}", body.trim())))
}

/// Gives each server-sent event of the body to `on_event` until the body ends, `on_event`
/// breaks, or `cancel` fires.
pub async fn for_each_event(
    mut response: Response,
    cancel: &CancellationToken,
    mut on_event: impl FnMut(ServerSentEvent) -> Result<ControlFlow<()>, ModelError>,
) -> Result<(), ModelError> {
    let mut parser = SseParser::default();
    loop {
        let chunk = tokio::select! {
            () = cancel.cancelled() => return Ok(()),
            chunk = response.chunk() => chunk.map_err(|error| ModelError::Request(error.to_string()))?,
        };
        let Some(chunk) = chunk else {
            return Ok(());
        };
        for event in parser.feed(&chunk) {
            if on_event(event)?.is_break() {
                return Ok(());
            }
        }
    }
}
