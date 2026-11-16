use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::error::ToolError;

pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub async fn run_with_timeout(
    program: &str,
    args: &[String],
    timeout: Duration,
) -> Result<CommandOutput, ToolError> {
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(timeout, child)
        .await
        .map_err(|_| {
            ToolError::Failed(format!(
                "`{program}` timed out after {}s",
                timeout.as_secs()
            ))
        })?
        .map_err(|e| ToolError::Failed(format!("`{program}`: {e}")))?;
    Ok(CommandOutput {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}
