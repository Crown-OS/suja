use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use suja_agent::{Agent, WorkerHandle};
use suja_llm::models::{DEFAULT_MODEL_FILE, find_model};
use suja_llm::{EngineOptions, SessionOptions};
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
    let options = Agent::worker_options(
        &registry,
        EngineOptions {
            model_path,
            gpu_layers: 1000,
        },
        SessionOptions::default(),
    )?;
    eprintln!(
        "tool grammar: {} bytes, system prefix: {} chars",
        options.tool_grammar.as_deref().map_or(0, str::len),
        options.prefix.len()
    );
    let started = Instant::now();
    let worker = WorkerHandle::spawn(options).await?;
    let stats = worker.stats().await?;
    eprintln!(
        "model ready in {:.1}s, prefix {} tokens, {} tokens free",
        started.elapsed().as_secs_f32(),
        stats.tokens_in_context,
        stats.remaining_tokens
    );
    Ok(Agent::new(worker, registry))
}
