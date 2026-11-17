//! The "clip saved" sound: the built-in shutter, or a file the user picked.

use std::path::Path;
use std::time::Duration;

const SHUTTER: &[u8] = include_bytes!("../assets/shutter.ogg");
/// A picked file plays at most this long, so a whole song can't run over the game.
const MAX_LENGTH: Duration = Duration::from_secs(4);

/// Audio files the picker offers (formats rodio can decode).
pub const EXTENSIONS: &[&str] = &["ogg", "wav", "mp3", "flac"];

/// Checks that a file can be played; returns the reason when it can't.
pub fn check(path: &Path) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    rodio::Decoder::try_from(file).map(|_| ()).map_err(|e| e.to_string())
}

/// Plays the save sound in the background: `custom` if set and playable, else the shutter.
pub fn play_save_sound(custom: &str) {
    let custom = custom.trim().to_string();
    std::thread::spawn(move || {
        let Ok(mut handle) = rodio::DeviceSinkBuilder::open_default_sink() else { return };
        handle.log_on_drop(false);
        let player = rodio::Player::connect_new(&handle.mixer());
        let picked = (!custom.is_empty())
            .then(|| std::fs::File::open(&custom).ok())
            .flatten()
            .and_then(|f| rodio::Decoder::try_from(f).ok());
        match picked {
            Some(decoder) => {
                use rodio::Source;
                player.append(decoder.take_duration(MAX_LENGTH));
            }
            None => match rodio::Decoder::new(std::io::Cursor::new(SHUTTER)) {
                Ok(decoder) => player.append(decoder),
                Err(_) => return,
            },
        }
        player.sleep_until_end();
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn built_in_shutter_decodes_and_junk_does_not() {
        assert!(super::check(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/shutter.ogg"))).is_ok());
        assert!(super::check(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))).is_err());
    }
}
