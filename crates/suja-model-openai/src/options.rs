use serde::Deserialize;
use suja_model_http::Credential;

const DEFAULT_CONTEXT_TOKENS: u32 = 32_768;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct OpenAiOptions {
    /// For example `http://127.0.0.1:1234/v1` (LM Studio) or `https://api.openai.com/v1`.
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub credential: Credential,
    #[serde(default = "default_context_tokens")]
    pub context_tokens: u32,
    #[serde(default)]
    pub token_limit: TokenLimitField,
}

/// The request field for the reply length. Newer OpenAI models accept only `max_completion_tokens`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum TokenLimitField {
    #[default]
    MaxTokens,
    MaxCompletionTokens,
}

impl TokenLimitField {
    pub fn field_name(self) -> &'static str {
        match self {
            Self::MaxTokens => "max_tokens",
            Self::MaxCompletionTokens => "max_completion_tokens",
        }
    }
}

fn default_context_tokens() -> u32 {
    DEFAULT_CONTEXT_TOKENS
}
