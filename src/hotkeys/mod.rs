//! Global hotkeys. Two backends can run side by side:
//! - `portal`: xdg-desktop-portal GlobalShortcuts, works everywhere on Wayland;
//! - `x11`: classic key grabs, for X11 sessions and as a fallback.
//! Events from both are merged and de-duplicated. A gamepad button combination can
//! save a replay too (`gamepad`).

mod gamepad;
mod portal;
mod x11;

pub use gamepad::connected_names as connected_gamepads;

use crate::config::Config;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    SaveReplay,
    ToggleRecord,
    SaveShort,
    Screenshot,
    Marker,
}

impl HotkeyEvent {
    /// Stable id (portal shortcut id) and the description the desktop shows.
    pub fn portal_info(self) -> (&'static str, &'static str) {
        match self {
            HotkeyEvent::SaveReplay => ("save-replay", "Save instant replay"),
            HotkeyEvent::ToggleRecord => ("toggle-recording", "Start / stop recording"),
            HotkeyEvent::SaveShort => ("save-short", "Save the last seconds"),
            HotkeyEvent::Screenshot => ("screenshot", "Take a screenshot"),
            HotkeyEvent::Marker => ("marker", "Mark this moment"),
        }
    }
}

pub struct HotkeyManager {
    x11: Option<x11::X11Hotkeys>,
    gamepads: gamepad::Gamepads,
    merged_tx: Sender<HotkeyEvent>,
    /// The last portal trigger set, so we only rebind (and re-prompt) when it changes.
    portal_triggers: Mutex<Option<Vec<String>>>,
}

impl HotkeyManager {
    pub fn new(tx_hotkey: Sender<HotkeyEvent>) -> Self {
        let (merged_tx, merged_rx) = mpsc::channel::<HotkeyEvent>();

        // Both backends may report the same key press; drop repeats within a short window.
        std::thread::spawn(move || {
            let mut last: Option<(HotkeyEvent, Instant)> = None;
            while let Ok(ev) = merged_rx.recv() {
                let duplicate = last
                    .map(|(e, t)| e == ev && t.elapsed() < Duration::from_millis(400))
                    .unwrap_or(false);
                last = Some((ev, Instant::now()));
                if !duplicate {
                    let _ = tx_hotkey.send(ev);
                }
            }
        });

        Self {
            x11: x11::X11Hotkeys::new(merged_tx.clone()),
            gamepads: gamepad::Gamepads::new(merged_tx.clone()),
            merged_tx,
            portal_triggers: Mutex::new(None),
        }
    }

    pub fn register_from_config(&self, cfg: &Config) {
        self.gamepads.configure(cfg.gamepad_hotkey, &cfg.gamepad_buttons);
        let backend = cfg.hotkey_backend.as_str();
        let use_portal = backend != "x11" && std::env::var_os("WAYLAND_DISPLAY").is_some();
        let use_x11 = backend != "portal" || !use_portal;

        match &self.x11 {
            Some(x) if use_x11 => x.register_from_config(cfg),
            Some(x) => x.unregister(),
            None => {}
        }

        if use_portal {
            let shortcuts: Vec<portal::PortalShortcut> = cfg
                .hotkey_bindings()
                .into_iter()
                .map(|(event, code, mods)| {
                    let (id, description) = event.portal_info();
                    portal::PortalShortcut { id, description, trigger: portal::trigger_string(code, mods), event }
                })
                .collect();
            let changed = {
                let mut last = self.portal_triggers.lock().unwrap_or_else(|e| e.into_inner());
                let new = Some(shortcuts.iter().map(|s| format!("{}={}", s.id, s.trigger)).collect());
                let changed = *last != new;
                *last = new;
                changed
            };
            if changed {
                portal::bind(
                    shortcuts,
                    self.merged_tx.clone(),
                    Arc::new(|err| eprintln!("Lapse: shortcut portal unavailable: {}", err)),
                );
            }
        }
    }
}

pub fn portal_available() -> bool {
    portal::is_available()
}
