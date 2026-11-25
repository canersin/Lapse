//! Text, image, and video overlays for the clip editor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use eframe::egui::{self, Color32, ColorImage, Context, CursorIcon, FontId, Margin, Pos2, Rect, RichText, Rounding, Sense, Stroke, TextureHandle, Vec2};

use crate::gui::components::button::{m3_field_style, m3_small_button, m3_small_button_colored};
use crate::gui::components::chip::m3_chip;
use crate::gui::components::motion::m3_switch_labeled;
use crate::gui::components::slider::{m3_slider, SliderOpts};
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};
use crate::gui::views::trimmer::engine::PlayerState;

/// An overlay placed on top of the exported clip.
#[derive(Clone, Debug, PartialEq)]
pub struct Overlay {
    pub kind: OverlayKind,
    /// Normalized center X (0.0..=1.0) in the output frame.
    pub x: f32,
    /// Normalized center Y (0.0..=1.0) in the output frame.
    pub y: f32,
    /// Start time in seconds in the clip.
    pub start: f64,
    /// End time in seconds in the clip.
    pub end: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OverlayKind {
    Text {
        text: String,
        /// Fraction of output height (default 0.06).
        size: f32,
        /// Color with alpha.
        color: Color32,
        /// Dark outline toggle (default true).
        outline: bool,
    },
    Image {
        path: PathBuf,
        /// Fraction of output width (default 0.25).
        scale: f32,
        /// Opacity in 0.0..=1.0 (default 1.0).
        opacity: f32,
    },
    Video {
        path: PathBuf,
        /// Fraction of output width (default 0.25).
        scale: f32,
        /// Opacity in 0.0..=1.0 (default 1.0).
        opacity: f32,
        /// Whether sound is played/exported (default false).
        sound: bool,
        /// Volume in 0.0..=2.0 (default 1.0).
        volume: f32,
    },
    Audio {
        path: PathBuf,
        /// Volume in 0.0..=2.0 (default 1.0).
        volume: f32,
    },
}

impl Overlay {
    pub fn new_text(text: impl Into<String>, start: f64, end: f64) -> Self {
        Self {
            kind: OverlayKind::Text {
                text: text.into(),
                size: 0.06,
                color: Color32::WHITE,
                outline: true,
            },
            x: 0.5,
            y: 0.5,
            start,
            end,
        }
    }

    pub fn new_image(path: PathBuf, start: f64, end: f64) -> Self {
        Self {
            kind: OverlayKind::Image {
                path,
                scale: 0.25,
                opacity: 1.0,
            },
            x: 0.5,
            y: 0.5,
            start,
            end,
        }
    }

    pub fn new_video(path: PathBuf, start: f64, end: f64) -> Self {
        Self {
            kind: OverlayKind::Video {
                path,
                scale: 0.25,
                opacity: 1.0,
                sound: false,
                volume: 1.0,
            },
            x: 0.5,
            y: 0.5,
            start,
            end,
        }
    }

    pub fn new_audio(path: PathBuf, start: f64, end: f64) -> Self {
        Self {
            kind: OverlayKind::Audio {
                path,
                volume: 1.0,
            },
            x: 0.5,
            y: 0.5,
            start,
            end,
        }
    }

    pub fn is_visible_at(&self, time_secs: f64) -> bool {
        time_secs >= self.start && time_secs <= self.end
    }
}

fn image_cache() -> &'static Mutex<HashMap<PathBuf, TextureHandle>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, TextureHandle>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

pub fn get_or_load_image(ctx: &Context, path: &Path) -> Option<TextureHandle> {
    let mut cache = image_cache().lock().ok()?;
    if let Some(tex) = cache.get(path) {
        return Some(tex.clone());
    }
    let img = image::open(path).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    let color_image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img.into_raw());
    let handle = ctx.load_texture(path.to_string_lossy(), color_image, egui::TextureOptions::LINEAR);
    cache.insert(path.to_path_buf(), handle.clone());
    Some(handle)
}

pub struct CachedGif {
    pub frames: Vec<(TextureHandle, f64)>,
    pub total_duration: f64,
}

fn gif_cache() -> &'static Mutex<HashMap<PathBuf, Arc<CachedGif>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<CachedGif>>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

pub fn get_or_load_gif(ctx: &Context, path: &Path) -> Option<Arc<CachedGif>> {
    let mut cache = gif_cache().lock().ok()?;
    if let Some(cg) = cache.get(path) {
        return Some(Arc::clone(cg));
    }
    use image::AnimationDecoder;
    use image::codecs::gif::GifDecoder;
    use std::io::BufReader;

    let file = std::fs::File::open(path).ok()?;
    let decoder = GifDecoder::new(BufReader::new(file)).ok()?;
    let frames = decoder.into_frames().collect_frames().ok()?;
    if frames.is_empty() {
        return None;
    }

    let mut tex_frames = Vec::new();
    let mut total_duration = 0.0f64;

    for (idx, frame) in frames.into_iter().enumerate() {
        let (numer, denom) = frame.delay().numer_denom_ms();
        let mut dur = (numer as f64 / denom.max(1) as f64) / 1000.0;
        if dur < 0.01 {
            dur = 0.1;
        }
        total_duration += dur;

        let buffer = frame.into_buffer();
        let (w, h) = buffer.dimensions();
        let (new_w, new_h) = if w >= h {
            let nw = w.min(512);
            let nh = ((h as f32 * nw as f32 / w as f32).round() as u32).max(1);
            (nw, nh)
        } else {
            let nh = h.min(512);
            let nw = ((w as f32 * nh as f32 / h as f32).round() as u32).max(1);
            (nw, nh)
        };

        let (cw, ch, raw) = if new_w != w || new_h != h {
            let resized = image::imageops::resize(&buffer, new_w, new_h, image::imageops::FilterType::Triangle);
            (new_w as usize, new_h as usize, resized.into_raw())
        } else {
            (w as usize, h as usize, buffer.into_raw())
        };

        let color_image = ColorImage::from_rgba_unmultiplied([cw, ch], &raw);
        let tex_name = format!("{}_frame_{}", path.to_string_lossy(), idx);
        let handle = ctx.load_texture(tex_name, color_image, egui::TextureOptions::LINEAR);
        tex_frames.push((handle, dur));
    }

    let cached = Arc::new(CachedGif {
        frames: tex_frames,
        total_duration,
    });
    cache.insert(path.to_path_buf(), Arc::clone(&cached));
    Some(cached)
}

