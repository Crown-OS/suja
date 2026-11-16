use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::error::ToolError;
use crate::outcome::ToolOutcome;
use crate::{Tool, ToolSpec, argument_str, optional_argument_str};

pub struct SaveNote {
    notes_dir: PathBuf,
}

impl SaveNote {
    pub fn new(notes_dir: PathBuf) -> Self {
        Self { notes_dir }
    }
}

#[async_trait]
impl Tool for SaveNote {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "save_note",
            description: "Append a note or reminder to today's notes file.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "title": {"type": "string"}
                },
                "required": ["text"]
            }),
        }
    }

    async fn call(&self, arguments: Value) -> Result<ToolOutcome, ToolError> {
        let text = argument_str(&arguments, "text")?;
        let title = optional_argument_str(&arguments, "title").unwrap_or("Note");
        let now = chrono::Local::now();
        let path = self
            .notes_dir
            .join(format!("{}.md", now.format("%Y-%m-%d")));
        tokio::fs::create_dir_all(&self.notes_dir)
            .await
            .map_err(|e| ToolError::Failed(e.to_string()))?;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .map_err(|e| ToolError::Failed(e.to_string()))?;
        let entry = format!("\n## {title} ({})\n{text}\n", now.format("%H:%M"));
        file.write_all(entry.as_bytes())
            .await
            .map_err(|e| ToolError::Failed(e.to_string()))?;
        Ok(ToolOutcome::success(
            "saved the note",
            format!("appended to {}", path.display()),
        ))
    }
}
