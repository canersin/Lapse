use eframe::egui::{self, ColorImage, TextureHandle, Color32, RichText, Slider, Key};
use std::process::{Command, Stdio, Child};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::io::{Read, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use rodio::{MixerDeviceSink, Player, DeviceSinkBuilder, buffer::SamplesBuffer, queue::queue};
use std::num::{NonZeroU16, NonZeroU32};

pub struct PlayerState {
    pub video_path: PathBuf,
    pub texture: Option<TextureHandle>,
    pub frame_receiver: Option<Receiver<ColorImage>>,
    pub video_child: Option<Child>,
    pub audio_child: Option<Child>,
    pub width: usize,
    pub height: usize,
    
    // Playback state
    pub duration: Duration,
    pub current_time: Duration,
    pub last_update: Instant,
    pub is_playing: bool,
    pub volume: f32,
    pub muted: bool,

    // Audio
    pub sink_handle: MixerDeviceSink,
    pub player: Player,
}

impl PlayerState {
    pub fn new(path: PathBuf) -> Option<Self> {
        let probe_out = Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=s=x:p=0"])
            .arg(&path)
            .output()
            .ok()?;
            
        let probe_str = String::from_utf8_lossy(&probe_out.stdout);
        let dims: Vec<&str> = probe_str.trim().split('x').collect();
        if dims.len() != 2 { return None; }
        let width: usize = dims[0].parse().ok()?;
        let height: usize = dims[1].parse().ok()?;

        let probe_dur = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1"])
            .arg(&path)
            .output()
            .ok()?;
        let dur_str = String::from_utf8_lossy(&probe_dur.stdout);
        let duration_secs: f32 = dur_str.trim().parse().unwrap_or(0.0);
        let duration = Duration::from_secs_f32(duration_secs);

        let sink_handle = DeviceSinkBuilder::open_default_sink().ok()?;
        let player = Player::connect_new(sink_handle.mixer());
        
        let mut state = Self {
            video_path: path,
            texture: None,
            frame_receiver: None,
            video_child: None,
            audio_child: None,
            width,
            height,
            duration,
            current_time: Duration::ZERO,
            last_update: Instant::now(),
            is_playing: true,
            volume: 1.0,
            muted: false,
            sink_handle,
            player,
        };
        
        state.seek_to(Duration::ZERO);
        
        Some(state)
    }

    pub fn seek_to(&mut self, time: Duration) {
        self.stop_processes();
        self.current_time = time;
        self.last_update = Instant::now();
        self.texture = None;
        self.player.clear();
        
        // Setup async audio streaming
        let mut audio_cmd = Command::new("ffmpeg");
        if time.as_secs_f32() > 0.0 {
            audio_cmd.args(["-ss", &format!("{:.3}", time.as_secs_f32())]);
        }
        audio_cmd.args(["-i", self.video_path.to_str().unwrap()]);
        audio_cmd.args(["-f", "s16le", "-acodec", "pcm_s16le", "-ar", "44100", "-ac", "2", "-"]);
        
        if let Ok(mut child) = audio_cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
            if let Some(stdout) = child.stdout.take() {
                let (queue_tx, queue_rx) = queue(true);
                self.player.append(queue_rx);
                
                thread::spawn(move || {
                    let mut reader = BufReader::new(stdout);
                    let mut buf = [0u8; 8192];
                    while let Ok(()) = reader.read_exact(&mut buf) {
                        let mut samples = Vec::with_capacity(buf.len() / 2);
                        for chunk in buf.chunks_exact(2) {
                            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                            samples.push(sample as f32 / i16::MAX as f32);
                        }
                        if let (Some(channels), Some(rate)) = (NonZeroU16::new(2), NonZeroU32::new(44100)) {
                            let buffer = SamplesBuffer::new(channels, rate, samples);
                            queue_tx.append(buffer);
                        }
                    }
                });
                self.audio_child = Some(child);
            }
        }

        if !self.is_playing {
            self.player.pause();
        } else {
            self.player.play();
        }
        
        if self.muted {
            self.player.set_volume(0.0);
        } else {
            self.player.set_volume(self.volume);
        }

        // Spawn video
        let mut video_cmd = Command::new("ffmpeg");
        if time.as_secs_f32() > 0.0 {
            video_cmd.args(["-ss", &format!("{:.3}", time.as_secs_f32())]);
        }
        video_cmd.args(["-re", "-i", self.video_path.to_str().unwrap()]);
        video_cmd.args(["-f", "image2pipe", "-pix_fmt", "rgb24", "-vcodec", "rawvideo", "-"]);
        
        if let Ok(mut child) = video_cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
            if let Some(mut stdout) = child.stdout.take() {
                let (tx, rx) = mpsc::channel();
                self.frame_receiver = Some(rx);
                self.video_child = Some(child);
                
                let width = self.width;
                let height = self.height;
                let frame_size = width * height * 3;
                
                thread::spawn(move || {
                    let mut buffer = vec![0u8; frame_size];
                    while let Ok(()) = stdout.read_exact(&mut buffer) {
                        let image = ColorImage::from_rgb([width, height], &buffer);
                        if tx.send(image).is_err() {
                            break;
                        }
                    }
                });
            }
        }
    }

    pub fn stop_processes(&mut self) {
        if let Some(mut child) = self.video_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(mut child) = self.audio_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.frame_receiver = None;
    }

    pub fn stop(&mut self) {
        self.stop_processes();
        self.player.clear();
        self.player.pause(); // Ensure audio thread won't play residual if not immediately dropped
    }
}

