//! Global press-and-hold detection for one configurable chord, via evdev.
//!
//! Reads /dev/input/event* directly so it works on any Wayland compositor
//! without compositor-specific keybind support. Requires the user to be in
//! the `input` group (or equivalent read access to /dev/input).
//!
//! # What "the chord is held" means
//!
//! The chord comes from `input.ron` as a [`Keybind`] — a set of modifiers plus
//! at most one ordinary key — and can change while the daemon runs, so nothing
//! here is specialised to any particular keys. The watcher keeps the set of
//! keys currently down and re-derives the answer from it after every event and
//! after every rebinding, rather than tracking "is Super down" and "is Space
//! down" as separate facts the way a hardcoded chord could afford to.
//!
//! The modifier match is *exact*: `Super+Space` fires on Super+Space and not on
//! Ctrl+Super+Space, because the second one is somebody reaching for a different
//! shortcut and starting to record over the top of it would be worse than
//! missing it. The ordinary key is required to be down but nothing else is
//! forbidden — a stray key struck mid-utterance does not cut the recording off.
//!
//! A modifier-only chord (`Super` on its own) is a real binding and the natural
//! shape for push-to-talk, which is why the key half is optional.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crownos_config::{KeyCode, Keybind, Mods};
use evdev::{Device, EventType, KeyCode as EvKey};

/// How long between sweeps of /dev/input for keyboards that have appeared.
const RESCAN: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
}

/// Where edges are reported.
///
/// A callback rather than a `Sender<HotkeyEvent>` so the controller can put its
/// own message type on the channel directly — it waits on settings changes as
/// well as on the keyboard, and a channel of exactly one of the two would need
/// a thread in the middle to relay. Shared because a copy goes to every device
/// thread.
type Emit = Arc<dyn Fn(HotkeyEvent) + Send + Sync>;

// --- MARK: Chord ---

/// Everything needed to answer "is the chord held right now": what the chord
/// is, and which keys are down.
struct Chord {
    binding: Keybind,
    /// Whether the daemon is listening at all. A disabled chord is never held,
    /// however the keyboard is being used.
    enabled: bool,
    /// Every key currently down, as `(device, evdev code)`.
    ///
    /// Every key, not just the interesting ones: which keys are interesting
    /// depends on the binding, and the binding can change between the press and
    /// the release. Keeping the lot means a rebinding is re-derivable from what
    /// is already known rather than needing the user to let go and start again.
    /// It is bounded by how many keys a person can hold down at once.
    down: HashSet<(usize, u16)>,
    /// What was last reported, so only edges are sent.
    active: bool,
    emit: Emit,
}

impl Chord {
    fn new(binding: Keybind, enabled: bool, emit: Emit) -> Self {
        Self {
            binding,
            enabled,
            down: HashSet::new(),
            active: false,
            emit,
        }
    }

    /// Whether the configured chord is held, given what is down.
    fn holds(&self) -> bool {
        if !self.enabled || self.binding.is_empty() {
            return false;
        }

        let held = self
            .down
            .iter()
            .filter_map(|(_, code)| modifier_of(EvKey::new(*code)))
            .fold(Mods::NONE, union);
        if held != self.binding.mods {
            return false;
        }

        match self.binding.key {
            None => !self
                .down
                .iter()
                .any(|(_, code)| modifier_of(EvKey::new(*code)).is_none()),
            Some(key) => {
                let wanted = evdev_key(key).code();
                self.down.iter().any(|(_, code)| *code == wanted)
            }
        }
    }

    /// Report a press or release edge, if this changed one.
    fn settle(&mut self) {
        let holds = self.holds();
        if holds == self.active {
            return;
        }
        self.active = holds;
        (self.emit)(if holds {
            HotkeyEvent::Pressed
        } else {
            HotkeyEvent::Released
        });
    }

    fn update(&mut self, dev: usize, code: u16, value: i32) {
        match value {
            0 => {
                self.down.remove(&(dev, code));
            }
            1 => {
                self.down.insert((dev, code));
            }
            // Auto-repeat: the key is already down and nothing has changed.
            _ => return,
        }
        self.settle();
    }

    /// Forget everything a vanished device was holding.
    ///
    /// Without this, unplugging a keyboard mid-utterance would leave its keys
    /// down forever and the chord held forever with them.
    fn drop_device(&mut self, dev: usize) {
        self.down.retain(|(d, _)| *d != dev);
        self.settle();
    }
}

/// Both sets of modifiers at once.
fn union(a: Mods, b: Mods) -> Mods {
    Mods {
        meta: a.meta || b.meta,
        ctrl: a.ctrl || b.ctrl,
        alt: a.alt || b.alt,
        shift: a.shift || b.shift,
    }
}

