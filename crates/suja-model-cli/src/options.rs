use std::path::PathBuf;

use serde::Deserialize;

/// Claude Code with the user's own login. It reads and searches; it cannot run commands or edit.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ClaudeCodeOptions {
    pub program: String,
    pub model: Option<String>,
    /// Built-in tools it may use without asking.
    pub tools: String,
    /// Where it starts and what it may read. Default: the home folder.
    pub working_directory: Option<PathBuf>,
}

impl Default for ClaudeCodeOptions {
    fn default() -> Self {
        Self {
            program: "claude".to_owned(),
            model: None,
            tools: "Read,Grep,Glob,WebSearch,WebFetch".to_owned(),
            working_directory: None,
        }
    }
}

/// Codex with the user's own login, in a read-only sandbox.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct CodexOptions {
    pub program: String,
    pub model: Option<String>,
    pub working_directory: Option<PathBuf>,
}

impl Default for CodexOptions {
    fn default() -> Self {
        Self {
            program: "codex".to_owned(),
            model: None,
            working_directory: None,
        }
    }
}

pub fn working_directory(chosen: Option<&PathBuf>) -> PathBuf {
    chosen
        .cloned()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/"))
}
