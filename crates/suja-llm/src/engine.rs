use std::path::PathBuf;

use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;

use crate::error::LlmError;

#[derive(Clone, Debug)]
pub struct EngineOptions {
    pub model_path: PathBuf,
    pub gpu_layers: u32,
}

pub struct Engine {
    backend: LlamaBackend,
    model: LlamaModel,
}

impl Engine {
    pub fn load(options: &EngineOptions) -> Result<Self, LlmError> {
        if !options.model_path.is_file() {
            return Err(LlmError::MissingModel(
                options.model_path.display().to_string(),
            ));
        }
        let backend = LlamaBackend::init().map_err(|e| LlmError::Runtime(e.to_string()))?;
        let params = LlamaModelParams::default()
            .with_n_gpu_layers(options.gpu_layers)
            .with_use_mmap(true)
            .with_use_mlock(false);
        let model = LlamaModel::load_from_file(&backend, &options.model_path, &params)
            .map_err(|e| LlmError::Runtime(e.to_string()))?;
        Ok(Self { backend, model })
    }

    pub fn backend(&self) -> &LlamaBackend {
        &self.backend
    }

    pub fn model(&self) -> &LlamaModel {
        &self.model
    }
}
