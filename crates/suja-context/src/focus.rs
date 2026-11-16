use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use crownos_protocols::plugin_host::v1::client::crownos_plugin_host_v1::{
    self, CrownosPluginHostV1, Watch,
};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, Dispatch, QueueHandle};

use crate::error::ContextError;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FocusedWindow {
    pub app_id: String,
    pub title: String,
}

#[derive(Default)]
struct Windows {
    open: HashMap<u32, FocusedWindow>,
    focused: u32,
}

#[derive(Clone)]
pub struct FocusWatcher {
    windows: Arc<Mutex<Windows>>,
}

impl FocusWatcher {
    pub fn start() -> Result<Self, ContextError> {
        let connection =
            Connection::connect_to_env().map_err(|e| ContextError::Wayland(e.to_string()))?;
        let (globals, mut queue) = registry_queue_init::<Windows>(&connection)
            .map_err(|e| ContextError::Wayland(e.to_string()))?;
        let host: CrownosPluginHostV1 = globals.bind(&queue.handle(), 1..=1, ()).map_err(|_| {
            ContextError::Wayland("compositor does not offer crownos_plugin_host_v1".into())
        })?;
        host.watch(Watch::Windows);
        connection
            .flush()
            .map_err(|e| ContextError::Wayland(e.to_string()))?;
        let windows = Arc::new(Mutex::new(Windows::default()));
        let shared = windows.clone();
        thread::Builder::new()
            .name("suja-focus".into())
            .spawn(move || {
                let mut state = Windows::default();
                loop {
                    if queue.blocking_dispatch(&mut state).is_err() {
                        tracing::info!("focus watcher: compositor connection closed");
                        return;
                    }
                    if let Ok(mut snapshot) = shared.lock() {
                        snapshot.open = state.open.clone();
                        snapshot.focused = state.focused;
                    }
                }
            })
            .map_err(|e| ContextError::Wayland(e.to_string()))?;
        Ok(Self { windows })
    }

    pub fn current(&self) -> Option<FocusedWindow> {
        let windows = self.windows.lock().ok()?;
        windows.open.get(&windows.focused).cloned()
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Windows {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<CrownosPluginHostV1, ()> for Windows {
    fn event(
        windows: &mut Self,
        _: &CrownosPluginHostV1,
        event: crownos_plugin_host_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use crownos_plugin_host_v1::Event;
        match event {
            Event::WindowOpened {
                id, app_id, title, ..
            }
            | Event::WindowChanged { id, app_id, title } => {
                windows.open.insert(id, FocusedWindow { app_id, title });
            }
            Event::WindowClosed { id } => {
                windows.open.remove(&id);
            }
            Event::WindowFocused { id } => windows.focused = id,
            _ => {}
        }
    }
}
