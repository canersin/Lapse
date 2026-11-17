//! Optional storage limit: deletes the oldest clips once the clip folder grows
//! past the configured size. Off by default; only touches video files inside the
//! configured save folder.

use crate::config::Config;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct CleanupResult {
    pub removed: usize,
    pub freed_bytes: u64,
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, u64, SystemTime)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let path = entry.path();
        if meta.is_dir() {
            if depth < 3 {
                collect(&path, depth + 1, out);
            }
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| crate::clip_index::VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
            .unwrap_or(false)
        {
            out.push((path, meta.len(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        }
    }
}

/// Returns the clips that would have to go to get under `max_bytes`, oldest first.
/// Starred clips count towards the total but are never picked.
pub fn plan(save_path: &Path, max_bytes: u64) -> Vec<(PathBuf, u64)> {
    let mut files = Vec::new();
    collect(save_path, 0, &mut files);
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.2);
    let favorites = crate::favorites::load(save_path);

    let mut victims = Vec::new();
    for (path, size, _) in files {
        if total <= max_bytes {
            break;
        }
        if crate::favorites::contains(&favorites, save_path, &path) {
            continue;
        }
        total = total.saturating_sub(size);
        victims.push((path, size));
    }
    victims
}

pub fn enforce(config: &Config) -> Option<CleanupResult> {
    if !config.cleanup_enabled || config.cleanup_max_gb == 0 {
        return None;
    }
    let max_bytes = config.cleanup_max_gb as u64 * 1024 * 1024 * 1024;
    let mut result = CleanupResult { removed: 0, freed_bytes: 0 };
    for (path, size) in plan(&config.save_path, max_bytes) {
        if std::fs::remove_file(&path).is_ok() {
            let _ = std::fs::remove_file(crate::clip_index::thumb_path_for(&config.save_path, &path));
            result.removed += 1;
            result.freed_bytes += size;
        }
    }
    (result.removed > 0).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_oldest_until_under_limit() {
        let dir = std::env::temp_dir().join(format!("lapse-cleanup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Game")).unwrap();
        for (i, name) in ["a.mp4", "Game/b.mp4", "c.mp4"].iter().enumerate() {
            let p = dir.join(name);
            std::fs::write(&p, vec![0u8; 1000]).unwrap();
            let t = std::fs::FileTimes::new()
                .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000 + i as u64));
            std::fs::File::options().write(true).open(&p).unwrap().set_times(t).unwrap();
        }
        std::fs::write(dir.join("notes.txt"), vec![0u8; 5000]).unwrap();

        let victims = plan(&dir, 1500);
        let names: Vec<_> = victims.iter().map(|v| v.0.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["a.mp4", "b.mp4"]);

        // A starred clip is skipped; the next oldest goes instead.
        crate::favorites::toggle(&dir, &dir.join("a.mp4"));
        let victims = plan(&dir, 1500);
        let names: Vec<_> = victims.iter().map(|v| v.0.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["b.mp4", "c.mp4"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
