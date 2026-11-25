//! Renaming a clip together with everything that refers to it by name: the thumbnail,
//! the marker file, the probe cache entry and the favorite star.

use std::path::{Path, PathBuf};

/// Checks a new name typed by the user and returns the file name to use (".mp4" added).
pub fn clean_name(input: &str, extension: &str) -> Result<String, &'static str> {
    let name = input.trim();
    let name = name.strip_suffix(&format!(".{}", extension)).unwrap_or(name).trim();
    if name.is_empty() {
        return Err("The name can't be empty.");
    }
    if name.starts_with('.') {
        return Err("The name can't start with a dot.");
    }
    if name.contains(['/', '\\', '\0']) {
        return Err("The name can't contain / or \\.");
    }
    if name.len() > 200 {
        return Err("The name is too long.");
    }
    Ok(format!("{}.{}", name, extension))
}

/// Renames `clip` (staying in its folder). Returns the new path.
pub fn rename_clip(save_path: &Path, clip: &Path, input: &str) -> Result<PathBuf, String> {
    let ext = clip.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string();
    let file_name = clean_name(input, &ext).map_err(String::from)?;
    let target = clip.with_file_name(&file_name);
    if target == clip {
        return Ok(target);
    }
    if target.exists() {
        return Err(format!("{} already exists.", file_name));
    }
    std::fs::rename(clip, &target).map_err(|e| e.to_string())?;

    // Side files are keyed by file name; move them along (they may not exist).
    let thumbs = crate::clip_index::thumb_dir(save_path);
    let old = clip.file_name().unwrap_or_default().to_string_lossy().to_string();
    for suffix in [".jpg", ".markers.json", ".tracks.json"] {
        let _ = std::fs::rename(thumbs.join(format!("{}{}", old, suffix)), thumbs.join(format!("{}{}", file_name, suffix)));
    }
    let _ = std::fs::rename(
        clip.with_file_name(format!("{}.tracks.json", old)),
        target.with_file_name(format!("{}.tracks.json", file_name)),
    );
    if crate::favorites::is_favorite(save_path, clip) {
        crate::favorites::forget(save_path, clip);
        crate::favorites::toggle(save_path, &target);
    }
    crate::notes::rename(save_path, clip, &target);
    crate::clip_index::index().refresh();
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert_eq!(clean_name("  ace on mirage ", "mp4"), Ok("ace on mirage.mp4".into()));
        assert_eq!(clean_name("clip.mp4", "mp4"), Ok("clip.mp4".into()));
        assert!(clean_name("../x", "mp4").is_err());
        assert!(clean_name("   ", "mp4").is_err());
        assert!(clean_name(".hidden", "mp4").is_err());
    }
}
