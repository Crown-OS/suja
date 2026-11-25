use reqwest::Response;
use suja_model::{CancellationToken, ModelError};

const ERROR_BODY_LIMIT: usize = 600;

/// The response when its status is a success, else a [`ModelError::Request`] with the body.
pub async fn error_from_response(response: Response) -> Result<Response, ModelError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let body: String = body.chars().take(ERROR_BODY_LIMIT).collect();
    Err(ModelError::Request(format!("{status}: {}", body.trim())))
}

/// The next part of the body; `Ok(None)` at the end or when `cancel` fires first.
pub async fn next_chunk(
    response: &mut Response,
    cancel: &CancellationToken,
) -> Result<Option<bytes::Bytes>, ModelError> {
    tokio::select! {
        () = cancel.cancelled() => Ok(None),
        chunk = response.chunk() => chunk.map_err(|error| ModelError::Request(error.to_string())),
    }
}
