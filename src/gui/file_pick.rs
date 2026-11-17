//! File dialogs on a background thread. A dialog run on the UI thread stops the
//! frame loop, and GNOME then offers to force quit Lapse ("not responding").

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use eframe::egui;

enum Slot {
    Open,
    Done(Option<PathBuf>),
}

fn slots() -> &'static Mutex<HashMap<&'static str, Slot>> {
    static SLOTS: OnceLock<Mutex<HashMap<&'static str, Slot>>> = OnceLock::new();
    SLOTS.get_or_init(Mutex::default)
}

/// Shows `dialog` (an rfd call) without blocking; the answer comes from `take(id)`.
/// Does nothing while a dialog with this id is already open.
pub fn open(ctx: &egui::Context, id: &'static str, dialog: impl FnOnce() -> Option<PathBuf> + Send + 'static) {
    let mut map = slots().lock().unwrap_or_else(|e| e.into_inner());
    if matches!(map.get(id), Some(Slot::Open)) {
        return;
    }
    map.insert(id, Slot::Open);
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let picked = dialog();
        slots().lock().unwrap_or_else(|e| e.into_inner()).insert(id, Slot::Done(picked));
        ctx.request_repaint();
    });
}

/// True while the dialog is on screen, e.g. to disable its button.
pub fn is_open(id: &'static str) -> bool {
    matches!(slots().lock().unwrap_or_else(|e| e.into_inner()).get(id), Some(Slot::Open))
}

/// The chosen path, once, after the dialog closed with a choice.
pub fn take(id: &'static str) -> Option<PathBuf> {
    let mut map = slots().lock().unwrap_or_else(|e| e.into_inner());
    match map.get(id) {
        Some(Slot::Done(_)) => match map.remove(id) {
            Some(Slot::Done(path)) => path,
            _ => None,
        },
        _ => None,
    }
}
