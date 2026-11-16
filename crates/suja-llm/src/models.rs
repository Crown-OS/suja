use std::path::PathBuf;

pub const DEFAULT_MODEL_FILE: &str = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";

pub fn model_search_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/share/crownos/suja/models/llm")];
    if let Some(cache) = dirs::cache_dir() {
        dirs.push(cache.join("crownos/suja/models/llm"));
    }
    dirs
}

pub fn find_model(file_name: &str) -> Option<PathBuf> {
    model_search_dirs()
        .into_iter()
        .map(|dir| dir.join(file_name))
        .find(|path| path.is_file())
}
