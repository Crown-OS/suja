use std::sync::Arc;

use serde_json::{Value, json};
use suja_model::{
    CancellationToken, ChatModel, Entry, FinishReason, ModelError, ModelEvent, ToolSpec,
    TurnRequest,
};
use suja_model_http::Credential;
use suja_model_http::fake_server::serve_once;
use suja_model_openai::{OpenAiBackend, OpenAiOptions, TokenLimitField};

fn sse(chunks: &[Value]) -> String {
    let mut body: String = chunks
        .iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .collect();
    body.push_str("data: [DONE]\n\n");
    body
}

fn request() -> TurnRequest {
    TurnRequest {
        system: Arc::from("Be brief."),
        tools: Arc::from(vec![ToolSpec {
            name: "set_timer".into(),
            description: "Start a countdown".into(),
            parameters: json!({"type": "object", "properties": {"seconds": {"type": "integer"}}}),
        }]),
        transcript: vec![Entry::User("set a timer for a minute".into())],
        max_reply_tokens: 64,
    }
}

fn backend(base_url: String, credential: Credential) -> OpenAiBackend {
    OpenAiBackend::new(OpenAiOptions {
        base_url: format!("{base_url}/v1"),
        model: "test-model".into(),
        credential,
        context_tokens: 4096,
        token_limit: TokenLimitField::MaxTokens,
    })
    .unwrap_or_else(|e| panic!("{e}"))
}

async fn collect(backend: &OpenAiBackend) -> Vec<Result<ModelEvent, ModelError>> {
    let mut stream = backend.stream(request(), CancellationToken::new());
    let mut events = Vec::new();
    while let Some(event) = stream.recv().await {
        events.push(event);
    }
    events
}

#[tokio::test]
async fn text_and_a_tool_call_stream_out_and_the_request_has_the_tools() -> std::io::Result<()> {
    let body = sse(&[
        json!({"choices": [{"delta": {"content": "Starting"}}]}),
        json!({"choices": [{"delta": {"content": " it."}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_a", "function": {"name": "set_timer", "arguments": "{\"seconds\":"}}]}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": " 60}"}}]}, "finish_reason": "tool_calls"}]}),
        json!({"choices": [], "usage": {"prompt_tokens": 30, "completion_tokens": 12}}),
    ]);
    let (base_url, seen) = serve_once("200 OK", body).await?;
    let backend = backend(
        base_url,
        Credential::Environment {
            variable: "CARGO_PKG_NAME".into(),
        },
    );
    let events = collect(&backend).await;

    let texts: String = events
        .iter()
        .filter_map(|event| match event {
            Ok(ModelEvent::Text(text)) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, "Starting it.");
    assert!(events.iter().any(|event| matches!(
        event,
        Ok(ModelEvent::ToolCall(call)) if call.id == "call_a" && call.arguments == json!({"seconds": 60})
    )));
    assert!(matches!(
        events.last(),
        Some(Ok(ModelEvent::Finished(finish))) if finish.reason == FinishReason::ToolCall && finish.usage.total() == 42
    ));

    let request = seen.await.map_err(std::io::Error::other)?;
    let (head, body) = (request.head, request.body);
    assert!(
        head.to_ascii_lowercase()
            .contains("authorization: bearer suja-model-openai")
    );
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["max_tokens"], 64);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["tools"][0]["function"]["name"], "set_timer");
    Ok(())
}

#[tokio::test]
async fn an_error_status_becomes_a_request_error_with_the_body() -> std::io::Result<()> {
    let (base_url, _seen) =
        serve_once("401 Unauthorized", "{\"error\":\"bad key\"}".into()).await?;
    let events = collect(&backend(base_url, Credential::None)).await;
    assert!(matches!(
        events.as_slice(),
        [Err(ModelError::Request(message))] if message.contains("401") && message.contains("bad key")
    ));
    Ok(())
}

#[tokio::test]
async fn a_missing_key_fails_before_any_request() {
    let events = collect(&backend(
        "http://127.0.0.1:9/v1".into(),
        Credential::Environment {
            variable: "SUJA_TEST_KEY_THAT_IS_NOT_SET".into(),
        },
    ))
    .await;
    assert!(matches!(
        events.as_slice(),
        [Err(ModelError::Unavailable(_))]
    ));
}
