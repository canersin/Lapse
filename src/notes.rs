//! Clip notes and tags, stored next to the clip index as paths relative to the clip folder.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipNotes {
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl ClipNotes {
    pub fn is_empty(&self) -> bool {
        self.note.is_empty() && self.tags.is_empty()
    }
}

fn file(save_path: &Path) -> PathBuf {
    save_path.join(".thumbnails").join("notes.json")
}

fn key(save_path: &Path, clip: &Path) -> String {
    clip.strip_prefix(save_path).unwrap_or(clip).to_string_lossy().to_string()
}

/// Normalises a tag: trimmed, lowercased, leading # removed, max 24 characters.
pub fn clean_tag(raw: &str) -> Option<String> {
    let s = raw.trim();
    let s = s.strip_prefix('#').unwrap_or(s).trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_lowercase();
    let truncated: String = lower.chars().take(24).collect();
    if truncated.is_empty() {
        None
    } else {
        Some(truncated)
    }
}

pub fn load(save_path: &Path) -> BTreeMap<String, ClipNotes> {
    std::fs::read_to_string(file(save_path))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn store(save_path: &Path, map: &BTreeMap<String, ClipNotes>) {
    let path = file(save_path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(json) = serde_json::to_string_pretty(map) {
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

static CACHE: Mutex<Option<(PathBuf, BTreeMap<String, ClipNotes>)>> = Mutex::new(None);

fn with_cache<R>(save_path: &Path, f: impl FnOnce(&mut BTreeMap<String, ClipNotes>) -> R) -> R {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().map(|(p, _)| p != save_path).unwrap_or(true) {
        *guard = Some((save_path.to_path_buf(), load(save_path)));
    }
    f(&mut guard.as_mut().unwrap().1)
}

/// Whether `clip` has a note or a tag in `map` (a fresh `load`, for the daemon,
/// whose cache would miss notes added in the window since).
pub fn has_any(map: &BTreeMap<String, ClipNotes>, save_path: &Path, clip: &Path) -> bool {
    map.get(&key(save_path, clip)).is_some_and(|n| !n.is_empty())
}

pub fn get(save_path: &Path, clip: &Path) -> ClipNotes {
    with_cache(save_path, |map| {
        map.get(&key(save_path, clip)).cloned().unwrap_or_default()
    })
}

pub fn set_note(save_path: &Path, clip: &Path, note: &str) {
    let note = note.trim().to_string();
    with_cache(save_path, |map| {
        let k = key(save_path, clip);
        if note.is_empty() {
            if let Some(entry) = map.get_mut(&k) {
                entry.note.clear();
                if entry.is_empty() {
                    map.remove(&k);
                }
                store(save_path, map);
            }
        } else {
            let entry = map.entry(k).or_default();
            entry.note = note;
            store(save_path, map);
        }
    })
}

pub fn add_tag(save_path: &Path, clip: &Path, tag: &str) -> bool {
    let Some(tag) = clean_tag(tag) else {
        return false;
    };
    with_cache(save_path, |map| {
        let k = key(save_path, clip);
        let entry = map.entry(k).or_default();
        if entry.tags.contains(&tag) {
            return false;
        }
        entry.tags.push(tag);
        store(save_path, map);
        true
    })
}

pub fn remove_tag(save_path: &Path, clip: &Path, tag: &str) -> bool {
    let norm = clean_tag(tag).unwrap_or_else(|| tag.trim().to_lowercase());
    with_cache(save_path, |map| {
        let k = key(save_path, clip);
        let Some(entry) = map.get_mut(&k) else {
            return false;
        };
        let before_len = entry.tags.len();
        entry.tags.retain(|t| t != &norm && t != tag);
        let removed = entry.tags.len() < before_len;
        if removed {
            if entry.is_empty() {
                map.remove(&k);
            }
            store(save_path, map);
        }
        removed
    })
}

pub fn all_tags(save_path: &Path) -> Vec<String> {
    with_cache(save_path, |map| {
        let mut set = BTreeSet::new();
        for entry in map.values() {
            for tag in &entry.tags {
                set.insert(tag.clone());
            }
        }
        set.into_iter().collect()
    })
}

pub fn rename(save_path: &Path, old: &Path, new: &Path) {
    let old_k = key(save_path, old);
    let new_k = key(save_path, new);
    if old_k == new_k {
        return;
    }
    with_cache(save_path, |map| {
        if let Some(notes) = map.remove(&old_k) {
            map.insert(new_k, notes);
            store(save_path, map);
        }
    })
}

#[allow(dead_code)]
pub fn forget(save_path: &Path, clip: &Path) {
    with_cache(save_path, |map| {
        if map.remove(&key(save_path, clip)).is_some() {
            store(save_path, map);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_normalisation() {
        assert_eq!(clean_tag(""), None);
        assert_eq!(clean_tag("   "), None);
        assert_eq!(clean_tag("#"), None);
        assert_eq!(clean_tag("  #Clutch  "), Some("clutch".into()));
        assert_eq!(clean_tag("  ACE  "), Some("ace".into()));
        assert_eq!(
            clean_tag("this-tag-is-way-too-long-for-the-limit"),
            Some("this-tag-is-way-too-long".into())
        );
        assert_eq!(clean_tag("this-tag-is-way-too-long").unwrap().len(), 24);
    }

    #[test]
    fn add_and_rename_notes() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("lapse-notes-test-{}", unique));
        let _ = std::fs::create_dir_all(&dir);

        let clip1 = dir.join("clip1.mp4");
        let clip2 = dir.join("clip2.mp4");

        assert!(add_tag(&dir, &clip1, "  Ace  "));
        assert!(!add_tag(&dir, &clip1, "ace"));
        assert!(add_tag(&dir, &clip1, "clutch"));
        set_note(&dir, &clip1, "Great round!");

        let notes = get(&dir, &clip1);
        assert_eq!(notes.note, "Great round!");
        assert_eq!(notes.tags, vec!["ace", "clutch"]);

        rename(&dir, &clip1, &clip2);

        let old_notes = get(&dir, &clip1);
        assert!(old_notes.note.is_empty());
        assert!(old_notes.tags.is_empty());

        let new_notes = get(&dir, &clip2);
        assert_eq!(new_notes.note, "Great round!");
        assert_eq!(new_notes.tags, vec!["ace", "clutch"]);

        assert_eq!(all_tags(&dir), vec!["ace", "clutch"]);

        assert!(remove_tag(&dir, &clip2, "ace"));
        assert_eq!(get(&dir, &clip2).tags, vec!["clutch"]);

        forget(&dir, &clip2);
        assert!(get(&dir, &clip2).tags.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
