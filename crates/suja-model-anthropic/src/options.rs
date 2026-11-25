use serde::Deserialize;
use suja_model_http::Credential;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct AnthropicOptions {
    #[serde(default = "default_model")]
    pub model: String,
    /// `low`, `medium`, `high`, `xhigh` or `max`. Lower is faster; thinking cannot be off.
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub credential: Credential,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// The conversation starts again after this many tokens, which also limits the cost of a turn.
    #[serde(default = "default_context_tokens")]
    pub context_tokens: u32,
    /// The least `max_tokens` of a request. Thinking counts against it, so the short limit of a
    /// spoken reply is too small.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Lets the server send a refused request to a different model.
    #[serde(default = "default_fallbacks")]
    pub fallbacks: bool,
}

fn default_model() -> String {
    "claude-opus-5-5".to_owned()
}

fn default_base_url() -> String {
    "https://api.anthropic.com".to_owned()
}

fn default_context_tokens() -> u32 {
    200_000
}

fn default_max_tokens() -> u32 {
    16_000
}

fn default_fallbacks() -> bool {
    true
}
