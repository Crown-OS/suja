use ron::extensions::Extensions;
use serde::Deserialize;
use suja_model_anthropic::AnthropicOptions;
use suja_model_cli::{ClaudeCodeOptions, CodexOptions};
use suja_model_openai::OpenAiOptions;

const DEFAULT_MODEL_FILE: &str = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";

/// The `model` section of `suja.ron`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    /// Talks to the user. A fast model keeps the first word quick.
    pub base: BackendConfig,
    /// Used when the base fails to load, for example with too little free VRAM.
    pub base_fallback: Option<BackendConfig>,
    /// Stronger models that the base can hand a request to.
    pub experts: Vec<ExpertConfig>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            base: BackendConfig::Llama(LlamaConfig::default()),
            base_fallback: None,
            experts: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ExpertConfig {
    pub name: String,
    pub use_for: String,
    pub backend: BackendConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum BackendConfig {
    Llama(LlamaConfig),
    OpenAiCompatible(OpenAiOptions),
    Anthropic(AnthropicOptions),
    /// `claude -p` with your Claude Code login and its own read-only tools.
    ClaudeCode(ClaudeCodeOptions),
    /// `codex exec` with your Codex login in a read-only sandbox.
    Codex(CodexOptions),
}

impl BackendConfig {
    pub fn describe(&self) -> String {
        match self {
            Self::Llama(llama) => format!("llama.cpp {}", llama.model_file),
            Self::OpenAiCompatible(options) => format!("{} at {}", options.model, options.base_url),
            Self::Anthropic(options) => format!("Claude {}", options.model),
            Self::ClaudeCode(options) => format!(
                "Claude Code {}",
                options.model.as_deref().unwrap_or("default model")
            ),
            Self::Codex(options) => format!(
                "Codex {}",
                options.model.as_deref().unwrap_or("default model")
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct LlamaConfig {
    /// A file name in the model folders, or an absolute path.
    pub model_file: String,
    pub gpu_layers: u32,
    pub context_tokens: u32,
    pub min_free_vram_mib: u64,
}

impl Default for LlamaConfig {
    fn default() -> Self {
        Self {
            model_file: DEFAULT_MODEL_FILE.to_owned(),
            gpu_layers: 1000,
            context_tokens: 8192,
            min_free_vram_mib: 4500,
        }
    }
}

/// RON as `suja.ron` is written: `Anthropic(model: "...")` for a backend and no `Some(...)`.
pub fn ron_options() -> ron::Options {
    ron::Options::default()
        .with_default_extension(Extensions::UNWRAP_VARIANT_NEWTYPES | Extensions::IMPLICIT_SOME)
}

#[cfg(test)]
mod tests {
    use suja_model_http::Credential;

    use super::*;

    #[test]
    fn a_config_with_a_fallback_and_experts_parses() -> Result<(), ron::error::SpannedError> {
        let config: ModelConfig = ron_options().from_str(
            r#"(
                base: Llama(model_file: "small.gguf"),
                base_fallback: OpenAiCompatible(base_url: "http://127.0.0.1:1234/v1", model: "qwen3-4b"),
                experts: [
                    (name: "claude", use_for: "hard reasoning",
                     backend: Anthropic(effort: "low", credential: Secret(name: "anthropic_api_key"))),
                    (name: "claude-code", use_for: "questions about files and code", backend: ClaudeCode()),
                    (name: "codex", use_for: "a second opinion", backend: Codex(model: "gpt-5-codex")),
                ],
            )"#,
        )?;
        assert!(
            matches!(&config.base, BackendConfig::Llama(llama) if llama.model_file == "small.gguf" && llama.gpu_layers == 1000)
        );
        assert!(
            matches!(&config.base_fallback, Some(BackendConfig::OpenAiCompatible(options)) if options.credential == Credential::None)
        );
        let BackendConfig::Anthropic(claude) = &config.experts[0].backend else {
            panic!("expected Anthropic");
        };
        assert_eq!(claude.model, "claude-opus-5-5");
        assert_eq!(claude.effort.as_deref(), Some("low"));
        assert_eq!(
            claude.credential,
            Credential::Secret {
                name: "anthropic_api_key".into()
            }
        );
        Ok(())
    }
}
