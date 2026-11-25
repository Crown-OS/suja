use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use suja_agent::Agent;
use suja_router::{BackendConfig, LlamaConfig, ModelConfig, load_models, ron_options};
use suja_tools::{Registry, ToolSettings};

pub async fn load_agent(model: Option<&Path>, models: Option<&Path>) -> Result<Agent> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();
    let mut config: ModelConfig = match models {
        Some(path) => ron_options()
            .from_str(&std::fs::read_to_string(path).context("read the models file")?)
            .context("parse the models file")?,
        None => ModelConfig::default(),
    };
    if let Some(path) = model {
        config.base = BackendConfig::Llama(LlamaConfig {
            model_file: path.display().to_string(),
            ..LlamaConfig::default()
        });
    }
    let started = Instant::now();
    let loaded = load_models(&config).await?;
    eprintln!(
        "models ready in {:.1}s: {}",
        started.elapsed().as_secs_f32(),
        loaded.description
    );
    let registry = Arc::new(Registry::with_defaults(&ToolSettings::default()));
    Ok(Agent::new(loaded.base, loaded.experts, registry))
}
