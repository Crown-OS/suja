use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use suja_model::{CancellationToken, ChatModel, Entry, FinishReason, ModelEvent, TurnRequest};
use suja_model_cli::{ClaudeCodeOptions, CliBackend};

/// A program that saves its arguments and stdin next to itself and prints `lines`.
async fn fake_program(name: &str, lines: &[String]) -> std::io::Result<PathBuf> {
    let directory =
        std::env::temp_dir().join(format!("suja-cli-test-{}-{name}", std::process::id()));
    tokio::fs::create_dir_all(&directory).await?;
    let output: String = lines
        .iter()
        .map(|line| format!("printf '%s\\n' '{line}'\n"))
        .collect();
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{dir}/arguments'\ncat > '{dir}/stdin'\n{output}",
        dir = directory.display()
    );
    let path = directory.join("program");
    tokio::fs::write(&path, script).await?;
    tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).await?;
    Ok(path)
}

#[tokio::test]
async fn claude_code_gets_the_conversation_on_stdin_and_its_text_streams_out() -> std::io::Result<()>
{
    let lines = [
        json!({"type": "system", "subtype": "init"}).to_string(),
        json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Kyoto in spring."}}}).to_string(),
        json!({"type": "result", "subtype": "success", "is_error": false, "result": "Kyoto in spring.", "usage": {"input_tokens": 10, "output_tokens": 5}}).to_string(),
    ];
    let program = fake_program("claude", &lines).await?;
    let backend = CliBackend::claude_code(ClaudeCodeOptions {
        program: program.display().to_string(),
        model: Some("opus".into()),
        working_directory: Some(std::env::temp_dir()),
        ..ClaudeCodeOptions::default()
    });
    let mut stream = backend.stream(
        TurnRequest {
            system: Arc::from("Speak briefly."),
            tools: Arc::from(Vec::new()),
            transcript: vec![Entry::User("when should I visit Kyoto".into())],
            max_reply_tokens: 512,
        },
        CancellationToken::new(),
    );
    let mut events = Vec::new();
    while let Some(event) = stream.recv().await {
        events.push(event);
    }
    assert!(matches!(&events[0], Ok(ModelEvent::Text(text)) if text == "Kyoto in spring."));
    assert!(
        matches!(&events[1], Ok(ModelEvent::Finished(finish)) if finish.reason == FinishReason::EndOfTurn)
    );

    let directory = program.parent().map(PathBuf::from).unwrap_or_default();
    let arguments = tokio::fs::read_to_string(directory.join("arguments")).await?;
    let arguments: Vec<&str> = arguments.lines().collect();
    for expected in [
        "-p",
        "--strict-mcp-config",
        "--no-session-persistence",
        "dontAsk",
        "Speak briefly.",
        "opus",
        "WebSearch",
    ] {
        assert!(
            arguments.contains(&expected),
            "{expected} missing from {arguments:?}"
        );
    }
    assert!(!arguments.contains(&"--bare"));
    assert_eq!(
        tokio::fs::read_to_string(directory.join("stdin")).await?,
        "when should I visit Kyoto"
    );
    tokio::fs::remove_dir_all(directory).await
}

#[tokio::test]
async fn a_missing_program_is_unavailable() {
    let backend = CliBackend::claude_code(ClaudeCodeOptions {
        program: "/no/such/claude".into(),
        ..ClaudeCodeOptions::default()
    });
    let mut stream = backend.stream(
        TurnRequest {
            system: Arc::from(""),
            tools: Arc::from(Vec::new()),
            transcript: vec![Entry::User("hi".into())],
            max_reply_tokens: 16,
        },
        CancellationToken::new(),
    );
    assert!(matches!(
        stream.recv().await,
        Some(Err(suja_model::ModelError::Unavailable(_)))
    ));
}
