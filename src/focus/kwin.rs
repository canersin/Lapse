//! The focused window on KDE Plasma (Wayland): a small script in KWin reports it over D-Bus.

use super::{Report, Source, Window};
use std::time::Duration;

const SERVICE: &str = "io.github.canersin.Lapse.Focus";
const SCRIPT_NAME: &str = "lapse-focus";
const SCRIPT: &str = r#"
function report(w) {
    callDBus("io.github.canersin.Lapse.Focus", "/", "io.github.canersin.Lapse.Focus", "Focused",
        w ? String(w.pid) : "", w ? String(w.resourceClass) : "", w ? String(w.caption) : "", w && w.output ? String(w.output.name) : "");
}
workspace.windowActivated.connect(report);
report(workspace.activeWindow);
"#;

struct Listener;

#[zbus::interface(name = "io.github.canersin.Lapse.Focus")]
impl Listener {
    /// `pid` is empty when no window has the focus.
    fn focused(&self, pid: String, class: String, title: String, output: String) {
        let report = if pid.is_empty() {
            Report::Nothing
        } else {
            let monitor = (!output.is_empty()).then_some(output);
            Report::Window(Window { pid: pid.parse().ok().filter(|p| *p > 0), class, title, monitor, x11: None })
        };
        super::report(Source::Compositor, Some(report));
    }
}

pub fn run() {
    let kde = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').any(|d| d.eq_ignore_ascii_case("kde"));
    if !kde || std::env::var_os("WAYLAND_DISPLAY").is_none_or(|d| d.is_empty()) {
        return;
    }
    loop {
        if let Err(e) = watch() {
            eprintln!("Lapse: KWin focus watcher: {e}");
        }
        super::report(Source::Compositor, None);
        std::thread::sleep(Duration::from_secs(10));
    }
}

fn watch() -> anyhow::Result<()> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let file = dir.join("lapse-focus.js");
    std::fs::write(&file, SCRIPT)?;
    let conn = zbus::blocking::connection::Builder::session()?.name(SERVICE)?.serve_at("/", Listener)?.build()?;
    let scripting = zbus::blocking::Proxy::new(&conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting")?;
    let load = || -> zbus::Result<()> {
        let _: bool = scripting.call("unloadScript", &(SCRIPT_NAME,))?;
        let _: i32 = scripting.call("loadScript", &(file.to_string_lossy().as_ref(), SCRIPT_NAME))?;
        scripting.call("start", &())
    };
    load()?;
    loop {
        std::thread::sleep(Duration::from_secs(10));
        // KWin restarts drop it.
        let loaded: bool = scripting.call("isScriptLoaded", &(SCRIPT_NAME,))?;
        if !loaded {
            load()?;
        }
    }
}
