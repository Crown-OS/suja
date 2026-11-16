use std::fmt::Write;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::outcome::ToolOutcome;
use crate::web::http_client;
use crate::{Tool, ToolSpec, argument_str};

const MAX_RESULTS: usize = 5;

pub struct WebSearch {
    searxng_url: Option<String>,
    brave_api_key: Option<String>,
}

impl WebSearch {
    pub fn new(searxng_url: Option<String>, brave_api_key: Option<String>) -> Self {
        Self {
            searxng_url,
            brave_api_key,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Hit {
    title: String,
    url: String,
    #[serde(default, alias = "description")]
    content: String,
}

#[derive(Deserialize)]
struct SearxResponse {
    results: Vec<Hit>,
}

#[derive(Deserialize)]
struct BraveResponse {
    web: BraveWeb,
}

#[derive(Deserialize)]
struct BraveWeb {
    results: Vec<Hit>,
}

async fn searxng(base: &str, query: &str) -> Result<Vec<Hit>, ToolError> {
    let response: SearxResponse = http_client()?
        .get(format!("{}/search", base.trim_end_matches('/')))
        .query(&[("q", query), ("format", "json")])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| ToolError::Failed(format!("searxng: {e}")))?
        .json()
        .await
        .map_err(|e| ToolError::Failed(format!("searxng json: {e}")))?;
    Ok(response.results)
}

async fn brave(key: &str, query: &str) -> Result<Vec<Hit>, ToolError> {
    let response: BraveResponse = http_client()?
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", key)
        .header("Accept", "application/json")
        .query(&[("q", query), ("count", "5")])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| ToolError::Failed(format!("brave: {e}")))?
        .json()
        .await
        .map_err(|e| ToolError::Failed(format!("brave json: {e}")))?;
    Ok(response.web.results)
}

#[async_trait]
impl Tool for WebSearch {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web_search",
            description: "Search the web and return the top results with short snippets.",
            parameters: json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, arguments: Value) -> Result<ToolOutcome, ToolError> {
        let query = argument_str(&arguments, "query")?;
        let hits = match (&self.searxng_url, &self.brave_api_key) {
            (Some(base), _) => searxng(base, query).await?,
            (None, Some(key)) => brave(key, query).await?,
            (None, None) => return Ok(ToolOutcome::failure("web search is not configured")),
        };
        let mut detail = String::new();
        for hit in hits.iter().take(MAX_RESULTS) {
            let _ = writeln!(
                detail,
                "- {} <{}>\n  {}",
                hit.title,
                hit.url,
                hit.content.trim()
            );
        }
        Ok(ToolOutcome::success(
            format!("found {} results", hits.len().min(MAX_RESULTS)),
            if detail.is_empty() {
                "no results".to_owned()
            } else {
                detail
            },
        ))
    }
}
