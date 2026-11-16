use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::outcome::ToolOutcome;
use crate::read_file::resolve_user_path;
use crate::util::command::run_with_timeout;
use crate::{Tool, ToolSpec, argument_str, optional_argument_str};

const MAX_HITS: usize = 20;

pub struct SearchFiles;

#[async_trait]
impl Tool for SearchFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "search_files",
            description: "Find files under the home directory whose name or contents match a query.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "directory": {"type": "string", "description": "Where to search, default ~"}
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, arguments: Value) -> Result<ToolOutcome, ToolError> {
        let query = argument_str(&arguments, "query")?;
        let dir = resolve_user_path(optional_argument_str(&arguments, "directory").unwrap_or("~"))?;
        let dir_text = dir.display().to_string();
        let by_name = run_with_timeout(
            "find",
            &[
                dir_text.clone(),
                "-iname".into(),
                format!("*{query}*"),
                "-not".into(),
                "-path".into(),
                "*/.*".into(),
            ],
            Duration::from_secs(8),
        )
        .await?;
        let by_content = run_with_timeout(
            "grep",
            &[
                "-rIil".into(),
                "--exclude-dir=.*".into(),
                query.to_owned(),
                dir_text,
            ],
            Duration::from_secs(8),
        )
        .await?;
        let hits: Vec<&str> = by_name
            .stdout
            .lines()
            .chain(by_content.stdout.lines())
            .filter(|l| !l.is_empty())
            .take(MAX_HITS)
            .collect();
        Ok(ToolOutcome::success(
            format!("found {} matches for {query}", hits.len()),
            if hits.is_empty() {
                "no matches".to_owned()
            } else {
                hits.join("\n")
            },
        ))
    }
}
