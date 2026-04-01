mod config;
mod recorder;
mod hotkeys;
mod gui;
mod audio;

use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use crate::config::Config;
use crate::recorder::Recorder;
use crate::hotkeys::{start_listener, HotkeyEvent};
use eframe::egui;

fn load_icon() -> (Vec<u8>, u32, u32) {
    let (width, height) = (32, 32);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let cx = x as f32 - 16.0;
            let cy = y as f32 - 16.0;
            let dist = (cx * cx + cy * cy).sqrt();
            if dist < 6.0 {
                rgba.extend_from_slice(&[255, 60, 80, 255]); // Vibrant red dot
            } else if dist < 12.0 {
                rgba.extend_from_slice(&[45, 45, 50, 255]); // Dark ring
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]); // Transparent
            }
        }
    }
    (rgba, width, height)
}

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

    // Start GUI (options declared later)
    // We need to setup tray logic. 
    // eframe uses winit on the main thread. To avoid Wayland/X11 GTK conflicts,
    // we initialize GTK and the TrayIcon in a separate background thread!
    std::thread::spawn(|| {
        #[cfg(target_os = "linux")]
        let _ = gtk::init().expect("Failed to initialize GTK on background thread");
        
        use muda::{Menu, MenuItem, PredefinedMenuItem};
        use tray_icon::TrayIconBuilder;
        
        let tray_menu = Menu::new();
        let show_i = MenuItem::with_id("show", "Show Lapse", true, None);
        let start_replay_i = MenuItem::with_id("start_replay", "Start Replay Buffer", true, None);
        let save_replay_i = MenuItem::with_id("save_replay", "Save Replay Clip", true, None);
        let start_record_i = MenuItem::with_id("start_record", "Start Recording", true, None);
        let stop_record_i = MenuItem::with_id("stop_record", "Stop Recording", true, None);
        let quit_i = MenuItem::with_id("quit", "Quit Lapse", true, None);
        
        let _ = tray_menu.append_items(&[
            &show_i,
            &PredefinedMenuItem::separator(),
            &start_replay_i,
            &save_replay_i,
            &PredefinedMenuItem::separator(),
            &start_record_i,
            &stop_record_i,
            &PredefinedMenuItem::separator(),
            &quit_i
        ]);
        
        let (icon_rgba, icon_width, icon_height) = load_icon();
        let _tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("Lapse")
            .with_icon(tray_icon::Icon::from_rgba(icon_rgba.clone(), icon_width, icon_height).unwrap())
            .build()
            .unwrap();
            
        #[cfg(target_os = "linux")]
        gtk::main(); // Pump the glib DBus event loop forever so the tray icon stays alive
    });

    let show_id = muda::MenuId::new("show");
    let quit_id = muda::MenuId::new("quit");
    let start_replay_id = muda::MenuId::new("start_replay");
    let save_replay_id = muda::MenuId::new("save_replay");
    let start_record_id = muda::MenuId::new("start_record");
    let stop_record_id = muda::MenuId::new("stop_record");
    
    let show_gui = Arc::new(Mutex::new(true));
    let show_gui_app = Arc::clone(&show_gui);

    let (tx_show, rx_show) = std::sync::mpsc::channel();
    
    let eframe_icon = {
        let (rgba, width, height) = load_icon();
        Arc::new(eframe::egui::IconData { rgba, width, height })
    };

    let recorder_tray = Arc::clone(&recorder);
    std::thread::spawn(move || {
        let rx = tray_icon::menu::MenuEvent::receiver();
        while let Ok(event) = rx.recv() {
            if event.id == quit_id {
                if let Ok(mut rec) = recorder_tray.lock() {
                    let _ = rec.stop();
                }
                std::process::exit(0);
            } else if event.id == show_id {
                if let Ok(mut sg) = show_gui.lock() { *sg = true; }
                let _ = tx_show.send(());
            } else if event.id == start_replay_id {
                if let Ok(mut rec) = recorder_tray.lock() { let _ = rec.start_replay(); }
            } else if event.id == save_replay_id {
                if let Ok(rec) = recorder_tray.lock() { let _ = rec.save_replay(); }
            } else if event.id == start_record_id {
                if let Ok(mut rec) = recorder_tray.lock() { let _ = rec.start_recording(); }
            } else if event.id == stop_record_id {
                if let Ok(mut rec) = recorder_tray.lock() { let _ = rec.stop(); }
            }
        }
    });

    // Invisible Daemon Root Window Frame
    let options = eframe::NativeOptions {
        run_and_return: false,
        viewport: egui::ViewportBuilder::default()
            .with_visible(false)
            .with_transparent(true)
            .with_decorations(false)
            .with_close_button(false)
            .with_minimize_button(false)
            .with_maximize_button(false)
            .with_inner_size([1.0, 1.0])
            .with_position([0.0, 0.0])
            .with_active(false)
            .with_window_level(egui::WindowLevel::AlwaysOnBottom),
        ..Default::default()
    };

    eframe::run_native(
        "Lapse Daemon",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            
            // Wakeup thread to force repaints when Tray show is clicked
            let ctx_wakeup = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                while let Ok(_) = rx_show.recv() {
                    ctx_wakeup.request_repaint();
                }
            });

            Box::new(gui::LapseApp::new(cc, config, recorder, show_gui_app, eframe_icon))
        })
    ).map_err(|e| anyhow::anyhow!("Eframe error: {}", e))?;

    Ok(())
}
