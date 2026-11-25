//! Per-track metadata for recorded audio tracks in a clip.
//! Stored next to the clip as `<clip name>.tracks.json` (or next to thumbnails),
//! containing an array of track identifier strings, e.g. ["game", "chat", "mic"].

use std::path::{Path, PathBuf};

pub const TRACK_GAME: &str = "game";
pub const TRACK_CHAT: &str = "chat";
pub const TRACK_MIC: &str = "mic";

/// Returns the path to the sidecar file next to the clip video file.
pub fn path_for_clip(video: &Path) -> PathBuf {
    let filename = video.file_name().unwrap_or_default().to_string_lossy();
    video.with_file_name(format!("{}.tracks.json", filename))
}

/// Fallback path in thumb directory (like markers).
pub fn path_for_thumb(save_path: &Path, video: &Path) -> PathBuf {
    let name = video.file_name().unwrap_or_default().to_string_lossy();
    crate::clip_index::thumb_dir(save_path).join(format!("{}.tracks.json", name))
}

/// Saves the track names for a clip next to the clip and in the thumbnails directory.
pub fn save(save_path: &Path, video: &Path, tracks: &[String]) {
    if tracks.is_empty() {
        return;
    }
    let clip_sidecar = path_for_clip(video);
    if let Ok(json) = serde_json::to_vec_pretty(tracks) {
        let _ = std::fs::write(&clip_sidecar, &json);
    }
    let thumb_sidecar = path_for_thumb(save_path, video);
    if let Some(parent) = thumb_sidecar.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec(tracks) {
        let _ = std::fs::write(&thumb_sidecar, &json);
    }
}

/// Loads track identifiers for a clip.
/// If no sidecar file exists, falls back to default track assignment based on track_count:
/// 0 => []
/// 1 => ["game"]
/// 2 => ["game", "mic"]
/// 3 => ["game", "chat", "mic"]
pub fn load_for(video: &Path, track_count: usize) -> Vec<String> {
    // 1. Try next to clip: <clip.mp4>.tracks.json
    let p1 = path_for_clip(video);
    if let Ok(data) = std::fs::read(&p1) {
        if let Ok(list) = serde_json::from_slice::<Vec<String>>(&data) {
            if !list.is_empty() {
                return list;
            }
        }
    }
    // 2. Try <clip>.tracks.json (without .mp4 extension)
    let p2 = video.with_extension("tracks.json");
    if p2 != p1 {
        if let Ok(data) = std::fs::read(&p2) {
            if let Ok(list) = serde_json::from_slice::<Vec<String>>(&data) {
                if !list.is_empty() {
                    return list;
                }
            }
        }
    }
    // 3. Try in thumb dir
    let candidates = [video.parent(), video.parent().and_then(|p| p.parent())];
    for dir in candidates.into_iter().flatten() {
        let p3 = path_for_thumb(dir, video);
        if let Ok(data) = std::fs::read(&p3) {
            if let Ok(list) = serde_json::from_slice::<Vec<String>>(&data) {
                if !list.is_empty() {
                    return list;
                }
            }
        }
    }

    // Default fallback based on track_count:
    default_tracks_for_count(track_count)
}

pub fn default_tracks_for_count(track_count: usize) -> Vec<String> {
    match track_count {
        0 => Vec::new(),
        1 => vec![TRACK_GAME.into()],
        2 => vec![TRACK_GAME.into(), TRACK_MIC.into()],
        _ => {
            let mut list = vec![TRACK_GAME.into(), TRACK_CHAT.into(), TRACK_MIC.into()];
            for i in 3..track_count {
                list.push(format!("track_{}", i));
            }
            list
        }
    }
}

/// User-facing label for a track kind.
pub fn label_for_track(kind: &str) -> &'static str {
    match kind {
        TRACK_GAME => "Game & desktop",
        TRACK_CHAT => "Chat & music",
        TRACK_MIC => "Microphone",
        _ => "Audio track",
    }
}

/// User-facing label for a track name, handling known kinds (translated) and "app:<name>".
pub fn track_label(name: &str) -> String {
    if let Some(app) = name.strip_prefix("app:") {
        #[cfg(not(windows))]
        if let Some(id) = crate::audio_apps::identity(app).filter(|i| !i.name.is_empty()) {
            return id.name;
        }
        let app = app.strip_suffix(".exe").or_else(|| app.strip_suffix(".EXE")).unwrap_or(app);
        let mut chars = app.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        }
    } else {
        crate::gui::i18n::tr_any(label_for_track(name)).to_string()
    }
}