/// The modifier one physical key sets, or `None` if it is not a modifier.
fn modifier_of(key: EvKey) -> Option<Mods> {
    let mods = match key {
        EvKey::KEY_LEFTMETA | EvKey::KEY_RIGHTMETA => Mods::META,
        EvKey::KEY_LEFTCTRL | EvKey::KEY_RIGHTCTRL => Mods {
            ctrl: true,
            ..Mods::NONE
        },
        EvKey::KEY_LEFTALT | EvKey::KEY_RIGHTALT => Mods {
            alt: true,
            ..Mods::NONE
        },
        EvKey::KEY_LEFTSHIFT | EvKey::KEY_RIGHTSHIFT => Mods {
            shift: true,
            ..Mods::NONE
        },
        _ => return None,
    };
    Some(mods)
}

/// The evdev key one schema key means.
///
/// Exhaustive on purpose: [`KeyCode`] is a closed enum, so a key added to the
/// schema stops this file from compiling until it has been given a code here,
/// rather than silently becoming a shortcut that never fires.
fn evdev_key(key: KeyCode) -> EvKey {
    match key {
        KeyCode::A => EvKey::KEY_A,
        KeyCode::B => EvKey::KEY_B,
        KeyCode::C => EvKey::KEY_C,
        KeyCode::D => EvKey::KEY_D,
        KeyCode::E => EvKey::KEY_E,
        KeyCode::F => EvKey::KEY_F,
        KeyCode::G => EvKey::KEY_G,
        KeyCode::H => EvKey::KEY_H,
        KeyCode::I => EvKey::KEY_I,
        KeyCode::J => EvKey::KEY_J,
        KeyCode::K => EvKey::KEY_K,
        KeyCode::L => EvKey::KEY_L,
        KeyCode::M => EvKey::KEY_M,
        KeyCode::N => EvKey::KEY_N,
        KeyCode::O => EvKey::KEY_O,
        KeyCode::P => EvKey::KEY_P,
        KeyCode::Q => EvKey::KEY_Q,
        KeyCode::R => EvKey::KEY_R,
        KeyCode::S => EvKey::KEY_S,
        KeyCode::T => EvKey::KEY_T,
        KeyCode::U => EvKey::KEY_U,
        KeyCode::V => EvKey::KEY_V,
        KeyCode::W => EvKey::KEY_W,
        KeyCode::X => EvKey::KEY_X,
        KeyCode::Y => EvKey::KEY_Y,
        KeyCode::Z => EvKey::KEY_Z,

        KeyCode::Digit0 => EvKey::KEY_0,
        KeyCode::Digit1 => EvKey::KEY_1,
        KeyCode::Digit2 => EvKey::KEY_2,
        KeyCode::Digit3 => EvKey::KEY_3,
        KeyCode::Digit4 => EvKey::KEY_4,
        KeyCode::Digit5 => EvKey::KEY_5,
        KeyCode::Digit6 => EvKey::KEY_6,
        KeyCode::Digit7 => EvKey::KEY_7,
        KeyCode::Digit8 => EvKey::KEY_8,
        KeyCode::Digit9 => EvKey::KEY_9,

        KeyCode::F1 => EvKey::KEY_F1,
        KeyCode::F2 => EvKey::KEY_F2,
        KeyCode::F3 => EvKey::KEY_F3,
        KeyCode::F4 => EvKey::KEY_F4,
        KeyCode::F5 => EvKey::KEY_F5,
        KeyCode::F6 => EvKey::KEY_F6,
        KeyCode::F7 => EvKey::KEY_F7,
        KeyCode::F8 => EvKey::KEY_F8,
        KeyCode::F9 => EvKey::KEY_F9,
        KeyCode::F10 => EvKey::KEY_F10,
        KeyCode::F11 => EvKey::KEY_F11,
        KeyCode::F12 => EvKey::KEY_F12,

        KeyCode::Space => EvKey::KEY_SPACE,
        KeyCode::Enter => EvKey::KEY_ENTER,
        KeyCode::Tab => EvKey::KEY_TAB,
        KeyCode::Escape => EvKey::KEY_ESC,
        KeyCode::Backspace => EvKey::KEY_BACKSPACE,
        KeyCode::Delete => EvKey::KEY_DELETE,
        KeyCode::Insert => EvKey::KEY_INSERT,
        KeyCode::Home => EvKey::KEY_HOME,
        KeyCode::End => EvKey::KEY_END,
        KeyCode::PageUp => EvKey::KEY_PAGEUP,
        KeyCode::PageDown => EvKey::KEY_PAGEDOWN,
        KeyCode::CapsLock => EvKey::KEY_CAPSLOCK,
        KeyCode::PrintScreen => EvKey::KEY_SYSRQ,

        KeyCode::ArrowUp => EvKey::KEY_UP,
        KeyCode::ArrowDown => EvKey::KEY_DOWN,
        KeyCode::ArrowLeft => EvKey::KEY_LEFT,
        KeyCode::ArrowRight => EvKey::KEY_RIGHT,

        KeyCode::Minus => EvKey::KEY_MINUS,
        KeyCode::Equal => EvKey::KEY_EQUAL,
        KeyCode::LeftBracket => EvKey::KEY_LEFTBRACE,
        KeyCode::RightBracket => EvKey::KEY_RIGHTBRACE,
        KeyCode::Backslash => EvKey::KEY_BACKSLASH,
        KeyCode::Semicolon => EvKey::KEY_SEMICOLON,
        KeyCode::Quote => EvKey::KEY_APOSTROPHE,
        KeyCode::Backquote => EvKey::KEY_GRAVE,
        KeyCode::Comma => EvKey::KEY_COMMA,
        KeyCode::Period => EvKey::KEY_DOT,
        KeyCode::Slash => EvKey::KEY_SLASH,
        KeyCode::AudioVolumeUp => EvKey::KEY_VOLUMEUP,
        KeyCode::AudioVolumeDown => EvKey::KEY_VOLUMEDOWN,
        KeyCode::AudioVolumeMute => EvKey::KEY_MUTE,
        KeyCode::MicrophoneMute => EvKey::KEY_MICMUTE,
        KeyCode::MediaPlayPause => EvKey::KEY_PLAYPAUSE,
        KeyCode::MediaStop => EvKey::KEY_STOPCD,
        KeyCode::MediaTrackNext => EvKey::KEY_NEXTSONG,
        KeyCode::MediaTrackPrevious => EvKey::KEY_PREVIOUSSONG,
        KeyCode::BrightnessUp => EvKey::KEY_BRIGHTNESSUP,
        KeyCode::BrightnessDown => EvKey::KEY_BRIGHTNESSDOWN,
        KeyCode::KeyboardBrightnessUp => EvKey::KEY_KBDILLUMUP,
        KeyCode::KeyboardBrightnessDown => EvKey::KEY_KBDILLUMDOWN,
    }
}

