//! The focused window as an X server (or XWayland) sees it. On Wayland it only knows X11
//! windows, so "nothing" also means that a Wayland window has the focus.

use super::{Report, Source, Window};
use std::os::fd::AsRawFd;
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, MapState};
use x11rb::rust_connection::RustConnection;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        _NET_ACTIVE_WINDOW,
        _NET_WM_PID,
        _NET_WM_NAME,
        _NET_WM_STATE,
        _NET_WM_STATE_HIDDEN,
        UTF8_STRING,
        GAMESCOPE_FOCUSED_APP,
    }
}

/// Steam's own interface in gamescope's Steam mode.
const STEAM_APP: u32 = 769;

pub fn run() {
    loop {
        if let Err(e) = watch() {
            super::set_monitor_count(Source::X11, 0);
            super::report(Source::X11, None);
            if !matches!(e, Error::NoServer) {
                eprintln!("Lapse: X11 focus watcher: {e:?}");
            }
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}

#[derive(Debug)]
enum Error {
    NoServer,
    #[allow(dead_code)]
    Lost(String),
}

impl<E: std::error::Error> From<E> for Error {
    fn from(e: E) -> Self {
        Error::Lost(e.to_string())
    }
}

fn watch() -> Result<(), Error> {
    if std::env::var_os("DISPLAY").is_none_or(|d| d.is_empty()) {
        return Err(Error::NoServer);
    }
    let (conn, screen) = x11rb::connect(None).map_err(|_| Error::NoServer)?;
    let root = conn.setup().roots[screen].root;
    let atoms = Atoms::new(&conn)?.reply()?;
    conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE))?;
    conn.flush()?;

    let mut last = None;
    loop {
        let found = focused(&conn, root, &atoms)?;
        if last.as_ref() != Some(&found) {
            let count = conn.randr_get_monitors(root, true)?.reply().map(|r| r.monitors.len()).unwrap_or(0);
            super::set_monitor_count(Source::X11, count);
            super::report(Source::X11, Some(found.clone()));
            last = Some(found);
        }
        super::set_game_hidden(super::game_window().map(|w| hidden(&conn, w, &atoms)).unwrap_or(false));
        conn.flush()?;
        // The focus itself has no event on the root window; not every window manager sets
        // the property, so it is also asked for regularly.
        let mut fd = libc::pollfd { fd: conn.stream().as_raw_fd(), events: libc::POLLIN, revents: 0 };
        unsafe { libc::poll(&mut fd, 1, 50) };
        while conn.poll_for_event()?.is_some() {}
    }
}

fn card32(conn: &RustConnection, window: u32, property: u32) -> Result<Option<u32>, Error> {
    let reply = conn.get_property(false, window, property, AtomEnum::ANY, 0, 1)?.reply();
    Ok(reply.ok().and_then(|r| r.value32().and_then(|mut v| v.next())))
}

fn text(conn: &RustConnection, window: u32, property: u32, kind: u32) -> Vec<u8> {
    conn.get_property(false, window, property, kind, 0, 256).ok().and_then(|c| c.reply().ok()).map(|r| r.value).unwrap_or_default()
}

fn focused(conn: &RustConnection, root: u32, atoms: &Atoms) -> Result<Report, Error> {
    if let Some(app) = card32(conn, root, atoms.GAMESCOPE_FOCUSED_APP)? {
        let class = if app == STEAM_APP { "steam".to_string() } else { format!("steam_app_{app}") };
        return Ok(Report::Window(Window { class, ..Default::default() }));
    }
    let focus = conn.get_input_focus()?.reply()?.focus;
    let active = card32(conn, root, atoms._NET_ACTIVE_WINDOW)?.unwrap_or(0);
    // The focus is the live truth but may sit on a child or frame; the property names the
    // window, but some window managers leave it stale.
    let mut window = focus;
    for _ in 0..6 {
        if window <= 1 || window == root {
            break;
        }
        if let Some(w) = describe(conn, root, window, atoms)? {
            return Ok(Report::Window(w));
        }
        match conn.query_tree(window)?.reply() {
            Ok(tree) if tree.parent != tree.root => window = tree.parent,
            _ => break,
        }
    }
    if active > 1 {
        if let Some(w) = describe(conn, root, active, atoms)? {
            return Ok(Report::Window(w));
        }
    }
    Ok(Report::Nothing)
}

fn describe(conn: &RustConnection, root: u32, window: u32, atoms: &Atoms) -> Result<Option<Window>, Error> {
    let class = text(conn, window, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into());
    let class = class.split(|&b| b == 0).filter(|p| !p.is_empty()).last().unwrap_or_default();
    let title = text(conn, window, atoms._NET_WM_NAME, atoms.UTF8_STRING);
    let pid = card32(conn, window, atoms._NET_WM_PID)?;
    if class.is_empty() && title.is_empty() && pid.is_none() {
        return Ok(None);
    }
    Ok(Some(Window {
        pid,
        class: String::from_utf8_lossy(class).into_owned(),
        title: String::from_utf8_lossy(&title).into_owned(),
        monitor: monitor(conn, root, window),
        x11: Some(window),
    }))
}

/// The monitor holding the middle of the window.
fn monitor(conn: &RustConnection, root: u32, window: u32) -> Option<String> {
    let size = conn.get_geometry(window).ok()?.reply().ok()?;
    let pos = conn.translate_coordinates(window, root, 0, 0).ok()?.reply().ok()?;
    let (x, y) = (pos.dst_x as i32 + size.width as i32 / 2, pos.dst_y as i32 + size.height as i32 / 2);
    let monitors = conn.randr_get_monitors(root, true).ok()?.reply().ok()?.monitors;
    let m = monitors.iter().find(|m| {
        (m.x as i32..m.x as i32 + m.width as i32).contains(&x) && (m.y as i32..m.y as i32 + m.height as i32).contains(&y)
    })?;
    let name = conn.get_atom_name(m.name).ok()?.reply().ok()?.name;
    Some(String::from_utf8_lossy(&name).into_owned())
}

/// Whether a window is minimized or on another workspace.
fn hidden(conn: &RustConnection, window: u32, atoms: &Atoms) -> bool {
    let Some(attributes) = conn.get_window_attributes(window).ok().and_then(|c| c.reply().ok()) else { return false };
    if attributes.map_state != MapState::VIEWABLE {
        return true;
    }
    conn.get_property(false, window, atoms._NET_WM_STATE, AtomEnum::ATOM, 0, 32)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().map(|mut v| v.any(|a| a == atoms._NET_WM_STATE_HIDDEN)))
        .unwrap_or(false)
}
