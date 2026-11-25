use std::path::PathBuf;
use std::process::Stdio;

use suja_model::{CancellationToken, Finish, FinishReason, ModelError, ModelEvent, Usage};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

const STDERR_TAIL: usize = 400;

pub type EventSender = mpsc::UnboundedSender<Result<ModelEvent, ModelError>>;

/// Reads the JSON lines of one agent CLI run.
pub trait LineParser {
    /// The new spoken text in `line`, if any.
    fn parse(&mut self, line: &str) -> Result<Option<String>, ModelError>;

    /// Text that was not streamed yet, and how the run ended.
    fn finish(
        self,
        exited_ok: bool,
        stderr_tail: &str,
    ) -> Result<(Option<String>, Finish), ModelError>;
}

pub struct CliRun {
    pub program: String,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    pub prompt: String,
}

pub async fn run(
    cli: CliRun,
    mut parser: impl LineParser,
    cancel: &CancellationToken,
    events: &EventSender,
) -> Result<Finish, ModelError> {
    let mut child = Command::new(&cli.program)
        .args(&cli.arguments)
        .current_dir(&cli.working_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| ModelError::Unavailable(format!("start `{}`: {error}", cli.program)))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(cli.prompt.as_bytes()).await;
    }
    let stderr = child.stderr.take().map(|mut stderr| {
        tokio::spawn(async move {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text).await;
            text
        })
    });
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ModelError::Runtime("no stdout from the agent CLI".into()))?;
    let mut lines = BufReader::new(stdout).lines();
    loop {
        let line = tokio::select! {
            () = cancel.cancelled() => {
                let _ = child.kill().await;
                return Ok(Finish { reason: FinishReason::Cancelled, usage: Usage::default(), backend_data: None });
            }
            line = lines.next_line() => line.map_err(|error| ModelError::Runtime(error.to_string()))?,
        };
        let Some(line) = line else {
            break;
        };
        if let Some(text) = parser.parse(&line)? {
            let _ = events.send(Ok(ModelEvent::Text(text)));
        }
    }
    let exited_ok = child.wait().await.is_ok_and(|status| status.success());
    let stderr = match stderr {
        Some(task) => task.await.unwrap_or_default(),
        None => String::new(),
    };
    let (text, finish) = parser.finish(exited_ok, tail(&stderr))?;
    if let Some(text) = text {
        let _ = events.send(Ok(ModelEvent::Text(text)));
    }
    Ok(finish)
}

fn tail(text: &str) -> &str {
    let text = text.trim();
    let start = text
        .char_indices()
        .rev()
        .nth(STDERR_TAIL)
        .map_or(0, |(index, _)| index);
    &text[start..]
}