// --- MARK: Watcher ---

/// A running watcher, and the handle that rebinds it.
///
/// Holding one keeps nothing alive — the threads outlive it — but dropping it
/// means nothing can change the chord again, which is only correct at
/// shutdown.
pub struct Hotkeys;

fn is_keyboard(dev: &Device) -> bool {
    dev.supported_keys().is_some_and(|keys| {
        keys.contains(EvKey::KEY_A)
            && keys.contains(EvKey::KEY_Z)
            && keys.contains(EvKey::KEY_SPACE)
    })
}

/// Spawn the watcher. Scans for keyboards, follows hotplug, and calls `emit`
/// on every press/release edge of the configured chord.
pub fn spawn(
    emit: impl Fn(HotkeyEvent) + Send + Sync + 'static,
    binding: Keybind,
    enabled: bool,
) -> Hotkeys {
    let chord = Arc::new(Mutex::new(Chord::new(binding, enabled, Arc::new(emit))));
    let handle = Hotkeys;

    std::thread::Builder::new()
        .name("hotkey-scan".into())
        .spawn(move || {
            let open: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
            let mut dev_counter = 0usize;
            let mut warned = false;
            loop {
                let mut found_any = false;
                if let Ok(entries) = std::fs::read_dir("/dev/input") {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                            continue;
                        };
                        if !name.starts_with("event") {
                            continue;
                        }
                        if open
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .contains(&path)
                        {
                            found_any = true;
                            continue;
                        }
                        let Ok(dev) = Device::open(&path) else {
                            continue;
                        };
                        if !is_keyboard(&dev) {
                            continue;
                        }
                        found_any = true;
                        dev_counter += 1;
                        let dev_id = dev_counter;
                        tracing::info!(
                            "hotkey: watching {} ({})",
                            path.display(),
                            dev.name().unwrap_or("?")
                        );
                        open.lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(path.clone());
                        let chord = chord.clone();
                        let open = open.clone();
                        std::thread::Builder::new()
                            .name(format!("hotkey-dev{dev_id}"))
                            .spawn(move || {
                                let mut dev = dev;
                                loop {
                                    match dev.fetch_events() {
                                        Ok(events) => {
                                            let mut chord =
                                                chord.lock().unwrap_or_else(|e| e.into_inner());
                                            for ev in events {
                                                if ev.event_type() == EventType::KEY {
                                                    chord.update(dev_id, ev.code(), ev.value());
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::info!("hotkey: device gone ({e}), dropping");
                                            chord
                                                .lock()
                                                .unwrap_or_else(|e| e.into_inner())
                                                .drop_device(dev_id);
                                            open.lock()
                                                .unwrap_or_else(|e| e.into_inner())
                                                .remove(&path);
                                            return;
                                        }
                                    }
                                }
                            })
                            .ok();
                    }
                }
                if !found_any && !warned {
                    warned = true;
                    tracing::error!(
                        "hotkey: no readable keyboard found in /dev/input — \
                         add your user to the `input` group (or run with elevated permissions)"
                    );
                }
                std::thread::sleep(RESCAN);
            }
        })
        .ok();

    handle
}
