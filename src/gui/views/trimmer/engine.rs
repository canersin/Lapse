//! Clip playback: ffmpeg decodes video frames and audio into egui / rodio.

use eframe::egui::{self, ColorImage, TextureHandle};
use std::collections::HashSet;
use std::io::{BufReader, Read};
use std::num::{NonZeroU16, NonZeroU32};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use rodio::{buffer::SamplesBuffer, queue::queue, DeviceSinkBuilder, MixerDeviceSink, Player};

use super::export_panel::ExportPanel;
use super::overlays::OverlayKind;
use super::timeline::Filmstrip;

/// Playback frame rate of the trimmer preview; exports keep the clip's own rate.
const PREVIEW_FPS: f64 = 30.0;

/// Video decoding backends in order of preference.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DecodeBackend {
    #[allow(dead_code)]
    Vaapi(String),
    #[allow(dead_code)]
    Cuda,
    #[allow(dead_code)]
    D3d11va,
    Cpu,
}

impl std::fmt::Display for DecodeBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vaapi(node) => write!(f, "VAAPI ({node})"),
            Self::Cuda => write!(f, "CUDA"),
            Self::D3d11va => write!(f, "D3D11VA"),
            Self::Cpu => write!(f, "CPU"),
        }
    }
}

/// Backends that failed to decode a codec (or failed generally when codec is None).
static BROKEN_BACKENDS: Mutex<Vec<(DecodeBackend, Option<String>)>> = Mutex::new(Vec::new());

fn is_backend_broken(backend: &DecodeBackend, codec: Option<&str>) -> bool {
    if matches!(backend, DecodeBackend::Cpu) {
        return false;
    }
    let broken = BROKEN_BACKENDS.lock().unwrap_or_else(|e| e.into_inner());
    let codec_lower = codec.map(|c| c.to_lowercase());
    broken.iter().any(|(b, c)| {
        b == backend && (*c == codec_lower || c.is_none())
    })
}

fn mark_backend_broken(backend: &DecodeBackend, codec: Option<&str>) {
    if matches!(backend, DecodeBackend::Cpu) {
        return;
    }
    let mut broken = BROKEN_BACKENDS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = (backend.clone(), codec.map(|c| c.to_lowercase()));
    if !broken.contains(&entry) {
        broken.push(entry);
    }
}

#[cfg(not(windows))]
fn is_nvidia_render_node(node: &str) -> bool {
    let name = std::path::Path::new(node)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let path = format!("/sys/class/drm/{name}/device/vendor");
    if let Ok(v) = std::fs::read_to_string(path) {
        let v = v.trim().trim_start_matches("0x");
        if v.eq_ignore_ascii_case("10de") {
            return true;
        }
    }
    false
}

#[cfg(not(windows))]
fn is_nvidia_gpu(gpu: &crate::hardware::gpu::GpuDevice) -> bool {
    gpu.vendor_id.eq_ignore_ascii_case("10de")
        || is_nvidia_render_node(&gpu.render_node)
        || gpu.name.to_lowercase().contains("nvidia")
}

/// Decode backends in preference order; GPU enumeration runs once per session.
fn decode_candidates() -> Vec<DecodeBackend> {
    static CANDIDATES: OnceLock<Vec<DecodeBackend>> = OnceLock::new();
    CANDIDATES.get_or_init(probe_decode_candidates).clone()
}

fn probe_decode_candidates() -> Vec<DecodeBackend> {
    #[cfg(windows)]
    {
        vec![DecodeBackend::D3d11va, DecodeBackend::Cpu]
    }

    #[cfg(not(windows))]
    {
        let recording_gpu = crate::config::Config::load().map(|c| c.gpu).unwrap_or_default();
        let all_gpus = crate::hardware::gpu::get_gpus();
        let mut candidates = Vec::new();

        let mut vaapi_gpus: Vec<_> = all_gpus
            .iter()
            .filter(|g| {
                !g.render_node.is_empty()
                    && std::path::Path::new(&g.render_node).exists()
                    && !is_nvidia_gpu(g)
            })
            .collect();

        // Preference order: non-recording GPU first, then recording GPU.
        vaapi_gpus.sort_by_key(|g| g.matches(&recording_gpu));

        let mut seen = HashSet::new();
        for g in vaapi_gpus {
            if seen.insert(g.render_node.clone()) {
                candidates.push(DecodeBackend::Vaapi(g.render_node.clone()));
            }
        }

        let has_nvidia = all_gpus.iter().any(is_nvidia_gpu);
        if has_nvidia {
            candidates.push(DecodeBackend::Cuda);
        }

        candidates.push(DecodeBackend::Cpu);
        candidates
    }
}

fn pick_backend(codec: Option<&str>) -> DecodeBackend {
    for candidate in decode_candidates() {
        if !is_backend_broken(&candidate, codec) {
            return candidate;
        }
    }
    DecodeBackend::Cpu
}

fn next_backend(current: &DecodeBackend, codec: Option<&str>) -> Option<DecodeBackend> {
    let candidates = decode_candidates();
    if let Some(pos) = candidates.iter().position(|b| b == current) {
        for candidate in &candidates[pos + 1..] {
            if !is_backend_broken(candidate, codec) {
                return Some(candidate.clone());
            }
        }
        None
    } else {
        Some(DecodeBackend::Cpu)
    }
}

