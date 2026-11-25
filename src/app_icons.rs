//! Resolves desktop app names (e.g. "discord", "spotify") to local icon file paths.
//!
//! On Linux, parses `.desktop` files from standard XDG and Flatpak data directories,
//! matching by file name, `Name=`, `StartupWMClass=`, or `Exec=` binary.
//! On Windows, returns `None` (falling back to letter badge).

#[cfg(not(windows))]
use std::collections::HashMap;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::sync::Mutex;

#[cfg(not(windows))]
static ICON_CACHE: Mutex<Option<HashMap<String, Option<PathBuf>>>> = Mutex::new(None);

/// Looks up an icon path for the given app name. Results are cached in a static map.
pub fn get_app_icon(app_name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let _ = app_name;
        None
    }
    #[cfg(not(windows))]
    {
        let key = app_name.to_lowercase();
        if let Ok(mut lock) = ICON_CACHE.lock() {
            let cache = lock.get_or_insert_with(HashMap::new);
            if let Some(cached) = cache.get(&key) {
                return cached.clone();
            }
            let icon = find_system_app_icon(&key);
            cache.insert(key, icon.clone());
            icon
        } else {
            find_system_app_icon(&key)
        }
    }
}

/// Gathers the default Linux XDG and Flatpak data directories.
#[cfg(not(windows))]
pub fn default_linux_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);

    // $XDG_DATA_HOME or ~/.local/share
    if let Some(xdg_home) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        if !dirs.contains(&xdg_home) {
            dirs.push(xdg_home);
        }
    } else if let Some(ref h) = home {
        let p = h.join(".local/share");
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    }

    // $XDG_DATA_DIRS or /usr/local/share:/usr/share
    if let Some(xdg_dirs) = std::env::var_os("XDG_DATA_DIRS") {
        for part in std::env::split_paths(&xdg_dirs) {
            if !dirs.contains(&part) {
                dirs.push(part);
            }
        }
    } else {
        for fallback in ["/usr/local/share", "/usr/share"] {
            let p = PathBuf::from(fallback);
            if !dirs.contains(&p) {
                dirs.push(p);
            }
        }
    }

    // Flatpak exports: ~/.local/share/flatpak/exports/share and /var/lib/flatpak/exports/share
    if let Some(ref h) = home {
        let p = h.join(".local/share/flatpak/exports/share");
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    }
    let var_flatpak = PathBuf::from("/var/lib/flatpak/exports/share");
    if !dirs.contains(&var_flatpak) {
        dirs.push(var_flatpak);
    }

    dirs
}

#[cfg(not(windows))]
fn find_system_app_icon(app_name: &str) -> Option<PathBuf> {
    let dirs = default_linux_data_dirs();
    find_app_icon_in_dirs(app_name, &dirs)
}

