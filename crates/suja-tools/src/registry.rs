use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::error::ToolError;
use crate::outcome::ToolOutcome;
use crate::{Tool, ToolSpec};

#[derive(Clone, Debug)]
pub struct ToolSettings {
    pub allowed_commands: Vec<String>,
    pub notes_dir: PathBuf,
    pub searxng_url: Option<String>,
    pub brave_api_key: Option<String>,
}

impl Default for ToolSettings {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        Self {
            allowed_commands: [
                "ls",
                "cat",
                "head",
                "tail",
                "wc",
                "df",
                "du",
                "free",
                "uptime",
                "date",
                "ps",
                "top",
                "uname",
                "whoami",
                "pwd",
                "echo",
                "git",
                "systemctl",
                "journalctl",
                "pacman",
                "ip",
                "ping",
                "nvidia-smi",
                "wpctl",
                "playerctl",
                "brightnessctl",
                "notify-send",
                "xdg-open",
                "gio",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
            notes_dir: home.join("Notes"),
            searxng_url: None,
            brave_api_key: None,
        }
    }
}

#[derive(Default)]
pub struct Registry {
    tools: BTreeMap<&'static str, Arc<dyn Tool>>,
}

impl Registry {
    pub fn with_defaults(settings: &ToolSettings) -> Self {
        let mut registry = Self::default();
        registry.register(crate::run_command::RunCommand::new(
            settings.allowed_commands.clone(),
        ));
        registry.register(crate::read_file::ReadFile);
        registry.register(crate::search_files::SearchFiles);
        registry.register(crate::save_note::SaveNote::new(settings.notes_dir.clone()));
        registry.register(crate::adjust_setting::AdjustSetting);
        registry.register(crate::media_control::MediaControl);
        registry.register(crate::set_timer::SetTimer);
        registry.register(crate::web::search::WebSearch::new(
            settings.searxng_url.clone(),
            settings.brave_api_key.clone(),
        ));
        registry.register(crate::web::fetch::WebFetch::new());
        registry
    }

    pub fn register(&mut self, tool: impl Tool + 'static) {
        self.tools.insert(tool.spec().name, Arc::new(tool));
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.tools.keys().copied()
    }

    pub async fn call(&self, name: &str, arguments: Value) -> Result<ToolOutcome, ToolError> {
        self.tool(name)?.call(arguments).await
    }

    pub async fn call_confirmed(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<ToolOutcome, ToolError> {
        self.tool(name)?.call_confirmed(arguments).await
    }

    fn tool(&self, name: &str) -> Result<Arc<dyn Tool>, ToolError> {
        self.tools
            .get(name)
            .cloned()
            .ok_or_else(|| ToolError::UnknownTool(name.to_owned()))
    }
}
