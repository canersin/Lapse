//! Trim timeline: a filmstrip of the clip with draggable In/Out handles and a playhead.

use eframe::egui::{self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Rounding, Sense, Stroke, Vec2};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::engine::{HighlightJob, PlayerState, TimelineDragTarget};
use super::format_time;
use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::components::motion::{ease_out, lerp_color, FAST};
use crate::gui::components::button::{m3_small_button, m3_small_button_colored};
use crate::gui::i18n::t;
use crate::gui::theme::MonetTheme;

pub const FRAMES: usize = 24;

/// What a mark on the timeline stands for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MarkSource {
    /// Set with the marker hotkey.
    Marker,
    /// A kill the game reported while recording.
    Kill,
    /// Found by "Find highlights".
    Highlight,
}

impl MarkSource {
    pub fn color(self, theme: &MonetTheme) -> Color32 {
        match self {
            MarkSource::Marker => theme.tertiary,
            MarkSource::Kill => theme.error,
            MarkSource::Highlight => theme.secondary,
        }
    }
}

/// Every mark of the clip, in seconds, in time order.
pub fn all_marks(state: &PlayerState) -> Vec<(f64, MarkSource)> {
    use crate::markers::MarkKind;
    let mut v: Vec<(f64, MarkSource)> = state
        .markers
        .iter()
        .map(|m| (m.t, if m.kind == MarkKind::Kill { MarkSource::Kill } else { MarkSource::Marker }))
        .collect();
    if let Ok(job) = state.highlights.lock() {
        if let HighlightJob::Done(h) = &*job {
            v.extend(h.iter().map(|m| (*m, MarkSource::Highlight)));
        }
    }
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// Chip for a kill: crosshair and time.
fn kill_chip(ui: &mut egui::Ui, theme: &MonetTheme, time: &str) -> egui::Response {
    use crate::gui::components::motion::hover_t;
    let font = egui::FontId::proportional(12.5);
    let galley = ui.painter().layout_no_wrap(time.to_string(), font, theme.error);
    let size = Vec2::new(galley.size().x + 40.0, ui.spacing().interact_size.y.clamp(26.0, 30.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let h = hover_t(ui, &resp);
    let p = ui.painter();
    p.rect_filled(rect, rect.height() / 2.0, lerp_color(theme.surface_container_highest, theme.error_container, 0.35 + h * 0.4));
    p.rect_stroke(rect, rect.height() / 2.0, Stroke::new(1.0_f32, lerp_color(theme.outline_variant, theme.error, 0.5 + h * 0.5)));
    draw_vector_icon(p, Pos2::new(rect.left() + 16.0, rect.center().y), 15.0, IconKind::Crosshair, theme.error);
    p.galley(Pos2::new(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0), galley, theme.error);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Row under the timeline: a chip per marker (click to jump) and "Find highlights".
pub fn render_marker_bar(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState) -> Option<Duration> {
    let mut seek = None;
    let job_state = state.highlights.lock().map(|j| match &*j {
        HighlightJob::NotStarted => 0,
        HighlightJob::Running => 1,
        HighlightJob::Done(_) => 2,
        HighlightJob::Failed(_) => 3,
    });
    let failure = state.highlights.lock().ok().and_then(|j| match &*j {
        HighlightJob::Failed(e) => Some(e.clone()),
        _ => None,
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_space(10.0);
        for (m, source) in all_marks(state) {
            let time = format_time(Duration::from_secs_f64(m));
            let resp = match source {
                MarkSource::Kill => kill_chip(ui, theme, &time).on_hover_text(t("Kill")),
                MarkSource::Marker => m3_small_button_colored(ui, theme, &time, source.color(theme)).on_hover_text(t("Your marker")),
                MarkSource::Highlight => m3_small_button_colored(ui, theme, &time, source.color(theme)).on_hover_text(t("Loud moment")),
            };
            if resp.clicked() {
                seek = Some(Duration::from_secs_f64(m));
            }
        }
        match job_state {
            Ok(0) if state.audio_tracks > 0 => {
                if m3_small_button(ui, theme, t("Find highlights")).on_hover_text(t("Marks the loudest moments: shots, explosions, shouting.")).clicked() {
                    let job = Arc::clone(&state.highlights);
                    let path = state.video_path.clone();
                    *job.lock().unwrap_or_else(|e| e.into_inner()) = HighlightJob::Running;
                    std::thread::spawn(move || {
                        let result = match crate::highlights::find(&path) {
                            Ok(h) => HighlightJob::Done(h),
                            Err(e) => HighlightJob::Failed(e.to_string()),
                        };
                        *job.lock().unwrap_or_else(|e| e.into_inner()) = result;
                    });
                }
            }
            Ok(1) => {
                ui.label(egui::RichText::new(t("Looking for highlights...")).size(12.0).color(theme.on_surface_variant));
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
            Ok(2) if all_marks(state).iter().all(|(_, s)| *s != MarkSource::Highlight) => {
                ui.label(egui::RichText::new(t("No stand-out moments found.")).size(12.0).color(theme.on_surface_variant));
            }
            Ok(3) => {
                ui.label(egui::RichText::new(t("Couldn't analyze the sound.")).size(12.0).color(theme.error))
                    .on_hover_text(failure.unwrap_or_default());
            }
            _ => {}
        }
    });
    seek
}

/// Small preview frames along the clip, extracted in the background.
pub struct Filmstrip {
    frames: Arc<Mutex<Vec<Option<PathBuf>>>>,
}

impl Filmstrip {
    pub fn start(video: &Path, duration: f64) -> Self {
        let frames = Arc::new(Mutex::new(vec![None; FRAMES]));
        let dir = cache_dir(video);
        let slots = Arc::clone(&frames);
        let video = video.to_path_buf();
        std::thread::spawn(move || {
            let _ = std::fs::create_dir_all(&dir);
            for i in 0..FRAMES {
                let out = dir.join(format!("{:02}.jpg", i));
                if !out.exists() {
                    let t = (i as f64 + 0.5) * duration / FRAMES as f64;
                    let _ = Command::new("ffmpeg")
                        .args(["-nostdin", "-loglevel", "error", "-ss", &format!("{:.2}", t), "-i"])
                        .arg(&video)
                        .args(["-frames:v", "1", "-vf", "scale=-2:120", "-y"])
                        .arg(&out)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
                if out.exists() {
                    if let Ok(mut f) = slots.lock() {
                        f[i] = Some(out);
                    }
                }
            }
        });
        Self { frames }
    }

    pub fn frames(&self) -> Vec<Option<PathBuf>> {
        self.frames.lock().map(|f| f.clone()).unwrap_or_default()
    }
}

/// ~/.cache/lapse/filmstrip/<hash of path, size and mtime>
fn cache_dir(video: &Path) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    video.hash(&mut h);
    if let Ok(m) = std::fs::metadata(video) {
        m.len().hash(&mut h);
        m.modified().ok().hash(&mut h);
    }
    let base = directories::BaseDirs::new()
        .map(|d| d.cache_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir);
    // v2: 24 frames at 120 px (older caches had 12 at 72 px).
    base.join("lapse/filmstrip-v2").join(format!("{:016x}", h.finish()))
}

/// Draws the timeline and returns a seek target when the user scrubs or drags a handle.
pub fn render_timeline(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState) -> Option<Duration> {
    let mut seek = None;
    let width = ui.available_width();
    // Taller on wide windows, so the filmstrip doesn't turn into a thin ribbon.
    let strip_h = (width * 0.045).clamp(64.0, 104.0);
    let (outer, _) = ui.allocate_exact_size(Vec2::new(width, strip_h + 22.0), Sense::hover());
    let strip = Rect::from_min_size(outer.min + Vec2::new(10.0, 4.0), Vec2::new(width - 20.0, strip_h - 8.0));
    let resp = ui.interact(strip.expand2(Vec2::new(10.0, 6.0)), ui.id().with("trim_timeline"), Sense::click_and_drag());
    let painter = ui.painter();

    let dur = state.duration.as_secs_f32().max(0.1);
    let x_of = |secs: f32| strip.left() + (secs / dur).clamp(0.0, 1.0) * strip.width();
    let secs_of = |x: f32| ((x - strip.left()) / strip.width()).clamp(0.0, 1.0) * dur;

    // Filmstrip, or a plain track while frames are still being extracted.
    let rounding = Rounding::same(8.0);
    painter.rect_filled(strip, rounding, theme.surface_container_highest);
    // As many tiles as fit at the video's own aspect, each showing the frame nearest
    // to its place in the clip, so wide windows get more tiles instead of stretched ones.
    let frames = state.filmstrip.frames();
    let aspect = (state.orig_width as f32 / state.orig_height.max(1) as f32).clamp(0.5, 3.0);
    let tiles = ((strip.width() / (strip.height() * aspect)).ceil() as usize).max(1);
    let tile_w = strip.width() / tiles as f32;
    for i in 0..tiles {
        let index = ((i as f32 + 0.5) / tiles as f32 * FRAMES as f32) as usize;
        if let Some(Some(path)) = frames.get(index.min(FRAMES - 1)) {
            let tile = Rect::from_min_size(strip.min + Vec2::new(i as f32 * tile_w, 0.0), Vec2::new(tile_w + 0.5, strip.height()));
            // Crop to the tile's aspect so it fills without stretching.
            let frac = (tile.width() / tile.height() / aspect).min(1.0);
            let uv = Rect::from_min_max(Pos2::new(0.5 - frac / 2.0, 0.0), Pos2::new(0.5 + frac / 2.0, 1.0));
            egui::Image::new(format!("file://{}", path.display())).uv(uv).paint_at(ui, tile);
        }
    }

    let x_in = x_of(state.trim_start.as_secs_f32());
    let x_out = x_of(state.trim_end.as_secs_f32());

    // Dim everything outside the selection.
    let dim = Color32::from_black_alpha(150);
    if x_in > strip.left() {
        painter.rect_filled(Rect::from_min_max(strip.min, Pos2::new(x_in, strip.bottom())), Rounding { nw: 8.0, sw: 8.0, ne: 0.0, se: 0.0 }, dim);
    }
    if x_out < strip.right() {
        painter.rect_filled(Rect::from_min_max(Pos2::new(x_out, strip.top()), strip.max), Rounding { nw: 0.0, sw: 0.0, ne: 8.0, se: 8.0 }, dim);
    }

    // Decide what a press grabs: the nearest handle within reach, otherwise the playhead.
    let pointer = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
    let near = |x: f32| pointer.map(|p| (p.x - x).abs() < 12.0 && p.y > strip.top() - 8.0 && p.y < strip.bottom() + 8.0).unwrap_or(false);
    if resp.drag_started() || (resp.clicked() && state.active_drag.is_none()) {
        state.active_drag = Some(if near(x_in) && (!near(x_out) || pointer.map(|p| p.x < (x_in + x_out) / 2.0).unwrap_or(true)) {
            TimelineDragTarget::MarkIn
        } else if near(x_out) {
            TimelineDragTarget::MarkOut
        } else {
            TimelineDragTarget::Playhead
        });
    }
    if let (Some(target), Some(p)) = (state.active_drag, pointer) {
        if resp.dragged() || resp.clicked() || resp.drag_started() {
            let t = secs_of(p.x);
            let min_len = 0.1;
            match target {
                TimelineDragTarget::MarkIn => {
                    let t = t.min(state.trim_end.as_secs_f32() - min_len).max(0.0);
                    state.trim_start = Duration::from_secs_f32(t);
                    seek = Some(state.trim_start);
                }
                TimelineDragTarget::MarkOut => {
                    let t = t.max(state.trim_start.as_secs_f32() + min_len).min(dur);
                    state.trim_end = Duration::from_secs_f32(t);
                    seek = Some(state.trim_end);
                }
                TimelineDragTarget::Playhead => {
                    // A click right on a marker jumps exactly to it.
                    let snap = all_marks(state).into_iter().map(|(m, _)| m as f32).find(|m| (x_of(*m) - p.x).abs() < 6.0);
                    seek = Some(Duration::from_secs_f32(snap.unwrap_or(t)));
                }
            }
        }
    }
    if !resp.dragged() && !resp.drag_started() {
        state.active_drag = None;
    }

    // Selection frame.
    let sel = Rect::from_min_max(Pos2::new(x_in, strip.top()), Pos2::new(x_out, strip.bottom()));
    painter.rect_stroke(sel, Rounding::same(6.0), Stroke::new(2.5_f32, theme.primary));

    // Handles grow a bit when hovered or dragged.
    for (x, target) in [(x_in, TimelineDragTarget::MarkIn), (x_out, TimelineDragTarget::MarkOut)] {
        let active = state.active_drag == Some(target) || near(x);
        let t = ease_out(ui.ctx().animate_bool_with_time(ui.id().with(("handle", target as u8)), active, FAST));
        let w = 8.0 + 4.0 * t;
        let handle = Rect::from_center_size(Pos2::new(x, strip.center().y), Vec2::new(w, strip.height() + 10.0 + 4.0 * t));
        painter.rect_filled(handle, Rounding::same(4.0), lerp_color(theme.primary, theme.on_primary_container, t * 0.3));
        for dy in [-5.0, 0.0, 5.0] {
            painter.line_segment(
                [Pos2::new(x - 2.0, strip.center().y + dy), Pos2::new(x + 2.0, strip.center().y + dy)],
                Stroke::new(1.2_f32, theme.on_primary),
            );
        }
        if active {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
    }

    // Markers: your own (hotkey) as solid pins, found highlights as hollow ones,
    // kills as a crosshair badge at the top of the strip.
    for (m, source) in all_marks(state) {
        let x = x_of(m as f32);
        let color = source.color(theme);
        painter.line_segment([Pos2::new(x, strip.top()), Pos2::new(x, strip.bottom())], Stroke::new(1.5_f32, color.gamma_multiply(0.8)));
        if source == MarkSource::Kill {
            let c = Pos2::new(x, strip.top() + 11.0);
            painter.circle_filled(c, 9.0, theme.error);
            draw_vector_icon(&painter, c, 14.0, IconKind::Crosshair, theme.on_error);
            continue;
        }
        let own = source == MarkSource::Marker;
        let c = Pos2::new(x, strip.bottom() + 5.0);
        let diamond = vec![c + Vec2::new(0.0, -4.5), c + Vec2::new(4.5, 0.0), c + Vec2::new(0.0, 4.5), c + Vec2::new(-4.5, 0.0)];
        if own {
            painter.add(egui::Shape::convex_polygon(diamond, color, Stroke::NONE));
        } else {
            painter.add(egui::Shape::closed_line(diamond, Stroke::new(1.5_f32, color)));
        }
    }

    // Playhead.
    let x_now = x_of(state.current_time.as_secs_f32());
    painter.line_segment([Pos2::new(x_now, strip.top() - 4.0), Pos2::new(x_now, strip.bottom() + 4.0)], Stroke::new(2.0_f32, Color32::WHITE));
    painter.add(egui::Shape::convex_polygon(
        vec![Pos2::new(x_now - 6.0, strip.top() - 8.0), Pos2::new(x_now + 6.0, strip.top() - 8.0), Pos2::new(x_now, strip.top() - 1.0)],
        Color32::WHITE,
        Stroke::NONE,
    ));

    // Hover guide with the time under the cursor.
    if let Some(p) = resp.hover_pos() {
        if state.active_drag.is_none() {
            painter.line_segment([Pos2::new(p.x, strip.top()), Pos2::new(p.x, strip.bottom())], Stroke::new(1.0_f32, Color32::from_white_alpha(120)));
        }
        let label = format_time(Duration::from_secs_f32(secs_of(p.x)));
        let galley = painter.layout_no_wrap(label, FontId::monospace(11.0), theme.on_surface);
        let bubble = Rect::from_center_size(Pos2::new(p.x.clamp(strip.left() + 30.0, strip.right() - 30.0), strip.top() - 14.0), galley.size() + Vec2::new(10.0, 4.0));
        painter.rect_filled(bubble, Rounding::same(4.0), theme.surface_container_highest);
        painter.galley(bubble.center() - galley.size() / 2.0, galley, theme.on_surface);
    }

    // Second ruler.
    let step = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0]
        .into_iter()
        .find(|s| strip.width() / (dur / s) >= 60.0)
        .unwrap_or(600.0);
    let mut t = 0.0;
    while t <= dur + 0.001 {
        let x = x_of(t);
        painter.line_segment([Pos2::new(x, strip.bottom() + 5.0), Pos2::new(x, strip.bottom() + 9.0)], Stroke::new(1.0_f32, theme.outline));
        painter.text(
            Pos2::new(x, strip.bottom() + 11.0),
            Align2::CENTER_TOP,
            format!("{}:{:02}", (t as u32) / 60, (t as u32) % 60),
            FontId::proportional(10.0),
            theme.outline,
        );
        t += step;
    }

    seek
}
