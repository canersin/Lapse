mod config;
mod recorder;
mod hotkeys;
mod gui;

use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use crate::config::Config;
use crate::recorder::Recorder;
use crate::hotkeys::{start_listener, HotkeyEvent};
use eframe::egui;

fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    let recorder = Arc::new(Mutex::new(Recorder::new(config.clone())));
    
    // Set up hotkey listener
    let (tx, rx) = mpsc::channel();
    start_listener(tx, config.hotkey_replay.clone(), config.hotkey_record.clone());

    // Spawn recorder manager thread
    let recorder_clone = Arc::clone(&recorder);
    std::thread::spawn(move || {
        // Start replay mode by default
        if let Ok(mut rec) = recorder_clone.lock() {
            let _ = rec.start_replay();
        }

        while let Ok(event) = rx.recv() {
            match event {
                HotkeyEvent::SaveReplay => {
                    if let Ok(rec) = recorder_clone.lock() {
                        let _ = rec.save_replay();
                    }
                }
                HotkeyEvent::ToggleRecord => {
                    // TODO: Implement toggle record
                }
            }
        }
    });

    // Start GUI
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Lapse",
        options,
        Box::new(|cc| Box::new(gui::LapseApp::new(cc, config, recorder))),
    ).map_err(|e| anyhow::anyhow!("Eframe error: {}", e))?;

    Ok(())
}
