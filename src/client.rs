use crate::config::Config;
use crate::gui;
use crate::utils::load_icon;
use eframe::egui;
use std::sync::Arc;

pub fn run_gui_client() -> anyhow::Result<()> {
    if std::os::unix::net::UnixStream::connect("/tmp/lapse.sock").is_err() {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe)
                .arg("--daemon")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .stdin(std::process::Stdio::null())
                .spawn();
            std::thread::sleep(std::time::Duration::from_millis(500));
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
