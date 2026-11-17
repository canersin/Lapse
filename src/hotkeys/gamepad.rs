//! Gamepad shortcut: holding a button combination (default Select + R3) saves a replay.
//!
//! Reads controllers straight from /dev/input/event* (the user must be in the `input`
//! group). Works the same on X11 and Wayland, and for Steam Input's virtual pads.

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::HotkeyEvent;

/// BTN_SOUTH (A / cross): every gamepad has it, keyboards and mice don't.
const BTN_GAMEPAD: usize = 304;
const EV_KEY: u16 = 1;
/// struct input_event on 64-bit Linux: timeval (16) + type (2) + code (2) + value (4).
const EVENT_SIZE: usize = 24;

#[derive(Default)]
struct Settings {
    enabled: bool,
    buttons: Vec<u16>,
}

pub struct Gamepads {
    settings: Arc<Mutex<Settings>>,
}

/// Blocks of /proc/bus/input/devices that describe gamepads.
fn gamepad_blocks(devices: &str) -> impl Iterator<Item = &str> {
    devices.split("\n\n").filter(|block| {
        block
            .lines()
            .find_map(|l| l.strip_prefix("B: KEY="))
            .map(|bits| has_bit(bits, BTN_GAMEPAD))
            .unwrap_or(false)
    })
}

/// Event device paths of connected gamepads.
fn find_gamepads(devices: &str) -> Vec<PathBuf> {
    gamepad_blocks(devices)
        .filter_map(|block| {
            let handlers = block.lines().find_map(|l| l.strip_prefix("H: Handlers="))?;
            let event = handlers.split_whitespace().find(|h| h.starts_with("event"))?;
            Some(PathBuf::from("/dev/input").join(event))
        })
        .collect()
}

/// The kernel prints capability bitmaps as hex words, most significant first.
fn has_bit(words: &str, bit: usize) -> bool {
    let words: Vec<&str> = words.split_whitespace().collect();
    let (index, offset) = (bit / 64, bit % 64);
    words
        .len()
        .checked_sub(index + 1)
        .and_then(|i| u64::from_str_radix(words[i], 16).ok())
        .map(|w| w >> offset & 1 == 1)
        .unwrap_or(false)
}

/// Tracks held buttons; returns true once each time the whole combination is down.
#[derive(Default)]
struct Combo {
    held: HashSet<u16>,
    fired: bool,
}

impl Combo {
    fn feed(&mut self, code: u16, pressed: bool, combo: &[u16]) -> bool {
        if pressed {
            self.held.insert(code);
        } else {
            self.held.remove(&code);
        }
        let all = !combo.is_empty() && combo.iter().all(|b| self.held.contains(b));
        let fire = all && !self.fired;
        self.fired = all;
        fire
    }
}

fn watch(path: PathBuf, settings: Arc<Mutex<Settings>>, tx: Sender<HotkeyEvent>, watching: Arc<Mutex<HashSet<PathBuf>>>) {
    std::thread::spawn(move || {
        if let Ok(mut file) = File::open(&path) {
            let mut combo = Combo::default();
            let mut buf = [0u8; EVENT_SIZE];
            while file.read_exact(&mut buf).is_ok() {
                let kind = u16::from_ne_bytes([buf[16], buf[17]]);
                let code = u16::from_ne_bytes([buf[18], buf[19]]);
                let value = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);
                // value 2 is key repeat.
                if kind != EV_KEY || value == 2 {
                    continue;
                }
                let (enabled, buttons) = {
                    let s = settings.lock().unwrap_or_else(|e| e.into_inner());
                    (s.enabled, s.buttons.clone())
                };
                if combo.feed(code, value == 1, &buttons) && enabled {
                    let _ = tx.send(HotkeyEvent::SaveReplay);
                }
            }
        }
        // Unplugged (or no permission): let the scanner pick it up again later.
        watching.lock().unwrap_or_else(|e| e.into_inner()).remove(&path);
    });
}

impl Gamepads {
    pub fn new(tx: Sender<HotkeyEvent>) -> Self {
        let settings = Arc::new(Mutex::new(Settings::default()));
        let shared = Arc::clone(&settings);
        std::thread::spawn(move || {
            let watching: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();
            loop {
                let enabled = shared.lock().map(|s| s.enabled).unwrap_or(false);
                if enabled {
                    let devices = std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default();
                    for path in find_gamepads(&devices) {
                        if watching.lock().unwrap_or_else(|e| e.into_inner()).insert(path.clone()) {
                            watch(path, Arc::clone(&shared), tx.clone(), Arc::clone(&watching));
                        }
                    }
                }
                // Controllers get plugged in later; look again now and then.
                std::thread::sleep(Duration::from_secs(5));
            }
        });
        Self { settings }
    }

    pub fn configure(&self, enabled: bool, buttons: &[u16]) {
        let mut s = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        s.enabled = enabled;
        s.buttons = buttons.to_vec();
    }
}

/// Connected gamepads by name, for the settings page.
pub fn connected_names() -> Vec<String> {
    let devices = std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default();
    gamepad_blocks(&devices)
        .filter_map(|b| b.lines().find_map(|l| l.strip_prefix("N: Name=")).map(|n| n.trim_matches('"').to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICES: &str = "I: Bus=0003 Vendor=1462 Product=3fa4 Version=0111\nN: Name=\"MS MSI Gaming Controller\"\nH: Handlers=event13 \nB: EV=9\nB: ABS=10000000000\n\nI: Bus=0003 Vendor=28de Product=11ff Version=0001\nN: Name=\"Microsoft X-Box 360 pad 0\"\nH: Handlers=event27 js0 \nB: EV=20000b\nB: KEY=7cdb000000000000 0 0 0 0\nB: ABS=3003f\n\nN: Name=\"SINO WEALTH Gaming KB \"\nH: Handlers=kbd leds event3 \nB: EV=120013\nB: KEY=1000000000007 ff9f207ac14057ff febeffdfffefffff fffffffffffffffe\n";

    #[test]
    fn finds_only_real_gamepads() {
        assert_eq!(find_gamepads(DEVICES), vec![PathBuf::from("/dev/input/event27")]);
    }

    #[test]
    fn combo_fires_once_while_held() {
        let combo = [314, 318];
        let mut c = Combo::default();
        assert!(!c.feed(314, true, &combo));
        assert!(c.feed(318, true, &combo));
        assert!(!c.feed(304, true, &combo));
        assert!(!c.feed(318, false, &combo));
        assert!(c.feed(318, true, &combo));
    }
}
