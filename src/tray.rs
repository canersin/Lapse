use crate::recorder::Recorder;
use crate::utils::load_icon;
use std::sync::{Arc, Mutex};

pub fn setup_and_run(recorder: Arc<Mutex<Recorder>>) -> anyhow::Result<()> {
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
    std::thread::spawn(move || {
        while let Ok(event) = rx_tray.recv() {
            if event.id == "quit" {
                if let Ok(mut rec) = recorder.lock() {
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
