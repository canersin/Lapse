//! Fullscreen viewing: the video fills the window, controls fade in over the bottom
//! while the mouse moves (or playback is paused) and hide with the cursor otherwise.

use eframe::egui::{self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Vec2};
use std::time::{Duration, Instant};

use super::{format_time, handle_keys, round_button, timeline, PlayerState};
use crate::gui::components::icon::IconKind;
use crate::gui::components::motion::ease_out;
use crate::gui::components::slider::{m3_slider, SliderOpts};
use crate::gui::i18n::t;
use crate::gui::theme::MonetTheme;

/// Controls stay this long after the last mouse movement.
const IDLE_HIDE: Duration = Duration::from_millis(2500);
const BAR_H: f32 = 118.0;

pub fn render_fullscreen(player_state: &mut Option<PlayerState>, theme: &MonetTheme, ui: &mut egui::Ui, ctx: &egui::Context) {
    let Some(state) = player_state.as_mut() else { return };
    let mut seek: Option<Duration> = None;
    handle_keys(ui, state, &mut seek);
    state.tick(ctx);
    if state.is_playing && state.loop_cut_preview && state.current_time >= state.trim_end {
        seek = Some(state.trim_start);
    }

    let full = ui.max_rect();
    let (_, video_resp) = ui.allocate_exact_size(full.size(), Sense::click());
    let painter = ui.painter_at(full);
    painter.rect_filled(full, 0.0, Color32::BLACK);
    if state.has_video() {
        let aspect = state.orig_width as f32 / state.orig_height.max(1) as f32;
        let mut size = full.size();
        if size.x / aspect > size.y {
            size.x = size.y * aspect;
        } else {
            size.y = size.x / aspect;
        }
        let image = Rect::from_center_size(full.center(), size);
        state.paint_video(ui, image);
    } else {
        ui.put(Rect::from_center_size(full.center(), Vec2::splat(40.0)), egui::Spinner::new().size(40.0));
    }

    // Show the controls on mouse movement, keep them while paused or hovered.
    let moved = ui.input(|i| i.pointer.delta() != Vec2::ZERO || i.pointer.any_down());
    if moved {
        state.last_pointer_move = Instant::now();
    }
    let bar = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - BAR_H), full.max);
    let over_bar = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| bar.contains(p));
    let show = !state.is_playing || over_bar || state.last_pointer_move.elapsed() < IDLE_HIDE;
    let alpha = ease_out(ctx.animate_bool_with_time(egui::Id::new("lapse_fs_controls"), show, 0.18));
    if !show {
        ctx.set_cursor_icon(CursorIcon::None);
    } else {
        ctx.request_repaint_after(IDLE_HIDE);
    }

    if video_resp.clicked() && !over_bar {
        state.toggle_play();
    }
    if video_resp.double_clicked() && !over_bar {
        state.fullscreen = false;
    }

    if alpha > 0.01 {
        // Soft shade so white HUDs in the game stay readable under the controls.
        const BANDS: usize = 40;
        for i in 0..BANDS {
            let f = i as f32 / BANDS as f32;
            let band = Rect::from_min_max(Pos2::new(full.left(), bar.top() + BAR_H * f), Pos2::new(full.right(), bar.top() + BAR_H * (f + 1.0 / BANDS as f32)));
            painter.rect_filled(band, 0.0, Color32::from_black_alpha((190.0 * f * alpha) as u8));
        }
        if let Some(s) = seek_bar(ui, theme, state, bar, alpha) {
            seek = Some(s);
        }
        if alpha > 0.5 {
            controls(ui, theme, state, bar, &mut seek);
        }
    }

    if let Some(target) = seek {
        state.request_seek(target);
    }
}

