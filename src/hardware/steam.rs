//! Steam integration: resolves the running game from its `SteamAppId` and finds
//! names and cover art in the local Steam install (no network access).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// App ids that are Steam tools rather than games (runtimes, Proton, redistributables).
const TOOL_APP_IDS: &[u32] = &[0, 7, 769, 228980, 1070560, 1391110, 1628350, 1493710, 2180100, 2348590, 2805730, 3658110];

pub fn steam_roots() -> Vec<PathBuf> {
    let Ok(home) = std::env::var("HOME") else { return Vec::new() };
    let home = PathBuf::from(home);
    [
        ".local/share/Steam",
        ".steam/steam",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    ]
    .iter()
    .map(|p| home.join(p))
    .filter(|p| p.join("steamapps").is_dir())
    .collect()
}

/// All Steam library folders, from `libraryfolders.vdf`.
pub fn library_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for root in steam_roots() {
        if !dirs.contains(&root) {
            dirs.push(root.clone());
        }
        let vdf = root.join("steamapps/libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(vdf) {
            for line in text.lines() {
                if let Some(path) = vdf_value(line, "path") {
                    let p = PathBuf::from(path);
                    if p.is_dir() && !dirs.contains(&p) {
                        dirs.push(p);
                    }
                }
            }
        }
    }
    dirs
}

/// Parses a `"key"   "value"` line of a Valve KeyValues file.
fn vdf_value(line: &str, key: &str) -> Option<String> {
    let mut parts = line.split('"').filter(|s| !s.trim().is_empty());
    let k = parts.next()?;
    let v = parts.next()?;
    k.eq_ignore_ascii_case(key).then(|| v.replace("\\\\", "\\"))
}

fn names() -> &'static Mutex<HashMap<u32, String>> {
    static NAMES: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();
    NAMES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Reads every installed app's name once.
fn load_names() -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for lib in library_dirs() {
        let Ok(entries) = std::fs::read_dir(lib.join("steamapps")) else { continue };
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            let Some(id) = fname
                .strip_prefix("appmanifest_")
                .and_then(|r| r.strip_suffix(".acf"))
                .and_then(|r| r.parse::<u32>().ok())
            else {
                continue;
            };
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                if let Some(name) = text.lines().find_map(|l| vdf_value(l, "name")) {
                    map.insert(id, name);
                }
            }
        }
    }
    map
}

pub fn app_name(app_id: u32) -> Option<String> {
    let mut cache = names().lock().ok()?;
    if !cache.contains_key(&app_id) {
        *cache = load_names();
    }
    cache.get(&app_id).cloned()
}

/// App id of an installed game, by the (folder-safe) name Lapse shows for it.
pub fn app_id_for_name(name: &str) -> Option<u32> {
    let find = |map: &HashMap<u32, String>| map.iter().find(|(_, n)| sanitize_folder_name(n) == name).map(|(id, _)| *id);
    let mut cache = names().lock().ok()?;
    if let Some(id) = find(&cache) {
        return Some(id);
    }
    *cache = load_names();
    find(&cache)
}

pub fn is_tool(app_id: u32, name: &str) -> bool {
    TOOL_APP_IDS.contains(&app_id)
        || name.starts_with("Proton")
        || name.starts_with("Steam Linux Runtime")
        || name.starts_with("Steamworks")
}

/// Finds a running Steam game by looking for `SteamAppId` in process environments.
pub fn running_game() -> Option<(u32, String, u32)> {
    let mut best: Option<(u32, String, u32)> = None;
    for pid in super::procs::pids() {
        let Some(app_id) = app_id_of(pid) else { continue };
        let Some(name) = app_name(app_id) else { continue };
        if is_tool(app_id, &name) {
            continue;
        }
        // The newest process is usually the game itself rather than a launcher wrapper.
        if best.as_ref().map(|b| pid > b.2).unwrap_or(true) {
            best = Some((app_id, name, pid));
        }
    }
    best
}

pub fn app_id_of(pid: u32) -> Option<u32> {
    super::procs::steam_app_id(pid)
}

/// Local cover art for a game name, e.g. for playlist cards.
pub fn cover_for_name(game_name: &str) -> Option<PathBuf> {
    static COVERS: OnceLock<Mutex<HashMap<String, Option<PathBuf>>>> = OnceLock::new();
    let covers = COVERS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = covers.lock().ok()?.get(game_name) {
        return hit.clone();
    }

    let found = (|| {
        let id = {
            let mut cache = names().lock().ok()?;
            if cache.is_empty() {
                *cache = load_names();
            }
            cache
                .iter()
                .find(|(_, n)| sanitize_folder_name(n).eq_ignore_ascii_case(game_name))
                .map(|(id, _)| *id)?
        };
        steam_roots().into_iter().find_map(|root| cover_in(&root, id))
    })();

    if let Ok(mut c) = covers.lock() {
        c.insert(game_name.to_string(), found.clone());
    }
    found
}

fn cover_in(root: &Path, app_id: u32) -> Option<PathBuf> {
    let dir = root.join("appcache/librarycache").join(app_id.to_string());
    ["header.jpg", "library_header.jpg", "library_600x900.jpg", "library_hero.jpg"]
        .iter()
        .map(|f| dir.join(f))
        .find(|p| p.is_file())
        .or_else(|| {
            let legacy = root.join(format!("appcache/librarycache/{}_header.jpg", app_id));
            legacy.is_file().then_some(legacy)
        })
}

/// Makes a game name safe to use as a folder name.
pub fn sanitize_folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c == '/' || c == '\\' || c.is_control() { ' ' } else { c })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_matches('.').trim().to_string();
    if cleaned.is_empty() {
        "Gameplay".into()
    } else {
        cleaned.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vdf_lines() {
        assert_eq!(vdf_value("\t\t\"name\"\t\t\"Counter-Strike 2\"", "name").as_deref(), Some("Counter-Strike 2"));
        assert_eq!(vdf_value("\"path\" \"/mnt/Steam\"", "name"), None);
    }

    #[test]
    fn folder_names_are_safe() {
        assert_eq!(sanitize_folder_name("AC/DC: Live"), "AC DC: Live");
        assert_eq!(sanitize_folder_name("  ..  "), "Gameplay");
    }
}
