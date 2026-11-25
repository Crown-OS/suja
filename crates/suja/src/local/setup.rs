use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use suja_agent::Agent;
use suja_model_llama::grammar::SamplingOptions;
use suja_model_llama::models::{DEFAULT_MODEL_FILE, find_model};
use suja_model_llama::{EngineOptions, LlamaBackend, LlamaBackendOptions, SessionOptions};
use suja_tools::{Registry, ToolSettings};

pub async fn load_agent(model: Option<PathBuf>) -> Result<Agent> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();
    let model_path = model
        .or_else(|| find_model(DEFAULT_MODEL_FILE))
        .context("no model found; pass --model or download the default GGUF")?;
    let registry = Arc::new(Registry::with_defaults(&ToolSettings::default()));
    let started = Instant::now();
    let backend = LlamaBackend::load(LlamaBackendOptions {
        engine: EngineOptions {
            model_path,
            gpu_layers: 1000,
        },
        session: SessionOptions::default(),
        sampling: SamplingOptions::default(),
    })
    .await?;
    eprintln!("model ready in {:.1}s", started.elapsed().as_secs_f32());
    Ok(Agent::new(Arc::new(backend), registry))
}