/// Thin progress bar that thickens on hover; click or drag to seek. Marks and the
/// selection are shown on it.
fn seek_bar(ui: &mut egui::Ui, theme: &MonetTheme, state: &PlayerState, bar: Rect, alpha: f32) -> Option<Duration> {
    let track = Rect::from_min_max(Pos2::new(bar.left() + 24.0, bar.top() + 40.0), Pos2::new(bar.right() - 24.0, bar.top() + 56.0));
    let resp = ui.interact(track, egui::Id::new("lapse_fs_seek"), Sense::click_and_drag());
    let hot = ease_out(ui.ctx().animate_bool_with_time(resp.id.with("hot"), resp.hovered() || resp.dragged(), 0.12));
    let dur = state.duration.as_secs_f32().max(0.1);
    let x_of = |s: f32| track.left() + (s / dur).clamp(0.0, 1.0) * track.width();
    let h = 3.0 + 3.0 * hot;
    let line = Rect::from_center_size(track.center(), Vec2::new(track.width(), h));
    let p = ui.painter();
    let a = |c: Color32| c.gamma_multiply(alpha);

    p.rect_filled(line, h / 2.0, a(Color32::from_white_alpha(70)));
    let sel = Rect::from_min_max(Pos2::new(x_of(state.trim_start.as_secs_f32()), line.top()), Pos2::new(x_of(state.trim_end.as_secs_f32()), line.bottom()));
    if state.trim_start > Duration::ZERO || state.trim_end < state.duration {
        p.rect_filled(sel, h / 2.0, a(theme.primary.gamma_multiply(0.35)));
    }
    let now_x = x_of(state.current_time.as_secs_f32());
    p.rect_filled(Rect::from_min_max(line.min, Pos2::new(now_x, line.bottom())), h / 2.0, a(theme.primary));
    for (m, source) in timeline::all_marks(state) {
        let r = if source == timeline::MarkSource::Kill { 3.5 } else { 2.5 };
        p.circle_filled(Pos2::new(x_of(m as f32), line.center().y), r + hot, a(source.color(theme)));
    }
    p.circle_filled(Pos2::new(now_x, line.center().y), 4.0 + 3.0 * hot, a(theme.primary));

    // Time under the cursor.
    if let Some(pos) = resp.hover_pos() {
        let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) * dur;
        let label = format_time(Duration::from_secs_f32(t));
        let galley = p.layout_no_wrap(label, FontId::monospace(12.0), Color32::WHITE);
        let bubble = Rect::from_center_size(Pos2::new(pos.x.clamp(track.left() + 30.0, track.right() - 30.0), track.top() - 14.0), galley.size() + Vec2::new(12.0, 6.0));
        p.rect_filled(bubble, 6.0, a(Color32::from_black_alpha(200)));
        p.galley(bubble.center() - galley.size() / 2.0, galley, Color32::WHITE);
    }

    if resp.clicked() || resp.dragged() {
        let pos = resp.interact_pointer_pos()?;
        let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) * dur;
        return Some(Duration::from_secs_f32(t));
    }
    None
}

fn controls(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState, bar: Rect, seek: &mut Option<Duration>) {
    let row = Rect::from_min_max(Pos2::new(bar.left() + 20.0, bar.top() + 62.0), Pos2::new(bar.right() - 20.0, bar.bottom() - 8.0));
    ui.allocate_ui_at_rect(row, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let icon = if state.is_playing { IconKind::Pause } else { IconKind::Play };
            if round_button(ui, theme, icon, 42.0, true).on_hover_text(t("Play / pause (Space)")).clicked() {
                state.toggle_play();
            }
            if round_button(ui, theme, IconKind::Rewind, 34.0, false).on_hover_text(t("Back 10 s (J)")).clicked() {
                *seek = Some(state.current_time.saturating_sub(Duration::from_secs(10)));
            }
            if round_button(ui, theme, IconKind::FastForward, 34.0, false).on_hover_text(t("Forward 10 s (L)")).clicked() {
                *seek = Some((state.current_time + Duration::from_secs(10)).min(state.duration));
            }
            let mute_icon = if state.muted || state.volume == 0.0 { IconKind::Muted } else { IconKind::Audio };
            if round_button(ui, theme, mute_icon, 34.0, false).on_hover_text(t("Mute (M)")).clicked() {
                state.muted = !state.muted;
                state.set_audio_volume(state.volume);
            }
            let mut vol = if state.muted { 0.0 } else { state.volume as f64 };
            if m3_slider(ui, theme, &mut vol, 0.0..=1.0, 110.0, SliderOpts::default()).changed() {
                state.volume = vol as f32;
                state.muted = vol == 0.0;
                state.set_audio_volume(state.volume);
            }
            ui.add_space(8.0);
            let time = format!("{}  /  {}", format_time(state.current_time), format_time(state.duration));
            let (r, _) = ui.allocate_exact_size(Vec2::new(160.0, 34.0), Sense::hover());
            ui.painter().text(r.left_center(), Align2::LEFT_CENTER, time, FontId::monospace(13.0), Color32::WHITE);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if round_button(ui, theme, IconKind::ExitFullscreen, 34.0, false).on_hover_text(t("Exit fullscreen (F / Esc)")).clicked() {
                    state.fullscreen = false;
                }
                let name = state.video_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                ui.label(egui::RichText::new(name).size(12.5).color(Color32::from_white_alpha(200)));
            });
        });
    });
}