/// Searches the specified base data directories for a matching `.desktop` file and its icon.
/// Exported so unit tests can test matching on temporary directory hierarchies.
pub fn find_app_icon_in_dirs(app_name: &str, data_dirs: &[PathBuf]) -> Option<PathBuf> {
    let app_clean = app_name.strip_suffix(".exe").unwrap_or(app_name).trim();
    if app_clean.is_empty() {
        return None;
    }

    // 1. Search for a matching .desktop file across data_dirs/applications
    for dir in data_dirs {
        let apps_dir = dir.join("applications");
        if let Ok(entries) = std::fs::read_dir(&apps_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                if let Some((Some(icon_spec), _)) = match_desktop_file(&path, app_clean) {
                    // Check if icon_spec is an absolute path
                    let icon_path = Path::new(&icon_spec);
                    if icon_path.is_absolute() && icon_path.is_file() {
                        return Some(icon_path.to_path_buf());
                    }

                    // Otherwise look in data dirs under icons/hicolor/... and pixmaps/...
                    if let Some(found) = resolve_icon_name(&icon_spec, data_dirs) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

/// The `Name=` of the installed app matching `app_name` (e.g. "Serein" for
/// "cz.viceverse.serein").
#[cfg(not(windows))]
pub fn get_app_display_name(app_name: &str) -> Option<String> {
    let app = app_name.trim();
    if app.is_empty() {
        return None;
    }
    for dir in default_linux_data_dirs() {
        let Ok(entries) = std::fs::read_dir(dir.join("applications")) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("desktop") {
                if let Some((_, Some(name))) = match_desktop_file(&path, app) {
                    return Some(name);
                }
            }
        }
    }
    None
}

/// Checks if a .desktop file matches `app_name` by file name, `Name=`, `StartupWMClass=`,
/// or `Exec=` binary. If matched, returns its `Icon=` and `Name=` values.
fn match_desktop_file(path: &Path, app_name: &str) -> Option<(Option<String>, Option<String>)> {
    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let file_matches = file_stem.eq_ignore_ascii_case(app_name)
        || file_stem
            .split('.')
            .any(|part| part.eq_ignore_ascii_case(app_name));

    let content = std::fs::read_to_string(path).ok()?;
    let mut name_matches = false;
    let mut wm_matches = false;
    let mut exec_matches = false;
    let mut icon_val = None;
    let mut name_val = None;

    let mut in_desktop_entry = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_desktop_entry = trimmed == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }

        if let Some(val) = trimmed.strip_prefix("Name=") {
            if val.trim().eq_ignore_ascii_case(app_name) {
                name_matches = true;
            }
            if name_val.is_none() {
                name_val = Some(val.trim().to_string());
            }
        } else if let Some(val) = trimmed.strip_prefix("StartupWMClass=") {
            if val.trim().eq_ignore_ascii_case(app_name) {
                wm_matches = true;
            }
        } else if let Some(val) = trimmed.strip_prefix("Exec=") {
            let cmd = val.trim();
            // Get binary: first token, stripped of path and quotes
            let first_token = cmd.split_whitespace().next().unwrap_or("");
            let bin = Path::new(first_token)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(first_token)
                .trim_matches('"')
                .trim_matches('\'');
            let bin_clean = bin.strip_suffix(".exe").unwrap_or(bin);
            if bin_clean.eq_ignore_ascii_case(app_name) {
                exec_matches = true;
            }
        } else if let Some(val) = trimmed.strip_prefix("Icon=") {
            if icon_val.is_none() {
                icon_val = Some(val.trim().to_string());
            }
        }
    }

    if file_matches || name_matches || wm_matches || exec_matches {
        Some((icon_val, name_val))
    } else {
        None
    }
}

/// Resolves a relative icon name by looking in `data_dirs` under:
/// `icons/hicolor/{scalable,256x256,128x128,64x64,48x48}/apps/<icon>.{svg,png}`
/// and `pixmaps/<icon>.png`.
fn resolve_icon_name(icon: &str, data_dirs: &[PathBuf]) -> Option<PathBuf> {
    let icon_clean = icon
        .strip_suffix(".png")
        .or_else(|| icon.strip_suffix(".svg"))
        .unwrap_or(icon);
    let sizes = ["scalable", "256x256", "128x128", "64x64", "48x48"];
    let extensions = ["svg", "png"];

    for dir in data_dirs {
        for size in sizes {
            for ext in extensions {
                let candidate = dir.join(format!("icons/hicolor/{}/apps/{}.{}", size, icon_clean, ext));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        let pixmap = dir.join(format!("pixmaps/{}.png", icon_clean));
        if pixmap.is_file() {
            return Some(pixmap);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_parsing_and_matching() {
        let temp_dir = std::env::temp_dir().join(format!("lapse_test_icons_{}", std::process::id()));
        let apps_dir = temp_dir.join("applications");
        let hicolor_dir = temp_dir.join("icons/hicolor/scalable/apps");
        let pixmaps_dir = temp_dir.join("pixmaps");
        let _ = std::fs::create_dir_all(&apps_dir);
        let _ = std::fs::create_dir_all(&hicolor_dir);
        let _ = std::fs::create_dir_all(&pixmaps_dir);

        // App 1: Discord matched by file name (discord.desktop), icon in hicolor
        let discord_desktop = apps_dir.join("discord.desktop");
        std::fs::write(
            &discord_desktop,
            "[Desktop Entry]\nName=Discord Client\nIcon=discord\nExec=discord\n",
        )
        .unwrap();
        let discord_svg = hicolor_dir.join("discord.svg");
        std::fs::write(&discord_svg, "<svg></svg>").unwrap();

        // App 2: Spotify matched by Name=, icon in pixmaps
        let spotify_desktop = apps_dir.join("com.spotify.Client.desktop");
        std::fs::write(
            &spotify_desktop,
            "[Desktop Entry]\nName=Spotify\nIcon=spotify-client\nExec=/usr/share/spotify/spotify %U\n",
        )
        .unwrap();
        let spotify_png = pixmaps_dir.join("spotify-client.png");
        std::fs::write(&spotify_png, "fake png").unwrap();

        // App 3: OBS matched by StartupWMClass=, absolute icon path
        let obs_desktop = apps_dir.join("obs-studio.desktop");
        let abs_icon = temp_dir.join("obs_abs_icon.png");
        std::fs::write(&abs_icon, "fake icon").unwrap();
        std::fs::write(
            &obs_desktop,
            format!(
                "[Desktop Entry]\nName=OBS Studio\nStartupWMClass=obs\nIcon={}\nExec=obs\n",
                abs_icon.display()
            ),
        )
        .unwrap();

        // App 4: Game matched by Exec= binary name
        let game_desktop = apps_dir.join("steam_app_1234.desktop");
        std::fs::write(
            &game_desktop,
            "[Desktop Entry]\nName=Some Game\nIcon=mygame\nExec=\"/home/user/.steam/steamapps/common/MyGame/mygame.exe\" %u\n",
        )
        .unwrap();
        let game_png = hicolor_dir.join("mygame.png");
        std::fs::write(&game_png, "fake game png").unwrap();

        let data_dirs = vec![temp_dir.clone()];

        // Test matching
        assert_eq!(
            find_app_icon_in_dirs("discord", &data_dirs),
            Some(discord_svg)
        );
        assert_eq!(
            find_app_icon_in_dirs("spotify", &data_dirs),
            Some(spotify_png)
        );
        assert_eq!(
            find_app_icon_in_dirs("obs", &data_dirs),
            Some(abs_icon)
        );
        assert_eq!(
            find_app_icon_in_dirs("mygame", &data_dirs),
            Some(game_png)
        );

        // Unknown app returns None
        assert_eq!(find_app_icon_in_dirs("nonexistent_app", &data_dirs), None);
        assert_eq!(find_app_icon_in_dirs("", &data_dirs), None);

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
