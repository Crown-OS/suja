use std::sync::Arc;

use suja_model::{ChatModel, Expert};
use suja_model_anthropic::AnthropicBackend;
use suja_model_openai::OpenAiBackend;

use crate::config::{BackendConfig, LlamaConfig, ModelConfig};
use crate::error::RouterError;

pub struct LoadedModels {
    pub base: Arc<dyn ChatModel>,
    /// The base and the experts in words, for `suja status`.
    pub description: String,
    pub experts: Vec<Expert>,
}

pub async fn load_models(config: &ModelConfig) -> Result<LoadedModels, RouterError> {
    let (base, base_config) = match build_backend(&config.base).await {
        Ok(base) => (base, &config.base),
        Err(error) => {
            let Some(fallback) = &config.base_fallback else {
                return Err(error);
            };
            tracing::warn!(%error, fallback = %fallback.describe(), "base model unavailable, using the fallback");
            (build_backend(fallback).await?, fallback)
        }
    };
    let mut description = base_config.describe();
    let mut experts = Vec::with_capacity(config.experts.len());
    for expert in &config.experts {
        match build_backend(&expert.backend).await {
            Ok(model) => {
                description.push_str(&format!(
                    "; expert {}: {}",
                    expert.name,
                    expert.backend.describe()
                ));
                experts.push(Expert {
                    name: expert.name.clone(),
                    use_for: expert.use_for.clone(),
                    model,
                });
            }
            Err(error) => tracing::warn!(%error, expert = %expert.name, "expert unavailable"),
        }
    }
    Ok(LoadedModels {
        base,
        description,
        experts,
    })
}

async fn build_backend(config: &BackendConfig) -> Result<Arc<dyn ChatModel>, RouterError> {
    Ok(match config {
        BackendConfig::Llama(llama) => load_llama(llama).await?,
        BackendConfig::OpenAiCompatible(options) => Arc::new(OpenAiBackend::new(options.clone())?),
        BackendConfig::Anthropic(options) => Arc::new(AnthropicBackend::new(options.clone())?),
    })
}

#[cfg(feature = "llama")]
async fn load_llama(config: &LlamaConfig) -> Result<Arc<dyn ChatModel>, RouterError> {
    use std::path::Path;

    use suja_model_llama::grammar::SamplingOptions;
    use suja_model_llama::models::find_model;
    use suja_model_llama::{
        EngineOptions, LlamaBackend, LlamaBackendOptions, SessionOptions, vram,
    };

    let model_path = if Path::new(&config.model_file).is_absolute() {
        Some(config.model_file.clone().into())
    } else {
        find_model(&config.model_file)
    }
    .ok_or_else(|| RouterError::MissingModel(config.model_file.clone()))?;
    vram::preflight(config.min_free_vram_mib)
        .await
        .map_err(|error| RouterError::Llama(error.to_string()))?;
    let backend = LlamaBackend::load(LlamaBackendOptions {
        engine: EngineOptions {
            model_path,
            gpu_layers: config.gpu_layers,
        },
        session: SessionOptions {
            context_tokens: config.context_tokens,
            ..SessionOptions::default()
        },
        sampling: SamplingOptions::default(),
    })
    .await
    .map_err(|error| RouterError::Llama(error.to_string()))?;
    Ok(Arc::new(backend))
}

#[cfg(not(feature = "llama"))]
async fn load_llama(_: &LlamaConfig) -> Result<Arc<dyn ChatModel>, RouterError> {
    Err(RouterError::NoLlama)
}

#[cfg(test)]
mod tests {
    use suja_model_http::Credential;
    use suja_model_openai::{OpenAiOptions, TokenLimitField};

    use super::*;
    use crate::config::ExpertConfig;

    #[tokio::test]
    async fn a_missing_local_model_falls_back_and_experts_load() -> Result<(), RouterError> {
        let config = ModelConfig {
            base: BackendConfig::Llama(LlamaConfig {
                model_file: "/no/such/model.gguf".into(),
                ..LlamaConfig::default()
            }),
            base_fallback: Some(BackendConfig::OpenAiCompatible(OpenAiOptions {
                base_url: "http://127.0.0.1:1234/v1".into(),
                model: "qwen3-4b".into(),
                credential: Credential::None,
                context_tokens: 32_768,
                token_limit: TokenLimitField::MaxTokens,
            })),
            experts: vec![ExpertConfig {
                name: "claude".into(),
                use_for: "hard reasoning".into(),
                backend: BackendConfig::Anthropic(
                    crate::ron_options()
                        .from_str("()")
                        .map_err(|error| RouterError::Llama(error.to_string()))?,
                ),
            }],
        };
        let models = load_models(&config).await?;
        assert!(models.base.capabilities().runs_locally);
        assert_eq!(models.experts.len(), 1);
        assert_eq!(
            models.description,
            "qwen3-4b at http://127.0.0.1:1234/v1; expert claude: Claude claude-opus-5-5"
        );
        Ok(())
    }
}
