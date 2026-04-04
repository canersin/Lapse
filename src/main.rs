mod audio;
mod config;
mod gui;
mod hotkeys;
mod ipc;
mod recorder;

use crate::config::Config;
use crate::hotkeys::{HotkeyEvent, start_listener};
use crate::recorder::{Recorder, RecordingMode};
use eframe::egui;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

fn load_icon() -> (Vec<u8>, u32, u32) {
    let mut image = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .unwrap()
        .into_rgba8();
    let (width, height) = image.dimensions();

    // Crop to square if not square
    let size = width.min(height);
    let x = (width - size) / 2;
    let y = (height - size) / 2;

    let cropped = image::imageops::crop(&mut image, x, y, size, size).to_image();
    (cropped.into_raw(), size, size)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--gui") {
        return run_gui_client();
    }

    // Default to Daemon mode
    run_daemon()
}

fn run_daemon() -> anyhow::Result<()> {
    let config = Config::load()?;
    let recorder = Arc::new(Mutex::new(Recorder::new(config.clone())));

    // Cleanup old socket
    let socket_path = "/tmp/lapse.sock";
    let _ = std::fs::remove_file(socket_path);

    // Set up hotkey listener
    let (tx_hotkey, rx_hotkey) = mpsc::channel();
    start_listener(
        tx_hotkey,
        config.hotkey_replay.clone(),
        config.hotkey_record.clone(),
    );

    // Spawn recorder manager thread
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

    // Spawn IPC Listener thread
    let recorder_ipc = Arc::clone(&recorder);
    let listener = UnixListener::bind(socket_path)?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let recorder = Arc::clone(&recorder_ipc);
                std::thread::spawn(move || {
                    let mut buffer = [0; 1024];
                    if let Ok(n) = stream.read(&mut buffer) {
                        let cmd_str = String::from_utf8_lossy(&buffer[..n]);
                        if let Ok(cmd) = serde_json::from_str::<ipc::Command>(&cmd_str) {
                            let response = match cmd {
                                ipc::Command::GetStatus => {
                                    if let Ok(rec) = recorder.lock() {
                                        ipc::Response::Status(ipc::StatusResponse {
                                            recording: rec.current_mode() != RecordingMode::None,
                                            mode: format!("{:?}", rec.current_mode()),
                                            is_installed: rec.is_installed(),
                                        })
                                    } else {
                                        ipc::Response::Error("Lock failed".into())
                                    }
                                }
                                ipc::Command::StartReplay => {
                                    if let Ok(mut rec) = recorder.lock() {
                                        let _ = rec.start_replay();
                                        ipc::Response::Ok
                                    } else {
                                        ipc::Response::Error("Lock failed".into())
                                    }
                                }
                                ipc::Command::SaveReplay => {
                                    if let Ok(rec) = recorder.lock() {
                                        let _ = rec.save_replay();
                                        ipc::Response::Ok
                                    } else {
                                        ipc::Response::Error("Lock failed".into())
                                    }
                                }
                                ipc::Command::StartRecording => {
                                    if let Ok(mut rec) = recorder.lock() {
                                        let _ = rec.start_recording();
                                        ipc::Response::Ok
                                    } else {
                                        ipc::Response::Error("Lock failed".into())
                                    }
                                }
                                ipc::Command::Stop => {
                                    if let Ok(mut rec) = recorder.lock() {
                                        let _ = rec.stop();
                                        ipc::Response::Ok
                                    } else {
                                        ipc::Response::Error("Lock failed".into())
                                    }
                                }
                            };
                            let _ = stream
                                .write_all(serde_json::to_string(&response).unwrap().as_bytes());
                        }
                    }
                });
            }
        }
    });

    // Tray Setup logic
    #[cfg(target_os = "linux")]
    let _ = gtk::init().expect("Failed to initialize GTK");

    use muda::{Menu, MenuItem, PredefinedMenuItem};
    use tray_icon::TrayIconBuilder;
    let tray_menu = Menu::new();
    let show_i = MenuItem::with_id("show", "Show Lapse", true, None);
    let quit_i = MenuItem::with_id("quit", "Quit Lapse", true, None);
    let _ = tray_menu.append_items(&[&show_i, &PredefinedMenuItem::separator(), &quit_i]);

    let (icon_rgba, icon_width, icon_height) = load_icon();
    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Lapse")
        .with_icon(tray_icon::Icon::from_rgba(icon_rgba, icon_width, icon_height).unwrap())
        .build()?;

    let rx_tray = tray_icon::menu::MenuEvent::receiver();
    let recorder_quit = Arc::clone(&recorder);
    std::thread::spawn(move || {
        while let Ok(event) = rx_tray.recv() {
            if event.id == "quit" {
                if let Ok(mut rec) = recorder_quit.lock() {
                    let _ = rec.stop();
                }
                std::process::exit(0);
            } else if event.id == "show" {
                let _ = std::process::Command::new(std::env::current_exe().unwrap())
                    .arg("--gui")
                    .spawn();
            }
        }
    });

    #[cfg(target_os = "linux")]
    gtk::main();
    Ok(())
}

fn run_gui_client() -> anyhow::Result<()> {
    // Check if daemon is running. If not, autostart it.
    if std::os::unix::net::UnixStream::connect("/tmp/lapse.sock").is_err() {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe)
                .arg("--daemon")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .stdin(std::process::Stdio::null())
                .spawn();
            std::thread::sleep(std::time::Duration::from_millis(500)); // wait for socket to bind
        }
    }

    let config = Config::load()?;
    let icon_data = {
        let (rgba, width, height) = load_icon();
        Arc::new(eframe::egui::IconData {
            rgba,
            width,
            height,
        })
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lapse")
            .with_icon(icon_data)
            .with_inner_size([500.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Lapse",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Box::new(gui::LapseApp::new(cc, config))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Eframe error: {}", e))
}
