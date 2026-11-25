use crate::config::Config;
use crate::gui;
use crate::utils::load_icon;
use eframe::egui;
use std::sync::Arc;

pub fn run_gui_client() -> anyhow::Result<()> {
    if crate::gui::single_instance::raise_existing() {
        println!("Lapse is already open; brought its window to the front.");
        return Ok(());
    }
    #[cfg(windows)]
    if !crate::ipc::daemon_alive() {
        if let Ok(exe) = std::env::current_exe() {
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("--daemon")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x00000008;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
            if cmd.spawn().is_ok() {
                let start = std::time::Instant::now();
                while !crate::ipc::daemon_alive() {
                    if start.elapsed() >= std::time::Duration::from_secs(5) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    }
    #[cfg(not(windows))]
    if !crate::ipc::daemon_alive() {
        if crate::utils::spawn_daemon().is_ok() {
            // Wait for the daemon socket instead of a fixed sleep.
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if crate::ipc::daemon_alive() {
                    break;
                }
            }
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
            .with_title("Lapse - Replay Studio")
            // Wayland app id and X11 WM_CLASS: docks and task bars find the desktop file
            // by it (lapse.desktop, or com.canersin.Lapse.desktop in the Flatpak).
            .with_app_id(std::env::var("FLATPAK_ID").unwrap_or_else(|_| "lapse".into()))
            .with_icon(icon_data)
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([860.0, 560.0])
            .with_transparent(false),
        vsync: false,
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
