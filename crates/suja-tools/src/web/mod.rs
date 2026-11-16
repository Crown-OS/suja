pub mod fetch;
pub mod search;

pub(crate) fn http_client() -> Result<reqwest::Client, crate::error::ToolError> {
    reqwest::Client::builder()
        .user_agent("suja/0.1")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| crate::error::ToolError::Failed(e.to_string()))
}
