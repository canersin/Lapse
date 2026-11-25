//! Layout of the file the daemon and the gpu-screen-recorder plugin share: a header of
//! u32 fields, then two picture buffers (BGRA, tightly packed).

/// Environment variable with the file's path.
pub const ENV: &str = "LAPSE_CARD";
pub const MAGIC: u32 = 0x4C43_5244;

pub const MAGIC_AT: usize = 0;
/// 1 while the card replaces the picture.
pub const AWAY: usize = 1;
/// Number of the newest picture (0: none); its lowest bit says which buffer holds it.
pub const SEQ: usize = 2;
pub const CARD_W: usize = 3;
pub const CARD_H: usize = 4;
/// Size of the recorded frame, set by the plugin (width last).
pub const FRAME_W: usize = 5;
pub const FRAME_H: usize = 6;
/// 0xRRGGBB around the card.
pub const BACKGROUND: usize = 7;

pub const HEADER_SIZE: usize = 4096;
pub const BUFFER_SIZE: usize = 16 * 1024 * 1024;
pub const FILE_SIZE: usize = HEADER_SIZE + 2 * BUFFER_SIZE;