pub fn get_or_load_overlay_image(ctx: &Context, path: &Path, current_secs: f64, overlay_start: f64) -> Option<TextureHandle> {
    let is_gif = path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("gif")).unwrap_or(false);
    if is_gif {
        let gif = get_or_load_gif(ctx, path)?;
        if gif.frames.is_empty() {
            return None;
        }
        let rel_time = (current_secs - overlay_start).max(0.0);
        let anim_time = if gif.total_duration > 0.0 {
            rel_time % gif.total_duration
        } else {
            0.0
        };
        let mut elapsed = 0.0;
        let mut active_tex = &gif.frames[0].0;
        for (tex, dur) in &gif.frames {
            if anim_time < elapsed + dur {
                active_tex = tex;
                break;
            }
            elapsed += dur;
        }
        Some(active_tex.clone())
    } else {
        get_or_load_image(ctx, path)
    }
}

static VIDEO_META_CACHE: OnceLock<Mutex<HashMap<PathBuf, (usize, usize, f64)>>> = OnceLock::new();

pub fn get_or_probe_video_meta(path: &Path) -> (usize, usize, f64) {
    let cache = VIDEO_META_CACHE.get_or_init(Mutex::default);
    if let Ok(guard) = cache.lock() {
        if let Some(&meta) = guard.get(path) {
            return meta;
        }
    }

    let mut w = 0usize;
    let mut h = 0usize;
    let mut dur = 0.0f64;
    let mut rotation = 0i32;
    let mut sar = 1.0f64;

    if let Ok(output) = crate::utils::ffprobe_cmd()
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,sample_aspect_ratio:stream_side_data=rotation:format=duration",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                match k.trim() {
                    "width" => w = v.trim().parse().unwrap_or(0),
                    "height" => h = v.trim().parse().unwrap_or(0),
                    "duration" => dur = v.trim().parse().unwrap_or(0.0),
                    "rotation" => rotation = v.trim().parse().unwrap_or(0),
                    "sample_aspect_ratio" => {
                        if let Some((n, d)) = v.trim().split_once(':') {
                            let (n, d) = (n.parse::<f64>().unwrap_or(0.0), d.parse::<f64>().unwrap_or(0.0));
                            if n > 0.0 && d > 0.0 {
                                sar = n / d;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Report the size as displayed: ffmpeg applies the pixel aspect and phone rotation
    // metadata when decoding, so the preview pipe must be sized to match.
    w = (w as f64 * sar).round() as usize;
    if rotation.rem_euclid(180) == 90 {
        std::mem::swap(&mut w, &mut h);
    }
    if w == 0 || h == 0 {
        w = 480;
        h = 270;
    }

    let meta = (w, h, dur);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(path.to_path_buf(), meta);
    }
    meta
}

pub struct VideoOverlayDecoder {
    pub path: PathBuf,
    child: Option<std::process::Child>,
    stop_flag: Arc<std::sync::atomic::AtomicBool>,
    is_playing: Arc<std::sync::atomic::AtomicBool>,
    latest_frame: Arc<Mutex<Option<ColorImage>>>,
    pub texture: Option<TextureHandle>,
    pub aspect_ratio: f32,
    pub last_seek_seq: u64,
}

impl Drop for VideoOverlayDecoder {
    fn drop(&mut self) {
        self.stop();
    }
}

impl VideoOverlayDecoder {
    pub fn start(
        path: PathBuf,
        current_secs: f64,
        overlay_start: f64,
        overlay_end: f64,
        is_playing_val: bool,
        seek_seq: u64,
    ) -> Self {
        let (orig_w, orig_h, duration) = get_or_probe_video_meta(&path);
        let aspect = orig_w as f32 / orig_h.max(1) as f32;
        let (target_w, target_h) = if orig_w >= orig_h {
            let w = (orig_w.min(480) / 2) * 2;
            let h = ((w as f32 / aspect).round() as usize / 2) * 2;
            (w.max(2), h.max(2))
        } else {
            let h = (orig_h.min(480) / 2) * 2;
            let w = ((h as f32 * aspect).round() as usize / 2) * 2;
            (w.max(2), h.max(2))
        };

        let loop_len = if duration > 0.05 { duration } else { (overlay_end - overlay_start).max(0.05) };
        let rel_time = (current_secs - overlay_start).max(0.0);
        let start_offset = rel_time % loop_len;

        let mut child = crate::utils::ffmpeg_cmd()
            .args(["-nostdin", "-loglevel", "error"])
            .args(["-stream_loop", "-1"])
            .args(["-ss", &format!("{:.3}", start_offset)])
            .args(["-i", &path.to_string_lossy()])
            .args(["-an", "-sn"])
            .args(["-vf", &format!("format=rgba,scale={}:{}:flags=bilinear,fps=30", target_w, target_h)])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok();

        let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let is_playing = Arc::new(std::sync::atomic::AtomicBool::new(is_playing_val));
        let latest_frame = Arc::new(Mutex::new(None));

        if let Some(stdout) = child.as_mut().and_then(|c| c.stdout.take()) {
            let stop_clone = Arc::clone(&stop_flag);
            let play_clone = Arc::clone(&is_playing);
            let frame_store = Arc::clone(&latest_frame);
            std::thread::spawn(move || {
                let frame_size = target_w * target_h * 4;
                let mut reader = std::io::BufReader::with_capacity(frame_size * 2, stdout);
                let mut buffer = vec![0u8; frame_size];
                let frame_dur = std::time::Duration::from_millis(33);

                let mut first_frame = true;
                while !stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
                    if !first_frame && !play_clone.load(std::sync::atomic::Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(25));
                        continue;
                    }
                    let start = std::time::Instant::now();
                    if std::io::Read::read_exact(&mut reader, &mut buffer).is_err() {
                        break;
                    }
                    let color_image = ColorImage::from_rgba_unmultiplied([target_w, target_h], &buffer);
                    if let Ok(mut lock) = frame_store.lock() {
                        *lock = Some(color_image);
                    }
                    first_frame = false;
                    let elapsed = start.elapsed();
                    if elapsed < frame_dur {
                        std::thread::sleep(frame_dur - elapsed);
                    }
                }
            });
        }

        Self {
            path,
            child,
            stop_flag,
            is_playing,
            latest_frame,
            texture: None,
            aspect_ratio: aspect,
            last_seek_seq: seek_seq,
        }
    }

    pub fn restart(
        &mut self,
        path: PathBuf,
        current_secs: f64,
        overlay_start: f64,
        overlay_end: f64,
        is_playing_val: bool,
        seek_seq: u64,
    ) {
        self.stop();
        let mut new_decoder = Self::start(path, current_secs, overlay_start, overlay_end, is_playing_val, seek_seq);
        // Keep the last frame only for a seek in the same file, never across files.
        if new_decoder.path == self.path {
            new_decoder.texture = self.texture.take();
        }
        *self = new_decoder;
    }

    pub fn update_playing(&self, is_playing: bool) {
        self.is_playing.store(is_playing, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn update_texture(&mut self, ctx: &Context) {
        if let Ok(mut lock) = self.latest_frame.lock() {
            if let Some(color_image) = lock.take() {
                if let Some(tex) = &mut self.texture {
                    tex.set(color_image, egui::TextureOptions::LINEAR);
                } else {
                    let handle = ctx.load_texture("video_overlay_preview", color_image, egui::TextureOptions::LINEAR);
                    self.texture = Some(handle);
                }
            }
        }
    }

    pub fn stop(&mut self) {
        self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextOverlayEditState {
    pub index: usize,
    pub text: String,
    pub original_text: String,
    pub select_all: bool,
}

pub fn is_editing_text_overlay(ctx: &Context) -> bool {
    let edit_id = egui::Id::new("overlay_text_editing");
    ctx.data_mut(|d| d.get_temp::<TextOverlayEditState>(edit_id)).is_some()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum OverlayDragTarget {
    Move(usize),
    Resize(usize),
}

/// Draws visible overlays in the preview and handles dragging / selection / in-place editing.
pub fn render_preview_overlays(
    ui: &egui::Ui,
    painter: &egui::Painter,
    theme: &MonetTheme,
    state: &mut PlayerState,
    output_rect: Rect,
) -> bool {
    if matches!(state.export.preset, crate::export::ExportPreset::Audio(_)) || state.export.overlays.is_empty() {
        return false;
    }

    let current_secs = state.current_time.as_secs_f64();
    let is_down = ui.input(|i| i.pointer.primary_down());
    let is_released = ui.input(|i| i.pointer.primary_released());
    let delta = ui.input(|i| i.pointer.delta());

    let drag_id = ui.id().with("overlay_drag_target");
    let edit_id = egui::Id::new("overlay_text_editing");

    // Stored as the bare target: egui's temp store is keyed by type, so writing an
    // Option and reading the inner type would never find it.
    let mut active_drag: Option<OverlayDragTarget> = ui.ctx().data_mut(|d| d.get_temp(drag_id));
    let mut edit_state: Option<TextOverlayEditState> = ui.ctx().data_mut(|d| d.get_temp(edit_id));

    if let Some(ref s) = edit_state {
        if s.index >= state.export.overlays.len() {
            ui.ctx().data_mut(|d| d.remove::<TextOverlayEditState>(edit_id));
            edit_state = None;
        }
    }

    let mut interacted = false;

    // Update video overlay decoders for visible video overlays
    let mut visible_video_indices = std::collections::HashSet::new();
    for (i, overlay) in state.export.overlays.iter().enumerate() {
        if overlay.is_visible_at(current_secs) {
            if let OverlayKind::Video { path, .. } = &overlay.kind {
                visible_video_indices.insert(i);
                if let Some(decoder) = state.video_overlay_decoders.get_mut(&i) {
                    if decoder.path != *path || decoder.last_seek_seq != state.seek_seq {
                        decoder.restart(path.clone(), current_secs, overlay.start, overlay.end, state.is_playing, state.seek_seq);
                    } else {
                        decoder.update_playing(state.is_playing);
                    }
                    decoder.update_texture(ui.ctx());
                } else {
                    let mut decoder = VideoOverlayDecoder::start(
                        path.clone(),
                        current_secs,
                        overlay.start,
                        overlay.end,
                        state.is_playing,
                        state.seek_seq,
                    );
                    decoder.update_texture(ui.ctx());
                    state.video_overlay_decoders.insert(i, decoder);
                }
            }
        }
    }

    // Stop and prune decoders for video overlays that are no longer visible or deleted
    state.video_overlay_decoders.retain(|idx, decoder| {
        let keep = visible_video_indices.contains(idx);
        if !keep {
            decoder.stop();
        }
        keep
    });

    if state.is_playing && !visible_video_indices.is_empty() {
        ui.ctx().request_repaint();
    }

    // First pass: compute bounding boxes for all visible overlays.
    let mut visible_bboxes: Vec<(usize, Rect)> = Vec::new();
    for (i, overlay) in state.export.overlays.iter().enumerate() {
        if !overlay.is_visible_at(current_secs) || matches!(overlay.kind, OverlayKind::Audio { .. }) {
            continue;
        }
        let center = Pos2::new(
            output_rect.left() + overlay.x * output_rect.width(),
            output_rect.top() + overlay.y * output_rect.height(),
        );
        let bbox = match &overlay.kind {
            OverlayKind::Text { text, size, .. } => {
                let font_size = (size * output_rect.height()).max(8.0);
                let font_id = FontId::proportional(font_size);
                let text_to_measure = if let Some(ref es) = edit_state {
                    if es.index == i {
                        &es.text
                    } else {
                        text
                    }
                } else {
                    text
                };
                let galley = painter.layout_no_wrap(text_to_measure.clone(), font_id, Color32::WHITE);
                let sz = Vec2::new(galley.size().x.max(32.0) + 16.0, galley.size().y.max(16.0) + 8.0);
                Rect::from_center_size(center, sz)
            }
            OverlayKind::Image { path, scale, .. } => {
                let tex = get_or_load_overlay_image(ui.ctx(), path, current_secs, overlay.start);
                let aspect = tex.as_ref().map(|t| t.size()[0] as f32 / t.size()[1].max(1) as f32).unwrap_or(1.0);
                let w = (scale * output_rect.width()).max(16.0);
                let h = (w / aspect).max(16.0);
                Rect::from_center_size(center, Vec2::new(w, h))
            }
            OverlayKind::Video { path, scale, .. } => {
                let aspect = if let Some(decoder) = state.video_overlay_decoders.get(&i) {
                    if let Some(tex) = &decoder.texture {
                        tex.size()[0] as f32 / tex.size()[1].max(1) as f32
                    } else {
                        decoder.aspect_ratio
                    }
                } else {
                    let (w, h, _) = get_or_probe_video_meta(path);
                    w as f32 / h.max(1) as f32
                };
                let w = (scale * output_rect.width()).max(16.0);
                let h = (w / aspect).max(16.0);
                Rect::from_center_size(center, Vec2::new(w, h))
            }
            OverlayKind::Audio { .. } => unreachable!(),
        };
        visible_bboxes.push((i, bbox));
    }

    let editing_idx = edit_state.as_ref().map(|s| s.index);

    if active_drag.is_some() {
        interacted = true;
        if !is_down || is_released {
            active_drag = None;
            ui.ctx().data_mut(|d| d.remove::<OverlayDragTarget>(drag_id));
        }
    }

    // Active drag handling (disabled for the overlay being edited)
    if let Some(target) = active_drag {
        let target_idx = match target {
            OverlayDragTarget::Move(idx) => idx,
            OverlayDragTarget::Resize(idx) => idx,
        };
        if editing_idx != Some(target_idx) {
            interacted = true;
            match target {
                OverlayDragTarget::Move(idx) => {
                    ui.ctx().set_cursor_icon(CursorIcon::Move);
                    if idx < state.export.overlays.len() && delta != Vec2::ZERO && output_rect.width() > 0.0 && output_rect.height() > 0.0 {
                        let dx = delta.x / output_rect.width();
                        let dy = delta.y / output_rect.height();
                        state.export.overlays[idx].x = (state.export.overlays[idx].x + dx).clamp(0.0, 1.0);
                        state.export.overlays[idx].y = (state.export.overlays[idx].y + dy).clamp(0.0, 1.0);
                    }
                }
                OverlayDragTarget::Resize(idx) => {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeNwSe);
                    if idx < state.export.overlays.len() && delta != Vec2::ZERO {
                        let cur_bbox = visible_bboxes.iter().find(|(i, _)| *i == idx).map(|(_, b)| *b);
                        let base_dim = cur_bbox.map(|b| b.width() + b.height()).unwrap_or(100.0).max(10.0);
                        let factor = 1.0 + (delta.x + delta.y) / base_dim;
                        match &mut state.export.overlays[idx].kind {
                            OverlayKind::Text { size, .. } => {
                                *size = (*size * factor).clamp(0.01, 0.40);
                            }
                            OverlayKind::Image { scale, .. } | OverlayKind::Video { scale, .. } => {
                                *scale = (*scale * factor).clamp(0.02, 1.0);
                            }
                            OverlayKind::Audio { .. } => {}
                        }
                    }
                }
            }
        } else {
            active_drag = None;
            ui.ctx().data_mut(|d| d.remove::<OverlayDragTarget>(drag_id));
        }
    }

    // If not dragging: handle interaction via egui ui.interact
    let mut handle_interacted = false;
    if active_drag.is_none() {
        // 1. Check resize handle of the selected overlay first (if not editing this overlay)
        if let Some(sel_idx) = state.export.selected {
            if editing_idx != Some(sel_idx) {
                if let Some((_, sel_bbox)) = visible_bboxes.iter().find(|(i, _)| *i == sel_idx) {
                    let handle_center = sel_bbox.expand(3.0).right_bottom();
                    // Generous hit area: the visible dot is small and sits on the box corner.
                    let handle_rect = Rect::from_center_size(handle_center, Vec2::splat(24.0));
                    let handle_id = ui.id().with("overlay_handle").with(sel_idx);
                    let handle_resp = ui.interact(handle_rect, handle_id, Sense::click_and_drag());
                    // The overlay's own hit box overlaps the handle and egui hands the drag to
                    // the widget registered last, so the bodies are skipped while over it.
                    if ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| handle_rect.contains(p)) {
                        handle_interacted = true;
                        interacted = true;
                    }
                    if handle_resp.hovered() || handle_resp.dragged() {
                        ui.ctx().set_cursor_icon(CursorIcon::ResizeNwSe);
                        interacted = true;
                        handle_interacted = true;
                    }
                    if handle_resp.drag_started() {
                        active_drag = Some(OverlayDragTarget::Resize(sel_idx));
                        if let Some(target) = active_drag {
                            ui.ctx().data_mut(|d| d.insert_temp(drag_id, target));
                        }
                        interacted = true;
                        handle_interacted = true;
                    }
                    if handle_resp.clicked() || (handle_resp.is_pointer_button_down_on() && is_released) {
                        interacted = true;
                        handle_interacted = true;
                    }
                }
            }
        }

        // 2. Check overlay bodies (top-most overlay first)
        if !handle_interacted {
            // Only the top-most overlay under the pointer gets a widget: with several
            // overlapping hit boxes egui would hand the press to whichever registered first.
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let top = pointer.and_then(|p| visible_bboxes.iter().rev().find(|(_, b)| b.expand(4.0).contains(p)).map(|(i, _)| *i));
            for &(i, bbox) in visible_bboxes.iter().rev() {
                if top.is_some_and(|t| t != i) {
                    continue;
                }
                // If editing this overlay, body click/drag does not trigger drag
                if editing_idx == Some(i) {
                    interacted = true;
                    continue;
                }

                let body_id = ui.id().with("overlay_body").with(i);
                let body_resp = ui.interact(bbox.expand(4.0), body_id, Sense::click_and_drag());

                if body_resp.double_clicked() {
                    if let OverlayKind::Text { text, .. } = &state.export.overlays[i].kind {
                        let new_state = TextOverlayEditState {
                            index: i,
                            text: text.clone(),
                            original_text: text.clone(),
                            select_all: true,
                        };
                        ui.ctx().data_mut(|d| d.insert_temp(edit_id, new_state.clone()));
                        edit_state = Some(new_state);
                        state.export.selected = Some(i);
                        ui.ctx().data_mut(|d| d.remove::<OverlayDragTarget>(drag_id));
                        interacted = true;
                        break;
                    }
                }

                if body_resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::Move);
                    interacted = true;
                }
                // A move starts only once the pointer really moves: starting it on press would
                // stop registering this widget, so egui would never report the (double) click.
                if body_resp.is_pointer_button_down_on() {
                    interacted = true;
                }
                if body_resp.drag_started() {
                    active_drag = Some(OverlayDragTarget::Move(i));
                    state.export.selected = Some(i);
                    if let Some(target) = active_drag {
                        ui.ctx().data_mut(|d| d.insert_temp(drag_id, target));
                    }
                    interacted = true;
                    break;
                }
                if body_resp.clicked() {
                    state.export.selected = Some(i);
                    interacted = true;
                    break;
                }
                if body_resp.is_pointer_button_down_on() && is_released {
                    interacted = true;
                    break;
                }
            }
        }
    }

    // Render pass: draw each visible overlay
    for (i, overlay) in state.export.overlays.iter_mut().enumerate() {
        if !overlay.is_visible_at(current_secs) || matches!(overlay.kind, OverlayKind::Audio { .. }) {
            continue;
        }
        let center = Pos2::new(
            output_rect.left() + overlay.x * output_rect.width(),
            output_rect.top() + overlay.y * output_rect.height(),
        );

        let is_editing_this = edit_state.as_ref().map(|s| s.index) == Some(i);

        let bbox = match &mut overlay.kind {
            OverlayKind::Text { text, size, color, outline } => {
                if is_editing_this {
                    let editing = edit_state.as_mut().unwrap();
                    let font_size = (*size * output_rect.height()).max(12.0);
                    let font_id = FontId::proportional(font_size);
                    let galley = painter.layout_no_wrap(editing.text.clone(), font_id.clone(), *color);
                    let edit_w = (galley.size().x + 28.0).max(80.0);
                    let edit_h = (font_size * 1.5).max(28.0);
                    let edit_rect = Rect::from_center_size(center, Vec2::new(edit_w, edit_h));

                    // Transparent-ish background with primary-colour outline
                    painter.rect_filled(edit_rect, Rounding::same(4.0), Color32::from_black_alpha(170));
                    painter.rect_stroke(edit_rect, Rounding::same(4.0), Stroke::new(1.5_f32, theme.primary));

                    let text_edit_id = ui.id().with("inplace_overlay_text_edit").with(i);
                    let mut resp_opt = None;

                    egui::Area::new(text_edit_id.with("area"))
                        .fixed_pos(edit_rect.min + Vec2::new(4.0, 2.0))
                        .order(egui::Order::Foreground)
                        .show(ui.ctx(), |area_ui| {
                            area_ui.spacing_mut().item_spacing = Vec2::ZERO;
                            let text_edit = egui::TextEdit::singleline(&mut editing.text)
                                .id(text_edit_id)
                                .font(font_id)
                                .text_color(*color)
                                .frame(false)
                                .vertical_align(egui::Align::Center)
                                .desired_width(edit_rect.width() - 10.0);
                            let resp = area_ui.add(text_edit);
                            resp_opt = Some(resp);
                        });

                    let text_resp = resp_opt.unwrap();

                    if editing.select_all {
                        text_resp.request_focus();
                        let char_count = editing.text.chars().count();
                        let mut ts = egui::text_edit::TextEditState::load(ui.ctx(), text_edit_id).unwrap_or_default();
                        ts.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(char_count),
                        )));
                        ts.store(ui.ctx(), text_edit_id);
                        editing.select_all = false;
                        ui.ctx().data_mut(|d| d.insert_temp(edit_id, editing.clone()));
                    }

                    let esc_pressed = ui.input(|inp| inp.key_pressed(egui::Key::Escape));
                    let enter_pressed = ui.input(|inp| inp.key_pressed(egui::Key::Enter));
                    let clicked_elsewhere = ui.input(|inp| inp.pointer.primary_pressed()) && !text_resp.hovered();

                    if esc_pressed {
                        // Escape cancels: restore old text
                        *text = editing.original_text.clone();
                        ui.ctx().data_mut(|d| d.remove::<TextOverlayEditState>(edit_id));
                    } else if enter_pressed || (text_resp.lost_focus() && !esc_pressed) || clicked_elsewhere {
                        // Enter or clicking elsewhere commits
                        *text = editing.text.clone();
                        ui.ctx().data_mut(|d| d.remove::<TextOverlayEditState>(edit_id));
                    } else {
                        *text = editing.text.clone();
                        ui.ctx().data_mut(|d| d.insert_temp(edit_id, editing.clone()));
                    }
                    edit_rect
                } else {
                    let font_size = (*size * output_rect.height()).max(8.0);
                    let font_id = FontId::proportional(font_size);
                    let galley = painter.layout_no_wrap(text.clone(), font_id, *color);
                    let top_left = center - galley.size() * 0.5;
                    let bbox = Rect::from_min_size(top_left, galley.size());
                    if *outline {
                        let outline_color = Color32::from_black_alpha(200);
                        for (dx, dy) in [
                            (-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5),
                            (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0),
                        ] {
                            painter.galley(top_left + Vec2::new(dx, dy), galley.clone(), outline_color);
                        }
                    }
                    painter.galley(top_left, galley, *color);
                    bbox
                }
            }
            OverlayKind::Image { path, scale, opacity } => {
                if let Some(tex) = get_or_load_overlay_image(ui.ctx(), path, current_secs, overlay.start) {
                    let aspect = tex.size()[0] as f32 / tex.size()[1].max(1) as f32;
                    let w = *scale * output_rect.width();
                    let h = w / aspect;
                    let bbox = Rect::from_center_size(center, Vec2::new(w, h));
                    let tint = Color32::from_white_alpha((opacity.clamp(0.0, 1.0) * 255.0) as u8);
                    painter.image(tex.id(), bbox, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
                    bbox
                } else {
                    Rect::from_center_size(center, Vec2::splat(16.0))
                }
            }
            OverlayKind::Video { scale, opacity, .. } => {
                let aspect = if let Some(decoder) = state.video_overlay_decoders.get(&i) {
                    if let Some(tex) = &decoder.texture {
                        tex.size()[0] as f32 / tex.size()[1].max(1) as f32
                    } else {
                        decoder.aspect_ratio
                    }
                } else {
                    16.0 / 9.0
                };
                let w = *scale * output_rect.width();
                let h = w / aspect;
                let bbox = Rect::from_center_size(center, Vec2::new(w, h));
                if let Some(decoder) = state.video_overlay_decoders.get(&i) {
                    if let Some(tex) = &decoder.texture {
                        let tint = Color32::from_white_alpha((opacity.clamp(0.0, 1.0) * 255.0) as u8);
                        painter.image(tex.id(), bbox, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
                    } else {
                        painter.rect_filled(bbox, Rounding::same(2.0), Color32::from_black_alpha(120));
                    }
                } else {
                    painter.rect_filled(bbox, Rounding::same(2.0), Color32::from_black_alpha(120));
                }
                bbox
            }
            OverlayKind::Audio { .. } => unreachable!(),
        };

        if state.export.selected == Some(i) && !is_editing_this {
            let sel_rect = bbox.expand(3.0);
            painter.rect_stroke(sel_rect, Rounding::same(2.0), Stroke::new(1.5_f32, theme.primary));
            // Corner resize handle (bottom-right)
            let hr = Rect::from_center_size(sel_rect.right_bottom(), Vec2::splat(8.0));
            painter.rect_filled(hr, Rounding::same(2.0), Color32::WHITE);
            painter.rect_stroke(hr, Rounding::same(2.0), Stroke::new(1.2_f32, theme.primary));
        }
    }

    if is_editing_text_overlay(ui.ctx()) {
        interacted = true;
    }

    interacted
}

