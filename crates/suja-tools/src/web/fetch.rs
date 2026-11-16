use async_trait::async_trait;
use scraper::{Html, Selector};
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::outcome::ToolOutcome;
use crate::util::truncate::truncate_chars;
use crate::web::http_client;
use crate::{Tool, ToolSpec, argument_str};

const BODY_LIMIT: usize = 6000;
const DOWNLOAD_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Default)]
pub struct WebFetch;

impl WebFetch {
    pub fn new() -> Self {
        Self
    }
}

pub fn readable_text(html: &str) -> String {
    let document = Html::parse_document(html);
    let candidates = ["article", "main", "body"];
    for name in candidates {
        let Ok(selector) = Selector::parse(name) else {
            continue;
        };
        if let Some(root) = document.select(&selector).next() {
            let text = root
                .text()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if text.len() > 200 || name == "body" {
                return text;
            }
        }
    }
    String::new()
}

#[async_trait]
impl Tool for WebFetch {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web_fetch",
            description: "Download a web page and return its readable text.",
            parameters: json!({
                "type": "object",
                "properties": {"url": {"type": "string", "format": "uri"}},
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, arguments: Value) -> Result<ToolOutcome, ToolError> {
        let url = argument_str(&arguments, "url")?;
        let response = http_client()?
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| ToolError::Failed(format!("fetch: {e}")))?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ToolError::Failed(format!("fetch body: {e}")))?;
        let slice = &bytes[..bytes.len().min(DOWNLOAD_LIMIT)];
        let text = readable_text(&String::from_utf8_lossy(slice));
        Ok(ToolOutcome::success(
            format!("fetched {} characters", text.len()),
            truncate_chars(&text, BODY_LIMIT).into_owned(),
        ))
    }
}
