//! One Lapse window at a time. The running window listens on a socket in
//! XDG_RUNTIME_DIR; a second `lapse --gui` (tray "Show", app menu, `lapse gui`)
//! only asks it to come to the front and exits. Quitting Lapse from the tray
//! asks it to close.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("lapse-gui.sock")
}

/// What other processes asked the open window to do.
#[derive(Default)]
pub struct Requests {
    pub raise: AtomicBool,
    pub quit: AtomicBool,
}

fn send(message: &[u8]) -> bool {
    match UnixStream::connect(socket_path()) {
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
pub fn close_window() {
    send(b"quit\n");
}

/// Starts listening for "show" and "quit" from other processes.
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

/// Removes the socket when the window closes, so the next launch starts fresh.
pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}