/// Renders the overlay controls and list in the export panel.
pub fn render_export_panel_overlays(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState) {
    if let Some(picked) = crate::gui::file_pick::take("trimmer_add_overlay_image") {
        let start = state.trim_start.as_secs_f64();
        let end = state.trim_end.as_secs_f64();
        state.export.overlays.push(Overlay::new_image(picked, start, end));
        state.export.selected = Some(state.export.overlays.len() - 1);
    }
    if let Some(picked) = crate::gui::file_pick::take("trimmer_change_overlay_image") {
        if let Some(idx) = state.export.selected {
            if idx < state.export.overlays.len() {
                if let OverlayKind::Image { path, .. } = &mut state.export.overlays[idx].kind {
                    *path = picked;
                }
            }
        }
    }

    if let Some(picked) = crate::gui::file_pick::take("trimmer_add_overlay_video") {
        let start = state.trim_start.as_secs_f64();
        let end = state.trim_end.as_secs_f64();
        state.export.overlays.push(Overlay::new_video(picked, start, end));
        state.export.selected = Some(state.export.overlays.len() - 1);
    }
    if let Some(picked) = crate::gui::file_pick::take("trimmer_change_overlay_video") {
        if let Some(idx) = state.export.selected {
            if idx < state.export.overlays.len() {
                if let OverlayKind::Video { path, .. } = &mut state.export.overlays[idx].kind {
                    *path = picked;
                }
            }
        }
    }

    if let Some(picked) = crate::gui::file_pick::take("trimmer_add_overlay_sound") {
        let start = state.trim_start.as_secs_f64();
        let end = state.trim_end.as_secs_f64();
        state.export.overlays.push(Overlay::new_audio(picked, start, end));
        state.export.selected = Some(state.export.overlays.len() - 1);
    }
    if let Some(picked) = crate::gui::file_pick::take("trimmer_change_overlay_sound") {
        if let Some(idx) = state.export.selected {
            if idx < state.export.overlays.len() {
                if let OverlayKind::Audio { path, .. } = &mut state.export.overlays[idx].kind {
                    *path = picked;
                }
            }
        }
    }

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new(t("Overlays")).size(13.0).strong().color(theme.on_surface));

        if m3_small_button(ui, theme, t("Add text")).clicked() {
            let start = state.trim_start.as_secs_f64();
            let end = state.trim_end.as_secs_f64();
            state.export.overlays.push(Overlay::new_text(t("Text"), start, end));
            state.export.selected = Some(state.export.overlays.len() - 1);
        }

        if m3_small_button(ui, theme, t("Add image")).clicked() {
            let exts = ["png", "PNG", "jpg", "JPG", "jpeg", "JPEG", "webp", "WEBP", "gif", "GIF"];
            crate::gui::file_pick::open(ui.ctx(), "trimmer_add_overlay_image", move || {
                rfd::FileDialog::new()
                    .set_title(t("Choose an overlay image"))
                    .add_filter(t("Images"), &exts)
                    .pick_file()
            });
        }

        if m3_small_button(ui, theme, t("Add video")).clicked() {
            let exts = ["mp4", "MP4", "webm", "WEBM", "mov", "MOV", "mkv", "MKV"];
            crate::gui::file_pick::open(ui.ctx(), "trimmer_add_overlay_video", move || {
                rfd::FileDialog::new()
                    .set_title(t("Choose an overlay video"))
                    .add_filter(t("Videos"), &exts)
                    .pick_file()
            });
        }

        if m3_small_button(ui, theme, t("Add sound")).clicked() {
            let exts = [
                "mp3", "MP3", "wav", "WAV", "ogg", "OGG", "opus", "OPUS", "flac", "FLAC", "m4a", "M4A", "aac", "AAC",
                "mp4", "MP4", "webm", "WEBM", "mov", "MOV", "mkv", "MKV",
            ];
            crate::gui::file_pick::open(ui.ctx(), "trimmer_add_overlay_sound", move || {
                rfd::FileDialog::new()
                    .set_title(t("Choose an audio file"))
                    .add_filter(t("Audio"), &exts)
                    .pick_file()
            });
        }
    });

    if state.export.overlays.is_empty() {
        state.export.selected = None;
        return;
    }

    ui.add_space(4.0);
    // Overlays list to select from
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for i in 0..state.export.overlays.len() {
            let is_sel = state.export.selected == Some(i);
            let label = match &state.export.overlays[i].kind {
                OverlayKind::Text { text, .. } => {
                    let mut preview = text.chars().take(12).collect::<String>();
                    if text.chars().count() > 12 {
                        preview.push('…');
                    }
                    if preview.is_empty() {
                        preview = t("Text").into();
                    }
                    format!("{}: {}", t("Text"), preview)
                }
                OverlayKind::Image { path, .. } => {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
                    format!("{}: {}", t("Image"), name)
                }
                OverlayKind::Video { path, .. } => {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("video");
                    format!("{}: {}", t("Video"), name)
                }
                OverlayKind::Audio { path, .. } => {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("audio");
                    format!("{}: {}", t("Sound"), name)
                }
            };
            if m3_chip(ui, theme, &label, is_sel).clicked() {
                state.export.selected = if is_sel { None } else { Some(i) };
            }
        }
    });

    let mut delete_idx = None;
    if let Some(idx) = state.export.selected {
        if idx < state.export.overlays.len() {
            ui.add_space(6.0);
            let overlay = &mut state.export.overlays[idx];
            let current_secs = state.current_time.as_secs_f64();
            let clip_start = state.trim_start.as_secs_f64();
            let clip_end = state.trim_end.as_secs_f64();
            let clip_dur = state.duration.as_secs_f64().max(0.1);

            egui::Frame::none()
                .fill(theme.surface_container_low)
                .stroke(Stroke::new(1.0_f32, theme.outline_variant))
                .rounding(M3Shapes::MEDIUM)
                .inner_margin(Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    egui::Grid::new("selected_overlay_settings_grid")
                        .num_columns(2)
                        .spacing(Vec2::new(14.0, 8.0))
                        .show(ui, |ui| {
                            match &mut overlay.kind {
                                OverlayKind::Text { text, size, color, outline } => {
                                    // Row 1: Text field
                                    ui.add(egui::Label::new(
                                        RichText::new(t("Text")).size(12.5).strong().color(theme.on_surface_variant),
                                    ));
                                    ui.horizontal(|ui| {
                                        let field_w = (ui.available_width() - 8.0).max(220.0);
                                        crate::gui::components::text_field::m3_text_field(
                                             ui,
                                             theme,
                                             text,
                                             crate::gui::components::text_field::TextFieldOpts {
                                                 hint: t("Text"),
                                                 width: Some(field_w),
                                                 multiline: false,
                                                 ..Default::default()
                                             },
                                        );
                                    });
                                    ui.end_row();

                                    // Row 2: Style (Size, Color, Outline)
                                    ui.add(egui::Label::new(
                                        RichText::new(t("Style")).size(12.5).strong().color(theme.on_surface_variant),
                                    ));
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                                        ui.set_min_height(28.0);
                                        ui.label(RichText::new(t("Size")).size(12.0).color(theme.on_surface_variant));
                                        let mut sz = *size as f64;
                                        if m3_slider(ui, theme, &mut sz, 0.01..=0.30, 90.0, SliderOpts::default()).changed() {
                                            *size = sz as f32;
                                        }
                                        ui.label(RichText::new(format!("{:.0}%", *size * 100.0)).size(12.0).color(theme.on_surface_variant));
                                        ui.add_space(10.0);

                                        ui.label(RichText::new(t("Color")).size(12.0).color(theme.on_surface_variant));
                                        ui.color_edit_button_srgba(color);
                                        ui.add_space(10.0);

                                        m3_switch_labeled(ui, theme, outline, t("Outline"));
                                    });
                                    ui.end_row();
                                }
                                OverlayKind::Image { path, scale, opacity } => {
                                    // Row 1: Image path info + replace button
                                    ui.add(egui::Label::new(
                                        RichText::new(t("Image")).size(12.5).strong().color(theme.on_surface_variant),
                                    ));
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
                                        ui.label(RichText::new(name).size(12.5).color(theme.on_surface));
                                        if m3_small_button(ui, theme, t("Change image...")).clicked() {
                                            let exts = ["png", "PNG", "jpg", "JPG", "jpeg", "JPEG", "webp", "WEBP", "gif", "GIF"];
                                            crate::gui::file_pick::open(ui.ctx(), "trimmer_change_overlay_image", move || {
                                                rfd::FileDialog::new()
                                                    .set_title(t("Choose an overlay image"))
                                                    .add_filter(t("Images"), &exts)
                                                    .pick_file()
                                            });
                                        }
                                    });
                                    ui.end_row();

                                    // Row 2: Style (Scale, Opacity)
                                    ui.add(egui::Label::new(
                                        RichText::new(t("Style")).size(12.5).strong().color(theme.on_surface_variant),
                                    ));
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                                        ui.set_min_height(28.0);
                                        ui.label(RichText::new(t("Scale")).size(12.0).color(theme.on_surface_variant));
                                        let mut sc = *scale as f64;
                                        if m3_slider(ui, theme, &mut sc, 0.05..=1.0, 90.0, SliderOpts::default()).changed() {
                                            *scale = sc as f32;
                                        }
                                        ui.label(RichText::new(format!("{:.0}%", *scale * 100.0)).size(12.0).color(theme.on_surface_variant));
                                        ui.add_space(12.0);

                                        ui.label(RichText::new(t("Opacity")).size(12.0).color(theme.on_surface_variant));
                                        let mut op = *opacity as f64;
                                        if m3_slider(ui, theme, &mut op, 0.0..=1.0, 90.0, SliderOpts::default()).changed() {
                                            *opacity = op as f32;
                                        }
                                        ui.label(RichText::new(format!("{:.0}%", *opacity * 100.0)).size(12.0).color(theme.on_surface_variant));
                                    });
                                    ui.end_row();
                                }
                                OverlayKind::Video { path, scale, opacity, sound, volume } => {
                                    // Row 1: Video path info + replace button
                                    ui.add(egui::Label::new(
                                        RichText::new(t("Video")).size(12.5).strong().color(theme.on_surface_variant),
                                    ));
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("video");
                                        ui.label(RichText::new(name).size(12.5).color(theme.on_surface));
                                        if m3_small_button(ui, theme, t("Change video...")).clicked() {
                                            let exts = ["mp4", "MP4", "webm", "WEBM", "mov", "MOV", "mkv", "MKV"];
                                            crate::gui::file_pick::open(ui.ctx(), "trimmer_change_overlay_video", move || {
                                                rfd::FileDialog::new()
                                                    .set_title(t("Choose an overlay video"))
                                                    .add_filter(t("Videos"), &exts)
                                                    .pick_file()
                                            });
                                        }
                                    });
                                    ui.end_row();

                                    // Row 2: Style (Scale, Opacity)
                                    ui.add(egui::Label::new(
                                        RichText::new(t("Style")).size(12.5).strong().color(theme.on_surface_variant),
                                    ));
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                                        ui.set_min_height(28.0);
                                        ui.label(RichText::new(t("Scale")).size(12.0).color(theme.on_surface_variant));
                                        let mut sc = *scale as f64;
                                        if m3_slider(ui, theme, &mut sc, 0.05..=1.0, 90.0, SliderOpts::default()).changed() {
                                            *scale = sc as f32;
                                        }
                                        ui.label(RichText::new(format!("{:.0}%", *scale * 100.0)).size(12.0).color(theme.on_surface_variant));
                                        ui.add_space(12.0);

                                        ui.label(RichText::new(t("Opacity")).size(12.0).color(theme.on_surface_variant));
                                        let mut op = *opacity as f64;
                                        if m3_slider(ui, theme, &mut op, 0.0..=1.0, 90.0, SliderOpts::default()).changed() {
                                            *opacity = op as f32;
                                        }
                                        ui.label(RichText::new(format!("{:.0}%", *opacity * 100.0)).size(12.0).color(theme.on_surface_variant));
                                    });
                                    ui.end_row();

                                    // Row 3: Sound (Switch + Volume)
                                    ui.add(egui::Label::new(
                                        RichText::new(t("Sound")).size(12.5).strong().color(theme.on_surface_variant),
                                    ));
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                                        ui.set_min_height(28.0);
                                        m3_switch_labeled(ui, theme, sound, t("Sound"));
                                        ui.add_space(8.0);
                                        ui.label(RichText::new(t("Volume")).size(12.0).color(theme.on_surface_variant));
                                        let mut v = *volume as f64;
                                        if m3_slider(ui, theme, &mut v, 0.0..=2.0, 90.0, SliderOpts::default()).changed() {
                                            *volume = v as f32;
                                        }
                                        ui.label(RichText::new(format!("{:.0}%", *volume * 100.0)).size(12.0).color(theme.on_surface_variant));
                                    });
                                    ui.end_row();
                                }
                                OverlayKind::Audio { path, volume } => {
                                    // Row 1: Audio path info + replace button
                                    ui.add(egui::Label::new(
                                        RichText::new(t("Sound")).size(12.5).strong().color(theme.on_surface_variant),
                                    ));
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("sound");
                                        ui.label(RichText::new(name).size(12.5).color(theme.on_surface));
                                        if m3_small_button(ui, theme, t("Change sound...")).clicked() {
                                            let exts = [
                                                "mp3", "MP3", "wav", "WAV", "ogg", "OGG", "opus", "OPUS", "flac", "FLAC", "m4a", "M4A", "aac", "AAC",
                                                "mp4", "MP4", "webm", "WEBM", "mov", "MOV", "mkv", "MKV",
                                            ];
                                            crate::gui::file_pick::open(ui.ctx(), "trimmer_change_overlay_sound", move || {
                                                rfd::FileDialog::new()
                                                    .set_title(t("Choose an audio file"))
                                                    .add_filter(t("Audio"), &exts)
                                                    .pick_file()
                                            });
                                        }
                                    });
                                    ui.end_row();

                                    // Row 2: Volume slider
                                    ui.add(egui::Label::new(
                                        RichText::new(t("Volume")).size(12.5).strong().color(theme.on_surface_variant),
                                    ));
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                                        ui.set_min_height(28.0);
                                        ui.label(RichText::new(t("Volume")).size(12.0).color(theme.on_surface_variant));
                                        let mut v = *volume as f64;
                                        if m3_slider(ui, theme, &mut v, 0.0..=2.0, 90.0, SliderOpts::default()).changed() {
                                            *volume = v as f32;
                                        }
                                        ui.label(RichText::new(format!("{:.0}%", *volume * 100.0)).size(12.0).color(theme.on_surface_variant));
                                    });
                                    ui.end_row();
                                }
                            }

                            // Row 3: Timing row
                            ui.add(egui::Label::new(
                                RichText::new(t("Timing")).size(12.5).strong().color(theme.on_surface_variant),
                            ));
                            m3_field_style(ui, theme);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                                ui.spacing_mut().interact_size.y = 26.0;

                                ui.label(RichText::new(t("Start")).size(12.0).color(theme.on_surface_variant));
                                crate::gui::views::trimmer::time_field_secs(
                                    ui,
                                    &mut overlay.start,
                                    0.0,
                                    (overlay.end - 0.1).max(0.0),
                                );
                                if m3_small_button(ui, theme, t("Set to playhead"))
                                    .on_hover_text(t("Set start to current playhead time"))
                                    .clicked()
                                {
                                    overlay.start = current_secs.min(overlay.end - 0.1).max(0.0);
                                }

                                ui.add_space(6.0);
                                ui.label(RichText::new(t("End")).size(12.0).color(theme.on_surface_variant));
                                crate::gui::views::trimmer::time_field_secs(
                                    ui,
                                    &mut overlay.end,
                                    overlay.start + 0.1,
                                    clip_dur,
                                );
                                if m3_small_button(ui, theme, t("Set to playhead"))
                                    .on_hover_text(t("Set end to current playhead time"))
                                    .clicked()
                                {
                                    overlay.end = current_secs.max(overlay.start + 0.1).min(clip_dur);
                                }

                                ui.add_space(6.0);
                                if m3_small_button(ui, theme, t("Whole clip")).clicked() {
                                    overlay.start = clip_start;
                                    overlay.end = clip_end;
                                }

                                ui.add_space(6.0);
                                if m3_small_button_colored(ui, theme, t("Delete"), theme.error).clicked() {
                                    delete_idx = Some(idx);
                                }
                            });
                            ui.end_row();
                        });
                });
        } else {
            state.export.selected = None;
        }
    }

    if let Some(d) = delete_idx {
        if d < state.export.overlays.len() {
            state.export.overlays.remove(d);
            state.export.selected = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_creation_and_visibility() {
        let text_ov = Overlay::new_text("Sample", 2.0, 5.0);
        assert!(!text_ov.is_visible_at(1.9));
        assert!(text_ov.is_visible_at(2.0));
        assert!(text_ov.is_visible_at(3.5));
        assert!(text_ov.is_visible_at(5.0));
        assert!(!text_ov.is_visible_at(5.1));

        let img_ov = Overlay::new_image(PathBuf::from("logo.png"), 1.0, 4.0);
        assert!(!img_ov.is_visible_at(0.5));
        assert!(img_ov.is_visible_at(1.0));
        assert!(img_ov.is_visible_at(4.0));
        assert!(!img_ov.is_visible_at(4.1));

        let vid_ov = Overlay::new_video(PathBuf::from("clip.mp4"), 3.0, 7.0);
        assert!(!vid_ov.is_visible_at(2.9));
        assert!(vid_ov.is_visible_at(3.0));
        assert!(vid_ov.is_visible_at(5.0));
        assert!(vid_ov.is_visible_at(7.0));
        assert!(!vid_ov.is_visible_at(7.1));
        if let OverlayKind::Video { sound, volume, .. } = vid_ov.kind {
            assert!(!sound);
            assert_eq!(volume, 1.0);
        } else {
            panic!("expected OverlayKind::Video");
        }

        let audio_ov = Overlay::new_audio(PathBuf::from("sound.mp3"), 1.0, 4.0);
        assert!(!audio_ov.is_visible_at(0.9));
        assert!(audio_ov.is_visible_at(1.0));
        assert!(audio_ov.is_visible_at(4.0));
        assert!(!audio_ov.is_visible_at(4.1));
        if let OverlayKind::Audio { volume, .. } = audio_ov.kind {
            assert_eq!(volume, 1.0);
        } else {
            panic!("expected OverlayKind::Audio");
        }
    }

    #[test]
    fn test_overlay_timing_clamping() {
        let mut ov = Overlay::new_text("ClampTest", 0.0, 10.0);
        let clip_dur = 8.0;

        // Simulate moving whole body by +3.0
        let dt = 3.0;
        let len = ov.end - ov.start;
        ov.start = (ov.start + dt).clamp(0.0, (clip_dur - len).max(0.0));
        ov.end = ov.start + len;
        assert_eq!(ov.start, 0.0); // since len (10.0) > clip_dur (8.0), start clamps to 0
        assert_eq!(ov.end, 10.0);

        // With a shorter overlay that fits
        let mut ov2 = Overlay::new_text("Short", 2.0, 5.0);
        let len2 = ov2.end - ov2.start; // 3.0
        let dt2 = 4.0;
        ov2.start = (ov2.start + dt2).clamp(0.0, (clip_dur - len2).max(0.0)); // 6.0 clamped to 8.0 - 3.0 = 5.0
        ov2.end = ov2.start + len2;
        assert_eq!(ov2.start, 5.0);
        assert_eq!(ov2.end, 8.0);
    }
}
