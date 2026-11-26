//! Clip export jobs (trim + optional re-encode) run with ffmpeg in the background.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};

use crate::gui::views::trimmer::overlays::{Overlay, OverlayKind};

/// Discord's free upload limit is 10 MiB; aim a little under it.
pub const DISCORD_LIMIT_BYTES: u64 = 10 * 1024 * 1024;
const DISCORD_TARGET_BYTES: f64 = 9.4 * 1024.0 * 1024.0;
/// Streamable's free upload limit is 250 MiB; aim a little under it.
const STREAMABLE_TARGET_BYTES: f64 = 240.0 * 1024.0 * 1024.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioFormat {
    Mp3,
    Opus,
    Aac,
    Flac,
    Wav,
}

impl AudioFormat {
    pub const ALL: [AudioFormat; 5] = [Self::Mp3, Self::Opus, Self::Aac, Self::Flac, Self::Wav];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::Opus => "Opus",
            Self::Aac => "AAC (M4A)",
            Self::Flac => "FLAC",
            Self::Wav => "WAV",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Opus => "opus",
            Self::Aac => "m4a",
            Self::Flac => "flac",
            Self::Wav => "wav",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportPreset {
    /// Stream copy: instant and lossless, but the cut snaps to the previous keyframe.
    Fast,
    /// Re-encode at high quality: frame-accurate cut points.
    Precise,
    /// Re-encode to fit Discord's 10 MB limit.
    Discord,
    /// Re-encode to fit Streamable's 250 MB limit.
    Streamable,
    /// 9:16 for TikTok / Shorts / Reels: a vertical slice of the picture.
    Vertical,
    Gif,
    Audio(AudioFormat),
}

impl ExportPreset {
    pub const ALL: [ExportPreset; 6] = [Self::Fast, Self::Precise, Self::Discord, Self::Vertical, Self::Gif, Self::Audio(AudioFormat::Mp3)];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fast => "Fast",
            Self::Precise => "Precise",
            Self::Discord => "Discord",
            Self::Streamable => "Streamable",
            Self::Vertical => "Vertical",
            Self::Gif => "GIF",
            Self::Audio(_) => "Audio",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Fast => "Instant, original quality. Cut points may shift up to ~2 s to the nearest keyframe.",
            Self::Precise => "Exact cut points, near-original quality. Takes a little longer.",
            Self::Discord => "Shrinks the clip to under 10 MB so it uploads to Discord without Nitro.",
            Self::Streamable => "Shrinks the clip to under 250 MB for Streamable.",
            Self::Vertical => "9:16 video (1080x1920) for Shorts, TikTok and Reels. Move the frame to pick which part of the picture is kept.",
            Self::Gif => "Animated GIF (480p, 15 fps, no sound). Best for short moments.",
            Self::Audio(_) => "Audio only. Pick the format below.",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::Audio(fmt) => fmt.extension(),
            _ => "mp4",
        }
    }

    pub fn has_audio_option(self) -> bool {
        !matches!(self, Self::Gif | Self::Audio(_))
    }
}

/// Per-track mixer setting in the trimmer / export.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackSetting {
    pub name: String,
    pub enabled: bool,
    pub volume: f32,
}

/// Which audio ends up in the export. Track 0 is game/desktop audio, track 1 the
/// microphone (when it was recorded as a separate track), and optionally track 2 chat/music.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioMix {
    pub game: bool,
    pub mic: bool,
    /// Microphone gain, 1.0 = unchanged.
    pub mic_volume: f32,
    /// EBU R128 loudness normalization, so every clip is about as loud.
    pub normalize: bool,
    /// Per-track mix settings when multiple audio tracks are present.
    pub tracks: Vec<TrackSetting>,
}

impl Default for AudioMix {
    fn default() -> Self {
        Self {
            game: true,
            mic: true,
            mic_volume: 1.0,
            normalize: false,
            tracks: Vec::new(),
        }
    }
}

impl AudioMix {
    pub fn muted() -> Self {
        Self {
            game: false,
            mic: false,
            mic_volume: 1.0,
            normalize: false,
            tracks: Vec::new(),
        }
    }

    /// Initializes per-track settings from track names (e.g. from `<clip>.tracks.json`).
    pub fn init_tracks(&mut self, track_names: &[String]) {
        self.tracks = track_names
            .iter()
            .map(|name| {
                let (enabled, volume) = match name.as_str() {
                    "game" => (self.game, 1.0),
                    "mic" => (self.mic, self.mic_volume),
                    "chat" => (true, 1.0),
                    s if s.starts_with("app:") => (true, 1.0),
                    _ => (true, 1.0),
                };
                TrackSetting {
                    name: name.clone(),
                    enabled,
                    volume,
                }
            })
            .collect();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CropRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CropAspect {
    Free,
    Vertical9x16,
    Square,
    Wide16x9,
}

impl CropAspect {
    pub const ALL: [CropAspect; 4] = [Self::Free, Self::Vertical9x16, Self::Square, Self::Wide16x9];

    pub fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Vertical9x16 => "9:16",
            Self::Square => "1:1",
            Self::Wide16x9 => "16:9",
        }
    }

