//! The hotkey, from the compositor through `crownos_keybind_v1`.
//!
//! sujad registers one action and asks for the chord in `suja.ron`. The
//! compositor decides the chord: the user can change it in the settings, and
//! a chord that is already in use is not given. The compositor sends the
//! press, the release, and a cancel when the user makes a different chord with
//! the hotkey held.

use std::thread;

use anyhow::{Context, Result};
use crownos_config::Keybind;
use crownos_protocols::keybind::v1::client::{
    crownos_keybind_manager_v1::CrownosKeybindManagerV1,
    crownos_keybind_v1::{self, CrownosKeybindV1},
};
use tokio::sync::mpsc::UnboundedSender;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, QueueHandle};

const APP_ID: &str = "org.crownos.Suja";
const ACTION_NAME: &str = "talk";
const ACTION_DESCRIPTION: &str = "Tap for the assistant, hold to speak to it";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// The chord the compositor gave the action. `None` means no chord.
    Assigned(String),
    Pressed,
    Released,
    Cancelled,
    /// The connection to the compositor closed.
    Lost(String),
}

/// Registers the action and sends its events to `events` from a thread.
pub fn spawn(preferred: Keybind, events: UnboundedSender<HotkeyEvent>) -> Result<()> {
    let connection = Connection::connect_to_env().context("connect to the compositor")?;
    let (globals, mut queue) =
        registry_queue_init::<Listener>(&connection).context("read the compositor globals")?;
    let manager: CrownosKeybindManagerV1 = globals
        .bind(&queue.handle(), 1..=1, ())
        .context("the compositor does not offer crownos_keybind_v1")?;
    let keybind = manager.register(
        APP_ID.to_owned(),
        ACTION_NAME.to_owned(),
        ACTION_DESCRIPTION.to_owned(),
        preferred.to_string(),
        &queue.handle(),
        (),
    );
    connection.flush().context("register the hotkey")?;

    thread::Builder::new()
        .name("suja-hotkey".into())
        .spawn(move || {
            let mut listener = Listener { events };
            let error = loop {
                if let Err(error) = queue.blocking_dispatch(&mut listener) {
                    break error;
                }
            };
            let _ = listener.events.send(HotkeyEvent::Lost(error.to_string()));
            drop((keybind, manager));
        })
        .context("spawn the hotkey thread")?;
    Ok(())
}

struct Listener {
    events: UnboundedSender<HotkeyEvent>,
}

impl Dispatch<CrownosKeybindV1, ()> for Listener {
    fn event(
        listener: &mut Self,
        _: &CrownosKeybindV1,
        event: crownos_keybind_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let event = match event {
            crownos_keybind_v1::Event::Assigned { keys } => HotkeyEvent::Assigned(keys),
            crownos_keybind_v1::Event::Pressed { .. } => HotkeyEvent::Pressed,
            crownos_keybind_v1::Event::Released { .. } => HotkeyEvent::Released,
            crownos_keybind_v1::Event::Cancelled => HotkeyEvent::Cancelled,
            _ => return,
        };
        let _ = listener.events.send(event);
    }
}

impl Dispatch<CrownosKeybindManagerV1, ()> for Listener {
    fn event(
        _: &mut Self,
        _: &CrownosKeybindManagerV1,
        _: <CrownosKeybindManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Listener {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
