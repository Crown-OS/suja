#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("wayland: {0}")]
    Wayland(String),
    #[error("clipboard: {0}")]
    Clipboard(String),
}
