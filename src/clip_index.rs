//! Background index of saved clips.
//!
//! Scanning the clip folder, probing durations with ffprobe and generating thumbnails
//! all happen on one worker thread, so the UI only ever copies a ready-made list.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "webm", "mov"];

#[derive(Clone, Debug)]
pub struct ClipMetadata {
    pub path: PathBuf,
    pub name: String,
    pub size_mb: f64,
    pub thumb_path: PathBuf,
    pub game_tag: String,
    pub created_str: String,
    pub duration_str: String,
    pub duration_secs: f64,
    pub modified_time: SystemTime,
}

/// Probe results cached on disk so durations are not re-read on every launch.
#[derive(Serialize, Deserialize, Clone, Default)]
struct ProbeCache {
    /// path -> (size, mtime secs, duration secs)
    entries: HashMap<String, (u64, u64, f64)>,
}

struct Shared {
    clips: Vec<ClipMetadata>,
    generation: u64,
    save_path: PathBuf,
}

pub struct ClipIndex {
    shared: Arc<Mutex<Shared>>,
    wake: Mutex<Sender<()>>,
}

static INDEX: OnceLock<ClipIndex> = OnceLock::new();

pub fn index() -> &'static ClipIndex {
    INDEX.get_or_init(|| {
        let shared = Arc::new(Mutex::new(Shared {
            clips: Vec::new(),
            generation: 0,
            save_path: PathBuf::new(),
        }));
        let (tx, rx) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("lapse-clip-index".into())
            .spawn(move || worker(worker_shared, rx))
            .ok();
        ClipIndex { shared, wake: Mutex::new(tx) }
    })
}

impl ClipIndex {
    /// Returns the clip list if it changed since `seen_generation`.
    pub fn poll(&self, save_path: &Path, seen_generation: u64) -> Option<(u64, Vec<ClipMetadata>)> {
        let mut s = self.shared.lock().ok()?;
        if s.save_path != save_path {
            s.save_path = save_path.to_path_buf();
            drop(s);
            self.refresh();
            return None;
        }
        (s.generation != seen_generation).then(|| (s.generation, s.clips.clone()))
    }

    /// Average total bitrate (kbps) of the newest saved replays; see `average_kbps`.
    pub fn recent_average_kbps(&self) -> Option<f64> {
        let s = self.shared.lock().ok()?;
        let mut recent: Vec<&ClipMetadata> = s.clips.iter().filter(|c| is_recording(&c.name) && c.duration_secs >= 30.0).collect();
        recent.sort_by(|a, b| b.modified_time.cmp(&a.modified_time));
        average_kbps(recent.iter().take(5).map(|c| (c.size_mb * 1024.0 * 1024.0, c.duration_secs)))
    }

    /// Asks the worker to rescan right away (after a save, delete or export).
    pub fn refresh(&self) {
        if let Ok(tx) = self.wake.lock() {
            let _ = tx.send(());
        }
    }
}

/// Replays and recordings saved by Lapse, as opposed to exports or other videos in the folder.
pub fn is_recording(name: &str) -> bool {
    name.starts_with("Replay_") || name.starts_with("record_")
}

/// Real bitrate of saved clips, from (bytes, seconds) pairs. With a bitrate limit the
/// encoder only spends what the scene needs, so this is usually well below the setting.
pub fn average_kbps(samples: impl Iterator<Item = (f64, f64)>) -> Option<f64> {
    let (bytes, secs) = samples.fold((0.0, 0.0), |(b, s), (bb, ss)| (b + bb, s + ss));
    (secs > 0.0).then(|| bytes * 8.0 / secs / 1000.0)
}

pub fn format_duration(secs: f64) -> String {
    if secs <= 0.0 {
        return "--:--".into();
    }
    let total = secs.round() as u64;
    if total >= 3600 {
        format!("{}:{:02}:{:02}", total / 3600, (total / 60) % 60, total % 60)
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    }
}

pub fn thumb_dir(save_path: &Path) -> PathBuf {
    save_path.join(".thumbnails")
}

pub fn thumb_path_for(save_path: &Path, video: &Path) -> PathBuf {
    let name = video.file_name().unwrap_or_default().to_string_lossy();
    thumb_dir(save_path).join(format!("{}.jpg", name))
}

