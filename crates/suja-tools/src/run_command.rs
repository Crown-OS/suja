use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::confirm::is_destructive;
use crate::error::ToolError;
use crate::outcome::ToolOutcome;
use crate::util::command::run_with_timeout;
use crate::util::truncate::truncate_chars;
use crate::{Tool, ToolSpec, argument_str};

const OUTPUT_LIMIT: usize = 2000;

pub struct RunCommand {
    allowed: Vec<String>,
}

impl RunCommand {
    pub fn new(allowed: Vec<String>) -> Self {
        Self { allowed }
    }
}

#[async_trait]
impl Tool for RunCommand {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "run_command",
            description: "Run one allow-listed local command without a shell and return its output. Use for system facts like disk, memory, processes, git status.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Program and arguments separated by spaces, e.g. `df -h /`"}
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, arguments: Value) -> Result<ToolOutcome, ToolError> {
        self.run(arguments, false).await
    }

    async fn call_confirmed(&self, arguments: Value) -> Result<ToolOutcome, ToolError> {
        self.run(arguments, true).await
    }
}

impl RunCommand {
    async fn run(&self, arguments: Value, confirmed: bool) -> Result<ToolOutcome, ToolError> {
        let line = argument_str(&arguments, "command")?;
        let mut words = line.split_whitespace().map(str::to_owned);
        let program = words
            .next()
            .ok_or_else(|| ToolError::InvalidArguments("empty command".into()))?;
        let args: Vec<String> = words.collect();
        let base = program.rsplit('/').next().unwrap_or(&program);
        if !self.allowed.iter().any(|a| a == base) {
            return Ok(ToolOutcome::failure(format!(
                "`{base}` is not on the allow list"
            )));
        }
        if !confirmed && is_destructive(&program, &args) {
            return Ok(ToolOutcome::needs_confirmation(format!(
                "Run `{line}`? It can change the system."
            )));
        }
        let output = run_with_timeout(&program, &args, Duration::from_secs(10)).await?;
        let body = if output.stdout.trim().is_empty() {
            &output.stderr
        } else {
            &output.stdout
        };
        let detail = truncate_chars(body.trim(), OUTPUT_LIMIT).into_owned();
        Ok(ToolOutcome {
            ok: output.status == 0,
            spoken_summary: format!("`{base}` finished with status {}", output.status),
            detail_for_model: format!("exit {}\n{detail}", output.status),
            needs_confirmation: false,
        })
    }
}
