//! Tray icon over StatusNotifierItem (D-Bus). The panel draws the icon by name
//! (`lapse-symbolic`), so it takes the panel's colours like the other tray icons;
//! a white pixmap is the fallback for panels that can't find the name.

use crate::recorder::Recorder;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const ICON_NAME: &str = "lapse-symbolic";
const SYMBOLIC_SVG: &[u8] = include_bytes!("../assets/lapse-symbolic.svg");
const FALLBACK_PNG: &[u8] = include_bytes!("../assets/tray.png");

struct LapseTray {
    recorder: Arc<Mutex<Recorder>>,
    icon_dir: Option<PathBuf>,
}

impl ksni::Tray for LapseTray {
    fn id(&self) -> String {
        "lapse".into()
    }

    fn title(&self) -> String {
        "Lapse".into()
    }

    fn icon_name(&self) -> String {
        ICON_NAME.into()
    }

    fn icon_theme_path(&self) -> String {
        self.icon_dir.as_ref().map(|d| d.to_string_lossy().to_string()).unwrap_or_default()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        fallback_pixmap().into_iter().collect()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "Lapse".into(), ..Default::default() }
    }

    /// Left click opens (or raises) the window.
    fn activate(&mut self, _x: i32, _y: i32) {
        show_window();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem};
        let item = |label: &str, activate: fn(&mut Self)| -> MenuItem<Self> {
            StandardItem { label: label.into(), activate: Box::new(activate), ..Default::default() }.into()
        };
        vec![
            item("Show Lapse", |_| show_window()),
            MenuItem::Separator,
            item("Save Replay Now", |t| {
                crate::daemon::handle_hotkey(&t.recorder, crate::hotkeys::HotkeyEvent::SaveReplay)
            }),
            item("Start / Stop Recording", |t| {
                crate::daemon::handle_hotkey(&t.recorder, crate::hotkeys::HotkeyEvent::ToggleRecord)
            }),
            item("Open Clips Folder", |_| {
                if let Ok(cfg) = crate::config::Config::load() {
                    let _ = open::that(&cfg.save_path);
                }
            }),
            MenuItem::Separator,
            item("Quit Lapse", |t| {
                if let Ok(mut rec) = t.recorder.lock() {
                    let _ = rec.stop();
                }
                let _ = std::fs::remove_file(crate::ipc::socket_path());
                crate::gui::single_instance::close_window();
                std::process::exit(0);
            }),
        ]
    }
}

/// Starts the GUI; a second window is refused there and raises the open one instead.
fn show_window() {
    let Ok(exe) = std::env::current_exe() else { return };
    if let Ok(mut child) = std::process::Command::new(exe).arg("--gui").spawn() {
        // Reap it so a closed window doesn't stay behind as a zombie of the daemon.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// Writes the symbolic icon where the panel can find it without an installed
/// icon theme: as a loose file and inside a hicolor layout.
fn install_icon() -> Option<PathBuf> {
    let dir = directories::ProjectDirs::from("com", "lapse", "lapse")?.data_dir().join("icons");
    let file = format!("{}.svg", ICON_NAME);
    for target in [dir.join(&file), dir.join("hicolor/scalable/apps").join(&file)] {
        std::fs::create_dir_all(target.parent()?).ok()?;
        if std::fs::read(&target).ok().as_deref() != Some(SYMBOLIC_SVG) {
            std::fs::write(&target, SYMBOLIC_SVG).ok()?;
        }
    }
    Some(dir)
}

/// The white PNG as ARGB32 in network byte order.
fn fallback_pixmap() -> Option<ksni::Icon> {
    let img = image::load_from_memory(FALLBACK_PNG).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    let data = img.pixels().flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
    Some(ksni::Icon { width: width as i32, height: height as i32, data })
}

pub fn setup_and_run(recorder: Arc<Mutex<Recorder>>) -> anyhow::Result<()> {
    use ksni::blocking::TrayMethods;
    let tray = LapseTray { recorder, icon_dir: install_icon() };
    // Flatpak can't own the well-known D-Bus name the spec asks for.
    let sandboxed = std::env::var_os("FLATPAK_ID").is_some();
    match tray.disable_dbus_name(sandboxed).spawn() {
        // The tray service runs on its own thread; keep the daemon's main thread alive.
        Ok(_handle) => loop {
            std::thread::park();
        },
        Err(e) => {
            eprintln!("Warning: no system tray ({}). Continuing without tray.", e);
            loop {
                std::thread::park();
            }
        }
    }
}
