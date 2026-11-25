//! Apps Lapse has heard playing audio, remembered so each can get its own audio track
//! ("Find apps automatically"). gpu-screen-recorder needs the app names when the buffer
//! starts, so an app heard for the first time joins on the next buffer start.
//! Stored as a JSON list (most recently heard first) next to config.toml.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// More apps than this would mean a pile of mostly empty tracks in every clip.
const MAX_APPS: usize = 16;

fn store_path() -> Option<PathBuf> {
    crate::config::Config::get_config_dir().ok().map(|d| d.join("audio_apps.json"))
}

/// What is behind an audio stream name: apps built on generic audio libraries show up as
/// e.g. "cpal-pulseaudio-2", so the stream name alone says nothing about the app.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct AppIdentity {
    /// Shown in the trimmer and settings ("Serein").
    pub name: String,
    /// Flatpak app id or .desktop id, used to find the icon ("cz.viceverse.serein").
    pub desktop_id: String,
    /// Program name ("helium-browser"), for matching apps picked from open windows.
    pub binary: String,
}

fn identities_path() -> Option<PathBuf> {
    crate::config::Config::get_config_dir().ok().map(|d| d.join("audio_app_names.json"))
}

fn identities() -> HashMap<String, AppIdentity> {
    identities_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default()
}

/// The identity learned for an audio stream name, if any.
pub fn identity(key: &str) -> Option<AppIdentity> {
    let all = identities();
    all.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.clone())
}

/// Human name for an audio stream name: the app's own name when known.
pub fn display_name(key: &str) -> String {
    identity(key).map(|i| i.name).filter(|n| !n.is_empty()).unwrap_or_else(|| key.to_string())
}

/// The audio stream name to record for an app the user picked by its program, flatpak id
/// or display name; unknown names are used as written.
pub fn resolve_key(entry: &str) -> String {
    let matches = |id: &AppIdentity| {
        let last = id.desktop_id.rsplit('.').next().unwrap_or("");
        [id.name.as_str(), id.binary.as_str(), id.desktop_id.as_str(), last]
            .iter()
            .any(|v| !v.is_empty() && v.eq_ignore_ascii_case(entry))
    };
    identities().into_iter().find(|(_, id)| matches(id)).map(|(k, _)| k).unwrap_or_else(|| entry.to_string())
}

/// Identity of one PulseAudio/PipeWire client's properties.
fn identity_from_props(props: &serde_json::Map<String, serde_json::Value>) -> AppIdentity {
    let get = |k: &str| props.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let desktop_id = [get("pipewire.access.portal.app_id"), get("application.id")].into_iter().find(|v| !v.is_empty()).unwrap_or_default();
    let mut binary = get("application.process.binary");
    if binary.starts_with("pipewire") {
        // Flatpak apps are proxied: the "binary" is the PipeWire daemon itself.
        binary.clear();
    }
    let name = crate::app_icons::get_app_display_name(&desktop_id)
        .or_else(|| crate::app_icons::get_app_display_name(&binary))
        .unwrap_or_default();
    AppIdentity { name, desktop_id, binary }
}

/// Looks up the apps behind `keys` that have no identity yet (via `pactl -f json list clients`).
fn learn_identities(keys: &[String]) {
    let mut all = identities();
    let missing: Vec<&String> = keys.iter().filter(|k| !all.keys().any(|a| a.eq_ignore_ascii_case(k))).collect();
    if missing.is_empty() {
        return;
    }
    let Ok(out) = std::process::Command::new("pactl").args(["-f", "json", "list", "clients"]).output() else { return };
    let Ok(clients) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) else { return };
    let mut changed = false;
    for key in missing {
        let props = clients.iter().filter_map(|c| c.get("properties")?.as_object()).find(|p| {
            p.get("application.name").and_then(|v| v.as_str()).is_some_and(|n| n.eq_ignore_ascii_case(key))
        });
        if let Some(props) = props {
            let id = identity_from_props(props);
            if id != AppIdentity::default() {
                all.insert(key.clone(), id);
                changed = true;
            }
        }
    }
    if changed {
        if let (Some(p), Ok(json)) = (identities_path(), serde_json::to_vec_pretty(&all)) {
            let _ = std::fs::write(p, json);
        }
    }
}

/// Audio streams that are never worth a track of their own: the recorder, speech
/// dispatcher and Lapse itself. (Generic names like "cpal-pulseaudio-2" are real apps,
/// e.g. the Serein Discord client, so they are kept.)
pub fn is_ignored(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["gsr", "speech-dispatcher", "lapse"].iter().any(|p| n.starts_with(p)) || n.is_empty()
}

pub fn load() -> Vec<String> {
    let apps: Vec<String> = store_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default();
    apps.into_iter().filter(|a| !is_ignored(a)).collect()
}

fn save(apps: &[String]) {
    if let Some(p) = store_path() {
        if let Ok(json) = serde_json::to_vec_pretty(apps) {
            let _ = std::fs::write(p, json);
        }
    }
}

/// Moves `heard` to the front of `known` (adding new names); returns true if any was new.
fn merge(known: &mut Vec<String>, heard: &[String]) -> bool {
    let mut added = false;
    for name in heard.iter().rev().filter(|n| !is_ignored(n)) {
        match known.iter().position(|k| k.eq_ignore_ascii_case(name)) {
            Some(i) => {
                let k = known.remove(i);
                known.insert(0, k);
            }
            None => {
                known.insert(0, name.clone());
                added = true;
            }
        }
    }
    known.truncate(MAX_APPS);
    added
}

/// Records apps playing audio right now. Returns true if one was heard for the first time.
pub fn remember(heard: &[String]) -> bool {
    let heard: Vec<String> = heard.iter().filter(|h| !is_ignored(h)).cloned().collect();
    learn_identities(&heard);
    let heard = &heard[..];
    let mut known = load();
    let before = known.clone();
    let added = merge(&mut known, heard);
    if known != before {
        save(&known);
    }
    added
}

pub fn forget(name: &str) {
    let mut known = load();
    known.retain(|k| !k.eq_ignore_ascii_case(name));
    save(&known);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_orders_and_ignores() {
        let mut known = vec!["Helium".to_string(), "vesktop".to_string()];
        let heard = vec!["VESKTOP".to_string(), "mpv".to_string(), "gsr-default".to_string(), "speech-dispatcher-dummy".to_string()];
        assert!(merge(&mut known, &heard));
        assert_eq!(known, vec!["vesktop", "mpv", "Helium"]);
        assert!(!merge(&mut known, &["Helium".to_string()]));
        assert_eq!(known[0], "Helium");
    }

    #[test]
    fn test_merge_caps_list() {
        let mut known: Vec<String> = (0..MAX_APPS).map(|i| format!("app{i}")).collect();
        assert!(merge(&mut known, &["new".to_string()]));
        assert_eq!(known.len(), MAX_APPS);
        assert_eq!(known[0], "new");
        assert!(!known.contains(&format!("app{}", MAX_APPS - 1)));
    }
}
