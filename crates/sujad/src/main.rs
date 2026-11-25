mod audio;
mod config;
mod hold;
mod hotkey;
mod ipc;
mod orchestrator;
mod tts_thread;
mod turn;
mod vram;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::time::Duration;

use anyhow::{Context, Result};
use suja_agent::Agent;
use suja_model_llama::grammar::SamplingOptions;
use suja_model_llama::models::find_model;
use suja_model_llama::{EngineOptions, LlamaBackend, LlamaBackendOptions, SessionOptions};
use suja_proto::AssistantState;
use suja_speech::tts::SherpaSynthesizer;
use suja_speech::{ModelPaths, Synthesizer, TtsBackend, TtsOptions};
use suja_tools::Registry;
use tokio::sync::{mpsc, watch};

use crate::audio::playback::Player;
use crate::config::Config;
use crate::ipc::{IpcServer, Shared};
use crate::orchestrator::{Inputs, Orchestrator, Outputs, Services};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let config = Config::load()?;
    vram::preflight(config.min_free_vram_mib).await?;

    let model_path = find_model(&config.model_file)
        .with_context(|| format!("model {} not found", config.model_file))?;
    let registry = Arc::new(Registry::with_defaults(&config.tool_settings()));
    let backend = LlamaBackend::load(LlamaBackendOptions {
        engine: EngineOptions {
            model_path,
            gpu_layers: 1000,
        },
        session: SessionOptions::default(),
        sampling: SamplingOptions::default(),
    })
    .await
    .context("load language model")?;
    let agent = Arc::new(Agent::new(Arc::new(backend), registry));

    let paths = ModelPaths::discover(&config.stt_model_dir, &config.tts.model_dir)
        .context("speech models")?;
    tracing::info!(?paths, "loading text to speech");
    let synthesizer =
        SherpaSynthesizer::new(&tts_options(&config, &paths)?).context("load text to speech")?;
    tracing::info!(sample_rate = synthesizer.sample_rate(), "opening playback");
    let player = Player::start(synthesizer.sample_rate() as u32).context("open playback")?;
    tracing::info!("starting synthesis thread");
    let (tts_events_tx, tts_events) = mpsc::unbounded_channel();
    let tts = tts_thread::spawn(synthesizer, player, tts_events_tx.clone())?;

    let (speech_tx, speech) = mpsc::unbounded_channel();
    tracing::info!("loading speech recognizer");
    let listen = audio::capture::spawn(&paths, speech_tx).context("speech recognizer")?;
    tracing::info!("registering the hotkey");
    let (hotkey_tx, hotkeys) = mpsc::unbounded_channel();
    hotkey::spawn(config.hotkey, hotkey_tx).context("register the hotkey")?;

    let (requests_tx, requests) = mpsc::unbounded_channel();
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let (state_tx, state_rx) = watch::channel(AssistantState::Idle);
    let context_tokens = Arc::new(AtomicU32::new(0));
    let ipc = IpcServer::bind(
        Shared {
            state: state_rx,
            model: config.model_file.clone(),
            tts_backend: config.tts.backend.clone(),
            context_tokens: context_tokens.clone(),
        },
        requests_tx,
        events_rx,
    )?;
    let orchestrator = Orchestrator::new(
        Inputs {
            requests,
            hotkeys,
            speech,
            tts: tts_events,
        },
        Outputs {
            events: events_tx,
            state: state_tx,
            context_tokens,
        },
        Services {
            agent,
            context: Arc::new(suja_context::Collector::start(Duration::from_millis(200))),
            listen,
            tts,
            tts_events: tts_events_tx,
            idle_reset: Duration::from_secs(config.idle_reset_seconds),
        },
    );
    tracing::info!(preferred_hotkey = %config.hotkey, "sujad ready");
    tokio::select! {
        result = ipc.run() => result,
        result = orchestrator.run() => result,
        _ = terminate() => Ok(()),
    }
}

async fn terminate() {
    let mut sigterm = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        Ok(signal) => signal,
        Err(_) => return std::future::pending().await,
    };
    sigterm.recv().await;
}

fn tts_options(config: &Config, paths: &ModelPaths) -> Result<TtsOptions> {
    let backend = match config.tts.backend.as_str() {
        "pocket" => TtsBackend::Pocket {
            reference_voice: resolve_voice(&paths.tts_dir, &config.tts.voice),
        },
        "kokoro" => TtsBackend::Kokoro {
            speaker_id: config.tts.voice.parse().unwrap_or(0),
        },
        other => anyhow::bail!("unknown tts backend `{other}`"),
    };
    Ok(TtsOptions {
        model_dir: paths.tts_dir.clone(),
        backend,
        threads: config.tts.threads,
        speed: config.tts.speed,
    })
}

fn resolve_voice(model_dir: &std::path::Path, voice: &str) -> PathBuf {
    let path = PathBuf::from(voice);
    if path.is_absolute() {
        path
    } else {
        model_dir.join(path)
    }
}
