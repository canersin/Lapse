//! Moments marked while playing: with the marker hotkey, and kills the game reports
//! (auto clip games). They are stored next to the thumbnails as `<clip name>.markers.json`
//! and shown on the trimmer timeline.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Where a mark came from: the marker hotkey, or a kill reported by the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkKind {
    #[default]
    Marker,
    Kill,
}

/// A mark in a clip, `t` seconds from its start.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    pub t: f64,
    #[serde(default)]
    pub kind: MarkKind,
}

/// Older files hold bare numbers (all hotkey markers).
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    Seconds(f64),
    Mark(Mark),
}

impl From<Stored> for Mark {
    fn from(s: Stored) -> Self {
        match s {
            Stored::Seconds(t) => Mark { t, kind: MarkKind::Marker },
            Stored::Mark(m) => m,
        }
    }
}

fn path_for(save_path: &Path, video: &Path) -> PathBuf {
    let name = video.file_name().unwrap_or_default().to_string_lossy();
    crate::clip_index::thumb_dir(save_path).join(format!("{}.markers.json", name))
}

/// Marks of a clip in the clip folder or in a game folder inside it.
pub fn load_for(video: &Path) -> Vec<Mark> {
    let candidates = [video.parent(), video.parent().and_then(|p| p.parent())];
    candidates
        .into_iter()
        .flatten()
        .find_map(|dir| std::fs::read(path_for(dir, video)).ok())
        .and_then(|b| serde_json::from_slice::<Vec<Stored>>(&b).ok())
        .map(|v| v.into_iter().map(Mark::from).collect())
        .unwrap_or_default()
}

pub fn save(save_path: &Path, video: &Path, marks: &[Mark]) {
    let path = path_for(save_path, video);
    if marks.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec(marks) {
        let _ = std::fs::write(path, json);
    }
}

fn sorted(mut marks: Vec<Mark>) -> Vec<Mark> {
    marks.sort_by(|a, b| a.t.total_cmp(&b.t));
    marks
}

/// The marks that fall inside a clip of `duration` seconds that ended at `end`.
pub fn offsets_in_clip(marks: &[(SystemTime, MarkKind)], end: SystemTime, duration: f64) -> Vec<Mark> {
    sorted(
        marks
            .iter()
            .filter_map(|(m, kind)| end.duration_since(*m).ok().map(|ago| Mark { t: duration - ago.as_secs_f64(), kind: *kind }))
            .filter(|m| (0.0..=duration).contains(&m.t))
            .collect(),
    )
}

/// The marks made since a recording started at `start`.
pub fn offsets_since(marks: &[(SystemTime, MarkKind)], start: SystemTime, duration: f64) -> Vec<Mark> {
    sorted(
        marks
            .iter()
            .filter_map(|(m, kind)| m.duration_since(start).ok().map(|d| Mark { t: d.as_secs_f64(), kind: *kind }))
            .filter(|m| m.t <= duration)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn marks_map_to_clip_positions() {
        let end = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let marks = [
            (end - Duration::from_secs(5), MarkKind::Kill),
            (end - Duration::from_secs(50), MarkKind::Marker),
            (end - Duration::from_secs(400), MarkKind::Marker),
        ];
        let t = |v: Vec<Mark>| v.iter().map(|m| m.t).collect::<Vec<_>>();
        let in_clip = offsets_in_clip(&marks, end, 60.0);
        assert_eq!(t(in_clip.clone()), vec![10.0, 55.0]);
        assert_eq!(in_clip[1].kind, MarkKind::Kill);
        let start = end - Duration::from_secs(100);
        assert_eq!(t(offsets_since(&marks, start, 100.0)), vec![50.0, 95.0]);
    }

    #[test]
    fn old_files_still_load() {
        let old: Vec<Stored> = serde_json::from_str("[1.5, {\"t\": 3.0, \"kind\": \"kill\"}]").unwrap();
        let marks: Vec<Mark> = old.into_iter().map(Mark::from).collect();
        assert_eq!(marks, vec![Mark { t: 1.5, kind: MarkKind::Marker }, Mark { t: 3.0, kind: MarkKind::Kill }]);
    }
}
