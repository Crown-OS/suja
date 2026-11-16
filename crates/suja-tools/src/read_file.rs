use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::outcome::ToolOutcome;
use crate::util::truncate::truncate_chars;
use crate::{Tool, ToolSpec, argument_str};

const READ_LIMIT: usize = 4000;

pub struct ReadFile;

pub fn resolve_user_path(raw: &str) -> Result<PathBuf, ToolError> {
    let home = dirs::home_dir().ok_or_else(|| ToolError::Failed("no home directory".into()))?;
    let path = match raw.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if raw == "~" => home.clone(),
        None => PathBuf::from(raw),
    };
    let path = if path.is_absolute() {
        path
    } else {
        home.join(path)
    };
    if !path.starts_with(&home) {
        return Err(ToolError::InvalidArguments(
            "only paths under the home directory are allowed".into(),
        ));
    }
    Ok(path)
}

async fn read_text(path: &Path) -> Result<String, ToolError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| ToolError::Failed(format!("{}: {e}", path.display())))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[async_trait]
impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file",
            description: "Read a text file under the user's home directory.",
            parameters: json!({
                "type": "object",
                "properties": {"path": {"type": "string", "description": "Path, may start with ~/"}},
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, arguments: Value) -> Result<ToolOutcome, ToolError> {
        let path = resolve_user_path(argument_str(&arguments, "path")?)?;
        let text = read_text(&path).await?;
        let lines = text.lines().count();
        Ok(ToolOutcome::success(
            format!("read {lines} lines from {}", path.display()),
            truncate_chars(&text, READ_LIMIT).into_owned(),
        ))
    }
}
