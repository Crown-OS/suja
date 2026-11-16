use std::path::{Path, PathBuf};

use crate::error::SpeechError;

pub fn model_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/usr/share/crownos/suja/models")];
    if let Some(cache) = dirs::cache_dir() {
        roots.push(cache.join("crownos/suja/models"));
    }
    roots
}

#[derive(Clone, Debug)]
pub struct ModelPaths {
    pub vad_model: PathBuf,
    pub stt_dir: PathBuf,
    pub tts_dir: PathBuf,
}

impl ModelPaths {
    pub fn discover(stt_dir_name: &str, tts_dir_name: &str) -> Result<Self, SpeechError> {
        Ok(Self {
            vad_model: find_file("vad/silero_vad.onnx")?,
            stt_dir: find_file(&format!("stt/{stt_dir_name}"))?,
            tts_dir: find_file(&format!("tts/{tts_dir_name}"))?,
        })
    }
}

pub fn find_file(relative: &str) -> Result<PathBuf, SpeechError> {
    model_roots()
        .into_iter()
        .map(|root| root.join(relative))
        .find(|path| path.exists())
        .ok_or_else(|| SpeechError::MissingModel(relative.to_owned()))
}

pub fn require(path: &Path) -> Result<String, SpeechError> {
    if !path.exists() {
        return Err(SpeechError::MissingModel(path.display().to_string()));
    }
    Ok(path.display().to_string())
}
