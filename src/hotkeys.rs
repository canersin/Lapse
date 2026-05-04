use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, hotkey::{HotKey, Code}};
use std::thread;
use std::sync::mpsc::Sender;

pub enum HotkeyEvent {
    SaveReplay,
    ToggleRecord,
}

pub struct HotkeyContext {
    pub manager: GlobalHotKeyManager,
}

pub fn start_listener(tx: Sender<HotkeyEvent>, replay_key: String, record_key: String) -> Result<HotkeyContext, String> {
    let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;

    let replay_hotkey = parse_key(&replay_key).unwrap_or_else(|| HotKey::new(None, Code::F10));
    let record_hotkey = parse_key(&record_key).unwrap_or_else(|| HotKey::new(None, Code::F9));

    let _ = manager.register(replay_hotkey);
    let _ = manager.register(record_hotkey);

    thread::spawn(move || {
        let receiver = GlobalHotKeyEvent::receiver();
        while let Ok(event) = receiver.recv() {
            if event.id == replay_hotkey.id() {
                let _ = tx.send(HotkeyEvent::SaveReplay);
            } else if event.id == record_hotkey.id() {
                let _ = tx.send(HotkeyEvent::ToggleRecord);
            }
        }
    });

    Ok(HotkeyContext { manager })
}

fn parse_key(s: &str) -> Option<HotKey> {
    let code = match s.to_uppercase().as_str() {
        "F1" => Code::F1,
        "F2" => Code::F2,
        "F3" => Code::F3,
        "F4" => Code::F4,
        "F5" => Code::F5,
        "F6" => Code::F6,
        "F7" => Code::F7,
        "F8" => Code::F8,
        "F9" => Code::F9,
        "F10" => Code::F10,
        "F11" => Code::F11,
        "F12" => Code::F12,
        _ => return None,
    };
    Some(HotKey::new(None, code))
}