    pub fn pixel_ratio(self) -> Option<f32> {
        match self {
            Self::Free => None,
            Self::Vertical9x16 => Some(9.0 / 16.0),
            Self::Square => Some(1.0),
            Self::Wide16x9 => Some(16.0 / 9.0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CropHandle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl CropHandle {
    pub fn cursor(self) -> eframe::egui::CursorIcon {
        match self {
            Self::TopLeft => eframe::egui::CursorIcon::ResizeNorthWest,
            Self::Top => eframe::egui::CursorIcon::ResizeNorth,
            Self::TopRight => eframe::egui::CursorIcon::ResizeNorthEast,
            Self::Right => eframe::egui::CursorIcon::ResizeEast,
            Self::BottomRight => eframe::egui::CursorIcon::ResizeSouthEast,
            Self::Bottom => eframe::egui::CursorIcon::ResizeSouth,
            Self::BottomLeft => eframe::egui::CursorIcon::ResizeSouthWest,
            Self::Left => eframe::egui::CursorIcon::ResizeWest,
        }
    }
}

impl CropRect {
    /// Returns (w, h, x, y) in pixels, rounded down to even numbers.
    pub fn to_pixels(&self, video_w: u32, video_h: u32) -> (u32, u32, u32, u32) {
        let vw = video_w.max(2) as f32;
        let vh = video_h.max(2) as f32;

        let mut x = ((self.x.clamp(0.0, 1.0) * vw) as u32 / 2) * 2;
        let mut y = ((self.y.clamp(0.0, 1.0) * vh) as u32 / 2) * 2;
        let mut w = ((self.w.clamp(0.0, 1.0) * vw) as u32 / 2) * 2;
        let mut h = ((self.h.clamp(0.0, 1.0) * vh) as u32 / 2) * 2;

        w = w.max(2).min((video_w / 2) * 2);
        h = h.max(2).min((video_h / 2) * 2);

        if x + w > video_w {
            x = (video_w.saturating_sub(w) / 2) * 2;
        }
        if y + h > video_h {
            y = (video_h.saturating_sub(h) / 2) * 2;
        }

        (w, h, x, y)
    }

    pub fn min_size(video_w: usize, video_h: usize) -> (f32, f32) {
        let vw = (video_w.max(1)) as f32;
        let vh = (video_h.max(1)) as f32;
        let min_s = 0.10 * vw.min(vh);
        ((min_s / vw).max(0.001), (min_s / vh).max(0.001))
    }

    pub fn default_for_aspect(video_w: usize, video_h: usize, aspect: CropAspect) -> Self {
        let vw = (video_w.max(1)) as f32;
        let vh = (video_h.max(1)) as f32;
        let vid_ratio = vw / vh;

        match aspect.pixel_ratio() {
            None => Self { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            Some(target_ratio) => {
                if target_ratio <= vid_ratio {
                    // Full height, centered horizontally
                    let w = (target_ratio / vid_ratio).clamp(0.0, 1.0);
                    let x = (1.0 - w) / 2.0;
                    Self { x, y: 0.0, w, h: 1.0 }
                } else {
                    // Full width, centered vertically
                    let h = (vid_ratio / target_ratio).clamp(0.0, 1.0);
                    let y = (1.0 - h) / 2.0;
                    Self { x: 0.0, y, w: 1.0, h }
                }
            }
        }
    }

    pub fn adjust_to_aspect(&self, video_w: usize, video_h: usize, aspect: CropAspect) -> Self {
        let Some(target_ratio) = aspect.pixel_ratio() else {
            return *self;
        };
        let vw = (video_w.max(1)) as f32;
        let vh = (video_h.max(1)) as f32;
        let norm_ratio = target_ratio * (vh / vw);

        let (min_w, min_h) = Self::min_size(video_w, video_h);
        let min_w_eff = min_w.max(min_h * norm_ratio);

        let cx = self.x + self.w / 2.0;
        let cy = self.y + self.h / 2.0;

        let w_from_h = self.h * norm_ratio;
        let mut w = if self.w > w_from_h { w_from_h } else { self.w };

        w = w.max(min_w_eff);
        let mut h = w / norm_ratio;

        if w > 1.0 {
            w = 1.0;
            h = w / norm_ratio;
        }
        if h > 1.0 {
            h = 1.0;
            w = h * norm_ratio;
        }

        let mut x = cx - w / 2.0;
        let mut y = cy - h / 2.0;

        if x < 0.0 { x = 0.0; }
        if x + w > 1.0 { x = 1.0 - w; }
        if y < 0.0 { y = 0.0; }
        if y + h > 1.0 { y = 1.0 - h; }

        Self { x, y, w, h }
    }

    pub fn move_by(&mut self, dx: f32, dy: f32) {
        self.x = (self.x + dx).clamp(0.0, (1.0 - self.w).max(0.0));
        self.y = (self.y + dy).clamp(0.0, (1.0 - self.h).max(0.0));
    }

    pub fn resize(
        &self,
        handle: CropHandle,
        dx: f32,
        dy: f32,
        aspect: CropAspect,
        video_w: usize,
        video_h: usize,
    ) -> Self {
        let vw = (video_w.max(1)) as f32;
        let vh = (video_h.max(1)) as f32;
        let (min_w, min_h) = Self::min_size(video_w, video_h);

        match aspect.pixel_ratio() {
            None => self.resize_free(handle, dx, dy, min_w, min_h),
            Some(target_ratio) => {
                let r = target_ratio * (vh / vw);
                self.resize_fixed(handle, dx, dy, r, min_w, min_h)
            }
        }
    }

    fn resize_free(&self, handle: CropHandle, dx: f32, dy: f32, min_w: f32, min_h: f32) -> Self {
        let mut x0 = self.x;
        let mut y0 = self.y;
        let mut x1 = self.x + self.w;
        let mut y1 = self.y + self.h;

        match handle {
            CropHandle::TopLeft => {
                x0 = (x0 + dx).clamp(0.0, x1 - min_w);
                y0 = (y0 + dy).clamp(0.0, y1 - min_h);
            }
            CropHandle::Top => {
                y0 = (y0 + dy).clamp(0.0, y1 - min_h);
            }
            CropHandle::TopRight => {
                x1 = (x1 + dx).clamp(x0 + min_w, 1.0);
                y0 = (y0 + dy).clamp(0.0, y1 - min_h);
            }
            CropHandle::Right => {
                x1 = (x1 + dx).clamp(x0 + min_w, 1.0);
            }
            CropHandle::BottomRight => {
                x1 = (x1 + dx).clamp(x0 + min_w, 1.0);
                y1 = (y1 + dy).clamp(y0 + min_h, 1.0);
            }
            CropHandle::Bottom => {
                y1 = (y1 + dy).clamp(y0 + min_h, 1.0);
            }
            CropHandle::BottomLeft => {
                x0 = (x0 + dx).clamp(0.0, x1 - min_w);
                y1 = (y1 + dy).clamp(y0 + min_h, 1.0);
            }
            CropHandle::Left => {
                x0 = (x0 + dx).clamp(0.0, x1 - min_w);
            }
        }

        Self {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }

    fn resize_fixed(&self, handle: CropHandle, dx: f32, dy: f32, r: f32, min_w: f32, min_h: f32) -> Self {
        let min_w_eff = min_w.max(min_h * r);
        let min_h_eff = min_w_eff / r;

        match handle {
            CropHandle::BottomRight => {
                let x0 = self.x;
                let y0 = self.y;
                let dw = self.w + dx;
                let dh = self.h + dy;
                let mut w_val = if (dw - self.w).abs() >= (dh - self.h).abs() * r {
                    dw
                } else {
                    dh * r
                };
                let w_max = (1.0 - x0).min((1.0 - y0) * r);
                w_val = w_val.clamp(min_w_eff, w_max.max(min_w_eff));
                let h_val = w_val / r;
                Self { x: x0, y: y0, w: w_val, h: h_val }
            }
            CropHandle::BottomLeft => {
                let x1 = self.x + self.w;
                let y0 = self.y;
                let dw = self.w - dx;
                let dh = self.h + dy;
                let mut w_val = if (dw - self.w).abs() >= (dh - self.h).abs() * r {
                    dw
                } else {
                    dh * r
                };
                let w_max = x1.min((1.0 - y0) * r);
                w_val = w_val.clamp(min_w_eff, w_max.max(min_w_eff));
                let h_val = w_val / r;
                Self { x: x1 - w_val, y: y0, w: w_val, h: h_val }
            }
            CropHandle::TopRight => {
                let x0 = self.x;
                let y1 = self.y + self.h;
                let dw = self.w + dx;
                let dh = self.h - dy;
                let mut w_val = if (dw - self.w).abs() >= (dh - self.h).abs() * r {
                    dw
                } else {
                    dh * r
                };
                let w_max = (1.0 - x0).min(y1 * r);
                w_val = w_val.clamp(min_w_eff, w_max.max(min_w_eff));
                let h_val = w_val / r;
                Self { x: x0, y: y1 - h_val, w: w_val, h: h_val }
            }
            CropHandle::TopLeft => {
                let x1 = self.x + self.w;
                let y1 = self.y + self.h;
                let dw = self.w - dx;
                let dh = self.h - dy;
                let mut w_val = if (dw - self.w).abs() >= (dh - self.h).abs() * r {
                    dw
                } else {
                    dh * r
                };
                let w_max = x1.min(y1 * r);
                w_val = w_val.clamp(min_w_eff, w_max.max(min_w_eff));
                let h_val = w_val / r;
                Self { x: x1 - w_val, y: y1 - h_val, w: w_val, h: h_val }
            }
            CropHandle::Right => {
                let x0 = self.x;
                let yc = self.y + self.h / 2.0;
                let mut w_val = self.w + dx;
                let w_max = (1.0 - x0).min(2.0 * yc.min(1.0 - yc) * r);
                w_val = w_val.clamp(min_w_eff, w_max.max(min_w_eff));
                let h_val = w_val / r;
                Self { x: x0, y: yc - h_val / 2.0, w: w_val, h: h_val }
            }
            CropHandle::Left => {
                let x1 = self.x + self.w;
                let yc = self.y + self.h / 2.0;
                let mut w_val = self.w - dx;
                let w_max = x1.min(2.0 * yc.min(1.0 - yc) * r);
                w_val = w_val.clamp(min_w_eff, w_max.max(min_w_eff));
                let h_val = w_val / r;
                Self { x: x1 - w_val, y: yc - h_val / 2.0, w: w_val, h: h_val }
            }
            CropHandle::Bottom => {
                let y0 = self.y;
                let xc = self.x + self.w / 2.0;
                let mut h_val = self.h + dy;
                let h_max = (1.0 - y0).min(2.0 * xc.min(1.0 - xc) / r);
                h_val = h_val.clamp(min_h_eff, h_max.max(min_h_eff));
                let w_val = h_val * r;
                Self { x: xc - w_val / 2.0, y: y0, w: w_val, h: h_val }
            }
            CropHandle::Top => {
                let y1 = self.y + self.h;
                let xc = self.x + self.w / 2.0;
                let mut h_val = self.h - dy;
                let h_max = y1.min(2.0 * xc.min(1.0 - xc) / r);
                h_val = h_val.clamp(min_h_eff, h_max.max(min_h_eff));
                let w_val = h_val * r;
                Self { x: xc - w_val / 2.0, y: y1 - h_val, w: w_val, h: h_val }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExportRequest {
    pub input: PathBuf,
    pub start: f64,
    pub end: f64,
    pub preset: ExportPreset,
    pub audio: AudioMix,
    pub output: PathBuf,
    pub crop: Option<CropRect>,
    /// Text and image overlays, drawn after the crop.
    pub overlays: Vec<Overlay>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExportState {
    Running { fraction: f32, pass: u8 },
    Done { path: PathBuf, bytes: u64 },
    Failed(String),
    Cancelled,
}

pub struct ExportJob {
    state: Arc<Mutex<ExportState>>,
    child: Arc<Mutex<Option<Child>>>,
}

impl ExportJob {
    pub fn state(&self) -> ExportState {
        self.state.lock().map(|s| s.clone()).unwrap_or(ExportState::Failed("lock poisoned".into()))
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state(), ExportState::Running { .. })
    }

    pub fn cancel(&self) {
        if let Ok(mut s) = self.state.lock() {
            *s = ExportState::Cancelled;
        }
        if let Ok(mut c) = self.child.lock() {
            if let Some(child) = c.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

/// `clip_trim_143005.mp4` next to the input.
pub fn default_output(input: &Path, preset: ExportPreset) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("clip");
    let suffix = match preset {
        ExportPreset::Discord => "discord",
        ExportPreset::Streamable => "streamable",
        ExportPreset::Vertical => "vertical",
        ExportPreset::Gif => "gif",
        ExportPreset::Audio(_) => "audio",
        _ => "trim",
    };
    let time = chrono::Local::now().format("%H%M%S");
    input
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{}_{}_{}.{}", stem, suffix, time, preset.extension()))
}

fn cleanup_old_share_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let day = std::time::Duration::from_secs(24 * 3600);
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                if let Ok(mod_time) = meta.modified() {
                    if let Ok(age) = mod_time.elapsed() {
                        if age > day {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
    }
}

fn share_dir() -> PathBuf {
    std::env::temp_dir().join("lapse-share")
}

/// Destination in a temp folder for clips exported only to be shared, so they
/// do not clutter the user's clip library.
pub fn share_output(input: &Path, preset: ExportPreset) -> PathBuf {
    let dir = share_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        cleanup_old_share_files(&dir);
    }
    let file_name = default_output(input, preset)
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "clip.mp4".into());
    dir.join(file_name)
}

/// Whether `path` was made by `share_output`.
pub fn is_share_temp(path: &Path) -> bool {
    path.parent() == Some(share_dir().as_path())
}

/// Deletes `path` once it has been shared, if it is a `share_output` file; never a clip.
pub fn discard_share_temp(path: &Path) {
    if is_share_temp(path) {
        let _ = std::fs::remove_file(path);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub(crate) enum H264Encoder {
    X264,
    Vaapi,
    MediaFoundation,
}

pub(crate) fn h264_encoder() -> H264Encoder {
    #[cfg(windows)]
    return H264Encoder::MediaFoundation;

    #[cfg(not(windows))]
    {
        static ENC: std::sync::OnceLock<H264Encoder> = std::sync::OnceLock::new();
        *ENC.get_or_init(|| {
            let list = crate::utils::ffmpeg_cmd()
                .args(["-hide_banner", "-encoders"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();
            if list.contains("libx264") || !list.contains("h264_vaapi") {
                H264Encoder::X264
            } else {
                H264Encoder::Vaapi
            }
        })
    }
}

pub fn audio_stream_count(input: &Path) -> usize {
    crate::utils::ffprobe_cmd()
        .args(["-v", "error", "-select_streams", "a", "-show_entries", "stream=index", "-of", "csv=p=0"])
        .arg(input)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(1)
}

/// Bitrate budget for a whole clip to fit a size target, then resolution/fps that suit it.
pub fn size_budget_plan(target_bytes: f64, duration: f64, has_audio: bool, audio_kbps: f64, scale: f64) -> (u32, u32, u32) {
    let a_kbps = if has_audio { audio_kbps } else { 0.0 };
    let total_kbps = target_bytes * 8.0 / duration.max(0.5) / 1000.0 * scale;
    let video_kbps = (total_kbps - a_kbps).max(120.0);
    let (height, fps) = if video_kbps >= 4500.0 {
        (1080, 60)
    } else if video_kbps >= 2500.0 {
        (1080, 30)
    } else if video_kbps >= 1200.0 {
        (720, 30)
    } else {
        (480, 30)
    };
    (video_kbps as u32, height, fps)
}

/// Discord: bitrate budget for the whole clip, then resolution/fps that suit it.
pub fn discord_plan(duration: f64, has_audio: bool, scale: f64) -> (u32, u32, u32) {
    size_budget_plan(DISCORD_TARGET_BYTES, duration, has_audio, 96.0, scale)
}

/// Streamable: bitrate budget for the whole clip to fit 250 MB, with 128k AAC audio.
pub fn streamable_plan(duration: f64, has_audio: bool, scale: f64) -> (u32, u32, u32) {
    size_budget_plan(STREAMABLE_TARGET_BYTES, duration, has_audio, 128.0, scale)
}

/// Upload limit of a preset that shrinks clips for a site (Discord, Streamable).
fn size_limit(preset: ExportPreset) -> Option<u64> {
    match preset {
        ExportPreset::Discord => Some(DISCORD_LIMIT_BYTES),
        ExportPreset::Streamable => Some(crate::streamable::MAX_BYTES),
        _ => None,
    }
}

fn size_audio_bitrate(preset: ExportPreset) -> Option<&'static str> {
    match preset {
        ExportPreset::Discord => Some("96k"),
        ExportPreset::Streamable => Some("128k"),
        _ => None,
    }
}

fn size_plan(preset: ExportPreset, duration: f64, has_audio: bool, scale: f64) -> (u32, u32, u32) {
    if preset == ExportPreset::Streamable {
        streamable_plan(duration, has_audio, scale)
    } else {
        discord_plan(duration, has_audio, scale)
    }
}

/// Extracts output resolution and fps from filter strings, falling back to 1080p60.
fn parse_res_fps(filters: &[String]) -> (u32, u32, f64) {
    let mut width = 1920;
    let mut height = 1080;
    let mut fps = 60.0;
    for f in filters {
        for part in f.split(',') {
            let part = part.trim();
            if let Some(scale_args) = part.strip_prefix("scale=") {
                let pieces: Vec<&str> = scale_args.split(':').collect();
                if pieces.len() >= 2 {
                    if let (Ok(w), Ok(h)) = (pieces[0].parse::<u32>(), pieces[1].parse::<u32>()) {
                        width = w;
                        height = h;
                    }
                }
            } else if let Some(fps_arg) = part.strip_prefix("fps=") {
                if let Ok(v) = fps_arg.parse::<f64>() {
                    fps = v;
                }
            }
        }
    }
    (width, height, fps)
}

/// Maps CRF-style quality to bitrate (kbps): 1080p60 is ~12 Mbps at high quality (CRF 18),
/// scaling proportionally by pixels * fps and by 2^((18 - crf) / 6).
pub(crate) fn quality_to_bitrate_kbps(crf: u32, width: u32, height: u32, fps: f64) -> u32 {
    let base_kbps = 12_000.0 * 2.0_f64.powf((18.0 - crf as f64) / 6.0);
    let pixel_fps = (width as f64 * height as f64 * fps).max(1.0);
    let ref_pixel_fps = 1920.0 * 1080.0 * 60.0;
    let kbps = (base_kbps * (pixel_fps / ref_pixel_fps)).round() as u32;
    kbps.clamp(500, 100_000)
}

pub(crate) fn video_encode_args_for(
    encoder: H264Encoder,
    args: &mut Vec<String>,
    filters: &mut Vec<String>,
    quality: Option<u32>,
    bitrate_kbps: Option<u32>,
) {
    match encoder {
        H264Encoder::X264 => {
            args.extend(["-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p"].map(String::from));
            if let Some(crf) = quality {
                args.extend(["-crf".into(), crf.to_string()]);
            }
            if let Some(k) = bitrate_kbps {
                args.extend([
                    "-b:v".into(), format!("{}k", k),
                    "-maxrate".into(), format!("{}k", k * 12 / 10),
                    "-bufsize".into(), format!("{}k", k * 2),
                ]);
            }
        }
        H264Encoder::Vaapi => {
            filters.push("format=nv12".into());
            filters.push("hwupload".into());
            args.extend(["-c:v", "h264_vaapi"].map(String::from));
            if let Some(q) = quality {
                args.extend(["-qp".into(), (q + 2).to_string()]);
            }
            if let Some(k) = bitrate_kbps {
                args.extend(["-b:v".into(), format!("{}k", k), "-maxrate".into(), format!("{}k", k * 12 / 10)]);
            }
        }
        H264Encoder::MediaFoundation => {
            args.extend(["-c:v", "h264_mf", "-pix_fmt", "nv12"].map(String::from));
            let (rate_ctrl, k) = match (bitrate_kbps, quality) {
                (Some(k), Some(_)) => ("pc_vbr", k),
                (Some(k), None) => ("cbr", k),
                (None, Some(q)) => {
                    let (w, h, fps) = parse_res_fps(filters);
                    ("pc_vbr", quality_to_bitrate_kbps(q, w, h, fps))
                }
                (None, None) => ("pc_vbr", 12_000),
            };
            args.extend([
                "-rate_control".into(), rate_ctrl.into(),
                "-b:v".into(), format!("{}k", k),
                "-maxrate".into(), format!("{}k", k * 12 / 10),
            ]);
        }
    }
}

#[allow(dead_code)]
pub(crate) fn video_encode_args(args: &mut Vec<String>, filters: &mut Vec<String>, quality: Option<u32>, bitrate_kbps: Option<u32>) {
    video_encode_args_for(h264_encoder(), args, filters, quality, bitrate_kbps);
}

/// How the audio of an export is produced.
#[derive(Debug, PartialEq)]
enum AudioOut {
    None,
    /// Keep every track as it is (separate tracks stay separate).
    AllTracks,
    /// Only the game/desktop track, untouched (microphone dropped).
    GameTrack,
    /// One track from a filter graph ending in `[aout]`.
    Filter(String),
}

/// `single_track`: the target can only hold one audio track (Discord, MP3), so the
/// game and microphone tracks are mixed together.
fn plan_audio(mix: &AudioMix, tracks: usize, single_track: bool) -> AudioOut {
    if tracks == 0 {
        return AudioOut::None;
    }

    if !mix.tracks.is_empty() {
        let active_tracks: Vec<(usize, &TrackSetting)> = mix
            .tracks
            .iter()
            .enumerate()
            .take(tracks)
            .filter(|(_, t)| t.enabled)
            .collect();

        if active_tracks.is_empty() {
            return AudioOut::None;
        }

        let all_active = active_tracks.len() == tracks;
        let all_untouched = active_tracks.iter().all(|(_, t)| (t.volume - 1.0).abs() < 0.01);

        if all_active && all_untouched && !mix.normalize && (!single_track || tracks == 1) {
            return AudioOut::AllTracks;
        }

        if tracks == 2 && active_tracks.len() == 1 && active_tracks[0].0 == 0 && (active_tracks[0].1.volume - 1.0).abs() < 0.01 && !mix.normalize {
            return AudioOut::GameTrack;
        }

        let mut parts = Vec::new();
        let mut inputs = Vec::new();

        for (idx, track) in &active_tracks {
            let in_tag = format!("[0:a:{}]", idx);
            if (track.volume - 1.0).abs() >= 0.01 {
                let out_tag = if *idx == 1 && tracks == 2 && track.name == "mic" {
                    "[mic]".to_string()
                } else {
                    format!("[a_{}]", idx)
                };
                if track.volume > 1.0 {
                    parts.push(format!("{}volume={:.2},alimiter=limit=0.95{}", in_tag, track.volume, out_tag));
                } else {
                    parts.push(format!("{}volume={:.2}{}", in_tag, track.volume, out_tag));
                }
                inputs.push(out_tag);
            } else {
                inputs.push(in_tag);
            }
        }

        let mut chain = if inputs.len() > 1 {
            format!("{}amix=inputs={}:normalize=0", inputs.join(""), inputs.len())
        } else {
            format!("{}anull", inputs[0])
        };
        if mix.normalize {
            chain.push_str(",loudnorm=I=-16:TP=-1.5:LRA=11");
        }
        parts.push(format!("{}[aout]", chain));
        return AudioOut::Filter(parts.join(";"));
    }

    let mic_track = tracks > 1;
    let game = mix.game;
    let mic = mic_track && mix.mic;
    if !game && !mic {
        return AudioOut::None;
    }
    let untouched = (mic || !mic_track) && (mix.mic_volume - 1.0).abs() < 0.01 && !mix.normalize;
    if game && untouched && (!single_track || tracks == 1) {
        return AudioOut::AllTracks;
    }
    if game && !mic && !mix.normalize {
        return AudioOut::GameTrack;
    }

    let mut parts = Vec::new();
    let mut inputs = Vec::new();
    if game {
        inputs.push("[0:a:0]".to_string());
    }
    if mic {
        if (mix.mic_volume - 1.0).abs() >= 0.01 {
            if mix.mic_volume > 1.0 {
                parts.push(format!("[0:a:1]volume={:.2},alimiter=limit=0.95[mic]", mix.mic_volume));
            } else {
                parts.push(format!("[0:a:1]volume={:.2}[mic]", mix.mic_volume));
            }
            inputs.push("[mic]".into());
        } else {
            inputs.push("[0:a:1]".into());
        }
    }
    let mut chain = if inputs.len() > 1 {
        format!("{}amix=inputs={}:normalize=0", inputs.join(""), inputs.len())
    } else {
        format!("{}anull", inputs[0])
    };
    if mix.normalize {
        chain.push_str(",loudnorm=I=-16:TP=-1.5:LRA=11");
    }
    parts.push(format!("{}[aout]", chain));
    AudioOut::Filter(parts.join(";"))
}

/// Adds the audio maps/filters for `out` with the given encoder arguments.
fn push_audio(args: &mut Vec<String>, out: &AudioOut, codec: &[&str]) {
    match out {
        AudioOut::None => args.push("-an".into()),
        AudioOut::AllTracks => {
            args.extend(["-map", "0:a?"].map(String::from));
            args.extend(codec.iter().map(|s| s.to_string()));
        }
        AudioOut::GameTrack => {
            args.extend(["-map", "0:a:0"].map(String::from));
            args.extend(codec.iter().map(|s| s.to_string()));
        }
        AudioOut::Filter(graph) => {
            args.extend(["-filter_complex".into(), graph.clone(), "-map".into(), "[aout]".into()]);
            args.extend(codec.iter().map(|s| s.to_string()));
        }
    }
}

pub fn probe_video_dimensions(input: &Path) -> (u32, u32) {
    let out = crate::utils::ffprobe_cmd()
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0"])
        .arg(input)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let parts: Vec<&str> = out.trim().split([',', '\n', '\r']).map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if parts.len() >= 2 {
        if let (Ok(w), Ok(h)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
            if w > 0 && h > 0 {
                return (w, h);
            }
        }
    }
    (1920, 1080)
}

/// Escapes text for ffmpeg drawtext filter: \\, ', :, %, and newlines.
pub fn escape_drawtext_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '\'' => out.push_str(r"\'"),
            ':' => out.push_str(r"\:"),
            '%' => out.push_str(r"\\%"),
            '\n' => out.push_str("\n"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out
}

/// Escapes font path for drawtext on Windows and Unix.
pub fn escape_drawtext_font_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    s.replace(':', "\\:")
}

/// Writes egui's default proportional font once to the Lapse cache dir as overlay-font.ttf.
pub fn overlay_font_path() -> PathBuf {
    let cache_dir = directories::ProjectDirs::from("com", "lapse", "lapse")
        .map(|p| p.cache_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("lapse"));
    let font_path = cache_dir.join("overlay-font.ttf");
    if !font_path.exists() {
        let _ = std::fs::create_dir_all(&cache_dir);
        let defs = eframe::egui::FontDefinitions::default();
        let font_name = defs
            .families
            .get(&eframe::egui::FontFamily::Proportional)
            .and_then(|fonts| fonts.first())
            .map(|s| s.as_str())
            .unwrap_or("Ubuntu-Light");
        if let Some(data) = defs.font_data.get(font_name) {
            let _ = std::fs::write(&font_path, data.font.as_ref());
        }
    }
    font_path
}

fn drawtext_filter(overlay: &Overlay, req_start: f64, frame_w: u32, frame_h: u32, font_path_esc: &str) -> Option<String> {
    if let OverlayKind::Text { text, size, color, outline } = &overlay.kind {
        let s = (overlay.start - req_start).max(0.0);
        let e = (overlay.end - req_start).max(0.0);
        let px_x = (overlay.x * frame_w as f32).round() as i32;
        let px_y = (overlay.y * frame_h as f32).round() as i32;
        let font_sz = (size * frame_h as f32).round().max(1.0) as u32;
        let escaped = escape_drawtext_text(text);
        let hex = format!("{:02x}{:02x}{:02x}", color.r(), color.g(), color.b());
        let alpha = color.a() as f32 / 255.0;
        let border = if *outline { ":borderw=2:bordercolor=black@0.8" } else { ":borderw=0" };
        Some(format!(
            "drawtext=fontfile='{}':text='{}':fontsize={}:fontcolor={}@{:.2}:x={}-text_w/2:y={}-text_h/2{}:enable='between(t,{:.3},{:.3})'",
            font_path_esc, escaped, font_sz, hex, alpha, px_x, px_y, border, s, e
        ))
    } else {
        None
    }
}

/// Builds the ffmpeg argument list. `scale` < 1 shrinks the Discord bitrate on a retry.
pub fn build_args(req: &ExportRequest, audio_tracks: usize, scale: f64, output: &Path) -> Vec<String> {
    build_args_for(h264_encoder(), req, audio_tracks, scale, output)
}

pub(crate) fn build_args_for(encoder: H264Encoder, req: &ExportRequest, audio_tracks: usize, scale: f64, output: &Path) -> Vec<String> {
    let overlays = req.overlays.clone();
    build_args_with_overlays(encoder, req, &overlays, audio_tracks, scale, output)
}

fn append_overlay_to_filter_graph(
    overlay: &Overlay,
    i: usize,
    req_start: f64,
    frame_w: u32,
    frame_h: u32,
    font_path_esc: &str,
    cur_v: &str,
    overlay_input_map: &std::collections::HashMap<usize, usize>,
    graph_parts: &mut Vec<String>,
) -> String {
    match &overlay.kind {
        OverlayKind::Text { .. } => {
            if let Some(dt) = drawtext_filter(overlay, req_start, frame_w, frame_h, font_path_esc) {
                let next_v = format!("[v_ov{}]", i);
                graph_parts.push(format!("{}{}{}", cur_v, dt, next_v));
                next_v
            } else {
                cur_v.to_string()
            }
        }
        OverlayKind::Image { path, scale, opacity } => {
            let is_gif = path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("gif")).unwrap_or(false);
            let s = (overlay.start - req_start).max(0.0);
            let e = (overlay.end - req_start).max(0.0);
            let px_x = (overlay.x * frame_w as f32).round() as i32;
            let px_y = (overlay.y * frame_h as f32).round() as i32;
            let this_input = overlay_input_map.get(&i).copied().unwrap_or(1);
            let img_w = (scale * frame_w as f32).round().max(1.0) as u32;
            let img_label = format!("[o{}]", i);
            let next_v = format!("[v_ov{}]", i);
            if is_gif {
                graph_parts.push(format!(
                    "[{}:v]setpts=PTS-STARTPTS+{:.3}/TB,scale={}:-1,format=rgba,colorchannelmixer=aa={:.2}{}",
                    this_input, s, img_w, opacity.clamp(0.0, 1.0), img_label
                ));
                graph_parts.push(format!(
                    "{}{}overlay={}-w/2:{}-h/2:enable='between(t,{:.3},{:.3})':shortest=0:eof_action=pass{}",
                    cur_v, img_label, px_x, px_y, s, e, next_v
                ));
            } else {
                graph_parts.push(format!(
                    "[{}:v]scale={}:-1,format=rgba,colorchannelmixer=aa={:.2}{}",
                    this_input, img_w, opacity.clamp(0.0, 1.0), img_label
                ));
                graph_parts.push(format!(
                    "{}{}overlay={}-w/2:{}-h/2:enable='between(t,{:.3},{:.3})':shortest=1{}",
                    cur_v, img_label, px_x, px_y, s, e, next_v
                ));
            }
            next_v
        }
        OverlayKind::Video { scale, opacity, .. } => {
            let s = (overlay.start - req_start).max(0.0);
            let e = (overlay.end - req_start).max(0.0);
            let px_x = (overlay.x * frame_w as f32).round() as i32;
            let px_y = (overlay.y * frame_h as f32).round() as i32;
            let this_input = overlay_input_map.get(&i).copied().unwrap_or(1);
            let img_w = (scale * frame_w as f32).round().max(1.0) as u32;
            let img_label = format!("[o{}]", i);
            let next_v = format!("[v_ov{}]", i);
            graph_parts.push(format!(
                "[{}:v]setpts=PTS-STARTPTS+{:.3}/TB,scale={}:-1,format=rgba,colorchannelmixer=aa={:.2}{}",
                this_input, s, img_w, opacity.clamp(0.0, 1.0), img_label
            ));
            graph_parts.push(format!(
                "{}{}overlay={}-w/2:{}-h/2:enable='between(t,{:.3},{:.3})':shortest=0:eof_action=pass{}",
                cur_v, img_label, px_x, px_y, s, e, next_v
            ));
            next_v
        }
        OverlayKind::Audio { .. } => cur_v.to_string(),
    }
}

fn plan_overlay_audio(
    mix: &AudioMix,
    audio_tracks: usize,
    duration: f64,
    req_start: f64,
    overlays: &[Overlay],
    overlay_input_map: &std::collections::HashMap<usize, usize>,
) -> AudioOut {
    let mut parts: Vec<String> = Vec::new();

    if !mix.tracks.is_empty() {
        let active: Vec<(usize, &TrackSetting)> = mix
            .tracks
            .iter()
            .enumerate()
            .take(audio_tracks)
            .filter(|(_, t)| t.enabled)
            .collect();
        if !active.is_empty() {
            let mut inputs = Vec::new();
            for (idx, track) in &active {
                let in_tag = format!("[0:a:{}]", idx);
                if (track.volume - 1.0).abs() >= 0.01 {
                    let out_tag = if *idx == 1 && audio_tracks == 2 && track.name == "mic" {
                        "[mic]".to_string()
                    } else {
                        format!("[abase_{}]", idx)
                    };
                    if track.volume > 1.0 {
                        parts.push(format!("{}volume={:.2},alimiter=limit=0.95{}", in_tag, track.volume, out_tag));
                    } else {
                        parts.push(format!("{}volume={:.2}{}", in_tag, track.volume, out_tag));
                    }
                    inputs.push(out_tag);
                } else {
                    inputs.push(in_tag);
                }
            }
            if inputs.len() > 1 {
                parts.push(format!("{}amix=inputs={}:normalize=0[abase]", inputs.join(""), inputs.len()));
            } else {
                parts.push(format!("{}anull[abase]", inputs[0]));
            }
        } else {
            parts.push(format!(
                "anullsrc=channel_layout=stereo:sample_rate=48000,atrim=0:{:.3}[abase]",
                duration
            ));
        }
    } else {
        let mic_track = audio_tracks > 1;
        let game = mix.game && audio_tracks > 0;
        let mic = mic_track && mix.mic;
        let clip_has_audio = game || mic;

        if clip_has_audio {
            if game && mic {
                let mut inputs = vec!["[0:a:0]".to_string()];
                if (mix.mic_volume - 1.0).abs() >= 0.01 {
                    if mix.mic_volume > 1.0 {
                        parts.push(format!("[0:a:1]volume={:.2},alimiter=limit=0.95[mic]", mix.mic_volume));
                    } else {
                        parts.push(format!("[0:a:1]volume={:.2}[mic]", mix.mic_volume));
                    }
                    inputs.push("[mic]".to_string());
                } else {
                    inputs.push("[0:a:1]".to_string());
                }
                parts.push(format!("{}amix=inputs=2:normalize=0[abase]", inputs.join("")));
            } else if game {
                parts.push("[0:a:0]anull[abase]".to_string());
            } else if mic {
                if (mix.mic_volume - 1.0).abs() >= 0.01 {
                    if mix.mic_volume > 1.0 {
                        parts.push(format!("[0:a:1]volume={:.2},alimiter=limit=0.95[abase]", mix.mic_volume));
                    } else {
                        parts.push(format!("[0:a:1]volume={:.2}[abase]", mix.mic_volume));
                    }
                } else {
                    parts.push("[0:a:1]anull[abase]".to_string());
                }
            }
        } else {
            parts.push(format!(
                "anullsrc=channel_layout=stereo:sample_rate=48000,atrim=0:{:.3}[abase]",
                duration
            ));
        }
    }

    let mut sound_overlay_entries = Vec::new();
    for (i, o) in overlays.iter().enumerate() {
        match &o.kind {
            OverlayKind::Audio { volume, .. } => {
                if let Some(&inp) = overlay_input_map.get(&i) {
                    sound_overlay_entries.push((inp, *volume, o));
                }
            }
            OverlayKind::Video { sound: true, volume, .. } => {
                if let Some(&inp) = overlay_input_map.get(&i) {
                    sound_overlay_entries.push((inp, *volume, o));
                }
            }
            _ => {}
        }
    }

    for (k, &(inp, vol, o)) in sound_overlay_entries.iter().enumerate() {
        let len = (o.end - o.start).max(0.0);
        let rel_start = (o.start - req_start).max(0.0);
        let delay_ms = (rel_start * 1000.0).round() as u64;
        parts.push(format!(
            "[{}:a]atrim=0:{:.3},volume={:.2},adelay={}:all=1[a_ov{}]",
            inp, len, vol, delay_ms, k
        ));
    }

    let mut amix_inputs = vec!["[abase]".to_string()];
    for k in 0..sound_overlay_entries.len() {
        amix_inputs.push(format!("[a_ov{}]", k));
    }
    let total_inputs = amix_inputs.len();
    let mut final_chain = format!("{}amix=inputs={}:normalize=0:duration=first", amix_inputs.join(""), total_inputs);
    if mix.normalize {
        final_chain.push_str(",loudnorm=I=-16:TP=-1.5:LRA=11");
    }
    final_chain.push_str("[aout]");
    parts.push(final_chain);

    AudioOut::Filter(parts.join(";"))
}

pub fn build_args_with_overlays(
    encoder: H264Encoder,
    req: &ExportRequest,
    overlays: &[Overlay],
    audio_tracks: usize,
    scale: f64,
    output: &Path,
) -> Vec<String> {
    let duration = (req.end - req.start).max(0.05);
    let mut args: Vec<String> = ["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats"]
        .map(String::from)
        .to_vec();

    let (vw, vh) = probe_video_dimensions(&req.input);
    let effective_crop = req.crop.or_else(|| {
        if req.preset == ExportPreset::Vertical {
            Some(CropRect::default_for_aspect(vw as usize, vh as usize, CropAspect::Vertical9x16))
        } else {
            None
        }
    });

    let (frame_w, frame_h) = if let Some(c) = effective_crop {
        let (cw, ch, _, _) = c.to_pixels(vw, vh);
        (cw, ch)
    } else {
        (vw, vh)
    };

    let reencodes = matches!(req.preset, ExportPreset::Precise | ExportPreset::Discord | ExportPreset::Streamable | ExportPreset::Vertical)
        || (req.preset == ExportPreset::Fast && (effective_crop.is_some() || !overlays.is_empty()));
    let vaapi = !cfg!(windows) && encoder == H264Encoder::Vaapi && reencodes;
    if vaapi {
        args.extend(["-vaapi_device".into(), "/dev/dri/renderD128".into()]);
    }
    args.extend(["-ss".into(), format!("{:.3}", req.start), "-t".into(), format!("{:.3}", duration), "-i".into()]);
    args.push(req.input.to_string_lossy().to_string());

    let has_image_overlays = !matches!(req.preset, ExportPreset::Audio(_))
        && overlays.iter().any(|o| matches!(o.kind, OverlayKind::Image { .. } | OverlayKind::Video { .. }));

    let mut overlay_input_map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let mut next_input_idx = 1;

    if req.preset == ExportPreset::Gif {
        for (i, o) in overlays.iter().enumerate() {
            match &o.kind {
                OverlayKind::Image { path, .. } => {
                    let is_gif = path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("gif")).unwrap_or(false);
                    if is_gif {
                        args.extend(["-ignore_loop".into(), "0".into(), "-i".into(), path.to_string_lossy().to_string()]);
                    } else {
                        args.extend(["-loop".into(), "1".into(), "-i".into(), path.to_string_lossy().to_string()]);
                    }
                    overlay_input_map.insert(i, next_input_idx);
                    next_input_idx += 1;
                }
                OverlayKind::Video { path, .. } => {
                    args.extend(["-stream_loop".into(), "-1".into(), "-i".into(), path.to_string_lossy().to_string()]);
                    overlay_input_map.insert(i, next_input_idx);
                    next_input_idx += 1;
                }
                _ => {}
            }
        }
    } else if matches!(req.preset, ExportPreset::Audio(_)) {
        for (i, o) in overlays.iter().enumerate() {
            match &o.kind {
                OverlayKind::Audio { path, .. } => {
                    args.extend(["-i".into(), path.to_string_lossy().to_string()]);
                    overlay_input_map.insert(i, next_input_idx);
                    next_input_idx += 1;
                }
                OverlayKind::Video { path, sound: true, .. } => {
                    args.extend(["-stream_loop".into(), "-1".into(), "-i".into(), path.to_string_lossy().to_string()]);
                    overlay_input_map.insert(i, next_input_idx);
                    next_input_idx += 1;
                }
                _ => {}
            }
        }
    } else {
        for (i, o) in overlays.iter().enumerate() {
            match &o.kind {
                OverlayKind::Image { path, .. } => {
                    let is_gif = path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("gif")).unwrap_or(false);
                    if is_gif {
                        args.extend(["-ignore_loop".into(), "0".into(), "-i".into(), path.to_string_lossy().to_string()]);
                    } else {
                        args.extend(["-loop".into(), "1".into(), "-i".into(), path.to_string_lossy().to_string()]);
                    }
                    overlay_input_map.insert(i, next_input_idx);
                    next_input_idx += 1;
                }
                OverlayKind::Video { path, .. } => {
                    args.extend(["-stream_loop".into(), "-1".into(), "-i".into(), path.to_string_lossy().to_string()]);
                    overlay_input_map.insert(i, next_input_idx);
                    next_input_idx += 1;
                }
                OverlayKind::Audio { path, .. } => {
                    args.extend(["-i".into(), path.to_string_lossy().to_string()]);
                    overlay_input_map.insert(i, next_input_idx);
                    next_input_idx += 1;
                }
                OverlayKind::Text { .. } => {}
            }
        }
    }

    let single_track = matches!(req.preset, ExportPreset::Discord | ExportPreset::Streamable | ExportPreset::Vertical | ExportPreset::Audio(_));
    let has_sound_overlays = req.preset != ExportPreset::Gif
        && overlays.iter().any(|o| match &o.kind {
            OverlayKind::Audio { .. } => true,
            OverlayKind::Video { sound: true, .. } => true,
            _ => false,
        });

    let audio = if has_sound_overlays {
        plan_overlay_audio(&req.audio, audio_tracks, duration, req.start, overlays, &overlay_input_map)
    } else {
        plan_audio(&req.audio, audio_tracks, single_track)
    };

    if req.preset == ExportPreset::Gif {
        if overlays.is_empty() {
            let filter_graph = if let Some(c) = effective_crop {
                let (cw, ch, cx, cy) = c.to_pixels(vw, vh);
                format!("[0:v]crop={}:{}:{}:{},fps=15,scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4", cw, ch, cx, cy)
            } else {
                "[0:v]fps=15,scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4".into()
            };
            args.extend(["-filter_complex".into(), filter_graph, "-loop".into(), "0".into()]);
        } else {
            let mut graph_parts: Vec<String> = Vec::new();
            let mut cur_v = "[0:v]".to_string();
            if let Some(c) = effective_crop {
                let (cw, ch, cx, cy) = c.to_pixels(vw, vh);
                graph_parts.push(format!("{}crop={}:{}:{}:{}[vcrop]", cur_v, cw, ch, cx, cy));
                cur_v = "[vcrop]".to_string();
            }
            let font_path_esc = escape_drawtext_font_path(&overlay_font_path());
            for (i, overlay) in overlays.iter().enumerate() {
                cur_v = append_overlay_to_filter_graph(
                    overlay,
                    i,
                    req.start,
                    frame_w,
                    frame_h,
                    &font_path_esc,
                    &cur_v,
                    &overlay_input_map,
                    &mut graph_parts,
                );
            }
            graph_parts.push(format!(
                "{}fps=15,scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
                cur_v
            ));
            args.extend(["-filter_complex".into(), graph_parts.join(";"), "-loop".into(), "0".into()]);
        }
    } else if has_image_overlays {
        let mut graph_parts: Vec<String> = Vec::new();
        let mut cur_v = "[0:v]".to_string();
        if let Some(c) = effective_crop {
            let (cw, ch, cx, cy) = c.to_pixels(vw, vh);
            graph_parts.push(format!("{}crop={}:{}:{}:{}[vcrop]", cur_v, cw, ch, cx, cy));
            cur_v = "[vcrop]".to_string();
        }
        let font_path_esc = escape_drawtext_font_path(&overlay_font_path());
        for (i, overlay) in overlays.iter().enumerate() {
            cur_v = append_overlay_to_filter_graph(
                overlay,
                i,
                req.start,
                frame_w,
                frame_h,
                &font_path_esc,
                &cur_v,
                &overlay_input_map,
                &mut graph_parts,
            );
        }

        let mut enc_filters: Vec<String> = Vec::new();
        match req.preset {
            ExportPreset::Vertical => {
                graph_parts.push(format!("{}scale=1080:1920:flags=lanczos[vfinal]", cur_v));
                cur_v = "[vfinal]".to_string();
                enc_filters.push("scale=1080:1920:flags=lanczos".into());
                video_encode_args_for(encoder, &mut args, &mut enc_filters, Some(20), None);
            }
            ExportPreset::Discord | ExportPreset::Streamable => {
                let (video_kbps, height, fps) = size_plan(req.preset, duration, audio != AudioOut::None, scale);
                graph_parts.push(format!("{}scale=-2:'min({},ih)',fps={}[vfinal]", cur_v, height, fps));
                cur_v = "[vfinal]".to_string();
                enc_filters.push(format!("scale=-2:'min({},ih)'", height));
                enc_filters.push(format!("fps={}", fps));
                video_encode_args_for(encoder, &mut args, &mut enc_filters, None, Some(video_kbps));
            }
            _ => {
                video_encode_args_for(encoder, &mut args, &mut enc_filters, Some(18), None);
            }
        }
        if vaapi {
            graph_parts.push(format!("{}format=nv12,hwupload[vfinal]", cur_v));
            cur_v = "[vfinal]".to_string();
        }

        let video_complex = graph_parts.join(";");
        let (full_complex, map_a) = match &audio {
            AudioOut::Filter(a_graph) => (format!("{};{}", video_complex, a_graph), Some("[aout]".to_string())),
            _ => (video_complex, None),
        };
        args.extend(["-filter_complex".into(), full_complex]);
        args.extend(["-map".into(), cur_v]);

        let a_codec: &[&str] = &["-c:a", "aac", "-b:a", size_audio_bitrate(req.preset).unwrap_or("192k")];

        match audio {
            AudioOut::None => args.push("-an".into()),
            AudioOut::AllTracks => {
                args.extend(["-map", "0:a?"].map(String::from));
                args.extend(a_codec.iter().map(|s| s.to_string()));
            }
            AudioOut::GameTrack => {
                args.extend(["-map", "0:a:0"].map(String::from));
                args.extend(a_codec.iter().map(|s| s.to_string()));
            }
            AudioOut::Filter(_) => {
                if let Some(m) = map_a {
                    args.extend(["-map".into(), m]);
                }
                args.extend(a_codec.iter().map(|s| s.to_string()));
            }
        }
    } else {
        let mut filters: Vec<String> = Vec::new();
        if let Some(c) = effective_crop {
            if !matches!(req.preset, ExportPreset::Audio(_)) {
                let (cw, ch, cx, cy) = c.to_pixels(vw, vh);
                filters.push(format!("crop={}:{}:{}:{}", cw, ch, cx, cy));
            }
        }

        if !matches!(req.preset, ExportPreset::Audio(_)) {
            let font_path_esc = escape_drawtext_font_path(&overlay_font_path());
            for overlay in overlays {
                if let Some(dt) = drawtext_filter(overlay, req.start, frame_w, frame_h, &font_path_esc) {
                    filters.push(dt);
                }
            }
        }

        match req.preset {
            ExportPreset::Fast => {
                if effective_crop.is_some() || !overlays.is_empty() {
                    args.extend(["-map", "0:v:0"].map(String::from));
                    push_audio(&mut args, &audio, &["-c:a", "aac", "-b:a", "192k"]);
                    video_encode_args_for(encoder, &mut args, &mut filters, Some(18), None);
                } else {
                    args.extend(["-map", "0:v:0", "-c:v", "copy", "-avoid_negative_ts", "make_zero"].map(String::from));
                    let copy = matches!(audio, AudioOut::AllTracks | AudioOut::GameTrack);
                    let codec: &[&str] = if copy { &["-c:a", "copy"] } else { &["-c:a", "aac", "-b:a", "192k"] };
                    push_audio(&mut args, &audio, codec);
                }
            }
            ExportPreset::Precise => {
                args.extend(["-map", "0:v:0"].map(String::from));
                push_audio(&mut args, &audio, &["-c:a", "aac", "-b:a", "192k"]);
                video_encode_args_for(encoder, &mut args, &mut filters, Some(18), None);
            }
            ExportPreset::Discord | ExportPreset::Streamable => {
                let (video_kbps, height, fps) = size_plan(req.preset, duration, audio != AudioOut::None, scale);
                filters.push(format!("scale=-2:'min({},ih)'", height));
                filters.push(format!("fps={}", fps));
                video_encode_args_for(encoder, &mut args, &mut filters, None, Some(video_kbps));
                args.extend(["-map", "0:v:0"].map(String::from));
                push_audio(&mut args, &audio, &["-c:a", "aac", "-b:a", size_audio_bitrate(req.preset).unwrap_or("192k")]);
            }
            ExportPreset::Vertical => {
                filters.push("scale=1080:1920:flags=lanczos".into());
                video_encode_args_for(encoder, &mut args, &mut filters, Some(20), None);
                args.extend(["-map", "0:v:0"].map(String::from));
                push_audio(&mut args, &audio, &["-c:a", "aac", "-b:a", "192k"]);
            }
            ExportPreset::Gif => unreachable!(),
            ExportPreset::Audio(fmt) => {
                args.push("-vn".into());
                let codec: &[&str] = match fmt {
                    AudioFormat::Mp3 => &["-c:a", "libmp3lame", "-q:a", "2"],
                    AudioFormat::Opus => &["-c:a", "libopus", "-b:a", "160k"],
                    AudioFormat::Aac => &["-c:a", "aac", "-b:a", "192k"],
                    AudioFormat::Flac => &["-c:a", "flac"],
                    AudioFormat::Wav => &["-c:a", "pcm_s16le"],
                };
                push_audio(&mut args, &audio, codec);
            }
        }

        if !filters.is_empty() {
            args.extend(["-vf".into(), filters.join(",")]);
        }
    }

    if req.preset.extension() == "mp4" || matches!(req.preset, ExportPreset::Audio(AudioFormat::Aac)) {
        args.extend(["-movflags", "+faststart"].map(String::from));
    }
    args.push(output.to_string_lossy().to_string());
    args
}

/// Hidden temp name in the same folder, so the clip library ignores half-written files.
fn temp_output(output: &Path) -> PathBuf {
    let name = output.file_name().unwrap_or_default().to_string_lossy();
    output.with_file_name(format!(".{}.part.{}", name, output.extension().and_then(|e| e.to_str()).unwrap_or("mp4")))
}

pub fn start(req: ExportRequest) -> ExportJob {
    let duration = (req.end - req.start).max(0.05);
    let output = req.output.clone();
    let limit = size_limit(req.preset);
    run_job(duration, output, move |tmp| {
        let tracks = audio_stream_count(&req.input);
        // Discord and Streamable exports retry with a smaller budget if the first attempt is too big.
        let scales: &[f64] = if limit.is_some() { &[1.0, 0.85, 0.7] } else { &[1.0] };
        scales.iter().map(|scale| build_args(&req, tracks, *scale, tmp)).collect()
    }, move |bytes| limit.map_or(true, |l| bytes <= l))
}

/// Runs ffmpeg in the background with progress, cancel and a hidden temp file.
/// `attempts` builds one argument list per attempt (on the worker thread, so probing
/// the input never blocks the UI); the next one runs when `good_enough` rejects a result.
pub(crate) fn run_job(
    duration: f64,
    output: PathBuf,
    attempts: impl FnOnce(&Path) -> Vec<Vec<String>> + Send + 'static,
    good_enough: impl Fn(u64) -> bool + Send + 'static,
) -> ExportJob {
    let state = Arc::new(Mutex::new(ExportState::Running { fraction: 0.0, pass: 1 }));
    let child_slot: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let job = ExportJob { state: Arc::clone(&state), child: Arc::clone(&child_slot) };

    std::thread::spawn(move || {
        let tmp = temp_output(&output);
        let attempts = attempts(&tmp);
        let count = attempts.len();

        for (pass, args) in attempts.into_iter().enumerate() {
            if matches!(*state.lock().unwrap(), ExportState::Cancelled) {
                let _ = std::fs::remove_file(&tmp);
                return;
            }
            let spawned = crate::utils::ffmpeg_cmd()
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            let mut child = match spawned {
                Ok(c) => c,
                Err(e) => {
                    *state.lock().unwrap() = ExportState::Failed(format!("ffmpeg could not start: {}", e));
                    return;
                }
            };
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            *child_slot.lock().unwrap() = Some(child);

            let err_text = Arc::new(Mutex::new(String::new()));
            if let Some(err) = stderr {
                let err_text = Arc::clone(&err_text);
                std::thread::spawn(move || {
                    for line in BufReader::new(err).lines().map_while(|l| l.ok()) {
                        if let Ok(mut t) = err_text.lock() {
                            *t = line;
                        }
                    }
                });
            }
            if let Some(out) = stdout {
                for line in BufReader::new(out).lines().map_while(|l| l.ok()) {
                    // out_time_us=1234567 (older ffmpeg names the same value out_time_ms)
                    let us = line
                        .strip_prefix("out_time_us=")
                        .or_else(|| line.strip_prefix("out_time_ms="))
                        .and_then(|v| v.parse::<f64>().ok());
                    if let Some(us) = us {
                        if let Ok(mut s) = state.lock() {
                            if matches!(*s, ExportState::Running { .. }) {
                                *s = ExportState::Running {
                                    fraction: ((us / 1e6) / duration).clamp(0.0, 1.0) as f32,
                                    pass: pass as u8 + 1,
                                };
                            }
                        }
                    }
                }
            }
            let status = child_slot.lock().unwrap().take().and_then(|mut c| c.wait().ok());

            if matches!(*state.lock().unwrap(), ExportState::Cancelled) {
                let _ = std::fs::remove_file(&tmp);
                return;
            }
            if !status.map(|s| s.success()).unwrap_or(false) {
                let _ = std::fs::remove_file(&tmp);
                let msg = err_text.lock().map(|t| t.clone()).unwrap_or_default();
                *state.lock().unwrap() = ExportState::Failed(if msg.is_empty() { "ffmpeg failed".into() } else { msg });
                return;
            }

            let bytes = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
            if !good_enough(bytes) && pass + 1 < count {
                continue;
            }
            if std::fs::rename(&tmp, &output).is_err() {
                *state.lock().unwrap() = ExportState::Failed("could not move the exported file into place".into());
                return;
            }
            crate::clip_index::index().refresh();
            *state.lock().unwrap() = ExportState::Done { path: output.clone(), bytes };
            return;
        }
    });
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(preset: ExportPreset) -> ExportRequest {
        ExportRequest {
            input: "/v/in.mp4".into(),
            start: 1.5,
            end: 11.5,
            preset,
            audio: AudioMix::default(),
            output: "/v/out.mp4".into(),
            crop: None,
            overlays: Vec::new(),
        }
    }

    #[test]
    fn vertical_export_crops_to_9_16() {
        let args = build_args(&req(ExportPreset::Vertical), 2, 1.0, Path::new("/v/out.mp4"));
        let vf = args.iter().position(|a| a == "-vf").map(|i| args[i + 1].clone()).unwrap();
        assert!(vf.starts_with("crop=606:1080:656:0,scale=1080:1920"), "{}", vf);
    }

    #[test]
    fn test_normalized_to_pixel_even_rounding() {
        let crop = CropRect { x: 0.1, y: 0.2, w: 0.5, h: 0.6 };
        let (w, h, x, y) = crop.to_pixels(1920, 1080);
        assert_eq!((w, h, x, y), (960, 648, 192, 216));
        assert_eq!(w % 2, 0);
        assert_eq!(h % 2, 0);
        assert_eq!(x % 2, 0);
        assert_eq!(y % 2, 0);

        let odd_crop = CropRect { x: 0.051, y: 0.099, w: 0.333, h: 0.555 };
        let (ow, oh, ox, oy) = odd_crop.to_pixels(1920, 1080);
        assert_eq!(ow % 2, 0);
        assert_eq!(oh % 2, 0);
        assert_eq!(ox % 2, 0);
        assert_eq!(oy % 2, 0);
        assert!(ox + ow <= 1920);
        assert!(oy + oh <= 1080);

        // Clamping at boundary
        let edge_crop = CropRect { x: 0.9, y: 0.9, w: 0.4, h: 0.4 };
        let (ew, eh, ex, ey) = edge_crop.to_pixels(1920, 1080);
        assert_eq!(ew % 2, 0);
        assert_eq!(eh % 2, 0);
        assert_eq!(ex % 2, 0);
        assert_eq!(ey % 2, 0);
        assert!(ex + ew <= 1920);
        assert!(ey + eh <= 1080);
    }

    #[test]
    fn test_aspect_preserving_resize_math() {
        let crop = CropRect::default_for_aspect(1920, 1080, CropAspect::Vertical9x16);
        let ratio_init = (crop.w * 1920.0) / (crop.h * 1080.0);
        assert!((ratio_init - 9.0 / 16.0).abs() < 1e-3);

        // Corner resize preserves 9:16 aspect
        let r_corner = crop.resize(CropHandle::BottomRight, 0.05, 0.05, CropAspect::Vertical9x16, 1920, 1080);
        let ratio_corner = (r_corner.w * 1920.0) / (r_corner.h * 1080.0);
        assert!((ratio_corner - 9.0 / 16.0).abs() < 1e-3, "ratio: {}", ratio_corner);

        // Corner resize from TopLeft
        let r_tl = crop.resize(CropHandle::TopLeft, 0.02, 0.02, CropAspect::Vertical9x16, 1920, 1080);
        let ratio_tl = (r_tl.w * 1920.0) / (r_tl.h * 1080.0);
        assert!((ratio_tl - 9.0 / 16.0).abs() < 1e-3, "ratio: {}", ratio_tl);

        // Square aspect
        let r_sq = CropRect::default_for_aspect(1920, 1080, CropAspect::Square);
        let ratio_sq = (r_sq.w * 1920.0) / (r_sq.h * 1080.0);
        assert!((ratio_sq - 1.0).abs() < 1e-3);

        // 16:9 aspect
        let r_wide = CropRect::default_for_aspect(1920, 1080, CropAspect::Wide16x9);
        let ratio_wide = (r_wide.w * 1920.0) / (r_wide.h * 1080.0);
        assert!((ratio_wide - 16.0 / 9.0).abs() < 1e-3);

        // Edge resize adjusts other dimension around center
        let base_sq = CropRect { x: 0.3, y: 0.2, w: 0.3, h: 0.3 * (1920.0 / 1080.0) };
        let yc_before = base_sq.y + base_sq.h / 2.0;
        let r_edge_r = base_sq.resize(CropHandle::Right, 0.04, 0.0, CropAspect::Square, 1920, 1080);
        let yc_after = r_edge_r.y + r_edge_r.h / 2.0;
        assert!((yc_before - yc_after).abs() < 1e-4, "yc moved: {} vs {}", yc_before, yc_after);

        let xc_before = base_sq.x + base_sq.w / 2.0;
        let r_edge_b = base_sq.resize(CropHandle::Bottom, 0.0, 0.04, CropAspect::Square, 1920, 1080);
        let xc_after = r_edge_b.x + r_edge_b.w / 2.0;
        assert!((xc_before - xc_after).abs() < 1e-4, "xc moved: {} vs {}", xc_before, xc_after);
    }

    #[test]
    fn test_crop_clamping() {
        let mut crop = CropRect { x: 0.1, y: 0.1, w: 0.5, h: 0.5 };
        crop.move_by(10.0, 10.0);
        assert!(crop.x + crop.w <= 1.0);
        assert!(crop.y + crop.h <= 1.0);

        crop.move_by(-10.0, -10.0);
        assert!(crop.x >= 0.0);
        assert!(crop.y >= 0.0);

        let (min_w, min_h) = CropRect::min_size(1920, 1080);
        assert!((min_w - 108.0 / 1920.0).abs() < 1e-4);
        assert!((min_h - 108.0 / 1080.0).abs() < 1e-4);

        let shrunk = crop.resize(CropHandle::BottomRight, -0.9, -0.9, CropAspect::Free, 1920, 1080);
        assert!(shrunk.w >= min_w - 1e-5);
        assert!(shrunk.h >= min_h - 1e-5);
    }

    #[test]
    fn test_build_args_crop_filter() {
        // Fast with crop -> re-encodes, contains crop filter
        let mut r_fast = req(ExportPreset::Fast);
        r_fast.crop = Some(CropRect { x: 0.25, y: 0.25, w: 0.5, h: 0.5 });
        let args_fast = build_args(&r_fast, 2, 1.0, Path::new("/v/out.mp4"));
        let joined_fast = args_fast.join(" ");
        assert!(!joined_fast.contains("-c:v copy"));
        let vf_fast = args_fast.iter().position(|a| a == "-vf").map(|i| args_fast[i + 1].clone()).unwrap();
        assert!(vf_fast.starts_with("crop=960:540:480:270"));

        // Fast without crop -> stream copy
        let r_fast_clean = req(ExportPreset::Fast);
        let args_clean = build_args(&r_fast_clean, 2, 1.0, Path::new("/v/out.mp4"));
        assert!(args_clean.join(" ").contains("-c:v copy"));

        // Precise with crop
        let mut r_prec = req(ExportPreset::Precise);
        r_prec.crop = Some(CropRect { x: 0.25, y: 0.25, w: 0.5, h: 0.5 });
        let args_prec = build_args(&r_prec, 2, 1.0, Path::new("/v/out.mp4"));
        let vf_prec = args_prec.iter().position(|a| a == "-vf").map(|i| args_prec[i + 1].clone()).unwrap();
        assert!(vf_prec.starts_with("crop=960:540:480:270"));

        // Vertical with crop -> crop filter before scale 1080:1920
        let mut r_vert = req(ExportPreset::Vertical);
        r_vert.crop = Some(CropRect { x: 0.25, y: 0.25, w: 0.5, h: 0.5 });
        let args_vert = build_args(&r_vert, 2, 1.0, Path::new("/v/out.mp4"));
        let vf_vert = args_vert.iter().position(|a| a == "-vf").map(|i| args_vert[i + 1].clone()).unwrap();
        assert!(vf_vert.starts_with("crop=960:540:480:270,scale=1080:1920:flags=lanczos"));

        // Discord with crop -> crop filter before scale
        let mut r_disc = req(ExportPreset::Discord);
        r_disc.crop = Some(CropRect { x: 0.25, y: 0.25, w: 0.5, h: 0.5 });
        let args_disc = build_args(&r_disc, 2, 1.0, Path::new("/v/out.mp4"));
        let vf_disc = args_disc.iter().position(|a| a == "-vf").map(|i| args_disc[i + 1].clone()).unwrap();
        assert!(vf_disc.starts_with("crop=960:540:480:270,scale=-2:'min("));

        // Gif with crop -> crop filter inside -filter_complex
        let mut r_gif = req(ExportPreset::Gif);
        r_gif.crop = Some(CropRect { x: 0.25, y: 0.25, w: 0.5, h: 0.5 });
        let args_gif = build_args(&r_gif, 2, 1.0, Path::new("/v/out.gif"));
        let fc_gif = args_gif.iter().position(|a| a == "-filter_complex").map(|i| args_gif[i + 1].clone()).unwrap();
        assert!(fc_gif.starts_with("[0:v]crop=960:540:480:270,fps=15,scale=480:-1"));
    }

    #[test]
    fn discord_budget_fits_limit() {
        let (kbps, h, _) = discord_plan(30.0, true, 1.0);
        let total_bytes = (kbps as f64 + 96.0) * 1000.0 / 8.0 * 30.0;
        assert!(total_bytes < DISCORD_LIMIT_BYTES as f64, "{}", total_bytes);
        assert_eq!(h, 1080);
        assert_eq!(discord_plan(300.0, true, 1.0).1, 480);
    }

    #[test]
    fn streamable_budget_fits_limit() {
        let (kbps, h, _) = streamable_plan(540.0, true, 1.0);
        let total_bytes = (kbps as f64 + 128.0) * 1000.0 / 8.0 * 540.0;
        assert!(total_bytes < crate::streamable::MAX_BYTES as f64, "{}", total_bytes);
        assert_eq!(h, 1080);
    }

    #[test]
    fn fast_export_copies_all_audio_tracks() {
        let args = build_args(&req(ExportPreset::Fast), 2, 1.0, Path::new("/v/out.mp4"));
        let joined = args.join(" ");
        assert!(joined.contains("-ss 1.500 -t 10.000 -i /v/in.mp4"));
        assert!(joined.contains("-c:v copy"));
        assert!(joined.contains("-map 0:a? -c:a copy"));
    }

    #[test]
    fn muted_export_drops_audio() {
        let mut r = req(ExportPreset::Precise);
        r.audio = AudioMix::muted();
        let joined = build_args(&r, 1, 1.0, Path::new("/v/out.mp4")).join(" ");
        assert!(!joined.contains("0:a"));
    }

    #[test]
    fn audio_mix_choices() {
        let mix = |game, mic, vol, normalize| AudioMix { game, mic, mic_volume: vol, normalize, tracks: Vec::new() };
        assert_eq!(plan_audio(&mix(true, true, 1.0, false), 2, false), AudioOut::AllTracks);
        assert_eq!(plan_audio(&mix(true, false, 1.0, false), 2, false), AudioOut::GameTrack);
        assert_eq!(plan_audio(&mix(false, false, 1.0, false), 2, false), AudioOut::None);
        assert_eq!(plan_audio(&mix(false, true, 1.0, false), 2, false), AudioOut::Filter("[0:a:1]anull[aout]".into()));
        assert_eq!(
            plan_audio(&mix(true, true, 1.5, true), 2, false),
            AudioOut::Filter("[0:a:1]volume=1.50,alimiter=limit=0.95[mic];[0:a:0][mic]amix=inputs=2:normalize=0,loudnorm=I=-16:TP=-1.5:LRA=11[aout]".into())
        );
        assert_eq!(
            plan_audio(&mix(true, true, 2.5, false), 2, false),
            AudioOut::Filter("[0:a:1]volume=2.50,alimiter=limit=0.95[mic];[0:a:0][mic]amix=inputs=2:normalize=0[aout]".into())
        );
        // Discord/MP3 hold one track: game + mic are mixed.
        assert_eq!(plan_audio(&mix(true, true, 1.0, false), 2, true), AudioOut::Filter("[0:a:0][0:a:1]amix=inputs=2:normalize=0[aout]".into()));
        // A clip with only one track ignores the microphone switch.
        assert_eq!(plan_audio(&mix(true, false, 1.0, false), 1, false), AudioOut::AllTracks);
    }

    #[test]
    fn test_audio_export_args_all_formats() {
        let test_cases = [
            (AudioFormat::Mp3, "mp3", vec!["-vn", "-c:a", "libmp3lame", "-q:a", "2"], false),
            (AudioFormat::Opus, "opus", vec!["-vn", "-c:a", "libopus", "-b:a", "160k"], false),
            (AudioFormat::Aac, "m4a", vec!["-vn", "-c:a", "aac", "-b:a", "192k"], true),
            (AudioFormat::Flac, "flac", vec!["-vn", "-c:a", "flac"], false),
            (AudioFormat::Wav, "wav", vec!["-vn", "-c:a", "pcm_s16le"], false),
        ];

        for (fmt, ext, expected_codec, has_faststart) in test_cases {
            let out_name = format!("/v/out.{}", ext);
            let mut r = req(ExportPreset::Audio(fmt));
            r.output = PathBuf::from(&out_name);
            let args = build_args(&r, 2, 1.0, Path::new(&out_name));
            let joined = args.join(" ");

            assert!(args.contains(&"-vn".to_string()), "fmt {:?} should have -vn", fmt);
            for chunk in expected_codec {
                assert!(args.contains(&chunk.to_string()), "fmt {:?} missing arg {}", fmt, chunk);
            }
            if has_faststart {
                assert!(joined.contains("-movflags +faststart"), "fmt {:?} should have -movflags +faststart", fmt);
            } else {
                assert!(!joined.contains("-movflags"), "fmt {:?} should not have -movflags", fmt);
            }
            assert_eq!(r.preset.extension(), ext);
            assert!(joined.ends_with(&out_name));
        }
    }

    #[test]
    fn test_audio_format_metadata() {
        assert_eq!(AudioFormat::ALL.len(), 5);
        assert_eq!(AudioFormat::Mp3.label(), "MP3");
        assert_eq!(AudioFormat::Opus.label(), "Opus");
        assert_eq!(AudioFormat::Aac.label(), "AAC (M4A)");
        assert_eq!(AudioFormat::Flac.label(), "FLAC");
        assert_eq!(AudioFormat::Wav.label(), "WAV");

        assert_eq!(AudioFormat::Mp3.extension(), "mp3");
        assert_eq!(AudioFormat::Opus.extension(), "opus");
        assert_eq!(AudioFormat::Aac.extension(), "m4a");
        assert_eq!(AudioFormat::Flac.extension(), "flac");
        assert_eq!(AudioFormat::Wav.extension(), "wav");

        let preset = ExportPreset::Audio(AudioFormat::Mp3);
        assert_eq!(preset.label(), "Audio");
        assert_eq!(preset.description(), "Audio only. Pick the format below.");
        assert_eq!(preset.extension(), "mp3");
        assert!(!preset.has_audio_option());
    }

    #[test]
    fn test_export_args_at_250_percent_mic() {
        let mut r = req(ExportPreset::Precise);
        r.audio = AudioMix {
            game: true,
            mic: true,
            mic_volume: 2.5,
            normalize: false,
            tracks: Vec::new(),
        };
        let args = build_args_for(H264Encoder::X264, &r, 2, 1.0, Path::new("/v/out.mp4"));
        let joined = args.join(" ");
        assert!(
            joined.contains("volume=2.50,alimiter=limit=0.95"),
            "Expected 250% mic boost with limiter in build_args_for, got: {}",
            joined
        );

        let overlay_args = build_args_with_overlays(H264Encoder::X264, &r, &[], 2, 1.0, Path::new("/v/out.mp4"));
        let overlay_joined = overlay_args.join(" ");
        assert!(
            overlay_joined.contains("volume=2.50,alimiter=limit=0.95"),
            "Expected 250% mic boost with limiter in build_args_with_overlays, got: {}",
            overlay_joined
        );
    }

    #[test]
    fn test_multitrack_audio_mix() {
        let mut mix = AudioMix::default();
        mix.init_tracks(&["game".into(), "chat".into(), "mic".into()]);
        assert_eq!(mix.tracks.len(), 3);

        // All 3 tracks untouched -> AllTracks
        assert_eq!(plan_audio(&mix, 3, false), AudioOut::AllTracks);

        // Mute chat track -> mix game and mic
        mix.tracks[1].enabled = false;
        assert_eq!(plan_audio(&mix, 3, false), AudioOut::Filter("[0:a:0][0:a:2]amix=inputs=2:normalize=0[aout]".into()));

        // Boost chat track with 150% -> volume filter with limiter
        mix.tracks[1].enabled = true;
        mix.tracks[1].volume = 1.5;
        assert_eq!(plan_audio(&mix, 3, false), AudioOut::Filter("[0:a:1]volume=1.50,alimiter=limit=0.95[a_1];[0:a:0][a_1][0:a:2]amix=inputs=3:normalize=0[aout]".into()));
    }

    #[test]
    fn test_multitrack_audio_mix_5_tracks() {
        let mut mix = AudioMix::default();
        mix.init_tracks(&[
            "game".into(),
            "app:discord".into(),
            "app:spotify".into(),
            "app:browser".into(),
            "mic".into(),
        ]);
        assert_eq!(mix.tracks.len(), 5);

        // One app is muted (discord) and one at 50% (spotify)
        mix.tracks[1].enabled = false;
        mix.tracks[2].volume = 0.50;

        let plan = plan_audio(&mix, 5, false);
        assert_eq!(
            plan,
            AudioOut::Filter(
                "[0:a:2]volume=0.50[a_2];[0:a:0][a_2][0:a:3][0:a:4]amix=inputs=4:normalize=0[aout]".into()
            )
        );
    }

    #[test]
    fn temp_file_is_hidden() {
        let t = temp_output(Path::new("/v/clip_trim.mp4"));
        assert_eq!(t, PathBuf::from("/v/.clip_trim.mp4.part.mp4"));
    }

    #[test]
    fn media_foundation_encode_args() {
        let mut args = Vec::new();
        let mut filters = Vec::new();
        video_encode_args_for(H264Encoder::MediaFoundation, &mut args, &mut filters, Some(18), None);
        let joined = args.join(" ");
        assert!(joined.contains("h264_mf"), "{}", joined);
        assert!(joined.contains("nv12"), "{}", joined);
        assert!(joined.contains("-b:v "), "{}", joined);
        assert!(!joined.contains("libx264"), "{}", joined);
        assert!(!joined.contains("crf"), "{}", joined);
    }

    #[test]
    fn media_foundation_build_args() {
        let args = build_args_for(H264Encoder::MediaFoundation, &req(ExportPreset::Precise), 2, 1.0, Path::new("/v/out.mp4"));
        let joined = args.join(" ");
        assert!(joined.contains("h264_mf"), "{}", joined);
        assert!(joined.contains("nv12"), "{}", joined);
        assert!(joined.contains("-b:v "), "{}", joined);
        assert!(!joined.contains("libx264"), "{}", joined);
        assert!(!joined.contains("crf"), "{}", joined);

        let discord_args = build_args_for(H264Encoder::MediaFoundation, &req(ExportPreset::Discord), 2, 1.0, Path::new("/v/out.mp4"));
        let discord_joined = discord_args.join(" ");
        assert!(discord_joined.contains("h264_mf"), "{}", discord_joined);
        assert!(discord_joined.contains("nv12"), "{}", discord_joined);
        assert!(discord_joined.contains("-rate_control cbr"), "{}", discord_joined);
        assert!(discord_joined.contains("-b:v "), "{}", discord_joined);
        assert!(!discord_joined.contains("libx264"), "{}", discord_joined);
        assert!(!discord_joined.contains("crf"), "{}", discord_joined);
    }

    /// Runs every preset through real ffmpeg; `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn exports_real_video() {
        let dir = std::env::temp_dir().join(format!("lapse-export-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let input = dir.join("in.mp4");
        let mut cmd = crate::utils::ffmpeg_cmd();
        cmd.args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=60:duration=20",
               "-f", "lavfi", "-i", "sine=f=440:d=20", "-f", "lavfi", "-i", "sine=f=880:d=20",
               "-map", "0", "-map", "1", "-map", "2"]);
        let mut enc_args = Vec::new();
        let mut enc_filters = Vec::new();
        video_encode_args(&mut enc_args, &mut enc_filters, Some(23), None);
        if !enc_filters.is_empty() {
            cmd.args(["-vf", &enc_filters.join(",")]);
        }
        cmd.args(&enc_args);
        cmd.args(["-c:a", "aac", "-g", "120"]);
        let ok = cmd.arg(&input).status().unwrap().success();
        assert!(ok);

        for preset in ExportPreset::ALL.into_iter().chain([ExportPreset::Streamable]) {
            let out = dir.join(format!("out-{}.{}", preset.label(), preset.extension()));
            let job = start(ExportRequest { input: input.clone(), start: 3.0, end: 9.0, preset, audio: AudioMix::default(), output: out.clone(), crop: None, overlays: Vec::new() });
            let state = loop {
                let s = job.state();
                if !matches!(s, ExportState::Running { .. }) {
                    break s;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            };
            assert!(matches!(state, ExportState::Done { .. }), "{:?}: {:?}", preset, state);
            let d = crate::clip_index::probe_duration(&out);
            assert!(d > 5.0 && d < 9.5, "{:?} duration {}", preset, d);
            if let Some(limit) = size_limit(preset) {
                assert!(std::fs::metadata(&out).unwrap().len() <= limit);
            }
            eprintln!("{:?}: {:.2}s {:?}", preset, d, state);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_drawtext_escaping() {
        let input = "Title: 100% 'epic' \\ winning\nnext line";
        let escaped = escape_drawtext_text(input);
        assert_eq!(escaped, "Title\\: 100\\\\% \\'epic\\' \\\\ winning\nnext line");
    }

    #[test]
    fn test_windows_font_path_escaping() {
        let win_path = Path::new(r"C:\Users\corvus\AppData\Local\lapse\overlay-font.ttf");
        let escaped_win = escape_drawtext_font_path(win_path);
        assert_eq!(escaped_win, "C\\:/Users/corvus/AppData/Local/lapse/overlay-font.ttf");

        let linux_path = Path::new("/home/corvus/.cache/lapse/overlay-font.ttf");
        let escaped_linux = escape_drawtext_font_path(linux_path);
        assert_eq!(escaped_linux, "/home/corvus/.cache/lapse/overlay-font.ttf");
    }

    #[test]
    fn test_overlay_time_offsetting() {
        let mut r = req(ExportPreset::Precise);
        r.output = PathBuf::from("/v/out_offset.mp4");
        r.start = 5.0;
        r.end = 15.0;
        let r = ExportRequest { overlays: vec![Overlay {
            kind: OverlayKind::Text {
                text: "Mid".into(),
                size: 0.06,
                color: eframe::egui::Color32::WHITE,
                outline: true,
            },
            x: 0.5,
            y: 0.5,
            start: 7.5,
            end: 11.0,
        }], ..r };
        let args = build_args(&r, 2, 1.0, Path::new("/v/out_offset.mp4"));
        let vf = args.iter().position(|a| a == "-vf").map(|i| args[i + 1].clone()).unwrap();
        assert!(vf.contains("between(t,2.500,6.000)"), "expected offset times in: {}", vf);
    }

    #[test]
    fn test_filter_chain_order_crop_overlays_scale() {
        let mut r = req(ExportPreset::Vertical);
        r.output = PathBuf::from("/v/out_order.mp4");
        r.crop = Some(CropRect { x: 0.2, y: 0.2, w: 0.6, h: 0.6 });
        let r = ExportRequest { overlays: vec![Overlay {
            kind: OverlayKind::Text {
                text: "Header".into(),
                size: 0.08,
                color: eframe::egui::Color32::WHITE,
                outline: false,
            },
            x: 0.5,
            y: 0.2,
            start: 1.5,
            end: 11.5,
        }], ..r };
        let args = build_args(&r, 2, 1.0, Path::new("/v/out_order.mp4"));
        let vf = args.iter().position(|a| a == "-vf").map(|i| args[i + 1].clone()).unwrap();
        let crop_pos = vf.find("crop=").expect("missing crop");
        let dt_pos = vf.find("drawtext=").expect("missing drawtext");
        let scale_pos = vf.find("scale=1080:1920").expect("missing scale");
        assert!(crop_pos < dt_pos, "crop should precede drawtext");
        assert!(dt_pos < scale_pos, "drawtext should precede final scale");
    }

    #[test]
    fn test_image_input_arguments() {
        let mut r = req(ExportPreset::Precise);
        r.output = PathBuf::from("/v/out_img.mp4");
        let r = ExportRequest { overlays: vec![
            Overlay {
                kind: OverlayKind::Image {
                    path: PathBuf::from("/v/logo.png"),
                    scale: 0.2,
                    opacity: 0.9,
                },
                x: 0.8,
                y: 0.8,
                start: 1.5,
                end: 11.5,
            },
            Overlay {
                kind: OverlayKind::Image {
                    path: PathBuf::from("/v/watermark.png"),
                    scale: 0.15,
                    opacity: 0.5,
                },
                x: 0.2,
                y: 0.2,
                start: 1.5,
                end: 11.5,
            },
        ], ..r };
        let args = build_args(&r, 2, 1.0, Path::new("/v/out_img.mp4"));
        let joined = args.join(" ");
        assert!(joined.contains("-loop 1 -i /v/logo.png"));
        assert!(joined.contains("-loop 1 -i /v/watermark.png"));
        assert!(joined.contains("-filter_complex"));
        assert!(!args.contains(&"-vf".to_string()));
    }

    #[test]
    fn test_gif_and_video_overlay_ffmpeg_args() {
        let mut r = req(ExportPreset::Precise);
        r.output = PathBuf::from("/v/out_gif_vid.mp4");
        r.start = 2.0;
        r.end = 10.0;
        let r = ExportRequest {
            overlays: vec![
                Overlay {
                    kind: OverlayKind::Image {
                        path: PathBuf::from("/v/anim.gif"),
                        scale: 0.25,
                        opacity: 0.8,
                    },
                    x: 0.5,
                    y: 0.5,
                    start: 3.0,
                    end: 8.0,
                },
                Overlay {
                    kind: OverlayKind::Video {
                        path: PathBuf::from("/v/cam.mp4"),
                        scale: 0.30,
                        opacity: 0.9,
                        sound: false,
                        volume: 1.0,
                    },
                    x: 0.8,
                    y: 0.8,
                    start: 4.0,
                    end: 9.0,
                },
            ],
            ..r
        };
        let args = build_args(&r, 2, 1.0, Path::new("/v/out_gif_vid.mp4"));
        let joined = args.join(" ");

        // Check GIF input option (-ignore_loop 0) and Video input option (-stream_loop -1)
        assert!(joined.contains("-ignore_loop 0 -i /v/anim.gif"), "expected -ignore_loop 0 for GIF: {}", joined);
        assert!(joined.contains("-stream_loop -1 -i /v/cam.mp4"), "expected -stream_loop -1 for Video: {}", joined);

        // Check filter_complex presence and timestamp offsetting
        let fc = args.iter().position(|a| a == "-filter_complex").map(|i| args[i + 1].clone()).unwrap();
        // GIF is input 1, start relative to req.start (2.0) is 1.000s, end relative is 6.000s
        assert!(fc.contains("[1:v]setpts=PTS-STARTPTS+1.000/TB"), "missing GIF setpts in {}", fc);
        assert!(fc.contains("between(t,1.000,6.000)"), "missing GIF enable timing in {}", fc);
        assert!(fc.contains("shortest=0:eof_action=pass"), "missing GIF shortest/eof_action in {}", fc);

        // Video is input 2, start relative to req.start (2.0) is 2.000s, end relative is 7.000s
        assert!(fc.contains("[2:v]setpts=PTS-STARTPTS+2.000/TB"), "missing Video setpts in {}", fc);
        assert!(fc.contains("between(t,2.000,7.000)"), "missing Video enable timing in {}", fc);

        // Ensure audio mapping only takes from main input 0, not from overlay inputs 1 or 2
        assert!(!joined.contains("1:a"), "audio should not be mapped from GIF input: {}", joined);
        assert!(!joined.contains("2:a"), "audio should not be mapped from video overlay input: {}", joined);
    }

    #[test]
    #[ignore]
    fn test_real_export_with_text_and_image_overlays() {
        let dir = std::env::temp_dir().join(format!("lapse-overlay-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let input_vid = dir.join("test_vid.mp4");
        let input_img = dir.join("test_overlay.png");
        let out_vid = dir.join("out_overlay.mp4");

        // 1. Generate a 2-second video (640x360 @ 30fps)
        let v_ok = std::process::Command::new("ffmpeg")
            .args(["-y", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=30", "-t", "2", "-c:v", "libx264"])
            .arg(&input_vid)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(v_ok, "failed to generate test video");

        // 2. Generate a 64x64 test image
        let img_ok = std::process::Command::new("ffmpeg")
            .args(["-y", "-f", "lavfi", "-i", "color=c=red:size=64x64", "-frames:v", "1"])
            .arg(&input_img)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(img_ok, "failed to generate test image");

        // 3. Export with one text and one image overlay
        let req = ExportRequest {
            input: input_vid,
            start: 0.0,
            end: 2.0,
            preset: ExportPreset::Precise,
            audio: AudioMix::muted(),
            output: out_vid.clone(),
            crop: None,
            overlays: vec![
            Overlay {
                kind: OverlayKind::Text {
                    text: "Lapse Text Overlay".into(),
                    size: 0.08,
                    color: eframe::egui::Color32::YELLOW,
                    outline: true,
                },
                x: 0.5,
                y: 0.3,
                start: 0.0,
                end: 2.0,
            },
            Overlay {
                kind: OverlayKind::Image {
                    path: input_img,
                    scale: 0.25,
                    opacity: 0.8,
                },
                x: 0.5,
                y: 0.7,
                start: 0.0,
                end: 2.0,
            },
            ],
        };

        let job = start(req);
        let state = loop {
            let s = job.state();
            if !matches!(s, ExportState::Running { .. }) {
                break s;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        };

        assert!(matches!(state, ExportState::Done { .. }), "Export failed: {:?}", state);
        assert!(out_vid.exists(), "Output video was not created");
        assert!(std::fs::metadata(&out_vid).unwrap().len() > 0, "Output video is empty");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_real_export_with_gif_and_video_overlays() {
        let dir = std::env::temp_dir().join(format!("lapse-gif-vid-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let input_vid = dir.join("test_vid2.mp4");
        let input_gif = dir.join("test_overlay.gif");
        let input_ov_vid = dir.join("test_overlay_vid.mp4");
        let out_vid = dir.join("out_gif_vid_real.mp4");

        // 1. Generate a 2-second video (320x240 @ 30fps)
        let v_ok = std::process::Command::new("ffmpeg")
            .args(["-y", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=30", "-t", "2", "-c:v", "libx264"])
            .arg(&input_vid)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(v_ok, "failed to generate test video");

        // 2. Generate a small test GIF
        let gif_ok = std::process::Command::new("ffmpeg")
            .args(["-y", "-f", "lavfi", "-i", "color=c=blue:size=64x64", "-t", "1", "-r", "5"])
            .arg(&input_gif)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(gif_ok, "failed to generate test gif");

        // 3. Generate a small test overlay video
        let ov_ok = std::process::Command::new("ffmpeg")
            .args(["-y", "-f", "lavfi", "-i", "testsrc=size=160x120:rate=30", "-t", "1", "-c:v", "libx264"])
            .arg(&input_ov_vid)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ov_ok, "failed to generate test overlay video");

        // 4. Export with one GIF and one Video overlay
        let req = ExportRequest {
            input: input_vid,
            start: 0.0,
            end: 2.0,
            preset: ExportPreset::Precise,
            audio: AudioMix::muted(),
            output: out_vid.clone(),
            crop: None,
            overlays: vec![
                Overlay {
                    kind: OverlayKind::Image {
                        path: input_gif,
                        scale: 0.25,
                        opacity: 0.8,
                    },
                    x: 0.3,
                    y: 0.3,
                    start: 0.2,
                    end: 1.8,
                },
                Overlay {
                    kind: OverlayKind::Video {
                        path: input_ov_vid,
                        scale: 0.3,
                        opacity: 0.9,
                        sound: false,
                        volume: 1.0,
                    },
                    x: 0.7,
                    y: 0.7,
                    start: 0.5,
                    end: 1.9,
                },
            ],
        };

        let job = start(req);
        let state = loop {
            let s = job.state();
            if !matches!(s, ExportState::Running { .. }) {
                break s;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };

        assert!(matches!(state, ExportState::Done { .. }), "Export failed: {:?}", state);
        assert!(out_vid.exists(), "Output video was not created");
        assert!(std::fs::metadata(&out_vid).unwrap().len() > 0, "Output video is empty");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_export_args_with_sound_overlays_with_clip_audio() {
        let mut r = req(ExportPreset::Precise);
        r.output = PathBuf::from("/v/out_sound.mp4");
        r.start = 1.0;
        r.end = 10.0;
        r.audio = AudioMix {
            game: true,
            mic: false,
            mic_volume: 1.0,
            normalize: false,
            tracks: Vec::new(),
        };
        r.overlays = vec![
            Overlay {
                kind: OverlayKind::Audio {
                    path: PathBuf::from("/a/sound.mp3"),
                    volume: 0.8,
                },
                x: 0.0,
                y: 0.0,
                start: 2.0,
                end: 5.0,
            },
            Overlay {
                kind: OverlayKind::Video {
                    path: PathBuf::from("/v/cam.mp4"),
                    scale: 0.3,
                    opacity: 0.9,
                    sound: true,
                    volume: 1.5,
                },
                x: 0.8,
                y: 0.8,
                start: 3.5,
                end: 7.5,
            },
        ];
        let args = build_args(&r, 1, 1.0, Path::new("/v/out_sound.mp4"));
        let joined = args.join(" ");

        // Audio overlay is input 1 (-i /a/sound.mp3), Video overlay is input 2 (-stream_loop -1 -i /v/cam.mp4)
        assert!(joined.contains("-i /a/sound.mp3"), "expected -i for Audio overlay: {}", joined);
        assert!(!joined.contains("-stream_loop -1 -i /a/sound.mp3"), "Audio overlay must not loop: {}", joined);
        assert!(joined.contains("-stream_loop -1 -i /v/cam.mp4"), "expected -stream_loop -1 for Video: {}", joined);

        let fc = args.iter().position(|a| a == "-filter_complex").map(|i| args[i + 1].clone()).expect("missing filter_complex");
        // Video overlay uses input 2
        assert!(fc.contains("[2:v]setpts=PTS-STARTPTS+2.500/TB"), "missing video overlay setpts in {}", fc);

        // Base audio
        assert!(fc.contains("[0:a:0]anull[abase]"), "missing base audio in {}", fc);

        // Overlay audio chains:
        // Audio overlay is input 1: len = 3.0, delay = (2.0 - 1.0) * 1000 = 1000ms
        assert!(fc.contains("[1:a]atrim=0:3.000,volume=0.80,adelay=1000:all=1[a_ov0]"), "missing audio overlay filter in {}", fc);
        // Video overlay is input 2: len = 4.0, delay = (3.5 - 1.0) * 1000 = 2500ms
        assert!(fc.contains("[2:a]atrim=0:4.000,volume=1.50,adelay=2500:all=1[a_ov1]"), "missing video overlay sound filter in {}", fc);

        // Mixed with amix duration=first
        assert!(fc.contains("[abase][a_ov0][a_ov1]amix=inputs=3:normalize=0:duration=first[aout]"), "missing amix in {}", fc);

        // Audio map
        assert!(joined.contains("-map [aout]"), "missing -map [aout]: {}", joined);
        assert!(joined.contains("-c:a aac"), "missing audio codec: {}", joined);
    }

    #[test]
    fn test_export_args_with_sound_overlays_without_clip_audio() {
        let mut r = req(ExportPreset::Precise);
        r.output = PathBuf::from("/v/out_muted.mp4");
        r.start = 1.0;
        r.end = 10.0;
        r.audio = AudioMix::muted();
        r.overlays = vec![
            Overlay {
                kind: OverlayKind::Audio {
                    path: PathBuf::from("/a/sound.mp3"),
                    volume: 0.8,
                },
                x: 0.0,
                y: 0.0,
                start: 2.0,
                end: 5.0,
            },
            Overlay {
                kind: OverlayKind::Video {
                    path: PathBuf::from("/v/cam.mp4"),
                    scale: 0.3,
                    opacity: 0.9,
                    sound: true,
                    volume: 1.5,
                },
                x: 0.8,
                y: 0.8,
                start: 3.5,
                end: 7.5,
            },
        ];
        let args = build_args(&r, 0, 1.0, Path::new("/v/out_muted.mp4"));
        let joined = args.join(" ");

        let fc = args.iter().position(|a| a == "-filter_complex").map(|i| args[i + 1].clone()).expect("missing filter_complex");
        // Base audio must be anullsrc with duration 9.000
        assert!(fc.contains("anullsrc=channel_layout=stereo:sample_rate=48000,atrim=0:9.000[abase]"), "missing anullsrc base in {}", fc);

        // Overlays
        assert!(fc.contains("[1:a]atrim=0:3.000,volume=0.80,adelay=1000:all=1[a_ov0]"), "missing audio overlay filter in {}", fc);
        assert!(fc.contains("[2:a]atrim=0:4.000,volume=1.50,adelay=2500:all=1[a_ov1]"), "missing video overlay sound filter in {}", fc);
        assert!(fc.contains("[abase][a_ov0][a_ov1]amix=inputs=3:normalize=0:duration=first[aout]"), "missing amix in {}", fc);

        // Audio map must be present, NOT -an
        assert!(joined.contains("-map [aout]"), "missing -map [aout]: {}", joined);
        assert!(!joined.contains("-an"), "should not have -an when sound overlays present: {}", joined);
    }

    #[test]
    fn test_share_output_and_is_share_temp() {
        let input = Path::new("/clips/game_replay.mp4");
        let out = share_output(input, ExportPreset::Fast);
        let share_dir = std::env::temp_dir().join("lapse-share");
        assert!(out.starts_with(&share_dir));
        assert!(is_share_temp(&out));
        assert!(!is_share_temp(input));
        assert!(!is_share_temp(Path::new("/home/user/Videos/Lapse/clip.mp4")));
    }
}
