//! Discord Rich Presence: "Counter-Strike 2 with Lapse" on the user's profile
//! while a game is open, with the game's cover and the Lapse logo.
//!
//! Talks to the local Discord client over its IPC socket: frames are a u32
//! opcode and a u32 length (little endian) followed by JSON. No network, no token.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::recorder::{Recorder, RecordingMode};

/// The "Lapse" application on the Discord developer portal. Discord titles the card
/// with the application's name, so games Discord knows use their own application
/// (from Discord's game list) and the card says "Minecraft", not "Lapse".
const CLIENT_ID: &str = "1571945218943750164";
/// The logo as a web image: Discord shows any https image, so nothing has to be
/// uploaded to the developer portal. Discord keeps its own copy per address, so
/// raise `v` whenever assets/icon.png changes.
const LOGO_URL: &str = "https://raw.githubusercontent.com/canersin/Lapse/main/assets/icon.png?v=2";
const REPO_URL: &str = "https://github.com/canersin/Lapse";
const POLL: Duration = Duration::from_secs(5);

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;

/// What the profile shows; a new one is only sent when this changes.
#[derive(Clone, Debug, PartialEq)]
struct Presence {
    game: String,
    /// Game cover; the Lapse logo is shown alone without one.
    cover: Option<String>,
    /// Discord's own application for this game, when it has one.
    app_id: Option<String>,
    recording: bool,
    /// "Get Lapse" button linking to the repository.
    button: bool,
    clips: u64,
}

/// Places Discord puts its socket: native, Flatpak, Snap and Vesktop.
fn socket_candidates() -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    for var in ["XDG_RUNTIME_DIR", "TMPDIR"] {
        if let Some(dir) = std::env::var_os(var).filter(|d| !d.is_empty()) {
            bases.push(dir.into());
        }
    }
    bases.push("/tmp".into());
    let subdirs = [
        "",
        "app/com.discordapp.Discord",
        "app/com.discordapp.DiscordCanary",
        "app/dev.vencord.Vesktop",
        ".flatpak/dev.vencord.Vesktop/xdg-run",
        "snap.discord",
    ];
    let mut out = Vec::new();
    for base in &bases {
        for sub in subdirs {
            for i in 0..10 {
                out.push(base.join(sub).join(format!("discord-ipc-{}", i)));
            }
        }
    }
    out
}

