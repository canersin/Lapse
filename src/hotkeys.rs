use crate::config::Config;
use global_hotkey::hotkey::{HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;

pub enum HotkeyEvent {
    SaveReplay,
    ToggleRecord,
}

pub struct HotkeyManager {
    manager: Arc<GlobalHotKeyManager>,
    hotkey_ids: Arc<Mutex<(u32, u32)>>,
}

impl HotkeyManager {
    pub fn new(tx_hotkey: mpsc::Sender<HotkeyEvent>) -> Self {
        let manager = Arc::new(GlobalHotKeyManager::new().unwrap());
        let hotkey_ids = Arc::new(Mutex::new((0u32, 0u32)));
        let hotkey_ids_clone = Arc::clone(&hotkey_ids);
        
        std::thread::spawn(move || {
            let receiver = GlobalHotKeyEvent::receiver();
            while let Ok(event) = receiver.recv() {
                if event.state == HotKeyState::Released {
                    continue;
                }
                
                let ids = hotkey_ids_clone.lock().unwrap();
                if event.id == ids.0 {
                    let _ = tx_hotkey.send(HotkeyEvent::SaveReplay);
                } else if event.id == ids.1 {
                    let _ = tx_hotkey.send(HotkeyEvent::ToggleRecord);
                }
            }
        });

        Self { manager, hotkey_ids }
    }

    pub fn register_from_config(&self, cfg: &Config) {
        let r_hk = HotKey::new(Self::get_mod(cfg.hotkey_replay_mod), cfg.hotkey_replay);
        let rec_hk = HotKey::new(Self::get_mod(cfg.hotkey_record_mod), cfg.hotkey_record);
        
        let _ = self.manager.unregister_all(&[]);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = self.manager.register(r_hk);
        let _ = self.manager.register(rec_hk);
        
        if let Ok(mut ids) = self.hotkey_ids.lock() {
            *ids = (r_hk.id(), rec_hk.id());
        }
    }

    fn get_mod(bits: u32) -> Option<Modifiers> {
        let mut m = Modifiers::empty();
        if bits & (1 << 0) != 0 { m |= Modifiers::ALT; }
        if bits & (1 << 1) != 0 { m |= Modifiers::CONTROL; }
        if bits & (1 << 2) != 0 { m |= Modifiers::SHIFT; }
        if bits & (1 << 3) != 0 { m |= Modifiers::SUPER; }
        if m.is_empty() { None } else { Some(m) }
    }
}