/// Reads a video's duration with ffprobe. Returns 0.0 if it can't be read.
pub fn probe_duration(path: &Path) -> f64 {
    Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

pub fn generate_thumbnail(video: &Path, out: &Path) -> bool {
    if let Some(dir) = out.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Grab a frame one second in; the very first frame of a replay is often black.
    for seek in ["1", "0"] {
        let ok = Command::new("ffmpeg")
            .args(["-nostdin", "-loglevel", "error", "-ss", seek, "-i"])
            .arg(video)
            .args(["-vf", "scale=320:180:force_original_aspect_ratio=decrease,pad=320:180:-1:-1", "-frames:v", "1", "-update", "1", "-y"])
            .arg(out)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok && out.exists() {
            return true;
        }
    }
    false
}

fn worker(shared: Arc<Mutex<Shared>>, wake: Receiver<()>) {
    let mut cache_path = PathBuf::new();
    let mut cache = ProbeCache::default();
    let mut last_signature: Vec<(PathBuf, u64, u64)> = Vec::new();
    let mut failed_thumbs: Vec<PathBuf> = Vec::new();

    loop {
        let save_path = match shared.lock() {
            Ok(s) => s.save_path.clone(),
            Err(_) => return,
        };

        if !save_path.as_os_str().is_empty() {
            let new_cache_path = thumb_dir(&save_path).join("index.json");
            if new_cache_path != cache_path {
                cache_path = new_cache_path;
                cache = std::fs::read_to_string(&cache_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();
                last_signature.clear();
            }

            let files = list_videos(&save_path);
            let signature: Vec<(PathBuf, u64, u64)> = files
                .iter()
                .map(|(p, size, mtime)| (p.clone(), *size, secs(*mtime)))
                .collect();

            if signature != last_signature {
                // Publish immediately with whatever durations are cached...
                publish(&shared, build(&save_path, &files, &cache));

                // ...then probe the missing ones and publish again.
                let mut probed_any = false;
                for (path, size, mtime) in &files {
                    let key = path.to_string_lossy().to_string();
                    let fresh = cache
                        .entries
                        .get(&key)
                        .map(|(s, m, _)| *s == *size && *m == secs(*mtime))
                        .unwrap_or(false);
                    if !fresh {
                        let d = probe_duration(path);
                        cache.entries.insert(key, (*size, secs(*mtime), d));
                        probed_any = true;
                    }
                }
                let alive: std::collections::HashSet<String> =
                    files.iter().map(|(p, _, _)| p.to_string_lossy().to_string()).collect();
                cache.entries.retain(|k, _| alive.contains(k));
                if probed_any {
                    let _ = std::fs::create_dir_all(thumb_dir(&save_path));
                    if let Ok(json) = serde_json::to_string(&cache) {
                        let _ = std::fs::write(&cache_path, json);
                    }
                }

                // Thumbnails, one at a time, never twice for the same file.
                let mut made_thumb = false;
                for (path, _, _) in &files {
                    let t = thumb_path_for(&save_path, path);
                    if !t.exists() && !failed_thumbs.contains(path) {
                        if generate_thumbnail(path, &t) {
                            made_thumb = true;
                        } else {
                            failed_thumbs.push(path.clone());
                        }
                    }
                }

                if probed_any || made_thumb {
                    publish(&shared, build(&save_path, &files, &cache));
                }
                last_signature = signature;
            }
        }

        // Rescan every few seconds, or right away when asked.
        let _ = wake.recv_timeout(Duration::from_secs(4));
        while wake.try_recv().is_ok() {}
    }
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn publish(shared: &Arc<Mutex<Shared>>, clips: Vec<ClipMetadata>) {
    if let Ok(mut s) = shared.lock() {
        s.clips = clips;
        s.generation += 1;
    }
}

fn list_videos(save_path: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, u64, SystemTime)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if depth < 3 {
                    walk(&path, depth + 1, out);
                }
                continue;
            }
            let is_video = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false);
            // Skip files that are still being written by an in-progress export/recording.
            if is_video && meta.len() > 1024 {
                out.push((path, meta.len(), meta.modified().unwrap_or(UNIX_EPOCH)));
            }
        }
    }
    let mut out = Vec::new();
    walk(save_path, 0, &mut out);
    out
}

fn build(save_path: &Path, files: &[(PathBuf, u64, SystemTime)], cache: &ProbeCache) -> Vec<ClipMetadata> {
    let mut clips: Vec<ClipMetadata> = files
        .iter()
        .map(|(path, size, mtime)| {
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let game_tag = path
                .strip_prefix(save_path)
                .ok()
                .and_then(|rel| rel.parent())
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| "Gameplay".to_string());
            let duration_secs = cache
                .entries
                .get(&*path.to_string_lossy())
                .map(|(_, _, d)| *d)
                .unwrap_or(0.0);
            let datetime: chrono::DateTime<chrono::Local> = (*mtime).into();
            ClipMetadata {
                thumb_path: thumb_path_for(save_path, path),
                path: path.clone(),
                name,
                size_mb: *size as f64 / (1024.0 * 1024.0),
                game_tag,
                created_str: datetime.format("%d %b, %H:%M").to_string(),
                duration_str: format_duration(duration_secs),
                duration_secs,
                modified_time: *mtime,
            }
        })
        .collect();
    clips.sort_by(|a, b| b.modified_time.cmp(&a.modified_time).then_with(|| b.name.cmp(&a.name)));
    clips
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn average_bitrate_of_saved_clips() {
        // 1.7 GB over 601 s is ~22.6 Mbps, like a 50 Mbps-capped AV1 replay.
        let kbps = average_kbps([(1_696_220_503.0, 601.2)].into_iter()).unwrap();
        assert!((kbps - 22_571.0).abs() < 10.0);
        assert!(average_kbps(std::iter::empty()).is_none());
        assert!(is_recording("Replay_2026-11-15_22-31-57.mp4"));
        assert!(!is_recording("clip_trimmed.mp4"));
    }

    #[test]
    fn durations_format_like_a_player() {
        assert_eq!(format_duration(0.0), "--:--");
        assert_eq!(format_duration(20.36), "0:20");
        assert_eq!(format_duration(125.0), "2:05");
        assert_eq!(format_duration(3725.0), "1:02:05");
    }
}
