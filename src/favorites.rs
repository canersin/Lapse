//! Starred clips. Stored next to the clip index as paths relative to the clip folder,
//! so they survive moving the whole folder. The storage limit never deletes them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn file(save_path: &Path) -> PathBuf {
    save_path.join(".thumbnails").join("favorites.json")
}

fn key(save_path: &Path, clip: &Path) -> String {
    clip.strip_prefix(save_path).unwrap_or(clip).to_string_lossy().to_string()
}

/// Reads the favorites straight from disk (used by the daemon's storage limit).
pub fn load(save_path: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(file(save_path))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn store(save_path: &Path, set: &BTreeSet<String>) {
    let path = file(save_path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(json) = serde_json::to_string_pretty(set) {
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

pub fn contains(set: &BTreeSet<String>, save_path: &Path, clip: &Path) -> bool {
    set.contains(&key(save_path, clip))
}

/// In-memory copy for the GUI, which asks every frame.
static CACHE: Mutex<Option<(PathBuf, BTreeSet<String>)>> = Mutex::new(None);

fn with_cache<R>(save_path: &Path, f: impl FnOnce(&mut BTreeSet<String>) -> R) -> R {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().map(|(p, _)| p != save_path).unwrap_or(true) {
        *guard = Some((save_path.to_path_buf(), load(save_path)));
    }
    f(&mut guard.as_mut().unwrap().1)
}

pub fn is_favorite(save_path: &Path, clip: &Path) -> bool {
    with_cache(save_path, |set| contains(set, save_path, clip))
}

/// Stars or unstars a clip and returns the new state.
pub fn toggle(save_path: &Path, clip: &Path) -> bool {
    with_cache(save_path, |set| {
        let k = key(save_path, clip);
        let now = if set.remove(&k) {
            false
        } else {
            set.insert(k);
            true
        };
        store(save_path, set);
        now
    })
}

/// Forgets a clip that was deleted.
pub fn forget(save_path: &Path, clip: &Path) {
    with_cache(save_path, |set| {
        if set.remove(&key(save_path, clip)) {
            store(save_path, set);
        }
    })
}