/// For each audio track of `video`: true when it is an app track ("app:...") that stays
/// silent for the whole clip. An app on the list that never played (or isn't installed)
/// still gets a track; the trimmer hides those.
pub fn silent_app_tracks(video: &Path, tracks: &[String]) -> Vec<bool> {
    let mut silent = vec![false; tracks.len()];
    let apps: Vec<usize> = (0..tracks.len()).filter(|&i| tracks[i].starts_with("app:")).collect();
    if apps.is_empty() {
        return silent;
    }
    let graph: Vec<String> = apps.iter().enumerate().map(|(n, i)| format!("[0:a:{i}]volumedetect[o{n}]")).collect();
    let mut cmd = crate::utils::ffmpeg_cmd();
    cmd.args(["-nostdin", "-hide_banner", "-vn", "-i"]).arg(video).arg("-filter_complex").arg(graph.join(";"));
    for n in 0..apps.len() {
        cmd.args(["-map", &format!("[o{n}]"), "-f", "null", "-"]);
    }
    let Ok(out) = cmd.output() else { return silent };
    for (n, max) in parse_max_volumes(&String::from_utf8_lossy(&out.stderr)) {
        if let Some(&i) = apps.get(n) {
            // ffmpeg reports digital silence as -91 dB.
            silent[i] = max <= -90.0;
        }
    }
    silent
}

/// (filter index, max_volume dB) from ffmpeg volumedetect output.
fn parse_max_volumes(stderr: &str) -> Vec<(usize, f64)> {
    stderr
        .lines()
        .filter_map(|l| {
            let idx = l.split("Parsed_volumedetect_").nth(1)?.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
            let db = l.split("max_volume:").nth(1)?.trim().trim_end_matches("dB").trim().parse().ok()?;
            Some((idx, db))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_max_volumes() {
        let s = "[Parsed_volumedetect_0 @ 0x1] max_volume: -91.0 dB\n[Parsed_volumedetect_1 @ 0x2] mean_volume: -30.2 dB\n[Parsed_volumedetect_1 @ 0x2] max_volume: -0.8 dB";
        assert_eq!(parse_max_volumes(s), vec![(0, -91.0), (1, -0.8)]);
    }

    #[test]
    fn test_default_tracks_fallback() {
        assert_eq!(default_tracks_for_count(0), Vec::<String>::new());
        assert_eq!(default_tracks_for_count(1), vec!["game"]);
        assert_eq!(default_tracks_for_count(2), vec!["game", "mic"]);
        assert_eq!(default_tracks_for_count(3), vec!["game", "chat", "mic"]);
    }

    #[test]
    fn test_save_and_load_tracks() {
        let temp_dir = std::env::temp_dir().join("lapse_test_tracks");
        let _ = std::fs::create_dir_all(&temp_dir);
        let clip_path = temp_dir.join("test_clip.mp4");
        let tracks = vec!["game".to_string(), "chat".to_string(), "mic".to_string()];

        save(&temp_dir, &clip_path, &tracks);

        let loaded = load_for(&clip_path, 3);
        assert_eq!(loaded, tracks);

        // Cleanup
        let _ = std::fs::remove_file(path_for_clip(&clip_path));
        let _ = std::fs::remove_file(path_for_thumb(&temp_dir, &clip_path));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_track_labels() {
        assert_eq!(label_for_track(TRACK_GAME), "Game & desktop");
        assert_eq!(label_for_track(TRACK_CHAT), "Chat & music");
        assert_eq!(label_for_track(TRACK_MIC), "Microphone");
        assert_eq!(track_label(TRACK_GAME), "Game & desktop");
        assert_eq!(track_label(TRACK_CHAT), "Chat & music");
        assert_eq!(track_label(TRACK_MIC), "Microphone");
        assert_eq!(track_label("app:discord"), "Discord");
        assert_eq!(track_label("app:spotify"), "Spotify");
        assert_eq!(track_label("app:"), "");
        assert_eq!(track_label("other"), "Audio track");
    }
}
