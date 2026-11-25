use std::sync::Arc;

use serde_json::{Value, json};
use suja_model::{
    AssistantReply, CancellationToken, ChatModel, Entry, FinishReason, ModelError, ModelEvent,
    ToolResult, ToolSpec, TurnRequest,
};
use suja_model_anthropic::{AnthropicBackend, AnthropicOptions};
use suja_model_http::Credential;
use suja_model_http::fake_server::serve_once;

fn sse(events: &[Value]) -> String {
    events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap_or("x")
            )
        })
        .collect()
}

fn backend(base_url: String) -> AnthropicBackend {
    AnthropicBackend::new(AnthropicOptions {
        model: "claude-opus-5-5".into(),
        effort: Some("low".into()),
        credential: Credential::Environment {
            variable: "CARGO_PKG_NAME".into(),
        },
        base_url,
        context_tokens: 200_000,
        max_tokens: 16_000,
        fallbacks: true,
    })
    .unwrap_or_else(|error| panic!("{error}"))
}

fn request(transcript: Vec<Entry>) -> TurnRequest {
    TurnRequest {
        system: Arc::from("Be brief."),
        tools: Arc::from(vec![ToolSpec {
            name: "web_search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        }]),
        transcript,
        max_reply_tokens: 512,
    }
}

async fn collect(
    backend: &AnthropicBackend,
    request: TurnRequest,
) -> Vec<Result<ModelEvent, ModelError>> {
    let mut stream = backend.stream(request, CancellationToken::new());
    let mut events = Vec::new();
    while let Some(event) = stream.recv().await {
        events.push(event);
    }
    events
}

#[tokio::test]
async fn thinking_text_and_tool_use_come_back_and_the_thinking_is_kept() -> std::io::Result<()> {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 20, "cache_read_input_tokens": 1000, "output_tokens": 1}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig=="}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Let me look."}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "toolu_1", "name": "web_search", "input": {}}}),
        json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "{\"query\": "}}),
        json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "\"rust 2.0\"}"}}),
        json!({"type": "content_block_stop", "index": 2}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 30}}),
        json!({"type": "message_stop"}),
    ]);
    let (base_url, seen) = serve_once("200 OK", body).await?;
    let events = collect(
        &backend(base_url),
        request(vec![Entry::User("news on rust".into())]),
    )
    .await;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, Ok(ModelEvent::Text(text)) if text == "Let me look."))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        Ok(ModelEvent::ToolCall(call)) if call.id == "toolu_1" && call.arguments == json!({"query": "rust 2.0"})
    )));
    let Some(Ok(ModelEvent::Finished(finish))) = events.last() else {
        panic!("no finish: {events:?}");
    };
    assert_eq!(finish.reason, FinishReason::ToolCall);
    assert_eq!(finish.usage.input_tokens, 1020);
    let blocks = finish
        .backend_data
        .as_ref()
        .and_then(|data| data.content_for("anthropic"))
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        blocks[0],
        json!({"type": "thinking", "thinking": "", "signature": "sig=="})
    );

    let request = seen.await.map_err(std::io::Error::other)?;
    let head = request.head.to_ascii_lowercase();
    assert!(head.starts_with("post /v1/messages"));
    assert!(head.contains("anthropic-version: 2026-10-21"));
    assert!(head.contains("x-api-key: suja-model-anthropic"));
    assert!(head.contains("anthropic-beta: server-side-fallback-2026-07-01"));
    let body = request.body;
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["max_tokens"], 16_000);
    assert_eq!(body["output_config"]["effort"], "low");
    assert_eq!(body["fallbacks"], "default");
    assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
    assert!(body.get("temperature").is_none() && body.get("top_p").is_none());
    Ok(())
}

#[tokio::test]
async fn kept_blocks_go_back_unchanged_and_tool_results_join_the_user_turn() -> std::io::Result<()>
{
    let kept = json!([
        {"type": "thinking", "thinking": "", "signature": "sig=="},
        {"type": "tool_use", "id": "toolu_1", "name": "web_search", "input": {"query": "x"}},
    ]);
    let transcript = vec![
        Entry::User("first".into()),
        Entry::Assistant(AssistantReply {
            text: String::new(),
            tool_calls: Vec::new(),
            backend_data: Some(suja_model::BackendData {
                owner: "anthropic",
                content: kept.clone(),
            }),
        }),
        Entry::ToolResults(vec![ToolResult {
            call_id: "toolu_1".into(),
            name: "web_search".into(),
            content: "results".into(),
            is_error: false,
        }]),
        Entry::User("and then?".into()),
    ];
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
    ]);
    let (base_url, seen) = serve_once("200 OK", body).await?;
    let _ = collect(&backend(base_url), request(transcript)).await;
    let messages = seen.await.map_err(std::io::Error::other)?.body["messages"].clone();
    assert_eq!(messages[1]["content"], kept);
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");
    assert_eq!(
        messages[2]["content"][1],
        json!({"type": "text", "text": "and then?"})
    );
    Ok(())
}