/// The preferred VA-API render node for video decoding and filmstrip preview.
pub(crate) fn preferred_vaapi_node() -> Option<String> {
    #[cfg(windows)]
    return None;

    #[cfg(not(windows))]
    {
        for candidate in decode_candidates() {
            if let DecodeBackend::Vaapi(node) = candidate {
                if !is_backend_broken(&DecodeBackend::Vaapi(node.clone()), None) {
                    return Some(node);
                }
            }
        }
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimelineDragTarget {
    Playhead,
    MarkIn,
    MarkOut,
}

pub struct VideoFrame {
    pub pts: f64,
    pub image: ColorImage,
}

#[derive(Default)]
pub enum HighlightJob {
    #[default]
    NotStarted,
    Running,
    Done(Vec<f64>),
    Failed(String),
}

pub struct PlayerState {
    pub video_path: PathBuf,
    pub file_size_bytes: u64,
    /// Optional GPU video playback powered by runtime-loaded libmpv.
    pub mpv: Option<crate::gui::views::trimmer::mpv::MpvPlayer>,
    /// 2 when game audio and microphone were recorded as separate tracks.
    pub audio_tracks: usize,
    pub track_names: Vec<String>,
    /// App tracks that are silent in this clip (filled in the background), hidden in the mixer.
    pub silent_tracks: Arc<Mutex<Vec<bool>>>,
    pub texture: Option<TextureHandle>,
    pub frame_receiver: Option<Receiver<VideoFrame>>,
    pub pending_frame: Option<VideoFrame>,
    pub video_child: Option<Child>,
    pub audio_child: Option<Child>,
    pub audio_ready: Arc<AtomicBool>,
    /// The running video decoder uses the GPU.
    video_on_gpu: bool,
    /// The video decoder ended without producing a frame.
    video_failed: Arc<AtomicBool>,
    pub current_backend: DecodeBackend,
    pub video_codec: Option<String>,
    pub video_overlay_decoders: std::collections::HashMap<usize, crate::gui::views::trimmer::overlays::VideoOverlayDecoder>,
    pub seek_seq: u64,

    pub orig_width: usize,
    pub orig_height: usize,
    pub preview_width: usize,
    pub preview_height: usize,
    pub fps: f64,

    pub duration: Duration,
    pub current_time: Duration,
    pub clock_base: Duration,
    pub clock_origin: Instant,
    pub awaiting_first_frame: bool,
    pub first_frame_ready: bool,
    pub awaiting_since: Instant,
    pub is_playing: bool,
    pub loop_cut_preview: bool,

    pub volume: f32,
    pub muted: bool,
    /// Must stay alive while playing; dropping it closes the audio device.
    #[allow(dead_code)]
    pub sink_handle: Option<MixerDeviceSink>,
    pub player: Option<Player>,

    pub trim_start: Duration,
    pub trim_end: Duration,
    pub export: ExportPanel,
    pub filmstrip: Filmstrip,
    /// Moments marked with the marker hotkey (s from the start).
    pub markers: Vec<crate::markers::Mark>,
    /// Loudest moments, once "Find highlights" finished.
    pub highlights: std::sync::Arc<std::sync::Mutex<HighlightJob>>,
    pub active_drag: Option<TimelineDragTarget>,
    /// Fullscreen viewing (F); the app switches the window to match.
    pub fullscreen: bool,
    /// Last mouse movement, to hide the fullscreen controls and cursor.
    pub last_pointer_move: Instant,
    pub pending_seek: Option<Duration>,
    pub last_seek_spawn: Instant,
    pub last_sound_overlays: Vec<crate::gui::views::trimmer::mpv::SoundOverlayConfig>,
    pub last_audio_mix: Option<crate::export::AudioMix>,
    pub sound_overlays_dirty: bool,
    pub sound_overlays_dirty_since: Option<Instant>,
}

pub fn format_file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

fn parse_rate(value: &str) -> f64 {
    if let Some((n, d)) = value.split_once('/') {
        let n: f64 = n.trim().parse().unwrap_or(0.0);
        let d: f64 = d.trim().parse().unwrap_or(0.0);
        if d > 0.0 {
            return n / d;
        }
        return 0.0;
    }
    value.trim().parse().unwrap_or(0.0)
}

impl Drop for PlayerState {
    fn drop(&mut self) {
        self.stop();
        self.kill_children();
        self.mpv = None;
    }
}

impl PlayerState {
    pub fn new(path: PathBuf) -> Option<Self> {
        let file_size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let audio_tracks = crate::export::audio_stream_count(&path);

        let probe = crate::utils::ffprobe_cmd()
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name,width,height,r_frame_rate,avg_frame_rate:format=duration",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .output()
            .ok()?;

        let text = String::from_utf8_lossy(&probe.stdout).to_string();
        let mut video_codec = None;
        let mut orig_width = 0usize;
        let mut orig_height = 0usize;
        let mut avg_fps = 0.0f64;
        let mut r_fps = 0.0f64;
        let mut duration_secs = 0.0f64;
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                let value = value.trim();
                match key.trim() {
                    "codec_name" => {
                        if !value.is_empty() {
                            video_codec = Some(value.to_string());
                        }
                    }
                    "width" => orig_width = value.parse().unwrap_or(0),
                    "height" => orig_height = value.parse().unwrap_or(0),
                    "avg_frame_rate" => avg_fps = parse_rate(value),
                    "r_frame_rate" => r_fps = parse_rate(value),
                    "duration" => {
                        if let Ok(d) = value.parse::<f64>() {
                            duration_secs = d;
                        }
                    }
                    _ => {}
                }
            }
        }
        if orig_width == 0 || orig_height == 0 {
            return None;
        }

        let raw_fps = if avg_fps.is_finite() && avg_fps > 1.0 { avg_fps } else { r_fps };
        let fps = if raw_fps.is_finite() && raw_fps > 1.0 { raw_fps.min(60.0) } else { 60.0 };

        // 1080p keeps the preview sharp; playback runs at PREVIEW_FPS so copying the
        // frames out of the GPU costs no more CPU than a 720p60 preview did.
        let max_w = 1920.0f32;
        let max_h = 1080.0f32;
        let aspect = orig_width as f32 / orig_height.max(1) as f32;
        let (preview_width, preview_height) = if aspect >= (max_w / max_h) {
            let w = (orig_width as f32).min(max_w) as usize;
            let w = (w / 2) * 2;
            let h = ((w as f32 / aspect).round() as usize / 2) * 2;
            (w.max(2), h.max(2))
        } else {
            let h = (orig_height as f32).min(max_h) as usize;
            let h = (h / 2) * 2;
            let w = ((h as f32 * aspect).round() as usize / 2) * 2;
            (w.max(2), h.max(2))
        };

        let duration = Duration::from_secs_f64(duration_secs.max(0.1));

        let cfg = crate::config::Config::load().unwrap_or_default();
        let mut mpv_player = None;
        if cfg.mpv_preview && crate::gui::views::trimmer::mpv::mpv_available() {
            mpv_player = crate::gui::views::trimmer::mpv::MpvPlayer::new(
                &path,
                audio_tracks,
                orig_width,
                orig_height,
            );
        }
        let use_mpv = mpv_player.is_some();

        let (sink_handle, player) = if use_mpv {
            (None, None)
        } else if let Ok(mut sink) = DeviceSinkBuilder::open_default_sink() {
            sink.log_on_drop(false);
            let p = Player::connect_new(sink.mixer());
            (Some(sink), Some(p))
        } else {
            (None, None)
        };

        let current_backend = pick_backend(video_codec.as_deref());
        eprintln!("Trimmer: decoding on {current_backend}");

        let path_for_strip = path.clone();
        let track_names = crate::tracks::load_for(&path, audio_tracks);
        let silent_tracks = Arc::new(Mutex::new(Vec::new()));
        if track_names.iter().any(|t| t.starts_with("app:")) {
            let (out, video, names) = (Arc::clone(&silent_tracks), path.clone(), track_names.clone());
            std::thread::spawn(move || {
                let silent = crate::tracks::silent_app_tracks(&video, &names);
                if let Ok(mut s) = out.lock() {
                    *s = silent;
                }
            });
        }
        let mut export = ExportPanel::default();
        export.audio.init_tracks(&track_names);
        let mut state = Self {
            video_path: path,
            file_size_bytes,
            mpv: mpv_player,
            audio_tracks,
            track_names,
            silent_tracks,
            texture: None,
            frame_receiver: None,
            pending_frame: None,
            video_child: None,
            audio_child: None,
            audio_ready: Arc::new(AtomicBool::new(false)),
            video_on_gpu: false,
            video_failed: Arc::new(AtomicBool::new(false)),
            current_backend,
            video_codec,
            video_overlay_decoders: std::collections::HashMap::new(),
            seek_seq: 0,
            orig_width,
            orig_height,
            preview_width,
            preview_height,
            fps,
            duration,
            current_time: Duration::ZERO,
            clock_base: Duration::ZERO,
            clock_origin: Instant::now(),
            awaiting_first_frame: true,
            first_frame_ready: false,
            awaiting_since: Instant::now(),
            is_playing: true,
            loop_cut_preview: false,
            volume: 1.0,
            muted: false,
            sink_handle,
            player,
            trim_start: Duration::ZERO,
            trim_end: duration,
            export,
            filmstrip: Filmstrip::start(&path_for_strip, duration.as_secs_f64()),
            markers: crate::markers::load_for(&path_for_strip),
            highlights: Default::default(),
            active_drag: None,
            fullscreen: false,
            last_pointer_move: Instant::now(),
            pending_seek: None,
            last_seek_spawn: Instant::now() - Duration::from_secs(1),
            last_sound_overlays: Vec::new(),
            last_audio_mix: Some(crate::export::AudioMix::default()),
            sound_overlays_dirty: false,
            sound_overlays_dirty_since: None,
        };

        state.seek_to(Duration::ZERO);
        Some(state)
    }

    pub fn set_audio_volume(&self, vol: f32) {
        if let Some(mpv) = &self.mpv {
            mpv.set_volume(vol, self.muted);
        } else if let Some(player) = &self.player {
            if self.muted {
                player.set_volume(0.0);
            } else {
                player.set_volume(vol.clamp(0.0, 1.0));
            }
        }
    }

    fn kill_children(&mut self) {
        self.frame_receiver = None;
        self.pending_frame = None;
        for (_, mut d) in self.video_overlay_decoders.drain() {
            d.stop();
        }
        for child in [self.video_child.take(), self.audio_child.take()].into_iter().flatten() {
            let mut child = child;
            let _ = child.kill();
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }

    pub fn stop(&mut self) {
        self.is_playing = false;
        if let Some(mpv) = &self.mpv {
            mpv.pause();
        }
        self.kill_children();
        if let Some(player) = &self.player {
            player.stop();
        }
    }

    fn live_time(&self) -> Duration {
        if self.is_playing && !self.awaiting_first_frame {
            (self.clock_base + self.clock_origin.elapsed()).min(self.duration)
        } else {
            self.current_time
        }
    }

    pub fn pause(&mut self) {
        if !self.is_playing {
            return;
        }
        if let Some(mpv) = &self.mpv {
            if let Some(pos) = mpv.get_time_pos() {
                self.current_time = Duration::from_secs_f64(pos.clamp(0.0, self.duration.as_secs_f64()));
            }
            self.clock_base = self.current_time;
            self.is_playing = false;
            mpv.pause();
        } else {
            self.current_time = self.live_time();
            self.clock_base = self.current_time;
            self.is_playing = false;
            if let Some(player) = &self.player {
                player.pause();
            }
        }
    }

    pub fn play(&mut self) {
        if self.is_playing {
            return;
        }
        self.is_playing = true;
        if self.current_time + Duration::from_millis(60) >= self.duration {
            self.seek_to(Duration::ZERO);
            return;
        }
        self.clock_base = self.current_time;
        self.clock_origin = Instant::now();
        if let Some(mpv) = &self.mpv {
            mpv.play();
        } else if !self.awaiting_first_frame {
            if let Some(player) = &self.player {
                player.play();
            }
        }
    }

    /// Length of one displayed frame, for frame stepping.
    pub fn frame_duration(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.fps.max(1.0))
    }

    pub fn toggle_play(&mut self) {
        if self.is_playing {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn request_seek(&mut self, target: Duration) {
        let target = target.min(self.duration);
        self.current_time = target;
        self.clock_base = target;
        self.clock_origin = Instant::now();
        if self.last_seek_spawn.elapsed() >= Duration::from_millis(90) {
            self.seek_to(target);
        } else {
            self.pending_seek = Some(target);
        }
    }

    fn upload(&mut self, ctx: &egui::Context, image: ColorImage) {
        if let Some(tex) = &mut self.texture {
            tex.set(image, egui::TextureOptions::LINEAR);
        } else {
            self.texture = Some(ctx.load_texture("video_frame", image, egui::TextureOptions::LINEAR));
        }
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        if let Some(target) = self.pending_seek {
            if self.last_seek_spawn.elapsed() >= Duration::from_millis(90) {
                self.seek_to(target);
            }
        }

        // Overlay sounds update check (debounced)
        let current_sound_overlays: Vec<crate::gui::views::trimmer::mpv::SoundOverlayConfig> = self.export.overlays.iter().filter_map(|o| {
            match &o.kind {
                OverlayKind::Audio { path, volume } => Some(crate::gui::views::trimmer::mpv::SoundOverlayConfig {
                    path: path.clone(),
                    start: o.start,
                    end: o.end,
                    volume: *volume,
                }),
                OverlayKind::Video { path, sound: true, volume, .. } => Some(crate::gui::views::trimmer::mpv::SoundOverlayConfig {
                    path: path.clone(),
                    start: o.start,
                    end: o.end,
                    volume: *volume,
                }),
                _ => None,
            }
        }).collect();

        let audio_mix_changed = self.last_audio_mix.as_ref() != Some(&self.export.audio);

        if current_sound_overlays != self.last_sound_overlays || audio_mix_changed {
            self.sound_overlays_dirty = true;
            if self.sound_overlays_dirty_since.is_none() {
                self.sound_overlays_dirty_since = Some(Instant::now());
            }
        }

        if self.sound_overlays_dirty && self.active_drag.is_none() {
            let elapsed_ok = self.sound_overlays_dirty_since.map(|t| t.elapsed() >= Duration::from_millis(200)).unwrap_or(true);
            if elapsed_ok {
                self.sound_overlays_dirty = false;
                self.sound_overlays_dirty_since = None;
                self.last_sound_overlays = current_sound_overlays.clone();
                self.last_audio_mix = Some(self.export.audio.clone());
                if let Some(mpv) = &self.mpv {
                    mpv.update_audio_filters(self.audio_tracks, &self.export.audio, &current_sound_overlays);
                } else {
                    self.seek_to(self.current_time);
                }
            }
        }

        if let Some(mpv) = &self.mpv {
            if mpv.has_failed() {
                eprintln!("Trimmer: mpv playback failed, falling back to ffmpeg");
                self.mpv = None;
                self.seek_to(self.current_time);
                ctx.request_repaint();
                return;
            }

            if self.is_playing {
                if let Some(pos) = mpv.get_time_pos() {
                    self.current_time = Duration::from_secs_f64(pos.clamp(0.0, self.duration.as_secs_f64()));
                }
                if self.current_time >= self.duration {
                    self.pause();
                    return;
                }
            }

            return;
        }

        if self.awaiting_first_frame {
            // The decoder couldn't decode this clip: retry on the next candidate backend.
            if self.current_backend != DecodeBackend::Cpu
                && self.video_failed.load(Ordering::Acquire)
                && (self.current_time + Duration::from_secs(1) < self.duration || self.current_time == Duration::ZERO)
            {
                mark_backend_broken(&self.current_backend, self.video_codec.as_deref());
                if let Some(next) = next_backend(&self.current_backend, self.video_codec.as_deref()) {
                    eprintln!("Trimmer: {} failed, trying {next}", self.current_backend);
                    eprintln!("Trimmer: decoding on {next}");
                    self.current_backend = next;
                    self.seek_to(self.current_time);
                    ctx.request_repaint();
                    return;
                }
            }
            if !self.first_frame_ready {
                let frame = self.frame_receiver.as_ref().and_then(|rx| rx.try_recv().ok());
                if let Some(frame) = frame {
                    self.upload(ctx, frame.image);
                    self.first_frame_ready = true;
                } else if self.video_child.is_none() || self.awaiting_since.elapsed() > Duration::from_secs(4) {
                    self.first_frame_ready = true;
                }
            }
            if self.first_frame_ready {
                let audio_ok = self.audio_ready.load(Ordering::Acquire)
                    || self.awaiting_since.elapsed() > Duration::from_millis(450);
                if !self.is_playing || audio_ok {
                    self.awaiting_first_frame = false;
                    self.clock_base = self.current_time;
                    self.clock_origin = Instant::now();
                    if self.is_playing {
                        if let Some(player) = &self.player {
                            player.play();
                        }
                    }
                }
            }
            ctx.request_repaint_after(Duration::from_millis(4));
            return;
        }

        if !self.is_playing {
            return;
        }

        let now = self.live_time();
        self.current_time = now;
        let now_secs = now.as_secs_f64();
        let half_frame = 0.5 / self.fps;
        let mut latest = None;
        loop {
            let next = match self.pending_frame.take() {
                Some(f) => Some(f),
                None => self.frame_receiver.as_ref().and_then(|rx| rx.try_recv().ok()),
            };
            match next {
                Some(f) if f.pts <= now_secs + half_frame => latest = Some(f.image),
                Some(f) => {
                    self.pending_frame = Some(f);
                    break;
                }
                None => break,
            }
        }
        if let Some(image) = latest {
            self.upload(ctx, image);
        }

        if self.current_time >= self.duration {
            self.pause();
            return;
        }

        let wait = self
            .pending_frame
            .as_ref()
            .map(|f| (f.pts - now_secs).max(0.0))
            .unwrap_or(1.0 / self.fps);
        ctx.request_repaint_after(Duration::from_secs_f64(wait.clamp(0.002, 0.016)));
    }

    pub fn seek_to(&mut self, target: Duration) {
        self.seek_seq = self.seek_seq.wrapping_add(1);
        self.current_time = target.min(self.duration);
        self.clock_base = self.current_time;
        self.clock_origin = Instant::now();
        self.pending_seek = None;
        self.last_seek_spawn = Instant::now();

        if let Some(mpv) = &self.mpv {
            mpv.seek(self.current_time.as_secs_f64());
            if self.is_playing {
                mpv.play();
            } else {
                mpv.pause();
            }
            self.awaiting_first_frame = false;
            self.first_frame_ready = true;
            return;
        }

        self.kill_children();
        if let Some(player) = &self.player {
            player.stop();
        }

        self.awaiting_first_frame = true;
        self.first_frame_ready = false;
        self.awaiting_since = Instant::now();
        self.audio_ready = Arc::new(AtomicBool::new(self.player.is_none()));

        let seek_secs = self.current_time.as_secs_f64();
        let target_w = self.preview_width;
        let target_h = self.preview_height;
        let fps = self.fps.min(PREVIEW_FPS);
        let path_str = self.video_path.to_string_lossy().to_string();
        let ss = format!("{:.3}", seek_secs);
        let rate = format!("{:.6}", fps);
        // Decode and scale on the GPU, then copy the small frame out; on the CPU the
        // decoder only just keeps up with 4K60 AV1 and the picture falls behind the sound.
        let (decode_args, vf): (Vec<String>, String) = match &self.current_backend {
            DecodeBackend::Vaapi(node) => (
                ["-hwaccel", "vaapi", "-hwaccel_device", node, "-hwaccel_output_format", "vaapi"]
                    .map(String::from)
                    .to_vec(),
                format!(
                    "fps={rate},scale_vaapi=w={target_w}:h={target_h}:format=rgba:mode=hq,hwdownload,format=rgba"
                ),
            ),
            DecodeBackend::Cuda => (
                ["-hwaccel", "cuda", "-hwaccel_output_format", "cuda"]
                    .map(String::from)
                    .to_vec(),
                format!(
                    "fps={rate},scale_cuda=w={target_w}:h={target_h}:format=nv12,hwdownload,format=nv12,format=rgba"
                ),
            ),
            DecodeBackend::D3d11va => (
                ["-hwaccel", "d3d11va"].map(String::from).to_vec(),
                format!("fps={rate},scale={target_w}:{target_h}:flags=bilinear,format=rgba"),
            ),
            DecodeBackend::Cpu => {
                #[cfg(windows)]
                {
                    (Vec::new(), format!("fps={rate},scale={target_w}:{target_h}:flags=bilinear,format=rgba"))
                }
                #[cfg(not(windows))]
                {
                    (
                        ["-hwaccel", "auto"].map(String::from).to_vec(),
                        format!("fps={rate},scale={target_w}:{target_h}:flags=bilinear,format=rgba"),
                    )
                }
            }
        };
        self.video_on_gpu = self.current_backend != DecodeBackend::Cpu;
        self.video_failed = Arc::new(AtomicBool::new(false));

        let mut v_child = crate::utils::ffmpeg_cmd()
            .args(["-nostdin", "-loglevel", "error"])
            .args(&decode_args)
            .args([
                "-ss",
                &ss,
                "-i",
                &path_str,
                "-an",
                "-sn",
                "-vf",
                &vf,
                "-fps_mode",
                "cfr",
                "-r",
                &rate,
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok();

        if let Some(child) = v_child.as_mut() {
            if let Some(stdout) = child.stdout.take() {
                let (tx, rx) = mpsc::sync_channel::<VideoFrame>(8);
                self.frame_receiver = Some(rx);
                let failed = Arc::clone(&self.video_failed);

                thread::spawn(move || {
                    let frame_size = target_w * target_h * 4;
                    let mut reader = BufReader::with_capacity(frame_size, stdout);
                    let mut buffer = vec![0u8; frame_size];
                    let mut index: u64 = 0;

                    while reader.read_exact(&mut buffer).is_ok() {
                        let image = ColorImage::from_rgba_unmultiplied([target_w, target_h], &buffer);
                        let pts = seek_secs + index as f64 / fps;
                        index += 1;
                        if tx.send(VideoFrame { pts, image }).is_err() {
                            break;
                        }
                    }
                    if index == 0 {
                        failed.store(true, Ordering::Release);
                    }
                });
            } else {
                self.video_failed.store(true, Ordering::Release);
            }
        } else {
            self.video_failed.store(true, Ordering::Release);
        }
        self.video_child = v_child;

        if let Some(player) = &self.player {
            let mut active_sound_overlays = Vec::new();
            for (i, overlay) in self.export.overlays.iter().enumerate() {
                let (is_sound, vol) = match &overlay.kind {
                    OverlayKind::Audio { volume, .. } => (true, *volume),
                    OverlayKind::Video { sound: true, volume, .. } => (true, *volume),
                    _ => (false, 1.0),
                };
                if is_sound && seek_secs < overlay.end {
                    active_sound_overlays.push((i, overlay.clone(), vol));
                }
            }

            let mut a_cmd = crate::utils::ffmpeg_cmd();
            a_cmd.args(["-nostdin", "-loglevel", "error"]);
            a_cmd.args(["-ss", &ss, "-i", &path_str]);

            let mut cur_a_input = 1;
            let mut ov_input_indices = Vec::new();
            for (_, ov, _) in &active_sound_overlays {
                match &ov.kind {
                    OverlayKind::Video { path, .. } => {
                        a_cmd.args(["-stream_loop", "-1", "-i", &path.to_string_lossy()]);
                    }
                    OverlayKind::Audio { path, .. } => {
                        a_cmd.args(["-i", &path.to_string_lossy()]);
                    }
                    _ => {}
                }
                ov_input_indices.push(cur_a_input);
                cur_a_input += 1;
            }

            let mic_track = self.audio_tracks > 1;
            let game = self.export.audio.game && self.audio_tracks > 0;
            let mic = mic_track && self.export.audio.mic;

            let mix_args: Vec<String> = if !self.export.audio.tracks.is_empty() {
                let active: Vec<(usize, &crate::export::TrackSetting)> = self.export.audio
                    .tracks
                    .iter()
                    .enumerate()
                    .take(self.audio_tracks)
                    .filter(|(_, t)| t.enabled)
                    .collect();

                if !active_sound_overlays.is_empty() {
                    let mut parts = Vec::new();
                    if active.is_empty() {
                        let remaining = (self.duration.as_secs_f64() - seek_secs).max(0.05);
                        parts.push(format!("anullsrc=channel_layout=stereo:sample_rate=48000,atrim=0:{:.3}[abase]", remaining));
                    } else if active.len() == 1 {
                        let (idx, track) = active[0];
                        if (track.volume - 1.0).abs() >= 0.01 {
                            if track.volume > 1.0 {
                                parts.push(format!("[0:a:{idx}]volume={:.2},alimiter=limit=0.95[abase]", track.volume));
                            } else {
                                parts.push(format!("[0:a:{idx}]volume={:.2}[abase]", track.volume));
                            }
                        } else {
                            parts.push(format!("[0:a:{idx}]anull[abase]"));
                        }
                    } else {
                        let mut inputs = Vec::new();
                        for (idx, track) in &active {
                            let in_tag = format!("[0:a:{idx}]");
                            if (track.volume - 1.0).abs() >= 0.01 {
                                let tag = if *idx == 1 && self.audio_tracks == 2 && track.name == "mic" {
                                    "[mic]".to_string()
                                } else {
                                    format!("[abase_{idx}]")
                                };
                                if track.volume > 1.0 {
                                    parts.push(format!("{in_tag}volume={:.2},alimiter=limit=0.95{tag}", track.volume));
                                } else {
                                    parts.push(format!("{in_tag}volume={:.2}{tag}", track.volume));
                                }
                                inputs.push(tag);
                            } else {
                                inputs.push(in_tag);
                            }
                        }
                        parts.push(format!("{}amix=inputs={}:normalize=0[abase]", inputs.join(""), inputs.len()));
                    }

                    let mut amix_inputs = vec!["[abase]".to_string()];
                    for (k, ((_, ov, vol), input_idx)) in active_sound_overlays.iter().zip(ov_input_indices.iter()).enumerate() {
                        let ov_label = format!("[a_ov{}]", k);
                        if seek_secs < ov.start {
                            let delay_ms = ((ov.start - seek_secs) * 1000.0).round() as u64;
                            let len = ov.end - ov.start;
                            parts.push(format!(
                                "[{}:a]atrim=0:{:.3},volume={:.2},adelay={}:all=1{}",
                                input_idx, len, vol, delay_ms, ov_label
                            ));
                        } else {
                            let offset = seek_secs - ov.start;
                            let len = ov.end - ov.start;
                            parts.push(format!(
                                "[{}:a]atrim={:.3}:{:.3},volume={:.2},adelay=0:all=1{}",
                                input_idx, offset, len, vol, ov_label
                            ));
                        }
                        amix_inputs.push(ov_label);
                    }
                    parts.push(format!("{}amix=inputs={}:normalize=0:duration=first", amix_inputs.join(""), amix_inputs.len()));
                    vec!["-filter_complex".into(), parts.join(";")]
                } else if active.is_empty() {
                    vec!["-filter_complex".into(), "anullsrc=channel_layout=stereo:sample_rate=48000".into()]
                } else if active.len() == 1 {
                    let (idx, track) = active[0];
                    if (track.volume - 1.0).abs() >= 0.01 {
                        let filter = if track.volume > 1.0 {
                            format!("volume={:.2},alimiter=limit=0.95", track.volume)
                        } else {
                            format!("volume={:.2}", track.volume)
                        };
                        vec!["-map".into(), format!("0:a:{idx}"), "-af".into(), filter]
                    } else {
                        vec!["-map".into(), format!("0:a:{idx}")]
                    }
                } else {
                    let mut parts = Vec::new();
                    let mut inputs = Vec::new();
                    for (idx, track) in &active {
                        let in_tag = format!("[0:a:{idx}]");
                        if (track.volume - 1.0).abs() >= 0.01 {
                            let tag = if *idx == 1 && self.audio_tracks == 2 && track.name == "mic" {
                                "[mic]".to_string()
                            } else {
                                format!("[a_{idx}]")
                            };
                            if track.volume > 1.0 {
                                parts.push(format!("{in_tag}volume={:.2},alimiter=limit=0.95{tag}", track.volume));
                            } else {
                                parts.push(format!("{in_tag}volume={:.2}{tag}", track.volume));
                            }
                            inputs.push(tag);
                        } else {
                            inputs.push(in_tag);
                        }
                    }
                    let amix = format!("{}amix=inputs={}:normalize=0", inputs.join(""), inputs.len());
                    if parts.is_empty() {
                        vec!["-filter_complex".into(), amix]
                    } else {
                        vec!["-filter_complex".into(), format!("{};{}", parts.join(";"), amix)]
                    }
                }
            } else if !active_sound_overlays.is_empty() {
                let mut parts = Vec::new();
                if self.audio_tracks > 1 {
                    if game && mic {
                        if (self.export.audio.mic_volume - 1.0).abs() >= 0.01 {
                            if self.export.audio.mic_volume > 1.0 {
                                parts.push(format!("[0:a:1]volume={:.2},alimiter=limit=0.95[mic]", self.export.audio.mic_volume));
                            } else {
                                parts.push(format!("[0:a:1]volume={:.2}[mic]", self.export.audio.mic_volume));
                            }
                            parts.push("[0:a:0][mic]amix=inputs=2:normalize=0[abase]".to_string());
                        } else {
                            parts.push("[0:a:0][0:a:1]amix=inputs=2:normalize=0[abase]".to_string());
                        }
                    } else if game {
                        parts.push("[0:a:0]anull[abase]".to_string());
                    } else if mic {
                        if (self.export.audio.mic_volume - 1.0).abs() >= 0.01 {
                            if self.export.audio.mic_volume > 1.0 {
                                parts.push(format!("[0:a:1]volume={:.2},alimiter=limit=0.95[abase]", self.export.audio.mic_volume));
                            } else {
                                parts.push(format!("[0:a:1]volume={:.2}[abase]", self.export.audio.mic_volume));
                            }
                        } else {
                            parts.push("[0:a:1]anull[abase]".to_string());
                        }
                    } else {
                        let remaining = (self.duration.as_secs_f64() - seek_secs).max(0.05);
                        parts.push(format!("anullsrc=channel_layout=stereo:sample_rate=48000,atrim=0:{:.3}[abase]", remaining));
                    }
                } else if self.audio_tracks == 1 {
                    if game {
                        parts.push("[0:a:0]anull[abase]".to_string());
                    } else {
                        let remaining = (self.duration.as_secs_f64() - seek_secs).max(0.05);
                        parts.push(format!("anullsrc=channel_layout=stereo:sample_rate=48000,atrim=0:{:.3}[abase]", remaining));
                    }
                } else {
                    let remaining = (self.duration.as_secs_f64() - seek_secs).max(0.05);
                    parts.push(format!("anullsrc=channel_layout=stereo:sample_rate=48000,atrim=0:{:.3}[abase]", remaining));
                }

                let mut amix_inputs = vec!["[abase]".to_string()];
                for (k, ((_, ov, vol), input_idx)) in active_sound_overlays.iter().zip(ov_input_indices.iter()).enumerate() {
                    let ov_label = format!("[a_ov{}]", k);
                    if seek_secs < ov.start {
                        let delay_ms = ((ov.start - seek_secs) * 1000.0).round() as u64;
                        let len = ov.end - ov.start;
                        parts.push(format!(
                            "[{}:a]atrim=0:{:.3},volume={:.2},adelay={}:all=1{}",
                            input_idx, len, vol, delay_ms, ov_label
                        ));
                    } else {
                        let offset = seek_secs - ov.start;
                        let len = ov.end - ov.start;
                        parts.push(format!(
                            "[{}:a]atrim={:.3}:{:.3},volume={:.2},adelay=0:all=1{}",
                            input_idx, offset, len, vol, ov_label
                        ));
                    }
                    amix_inputs.push(ov_label);
                }
                parts.push(format!("{}amix=inputs={}:normalize=0:duration=first", amix_inputs.join(""), amix_inputs.len()));
                vec!["-filter_complex".into(), parts.join(";")]
            } else if self.audio_tracks > 1 {
                if game && mic {
                    if (self.export.audio.mic_volume - 1.0).abs() >= 0.01 {
                        let mic_filter = if self.export.audio.mic_volume > 1.0 {
                            format!("[0:a:1]volume={:.2},alimiter=limit=0.95[mic]", self.export.audio.mic_volume)
                        } else {
                            format!("[0:a:1]volume={:.2}[mic]", self.export.audio.mic_volume)
                        };
                        vec!["-filter_complex".into(), format!("{};[0:a:0][mic]amix=inputs=2:normalize=0", mic_filter)]
                    } else {
                        vec!["-filter_complex".into(), "[0:a:0][0:a:1]amix=inputs=2:normalize=0".into()]
                    }
                } else if game {
                    vec!["-map".into(), "0:a:0".into()]
                } else if mic {
                    if (self.export.audio.mic_volume - 1.0).abs() >= 0.01 {
                        let mic_filter = if self.export.audio.mic_volume > 1.0 {
                            format!("volume={:.2},alimiter=limit=0.95", self.export.audio.mic_volume)
                        } else {
                            format!("volume={:.2}", self.export.audio.mic_volume)
                        };
                        vec!["-map".into(), "0:a:1".into(), "-af".into(), mic_filter]
                    } else {
                        vec!["-map".into(), "0:a:1".into()]
                    }
                } else {
                    vec!["-filter_complex".into(), "anullsrc=channel_layout=stereo:sample_rate=48000".into()]
                }
            } else if self.audio_tracks == 1 && !game {
                vec!["-filter_complex".into(), "anullsrc=channel_layout=stereo:sample_rate=48000".into()]
            } else {
                Vec::new()
            };

            let mut a_child = a_cmd
                .args(&mix_args)
                .args([
                    "-vn",
                    "-sn",
                    "-f",
                    "f32le",
                    "-acodec",
                    "pcm_f32le",
                    "-ar",
                    "48000",
                    "-ac",
                    "2",
                    "-",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok();

            if let Some(child) = a_child.as_mut() {
                if let Some(stdout) = child.stdout.take() {
                    let (audio_tx, audio_rx) = queue(true);
                    player.pause();
                    player.append(audio_rx);
                    let ready = Arc::clone(&self.audio_ready);

                    // ffmpeg decodes as fast as it can; this thread stays a little ahead of
                    // playback by waiting on chunks being played. (Pacing ffmpeg with -re
                    // instead left no buffer: every hiccup became silence and the sound
                    // drifted further behind the picture.)
                    thread::spawn(move || {
                        const AHEAD: usize = 50; // chunks of 20 ms: one second
                        let mut playing: std::collections::VecDeque<std::sync::mpsc::Receiver<()>> = Default::default();
                        let mut reader = BufReader::new(stdout);
                        let mut chunk = vec![0u8; 48000 / 50 * 8];
                        let sample_rate = NonZeroU32::new(48000).unwrap();
                        let channels = NonZeroU16::new(2).unwrap();
                        loop {
                            let mut filled = 0;
                            while filled < chunk.len() {
                                match reader.read(&mut chunk[filled..]) {
                                    Ok(0) => break,
                                    Ok(n) => filled += n,
                                    Err(_) => break,
                                }
                            }
                            let usable = filled - filled % 8;
                            if usable == 0 {
                                break;
                            }
                            let samples: Vec<f32> = chunk[..usable]
                                .chunks_exact(4)
                                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                                .collect();
                            playing.retain(|done| matches!(done.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));
                            if playing.len() >= AHEAD {
                                let Some(oldest) = playing.pop_front() else { break };
                                // A seek or closing the clip replaces `audio_ready`; this
                                // thread then holds the only reference and stops.
                                while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = oldest.recv_timeout(Duration::from_millis(100)) {
                                    if Arc::strong_count(&ready) == 1 {
                                        return;
                                    }
                                }
                            }
                            playing.push_back(audio_tx.append_with_signal(SamplesBuffer::new(channels, sample_rate, samples)));
                            ready.store(true, Ordering::Release);
                            if usable < chunk.len() {
                                break;
                            }
                        }
                        ready.store(true, Ordering::Release);
                    });
                } else {
                    self.audio_ready.store(true, Ordering::Release);
                }
            } else {
                self.audio_ready.store(true, Ordering::Release);
            }
            self.audio_child = a_child;
        }
        self.set_audio_volume(self.volume);
    }

    pub fn has_video(&self) -> bool {
        if let Some(mpv) = &self.mpv {
            !mpv.has_failed()
        } else {
            self.texture.is_some()
        }
    }

    pub fn paint_video(&self, ui: &egui::Ui, rect: egui::Rect) {
        if let Some(mpv) = &self.mpv {
            mpv.paint(ui, rect);
        } else if let Some(texture) = &self.texture {
            let painter = ui.painter();
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broken_backends_memory() {
        let vaapi = DecodeBackend::Vaapi("/dev/dri/renderD128".into());
        let cuda = DecodeBackend::Cuda;
        let cpu = DecodeBackend::Cpu;

        assert!(!is_backend_broken(&vaapi, Some("av1")));
        mark_backend_broken(&vaapi, Some("av1"));
        assert!(is_backend_broken(&vaapi, Some("av1")));
        assert!(!is_backend_broken(&vaapi, Some("h264")));

        // CPU is never marked broken
        mark_backend_broken(&cpu, Some("av1"));
        assert!(!is_backend_broken(&cpu, Some("av1")));

        // Keyed by backend alone
        mark_backend_broken(&cuda, None);
        assert!(is_backend_broken(&cuda, Some("av1")));
        assert!(is_backend_broken(&cuda, Some("hevc")));
        assert!(is_backend_broken(&cuda, None));
    }

    #[test]
    fn test_decode_candidates_contains_cpu_last() {
        let candidates = decode_candidates();
        assert!(!candidates.is_empty());
        assert_eq!(candidates.last().unwrap(), &DecodeBackend::Cpu);
    }
}
