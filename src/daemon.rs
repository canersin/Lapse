use crate::config::Config;
use crate::hotkeys::{HotkeyEvent, HotkeyManager};
use crate::recorder::Recorder;
use crate::{ipc, tray};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;

pub fn run_daemon() -> anyhow::Result<()> {
    let config = Config::load()?;
    let recorder = Arc::new(Mutex::new(Recorder::new(config.clone())));

    let socket_path = "/tmp/lapse.sock";
    let _ = std::fs::remove_file(socket_path);

    let (tx_hotkey, rx_hotkey) = mpsc::channel();
    let hotkey_manager = Arc::new(HotkeyManager::new(tx_hotkey));

    hotkey_manager.register_from_config(&config);

    let recorder_clone = Arc::clone(&recorder);
    std::thread::spawn(move || {
        if let Ok(mut rec) = recorder_clone.lock() {
            let _ = rec.start_replay();
        }
        while let Ok(event) = rx_hotkey.recv() {
            match event {
                HotkeyEvent::SaveReplay => {
                    if let Ok(rec) = recorder_clone.lock() {
                        let _ = rec.save_replay();
                    }
                }
                _ => {}
            }
        }
    });

    ipc::start_server(socket_path, Arc::clone(&recorder), Arc::clone(&hotkey_manager))?;

    tray::setup_and_run(Arc::clone(&recorder))?;

    Ok(())
}
