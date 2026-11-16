use std::path::PathBuf;

use anyhow::{Context, Result};
use crownos_config::{Keybind, Mods};
use serde::Deserialize;
use suja_tools::ToolSettings;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotkey: Keybind,
    pub model_file: String,
    pub stt_model_dir: String,
    pub tts: TtsConfig,
    pub searxng_url: Option<String>,
    pub brave_api_key: Option<String>,
    pub allowed_commands: Option<Vec<String>>,
    pub idle_reset_seconds: u64,
    pub min_free_vram_mib: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    pub backend: String,
    pub model_dir: String,
    pub voice: String,
    pub speed: f32,
    pub threads: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: Keybind::new(
                Mods {
                    meta: true,
                    alt: true,
                    ..Mods::NONE
                },
                None,
            ),
            model_file: suja_llm::models::DEFAULT_MODEL_FILE.to_owned(),
            stt_model_dir: "sherpa-onnx-streaming-zipformer-en-2023-06-26".to_owned(),
            tts: TtsConfig::default(),
            searxng_url: None,
            brave_api_key: None,
            allowed_commands: None,
            idle_reset_seconds: 180,
            min_free_vram_mib: 4500,
        }
    }
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            backend: "pocket".to_owned(),
            model_dir: "sherpa-onnx-pocket-tts-int8-2026-01-26".to_owned(),
            voice: "test_wavs/bria.wav".to_owned(),
            speed: 1.0,
            threads: 4,
        }
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("crownos/suja.ron"))
    }

    pub fn load() -> Result<Self> {
        let Some(path) = Self::path() else {
            return Ok(Self::default());
        };
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        ron::from_str(&text).with_context(|| format!("parse {}", path.display()))
    }

    pub fn tool_settings(&self) -> ToolSettings {
        let mut settings = ToolSettings {
            searxng_url: self.searxng_url.clone(),
            brave_api_key: self.brave_api_key.clone(),
            ..ToolSettings::default()
        };
        if let Some(allowed) = &self.allowed_commands {
            settings.allowed_commands = allowed.clone();
        }
        settings
    }
}
