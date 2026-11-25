//! One Lapse window at a time. The running window listens on a socket in
//! XDG_RUNTIME_DIR (or localhost TCP on Windows); a second `lapse --gui` (tray "Show", app menu, `lapse gui`)
//! only asks it to come to the front and exits. Quitting Lapse from the tray
//! asks it to close.

use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(unix)]
fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("lapse-gui.sock")
}

#[cfg(windows)]
fn port_path() -> PathBuf {
    if let Some(proj) = directories::ProjectDirs::from("com", "lapse", "lapse") {
        let dir = proj.config_dir();
        let _ = std::fs::create_dir_all(dir);
        return dir.join("gui.port");
    }
    std::env::temp_dir().join("lapse-gui.port")
}

#[cfg(windows)]
const DEFAULT_GUI_PORT: u16 = 41821;

#[cfg(windows)]
fn get_gui_port() -> u16 {
    let path = port_path();
    if let Ok(s) = std::fs::read_to_string(&path) {
        if let Ok(port) = s.trim().parse::<u16>() {
            return port;
        }
    }
    let _ = std::fs::write(&path, DEFAULT_GUI_PORT.to_string());
    DEFAULT_GUI_PORT
}

/// What other processes asked the open window to do.
#[derive(Default)]
pub struct Requests {
    pub raise: AtomicBool,
    pub quit: AtomicBool,
}

#[cfg(unix)]
fn send(message: &[u8]) -> bool {
    match UnixStream::connect(socket_path()) {
        Ok(mut s) => {
            let _ = s.write_all(message);
            true
        }
        Err(_) => false,
    }
}

#[cfg(windows)]
fn send(message: &[u8]) -> bool {
    use std::net::TcpStream;
    let port = get_gui_port();
    match TcpStream::connect(("127.0.0.1", port)) {
        Ok(mut s) => {
            let _ = s.write_all(message);
            true
        }
        Err(_) => false,
    }
}

/// True when a window is already open (and was asked to show itself).
pub fn raise_existing() -> bool {
    send(b"show\n")
}

/// Closes the open window, if any (Lapse is quitting).
#[allow(dead_code)]
pub fn close_window() {
    send(b"quit\n");
}

/// Starts listening for "show" and "quit" from other processes.
#[cfg(unix)]
pub fn listen(ctx: eframe::egui::Context) -> Arc<Requests> {
    let requests = Arc::new(Requests::default());
    let path = socket_path();
    // Nothing answered in `raise_existing`, so a leftover file is stale.
    let _ = std::fs::remove_file(&path);
    if let Ok(listener) = UnixListener::bind(&path) {
        let requests = Arc::clone(&requests);
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = BufReader::new(conn).read_line(&mut line);
                let flag = if line.trim() == "quit" { &requests.quit } else { &requests.raise };
                flag.store(true, Ordering::SeqCst);
                ctx.request_repaint();
            }
        });
    }
    requests
}

#[cfg(windows)]
pub fn listen(ctx: eframe::egui::Context) -> Arc<Requests> {
    use std::net::TcpListener;
    let requests = Arc::new(Requests::default());
    let port = get_gui_port();
    if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
        let requests = Arc::clone(&requests);
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = BufReader::new(conn).read_line(&mut line);
                let flag = if line.trim() == "quit" { &requests.quit } else { &requests.raise };
                flag.store(true, Ordering::SeqCst);
                ctx.request_repaint();
            }
        });
    }
    requests
}

/// Removes the socket when the window closes, so the next launch starts fresh.
pub fn cleanup() {
    #[cfg(unix)]
    let _ = std::fs::remove_file(socket_path());
    #[cfg(windows)]
    let _ = std::fs::remove_file(port_path());
}
