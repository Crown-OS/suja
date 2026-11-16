pub mod clipboard;
pub mod error;
pub mod focus;
pub mod selection;

use std::fmt::Write;
use std::time::Duration;

pub use error::ContextError;
pub use focus::{FocusWatcher, FocusedWindow};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScreenContext {
    pub focused: Option<FocusedWindow>,
    pub selection: Option<String>,
    pub clipboard_preview: Option<String>,
}

impl ScreenContext {
    pub fn is_empty(&self) -> bool {
        self.focused.is_none() && self.selection.is_none() && self.clipboard_preview.is_none()
    }

    pub fn render(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut text = String::new();
        if let Some(window) = &self.focused {
            let _ = write!(
                text,
                "focused app={} title={:?}",
                window.app_id, window.title
            );
        }
        if let Some(selection) = &self.selection {
            let _ = write!(text, "; selected text={selection:?}");
        }
        if let Some(clip) = &self.clipboard_preview {
            let _ = write!(text, "; clipboard={clip:?}");
        }
        Some(text)
    }
}

pub struct Collector {
    focus: Option<FocusWatcher>,
    budget: Duration,
}

impl Collector {
    pub fn start(budget: Duration) -> Self {
        let focus = match FocusWatcher::start() {
            Ok(watcher) => Some(watcher),
            Err(error) => {
                tracing::info!(%error, "focused-window context unavailable");
                None
            }
        };
        Self { focus, budget }
    }

    pub async fn snapshot(&self) -> ScreenContext {
        let focused = self.focus.as_ref().and_then(FocusWatcher::current);
        let (selection, clipboard_preview) = tokio::join!(
            selection::primary_selection(self.budget),
            clipboard::latest_preview(self.budget),
        );
        ScreenContext {
            focused,
            selection: selection
                .filter(|s| !s.trim().is_empty())
                .map(|s| truncate(&s, 400)),
            clipboard_preview: clipboard_preview
                .filter(|s| !s.trim().is_empty())
                .map(|s| truncate(&s, 200)),
        }
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}
