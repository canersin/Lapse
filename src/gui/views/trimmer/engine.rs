//! Clip playback: ffmpeg decodes video frames and audio into egui / rodio.

use eframe::egui::{self, ColorImage, TextureHandle};
use std::io::{BufReader, Read};
use std::num::{NonZeroU16, NonZeroU32};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use rodio::{buffer::SamplesBuffer, queue::queue, DeviceSinkBuilder, MixerDeviceSink, Player};

use super::export_panel::ExportPanel;
use super::timeline::Filmstrip;

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
    /// 2 when game audio and microphone were recorded as separate tracks.
    pub audio_tracks: usize,
    pub texture: Option<TextureHandle>,
    pub frame_receiver: Option<Receiver<VideoFrame>>,
    pub pending_frame: Option<VideoFrame>,
    pub video_child: Option<Child>,
    pub audio_child: Option<Child>,
    pub audio_ready: Arc<AtomicBool>,

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
        self.kill_children();
    }
}

impl PlayerState {
    pub fn new(path: PathBuf) -> Option<Self> {
        let file_size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let audio_tracks = crate::export::audio_stream_count(&path);

        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height,r_frame_rate,avg_frame_rate:format=duration",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .output()
            .ok()?;

        let text = String::from_utf8_lossy(&probe.stdout).to_string();
        let mut orig_width = 0usize;
        let mut orig_height = 0usize;
        let mut avg_fps = 0.0f64;
        let mut r_fps = 0.0f64;
        let mut duration_secs = 0.0f64;
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                let value = value.trim();
                match key.trim() {
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

        let max_w = 2560.0f32;
        let max_h = 1440.0f32;
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

        let (sink_handle, player) = if let Ok(mut sink) = DeviceSinkBuilder::open_default_sink() {
            sink.log_on_drop(false);
            let p = Player::connect_new(sink.mixer());
            (Some(sink), Some(p))
        } else {
            (None, None)
        };

        let path_for_strip = path.clone();
        let mut state = Self {
            video_path: path,
            file_size_bytes,
            audio_tracks,
            texture: None,
            frame_receiver: None,
            pending_frame: None,
            video_child: None,
            audio_child: None,
            audio_ready: Arc::new(AtomicBool::new(false)),
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
            export: ExportPanel::default(),
            filmstrip: Filmstrip::start(&path_for_strip, duration.as_secs_f64()),
            markers: crate::markers::load_for(&path_for_strip),
            highlights: Default::default(),
            active_drag: None,
            fullscreen: false,
            last_pointer_move: Instant::now(),
            pending_seek: None,
            last_seek_spawn: Instant::now() - Duration::from_secs(1),
        };

        state.seek_to(Duration::ZERO);
        Some(state)
    }

    pub fn set_audio_volume(&self, vol: f32) {
        if let Some(player) = &self.player {
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
        self.current_time = self.live_time();
        self.clock_base = self.current_time;
        self.is_playing = false;
        if let Some(player) = &self.player {
            player.pause();
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
        if !self.awaiting_first_frame {
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

        if self.awaiting_first_frame {
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
        self.kill_children();
        if let Some(player) = &self.player {
            player.stop();
        }

        self.current_time = target.min(self.duration);
        self.clock_base = self.current_time;
        self.clock_origin = Instant::now();
        self.awaiting_first_frame = true;
        self.first_frame_ready = false;
        self.awaiting_since = Instant::now();
        self.pending_seek = None;
        self.last_seek_spawn = Instant::now();
        self.audio_ready = Arc::new(AtomicBool::new(self.player.is_none()));

        let seek_secs = self.current_time.as_secs_f64();
        let target_w = self.preview_width;
        let target_h = self.preview_height;
        let fps = self.fps;
        let path_str = self.video_path.to_string_lossy().to_string();
        let ss = format!("{:.3}", seek_secs);
        let vf = format!("scale={}:{}:flags=bicubic,format=rgba", target_w, target_h);
        let rate = format!("{:.6}", fps);

        let mut v_child = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-loglevel",
                "error",
                "-hwaccel",
                "auto",
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
                });
            }
        }
        self.video_child = v_child;

        if let Some(player) = &self.player {
            // Hear game and microphone together when they are separate tracks.
            let mix_args: Vec<String> = if self.audio_tracks > 1 {
                let inputs: String = (0..self.audio_tracks).map(|t| format!("[0:a:{}]", t)).collect();
                vec!["-filter_complex".into(), format!("{}amix=inputs={}:normalize=0", inputs, self.audio_tracks)]
            } else {
                Vec::new()
            };
            let mut a_child = Command::new("ffmpeg")
                .args([
                    "-nostdin",
                    "-loglevel",
                    "error",
                    "-ss",
                    &ss,
                    "-i",
                    &path_str,
                ])
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
}