fn write_frame(stream: &mut UnixStream, op: u32, body: &Value) -> std::io::Result<()> {
    let data = body.to_string();
    let mut frame = Vec::with_capacity(8 + data.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
    frame.extend_from_slice(data.as_bytes());
    stream.write_all(&frame)
}

fn read_frame(stream: &mut UnixStream) -> std::io::Result<(u32, Value)> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header)?;
    let op = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let mut body = vec![0u8; len.min(1 << 20)];
    stream.read_exact(&mut body)?;
    Ok((op, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

/// Connects to the first running Discord and identifies as Lapse.
fn connect(client_id: &str) -> Option<UnixStream> {
    for path in socket_candidates() {
        let Ok(mut stream) = UnixStream::connect(&path) else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        if write_frame(&mut stream, OP_HANDSHAKE, &json!({ "v": 1, "client_id": client_id })).is_err() {
            continue;
        }
        match read_frame(&mut stream) {
            Ok((OP_FRAME, _)) => return Some(stream),
            Ok((op, body)) => eprintln!("Lapse: Discord refused the presence (op {}): {}", op, body),
            Err(_) => {}
        }
    }
    None
}

/// True while Discord still holds the connection (it closes it when it quits).
fn still_open(stream: &mut UnixStream) -> bool {
    use std::os::fd::AsRawFd;
    let mut byte = 0u8;
    // Peek without blocking: 0 means Discord hung up, EAGAIN means it's quiet but there.
    let n = unsafe {
        libc::recv(stream.as_raw_fd(), &mut byte as *mut u8 as *mut libc::c_void, 1, libc::MSG_PEEK | libc::MSG_DONTWAIT)
    };
    n > 0 || (n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock)
}

/// Cover art of a Steam game. Newer games keep their art under hashed paths, so
/// ask the store where it is; older ones also answer at the plain address.
fn steam_cover(app_id: u32) -> String {
    store_cover(app_id).unwrap_or_else(|| format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{}/library_600x900.jpg", app_id))
}

fn store_cover(app_id: u32) -> Option<String> {
    let query = json!({
        "ids": [{ "appid": app_id }],
        "context": { "language": "english", "country_code": "US" },
        "data_request": { "include_assets": true },
    });
    let out = std::process::Command::new("curl")
        .args(["-sfG", "--max-time", "6", "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/", "--data-urlencode"])
        .arg(format!("input_json={}", query))
        .output()
        .ok()?;
    let body: Value = serde_json::from_slice(&out.stdout).ok()?;
    cover_from_store(&body)
}

fn cover_from_store(body: &Value) -> Option<String> {
    let assets = &body["response"]["store_items"][0]["assets"];
    let format = assets["asset_url_format"].as_str()?;
    // The tall library art keeps more of the picture when Discord crops it square.
    let file = ["library_capsule", "header", "main_capsule"].iter().find_map(|k| assets[*k].as_str())?;
    Some(format!("https://shared.akamai.steamstatic.com/store_item_assets/{}", format.replace("${FILENAME}", file)))
}

/// Cover for any game: its Steam art when it is installed through Steam, else the icon
/// from Discord's own list of games, else Steam art found by the name.
fn cover_for(game: &str, discord: Option<&DiscordGame>) -> Option<String> {
    if let Some(id) = crate::hardware::steam::app_id_for_name(game) {
        return Some(steam_cover(id));
    }
    discord.and_then(|d| d.icon.clone()).or_else(|| steam_search(game).map(steam_cover))
}

/// A game in Discord's list: its application id and icon.
#[derive(Clone, Debug, PartialEq)]
struct DiscordGame {
    id: String,
    icon: Option<String>,
}

/// Lowercase letters and digits only, so "Counter-Strike 2" and "counter strike 2" meet.
fn normalize(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Discord's list of known games (about 25 000, the one Discord uses to show
/// "Playing ..."), kept as "name<TAB>id<TAB>icon" lines and fetched again weekly.
fn discord_games() -> Option<String> {
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?
        .join("lapse");
    let path = dir.join("discord-games.tsv");
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().unwrap_or_default() < Duration::from_secs(7 * 24 * 3600))
        .unwrap_or(false);
    if !fresh {
        let out = std::process::Command::new("curl")
            .args(["-sf", "--max-time", "30", "https://discord.com/api/v9/applications/detectable"])
            .output();
        if let Some(list) = out.ok().and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok()).and_then(|v| v.as_array().cloned()) {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(&path, games_table(&list));
        }
    }
    std::fs::read_to_string(&path).ok()
}

fn games_table(list: &[Value]) -> String {
    let mut out = String::new();
    for app in list {
        let Some(id) = app["id"].as_str() else { continue };
        let icon = app["icon_hash"].as_str().unwrap_or("");
        let names = std::iter::once(&app["name"]).chain(app["aliases"].as_array().into_iter().flatten());
        for name in names.filter_map(Value::as_str) {
            out.push_str(&format!("{}\t{}\t{}\n", name.replace(['\t', '\n'], " "), id, icon));
        }
    }
    out
}

fn game_in_table(table: &str, game: &str) -> Option<DiscordGame> {
    let want = normalize(game);
    if want.is_empty() {
        return None;
    }
    table.lines().find_map(|line| {
        let mut parts = line.split('\t');
        let (name, id, icon) = (parts.next()?, parts.next()?, parts.next().unwrap_or(""));
        (normalize(name) == want).then(|| DiscordGame {
            id: id.to_string(),
            icon: (!icon.is_empty()).then(|| format!("https://cdn.discordapp.com/app-icons/{}/{}.png?size=512", id, icon)),
        })
    })
}

fn discord_game(game: &str) -> Option<DiscordGame> {
    game_in_table(&discord_games()?, game)
}

/// A Steam game with exactly this name (for games installed outside Steam).
fn steam_search(game: &str) -> Option<u32> {
    let out = std::process::Command::new("curl")
        .args(["-sfG", "--max-time", "6", "https://store.steampowered.com/api/storesearch/", "--data-urlencode"])
        .arg(format!("term={}", game))
        .args(["-d", "cc=US", "-d", "l=english"])
        .output()
        .ok()?;
    let body: Value = serde_json::from_slice(&out.stdout).ok()?;
    let want = normalize(game);
    body["items"].as_array()?.iter().find(|i| i["name"].as_str().map(normalize).as_deref() == Some(want.as_str()))?["id"].as_u64().map(|id| id as u32)
}

fn activity(p: &Presence, started: u64) -> Value {
    let state = if p.recording {
        Some("Recording".to_string())
    } else if p.clips > 0 {
        Some(format!("{} {} saved", p.clips, if p.clips == 1 { "clip" } else { "clips" }))
    } else {
        None
    };
    // Game cover large with the Lapse logo in the corner; the logo alone without a cover.
    let assets = match &p.cover {
        Some(cover) => json!({
            "large_image": cover,
            "large_text": p.game,
            "small_image": LOGO_URL,
            "small_text": "Recorded with Lapse",
        }),
        None => json!({ "large_image": LOGO_URL, "large_text": "Lapse" }),
    };
    let mut activity = json!({
        "type": 0,
        // Member list and status line show the details ("<game> with Lapse"), not "Lapse".
        "status_display_type": 2,
        "details": format!("{} with Lapse", p.game),
        "timestamps": { "start": started },
        "assets": assets,
    });
    if let Some(state) = state {
        activity["state"] = json!(state);
    }
    if p.button {
        activity["buttons"] = json!([{ "label": "Get Lapse", "url": REPO_URL }]);
    }
    activity
}

fn send(stream: &mut UnixStream, activity: Value) -> std::io::Result<()> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    write_frame(
        stream,
        OP_FRAME,
        &json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id(), "activity": activity },
            "nonce": nonce.to_string(),
        }),
    )?;
    match read_frame(stream)? {
        (OP_CLOSE, body) => Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, body.to_string())),
        (_, body) if body.get("evt").and_then(Value::as_str) == Some("ERROR") => {
            eprintln!("Lapse: Discord rejected the presence: {}", body["data"]);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// What should be on the profile now, or None for nothing.
fn wanted(recorder: &Arc<Mutex<Recorder>>) -> Option<Presence> {
    let mut rec = recorder.lock().ok()?;
    if !rec.config().discord_rpc {
        return None;
    }
    let status = rec.status();
    let game = status.game?;
    Some(Presence {
        cover: None,
        app_id: None,
        recording: rec.current_mode() == RecordingMode::Continuous && status.recording,
        button: rec.config().discord_rpc_button,
        clips: status.save_counter,
        game,
    })
}

/// Keeps the presence in step with the recorder for as long as the daemon runs.
pub fn start(recorder: Arc<Mutex<Recorder>>) {
    std::thread::spawn(move || {
        // The connection and the application it speaks as.
        let mut conn: Option<(UnixStream, String)> = None;
        let mut shown: Option<Presence> = None;
        let mut started: Option<(String, u64)> = None;
        let mut clips_at_start = 0u64;
        // Steam lookups read the app manifests and ask the store; once per game is enough.
        let mut looked_up: std::collections::HashMap<String, (Option<String>, Option<String>)> = Default::default();
        loop {
            std::thread::sleep(POLL);
            if let Some((stream, _)) = conn.as_mut() {
                if !still_open(stream) {
                    conn = None;
                    shown = None;
                }
            }

            let mut want = wanted(&recorder);
            if let Some(p) = want.as_mut() {
                // Session = this game: the timer and the clip count start when it opens.
                if started.as_ref().map(|(g, _)| g != &p.game).unwrap_or(true) {
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    started = Some((p.game.clone(), now));
                    clips_at_start = p.clips;
                }
                p.clips -= clips_at_start.min(p.clips);
                let (cover, app_id) = looked_up
                    .entry(p.game.clone())
                    .or_insert_with(|| {
                        let discord = discord_game(&p.game);
                        (cover_for(&p.game, discord.as_ref()), discord.map(|d| d.id))
                    })
                    .clone();
                p.cover = cover;
                p.app_id = app_id;
            } else {
                started = None;
            }
            if want == shown {
                continue;
            }

            if want.is_none() {
                // Closing the connection clears the presence.
                conn = None;
                shown = None;
                continue;
            }
            let client_id = want.as_ref().and_then(|p| p.app_id.clone()).unwrap_or_else(|| CLIENT_ID.to_string());
            if conn.as_ref().map(|(_, id)| id != &client_id).unwrap_or(false) {
                // Another game: closing clears the old card, then speak as the new game.
                conn = None;
            }
            if conn.is_none() {
                // A game's own application, else Lapse's.
                conn = connect(&client_id)
                    .map(|s| (s, client_id.clone()))
                    .or_else(|| connect(CLIENT_ID).map(|s| (s, CLIENT_ID.to_string())));
            }
            let (Some((stream, _)), Some(p)) = (conn.as_mut(), want.as_ref()) else { continue };
            let since = started.as_ref().map(|(_, t)| *t).unwrap_or(0);
            match send(stream, activity(p, since)) {
                Ok(()) => shown = want,
                Err(_) => {
                    conn = None;
                    shown = None;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_names_the_game() {
        let p = Presence { game: "Counter-Strike 2".into(), cover: Some(steam_cover(730)), app_id: None, recording: false, button: true, clips: 3 };
        let a = activity(&p, 100);
        assert_eq!(a["details"], "Counter-Strike 2 with Lapse");
        assert_eq!(a["state"], "3 clips saved");
        assert_eq!(a["assets"]["small_image"], LOGO_URL);
        assert_eq!(a["buttons"][0]["url"], REPO_URL);
        let quiet = activity(&Presence { clips: 0, button: false, ..p }, 100);
        assert!(quiet.get("state").is_none() && quiet.get("buttons").is_none());
        assert!(a["assets"]["large_image"].as_str().unwrap().contains("/730/"));
        let alone = activity(&Presence { game: "Minecraft".into(), cover: None, app_id: None, recording: true, button: false, clips: 0 }, 1);
        assert_eq!((alone["assets"]["large_image"].as_str(), alone["state"].as_str()), (Some(LOGO_URL), Some("Recording")));
    }

    #[test]
    fn any_game_finds_its_discord_icon() {
        let list = json!([
            { "id": "1402418491272986635", "name": "Minecraft", "icon_hash": "166f", "aliases": ["Minecraft Launcher"] },
            { "id": "1", "name": "No Icon", "icon_hash": null },
        ]);
        let table = games_table(list.as_array().unwrap());
        let mc = game_in_table(&table, "minecraft").unwrap();
        assert_eq!(mc.id, "1402418491272986635");
        assert_eq!(mc.icon.as_deref(), Some("https://cdn.discordapp.com/app-icons/1402418491272986635/166f.png?size=512"));
        assert!(game_in_table(&table, "Minecraft-Launcher").is_some());
        assert_eq!(game_in_table(&table, "No Icon").map(|g| g.icon), Some(None));
        assert_eq!(game_in_table(&table, ""), None);
    }

    /// Needs the internet: `cargo test live_covers -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_covers() {
        for game in ["Minecraft", "PEAK", "Hollow Knight", "osu!"] {
            let d = discord_game(game);
            println!("{} -> {:?} {:?}", game, d.as_ref().map(|d| &d.id), cover_for(game, d.as_ref()));
        }
    }

    #[test]
    fn new_games_get_their_hashed_cover() {
        let body = json!({ "response": { "store_items": [{ "assets": {
            "asset_url_format": "steam/apps/3527290/${FILENAME}?t=1",
            "library_capsule": "480bd8/library_600x900.jpg",
            "header": "31bac6/header.jpg",
        }}]}});
        assert_eq!(
            cover_from_store(&body).as_deref(),
            Some("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3527290/480bd8/library_600x900.jpg?t=1")
        );
        assert_eq!(cover_from_store(&json!({})), None);
    }

    #[test]
    fn frames_round_trip() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        write_frame(&mut a, OP_FRAME, &json!({ "x": 1 })).unwrap();
        let (op, body) = read_frame(&mut b).unwrap();
        assert_eq!((op, body), (OP_FRAME, json!({ "x": 1 })));
    }

    /// Needs the Discord app running: `cargo test live_presence -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_presence() {
        let id = std::env::var("LAPSE_TEST_APP").unwrap_or_else(|_| CLIENT_ID.to_string());
        let mut stream = connect(&id).expect("Discord is not running");
        let p = Presence { game: "PEAK".into(), cover: Some(steam_cover(3527290)), app_id: None, recording: false, button: true, clips: 2 };
        println!("cover {:?}", p.cover);
        let nonce = "lapse-test";
        write_frame(&mut stream, OP_FRAME, &json!({ "cmd": "SET_ACTIVITY", "args": { "pid": std::process::id(), "activity": activity(&p, 0) }, "nonce": nonce })).unwrap();
        let (op, body) = read_frame(&mut stream).unwrap();
        println!("op {}: {:#}", op, body);
        assert_ne!(body["evt"], "ERROR");
        std::thread::sleep(Duration::from_secs(std::env::var("LAPSE_TEST_WAIT").ok().and_then(|w| w.parse().ok()).unwrap_or(30)));
    }
}
