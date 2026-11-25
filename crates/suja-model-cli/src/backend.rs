use std::sync::Arc;

use suja_model::{
    CancellationToken, ChatModel, ModelCapabilities, ModelEvent, ModelStream, TurnRequest,
};
use tokio::sync::mpsc;

use crate::claude_code::ClaudeCodeParser;
use crate::codex::CodexParser;
use crate::options::{ClaudeCodeOptions, CodexOptions, working_directory};
use crate::process::{CliRun, run};
use crate::prompt::render_conversation;

const CONTEXT_TOKENS: u32 = 200_000;

/// An agent CLI that answers with its own tools. Suja's tools in a request are not used.
pub enum CliBackend {
    ClaudeCode(Arc<ClaudeCodeOptions>),
    Codex(Arc<CodexOptions>),
}

impl CliBackend {
    pub fn claude_code(options: ClaudeCodeOptions) -> Self {
        Self::ClaudeCode(Arc::new(options))
    }

    pub fn codex(options: CodexOptions) -> Self {
        Self::Codex(Arc::new(options))
    }

    fn command(&self, request: &TurnRequest) -> CliRun {
        let conversation = render_conversation(&request.transcript);
        match self {
            Self::ClaudeCode(options) => {
                let mut arguments: Vec<String> = [
                    "-p",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--include-partial-messages",
                    "--no-session-persistence",
                    "--strict-mcp-config",
                    "--permission-mode",
                    "dontAsk",
                    "--tools",
                    &options.tools,
                    "--append-system-prompt",
                    &request.system,
                ]
                .map(str::to_owned)
                .into();
                if let Some(model) = &options.model {
                    arguments.extend(["--model".to_owned(), model.clone()]);
                }
                arguments.push("--allowedTools".to_owned());
                arguments.extend(options.tools.split(',').map(|tool| tool.trim().to_owned()));
                CliRun {
                    program: options.program.clone(),
                    arguments,
                    working_directory: working_directory(options.working_directory.as_ref()),
                    prompt: conversation,
                }
            }
            Self::Codex(options) => {
                let directory = working_directory(options.working_directory.as_ref());
                let mut arguments: Vec<String> = [
                    "exec",
                    "--json",
                    "--skip-git-repo-check",
                    "--ephemeral",
                    "--sandbox",
                    "read-only",
                    "--color",
                    "never",
                ]
                .map(str::to_owned)
                .into();
                arguments.extend(["-C".to_owned(), directory.display().to_string()]);
                if let Some(model) = &options.model {
                    arguments.extend(["-m".to_owned(), model.clone()]);
                }
                arguments.push("-".to_owned());
                CliRun {
                    program: options.program.clone(),
                    arguments,
                    working_directory: directory,
                    prompt: format!("{}\n\n{conversation}", request.system),
                }
            }
        }
    }
}

impl ChatModel for CliBackend {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            context_tokens: CONTEXT_TOKENS,
            parallel_tool_calls: false,
            runs_locally: false,
        }
    }

    fn stream(&self, request: TurnRequest, cancel: CancellationToken) -> ModelStream {
        let (events, stream) = mpsc::unbounded_channel();
        let command = self.command(&request);
        let is_claude_code = matches!(self, Self::ClaudeCode(_));
        tokio::spawn(async move {
            let finish = if is_claude_code {
                run(command, ClaudeCodeParser::default(), &cancel, &events).await
            } else {
                run(command, CodexParser::default(), &cancel, &events).await
            };
            let _ = events.send(finish.map(ModelEvent::Finished));
        });
        stream
    }
}
