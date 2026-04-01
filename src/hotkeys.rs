use rdev::{listen, EventType, Key};
use std::thread;
use std::sync::mpsc::Sender;

pub enum HotkeyEvent {
    SaveReplay,
    ToggleRecord,
}

pub fn start_listener(tx: Sender<HotkeyEvent>, replay_key: String, record_key: String) {
    let replay_key = parse_key(&replay_key).unwrap_or(Key::F10);
    let record_key = parse_key(&record_key).unwrap_or(Key::F9);

    thread::spawn(move || {
        if let Err(error) = listen(move |event| {
            if let EventType::KeyPress(key) = event.event_type {
                if key == replay_key {
                    let _ = tx.send(HotkeyEvent::SaveReplay);
                } else if key == record_key {
                    let _ = tx.send(HotkeyEvent::ToggleRecord);
                }
            }
        }) {
            eprintln!("Error: {:?}", error);
        }
    });
}

fn parse_key(s: &str) -> Option<Key> {
    match s.to_uppercase().as_str() {
        "F1" => Some(Key::F1),
        "F2" => Some(Key::F2),
        "F3" => Some(Key::F3),
        "F4" => Some(Key::F4),
        "F5" => Some(Key::F5),
        "F6" => Some(Key::F6),
        "F7" => Some(Key::F7),
        "F8" => Some(Key::F8),
        "F9" => Some(Key::F9),
        "F10" => Some(Key::F10),
        "F11" => Some(Key::F11),
        "F12" => Some(Key::F12),
        _ => None,
    }
}