pub fn render(app: &mut super::LapseApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let mut close = false;
    let mut seek_request = None;
    
    if let Some(state) = &mut app.player_state {
        // Handle input events globally
        if ui.input(|i| i.key_pressed(Key::Space)) {
            state.is_playing = !state.is_playing;
            if state.is_playing {
                state.player.play();
                state.last_update = Instant::now();
            } else {
                state.player.pause();
            }
        }
        if ui.input(|i| i.key_pressed(Key::ArrowLeft)) {
            let target = state.current_time.saturating_sub(Duration::from_secs(5));
            seek_request = Some(target);
        }
        if ui.input(|i| i.key_pressed(Key::ArrowRight)) {
            let target = (state.current_time + Duration::from_secs(5)).min(state.duration);
            seek_request = Some(target);
        }
        if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
            state.volume = (state.volume + 0.1).clamp(0.0, 1.0);
            if !state.muted { state.player.set_volume(state.volume); }
        }
        if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
            state.volume = (state.volume - 0.1).clamp(0.0, 1.0);
            if !state.muted { state.player.set_volume(state.volume); }
        }
        if ui.input(|i| i.key_pressed(Key::M)) {
            state.muted = !state.muted;
            if state.muted { state.player.set_volume(0.0); } 
            else { state.player.set_volume(state.volume); }
        }

        // Update current time if playing
        if state.is_playing {
            let elapsed = state.last_update.elapsed();
            state.last_update = Instant::now();
            state.current_time = (state.current_time + elapsed).min(state.duration);
            
            // Consume frames efficiently (in-place update and drop intermediate frames)
            if let Some(rx) = &state.frame_receiver {
                let mut latest_image = None;
                while let Ok(image) = rx.try_recv() {
                    latest_image = Some(image);
                }
                if let Some(image) = latest_image {
                    if let Some(tex) = &mut state.texture {
                        tex.set(image, egui::TextureOptions::LINEAR);
                    } else {
                        state.texture = Some(ctx.load_texture("video_frame", image, egui::TextureOptions::LINEAR));
                    }
                }
            }
        } else {
            state.last_update = Instant::now();
        }

        if state.texture.is_some() || state.is_playing {
            ctx.request_repaint(); // constantly repaint while playing
        }

        ui.horizontal(|ui| {
            if ui.button(RichText::new("⬅ Back to Library").size(16.0)).clicked() {
                close = true;
            }
        });

        ui.add_space(10.0);

        // Video Area
        let mut video_rect = ui.available_rect_before_wrap();
        video_rect.set_height(video_rect.height() - 60.0); // reserve space for controls
        
        let (rect, _resp) = ui.allocate_exact_size(video_rect.size(), egui::Sense::hover());
        
        if let Some(texture) = &state.texture {
            let aspect_ratio = state.width as f32 / state.height as f32;
            let mut target_size = rect.size();
            
            if target_size.x / aspect_ratio > target_size.y {
                target_size.x = target_size.y * aspect_ratio;
            } else {
                target_size.y = target_size.x / aspect_ratio;
            }

            let pos = rect.center() - target_size / 2.0;
            let image_rect = egui::Rect::from_min_size(pos, target_size);
            
            ui.painter().image(
                texture.id(),
                image_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE
            );
        } else {
            // Loading spinner
            ui.put(rect, egui::Spinner::new().size(32.0));
        }

        // Controls Area
        ui.add_space(5.0);
        
        let mut current_secs = state.current_time.as_secs_f32();
        let duration_secs = state.duration.as_secs_f32();

        // 1. Timeline Slider (Full width)
        let prev_width = ui.style().spacing.slider_width;
        ui.style_mut().spacing.slider_width = ui.available_width();
        
        let slider = Slider::new(&mut current_secs, 0.0..=duration_secs)
            .show_value(false)
            .trailing_fill(true);
            
        let response = ui.add(slider);
        
        ui.style_mut().spacing.slider_width = prev_width;
        
        if response.drag_released() || response.changed() {
            if (current_secs - state.current_time.as_secs_f32()).abs() > 0.5 {
                seek_request = Some(Duration::from_secs_f32(current_secs));
            }
        }
        
        ui.add_space(4.0);

        // 2. Play/Pause, Volume, Time
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                // Play/Pause
                let icon = if state.is_playing { "⏸" } else { "▶" };
                if ui.button(RichText::new(icon).size(20.0)).clicked() {
                    state.is_playing = !state.is_playing;
                    if state.is_playing {
                        state.player.play();
                        state.last_update = Instant::now();
                    } else {
                        state.player.pause();
                    }
                }
                
                ui.add_space(8.0);
                
                // Volume
                let vol_icon = if state.muted || state.volume == 0.0 { "🔇" } else { "🔊" };
                if ui.button(RichText::new(vol_icon).size(18.0)).clicked() {
                    state.muted = !state.muted;
                    if state.muted { state.player.set_volume(0.0); } 
                    else { state.player.set_volume(state.volume); }
                }
                
                let mut vol = state.volume;
                let vol_slider = ui.add_sized([80.0, 10.0], Slider::new(&mut vol, 0.0..=1.0).show_value(false).trailing_fill(true));
                if vol_slider.changed() {
                    state.volume = vol;
                    state.muted = false;
                    state.player.set_volume(vol);
                }
                
                ui.add_space(10.0);
                
                // Time
                ui.label(RichText::new(format!("{} / {}", format_time(state.current_time), format_time(state.duration))).size(14.0));
            });
        });
    }

    if let Some(target) = seek_request {
        if let Some(state) = &mut app.player_state {
            state.seek_to(target);
        }
    }
    
    if close {
        if let Some(mut state) = app.player_state.take() {
            state.stop();
        }
    }
}

fn format_time(duration: Duration) -> String {
    let secs = duration.as_secs();
    let minutes = secs / 60;
    let seconds = secs % 60;
    format!("{:02}:{:02}", minutes, seconds)
}
