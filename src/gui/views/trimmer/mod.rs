//! Clip trimmer: preview, timeline, selection and export.
//!
//! - `engine`: ffmpeg-backed playback (`PlayerState`)
//! - `timeline`: filmstrip with In/Out handles
//! - `export_panel`: export presets and progress
//! - `fullscreen`: full-window viewing with auto-hiding controls

pub mod engine;
pub mod export_panel;
mod fullscreen;
pub mod mpv;
pub mod overlays;
pub mod timeline;

pub use fullscreen::render_fullscreen;

pub use engine::{format_file_size, PlayerState};

use eframe::egui::{self, Color32, Key, Margin, Pos2, Rect, Response, RichText, Sense, Stroke, Vec2};
use std::time::Duration;

use crate::export::{CropAspect, CropHandle, CropRect};
use crate::gui::components::button::{m3_field_style, m3_small_button};
use crate::gui::components::dropdown::m3_dropdown;
use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::components::motion::{centered_label, ease_out, hover_t, lerp_color, FAST};
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ActiveCropDrag {
    None,
    Inside,
    Handle(CropHandle),
}

pub fn format_time(d: Duration) -> String {
    let secs = d.as_secs();
    format!("{:02}:{:02}.{}", secs / 60, secs % 60, d.subsec_millis() / 100)
}

pub fn render_player(player_state: &mut Option<PlayerState>, theme: &MonetTheme, save_path: &std::path::Path, ui: &mut egui::Ui, ctx: &egui::Context) {
    let mut close = false;
    let mut seek: Option<Duration> = None;
    let mut open_other = None;

    let Some(state) = player_state.as_mut() else { return };
    handle_keys(ui, state, &mut seek);
    state.tick(ctx);
    if state.is_playing && state.loop_cut_preview && state.current_time >= state.trim_end {
        seek = Some(state.trim_start);
    }

    egui::ScrollArea::vertical()
        .id_source("trimmer_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            close = header(ui, theme, state, save_path);
            ui.add_space(10.0);

            // Video keeps 16:9 but always leaves room for the controls below it.
            let width = ui.available_width();
            let height = (ui.available_height() - 470.0).clamp(180.0, width * 9.0 / 16.0);
            video(ui, theme, state, Vec2::new(width, height));
            ui.add_space(8.0);

            if let Some(s) = transport(ui, theme, state) {
                seek = Some(s);
            }
            ui.add_space(4.0);
            if let Some(s) = timeline::render_timeline(ui, theme, state) {
                seek = Some(s);
            }
            if let Some(s) = timeline::render_marker_bar(ui, theme, state) {
                seek = Some(s);
            }
            ui.add_space(10.0);

            if let Some(s) = selection(ui, theme, state) {
                seek = Some(s);
            }
            ui.add_space(10.0);
            card(ui, theme, |ui| {
                open_other = export_panel::render_export_panel(ui, theme, state);
            });
            ui.add_space(16.0);
        });

    if let Some(target) = seek {
        state.request_seek(target);
    }
    if close {
        if let Some(mut s) = player_state.take() {
            s.stop();
        }
    } else if let Some(path) = open_other {
        if let Some(mut s) = player_state.take() {
            s.stop();
        }
        *player_state = PlayerState::new(path);
    }
}

/// YouTube-style keys: Space/K play, J/L 10 s, arrows 5 s and volume, F fullscreen,
/// M mute, 0-9 jump to 0-90 %, Home/End, , . frame step, I/O selection.
pub(super) fn handle_keys(ui: &egui::Ui, state: &mut PlayerState, seek: &mut Option<Duration>) {
    // Don't steal keys while the user types a file name or edits text overlay.
    if ui.ctx().wants_keyboard_input() || overlays::is_editing_text_overlay(ui.ctx()) {
        return;
    }
    let pressed = |k: Key| ui.input(|i| i.key_pressed(k));
    let frame = state.frame_duration();
    let back = |s: &PlayerState, secs: u64| s.current_time.saturating_sub(Duration::from_secs(secs));
    let fwd = |s: &PlayerState, secs: u64| (s.current_time + Duration::from_secs(secs)).min(s.duration);
    if pressed(Key::Space) || pressed(Key::K) {
        state.toggle_play();
    }
    if pressed(Key::ArrowLeft) {
        *seek = Some(back(state, 5));
    }
    if pressed(Key::ArrowRight) {
        *seek = Some(fwd(state, 5));
    }
    if pressed(Key::J) {
        *seek = Some(back(state, 10));
    }
    if pressed(Key::L) {
        *seek = Some(fwd(state, 10));
    }
    if pressed(Key::Home) {
        *seek = Some(Duration::ZERO);
    }
    if pressed(Key::End) {
        *seek = Some(state.duration.saturating_sub(frame));
    }
    let digits = [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
    if let Some(n) = digits.iter().position(|k| pressed(*k)) {
        *seek = Some(state.duration.mul_f32(n as f32 / 10.0));
    }
    if pressed(Key::Comma) {
        state.pause();
        *seek = Some(state.current_time.saturating_sub(frame));
    }
    if pressed(Key::Period) {
        state.pause();
        *seek = Some((state.current_time + frame).min(state.duration));
    }
    if pressed(Key::I) || pressed(Key::OpenBracket) {
        state.trim_start = state.current_time.min(state.trim_end.saturating_sub(Duration::from_millis(100)));
    }
    if pressed(Key::O) || pressed(Key::CloseBracket) {
        state.trim_end = state.current_time.max(state.trim_start + Duration::from_millis(100)).min(state.duration);
    }
    if pressed(Key::M) {
        state.muted = !state.muted;
        state.set_audio_volume(state.volume);
    }
    if pressed(Key::ArrowUp) || pressed(Key::ArrowDown) {
        let delta = if pressed(Key::ArrowUp) { 0.05 } else { -0.05 };
        state.volume = (state.volume + delta).clamp(0.0, 1.0);
        state.muted = false;
        state.set_audio_volume(state.volume);
    }
    if pressed(Key::F) {
        state.fullscreen = !state.fullscreen;
    }
    if pressed(Key::Escape) && state.fullscreen {
        state.fullscreen = false;
    }
    if pressed(Key::Delete) || pressed(Key::Backspace) {
        if let Some(idx) = state.export.selected {
            if idx < state.export.overlays.len() {
                state.export.overlays.remove(idx);
                state.export.selected = None;
            }
        }
    }
}

fn card(ui: &mut egui::Ui, theme: &MonetTheme, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(theme.surface_container)
        .stroke(Stroke::new(1.0_f32, theme.outline_variant))
        .rounding(M3Shapes::LARGE)
        .inner_margin(Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
}

fn pill(ui: &mut egui::Ui, theme: &MonetTheme, text: &str) {
    egui::Frame::none()
        .fill(theme.surface_container_high)
        .rounding(M3Shapes::FULL)
        .inner_margin(Margin::symmetric(9.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).color(theme.on_surface_variant));
        });
}


/// Returns true when the user wants to go back.
fn header(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState, save_path: &std::path::Path) -> bool {
    let mut back = false;
    ui.horizontal(|ui| {
        if round_button(ui, theme, IconKind::ArrowLeft, 34.0, false).on_hover_text(t("Back")).clicked() {
            back = true;
        }
        ui.add_space(4.0);
        let right_w = 280.0;
        ui.allocate_ui(Vec2::new((ui.available_width() - right_w).max(120.0), 34.0), |ui| {
            ui.vertical(|ui| {
                // The open file is renamed in place; later reads (seeks, export) use the new path.
                if let Some(new_path) = crate::gui::views::clip_card::editable_name(ui, theme, &state.video_path, save_path, 15.0) {
                    state.video_path = new_path;
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    pill(ui, theme, &format!("{}×{}", state.orig_width, state.orig_height));
                    pill(ui, theme, &format!("{:.0} fps", state.fps));
                    pill(ui, theme, &crate::clip_index::format_duration(state.duration.as_secs_f64()));
                    pill(ui, theme, &format_file_size(state.file_size_bytes));
                    if state.player.is_none() && state.mpv.is_none() {
                        pill(ui, theme, t("No audio output"));
                    }
                });
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(12.0);
            if m3_small_button(ui, theme, t("Show in folder")).clicked() {
                let _ = open::that(state.video_path.parent().unwrap_or(&state.video_path));
            }
        });
    });
    back
}

fn video(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState, size: Vec2) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, M3Shapes::LARGE, Color32::BLACK);

    let mut crop_interacted = false;

    if mpv::has_pending_gl_cleanup() {
        ui.painter().add(egui::PaintCallback {
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::ZERO),
            callback: std::sync::Arc::new(eframe::egui_glow::CallbackFn::new(|_info, painter| {
                mpv::drain_gl_cleanup(painter.gl());
            })),
        });
    }

    if state.has_video() {
        let aspect = state.orig_width as f32 / state.orig_height.max(1) as f32;
        let mut target = rect.size();
        if target.x / aspect > target.y {
            target.x = target.y * aspect;
        } else {
            target.y = target.x / aspect;
        }
        let image_rect = Rect::from_center_size(rect.center(), target);
        state.paint_video(ui, image_rect);

        let output_rect = if let Some(crop) = state.export.crop.as_ref() {
            Rect::from_min_max(
                Pos2::new(image_rect.left() + crop.x * image_rect.width(), image_rect.top() + crop.y * image_rect.height()),
                Pos2::new(image_rect.left() + (crop.x + crop.w) * image_rect.width(), image_rect.top() + (crop.y + crop.h) * image_rect.height()),
            )
        } else {
            image_rect
        };

        if overlays::render_preview_overlays(ui, painter, theme, state, output_rect) {
            crop_interacted = true;
        }

        if let Some(crop) = state.export.crop.as_mut() {
            let box_rect = Rect::from_min_max(
                Pos2::new(image_rect.left() + crop.x * image_rect.width(), image_rect.top() + crop.y * image_rect.height()),
                Pos2::new(image_rect.left() + (crop.x + crop.w) * image_rect.width(), image_rect.top() + (crop.y + crop.h) * image_rect.height()),
            );

            let handle_positions = [
                (CropHandle::TopLeft, box_rect.left_top()),
                (CropHandle::Top, Pos2::new(box_rect.center().x, box_rect.top())),
                (CropHandle::TopRight, box_rect.right_top()),
                (CropHandle::Right, Pos2::new(box_rect.right(), box_rect.center().y)),
                (CropHandle::BottomRight, box_rect.right_bottom()),
                (CropHandle::Bottom, Pos2::new(box_rect.center().x, box_rect.bottom())),
                (CropHandle::BottomLeft, box_rect.left_bottom()),
                (CropHandle::Left, Pos2::new(box_rect.left(), box_rect.center().y)),
            ];

            let drag_id = ui.id().with("crop_drag_active");
            let mut active: ActiveCropDrag = ui.ctx().data_mut(|d| d.get_temp(drag_id)).unwrap_or(ActiveCropDrag::None);

            let pointer_pos = ui.input(|i| i.pointer.hover_pos());
            let is_down = ui.input(|i| i.pointer.primary_down());
            let is_pressed = ui.input(|i| i.pointer.primary_pressed());
            let delta = ui.input(|i| i.pointer.delta());

            if !is_down {
                active = ActiveCropDrag::None;
                ui.ctx().data_mut(|d| d.insert_temp(drag_id, ActiveCropDrag::None));
            } else if is_pressed && active == ActiveCropDrag::None && !crop_interacted {
                if let Some(pos) = pointer_pos {
                    if let Some((handle, _)) = handle_positions.iter().find(|(_, hp)| pos.distance(*hp) <= 12.0) {
                        active = ActiveCropDrag::Handle(*handle);
                        ui.ctx().data_mut(|d| d.insert_temp(drag_id, active));
                    } else if box_rect.contains(pos) {
                        active = ActiveCropDrag::Inside;
                        ui.ctx().data_mut(|d| d.insert_temp(drag_id, active));
                    }
                }
            }

            if active != ActiveCropDrag::None {
                crop_interacted = true;
                match active {
                    ActiveCropDrag::Handle(h) => ui.ctx().set_cursor_icon(h.cursor()),
                    ActiveCropDrag::Inside => ui.ctx().set_cursor_icon(egui::CursorIcon::Move),
                    ActiveCropDrag::None => {}
                }

                if delta != Vec2::ZERO && image_rect.width() > 0.0 && image_rect.height() > 0.0 {
                    let dx = delta.x / image_rect.width();
                    let dy = delta.y / image_rect.height();
                    match active {
                        ActiveCropDrag::Inside => {
                            crop.move_by(dx, dy);
                        }
                        ActiveCropDrag::Handle(h) => {
                            *crop = crop.resize(
                                h,
                                dx,
                                dy,
                                state.export.crop_aspect,
                                state.orig_width,
                                state.orig_height,
                            );
                        }
                        ActiveCropDrag::None => {}
                    }
                }
            } else if let Some(pos) = pointer_pos {
                if !crop_interacted {
                    if let Some((handle, _)) = handle_positions.iter().find(|(_, hp)| pos.distance(*hp) <= 12.0) {
                        ui.ctx().set_cursor_icon(handle.cursor());
                        crop_interacted = true;
                    } else if box_rect.contains(pos) {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                        crop_interacted = true;
                    }
                }
            }

            // Recompute box_rect after potential move/resize for rendering
            let box_rect = Rect::from_min_max(
                Pos2::new(image_rect.left() + crop.x * image_rect.width(), image_rect.top() + crop.y * image_rect.height()),
                Pos2::new(image_rect.left() + (crop.x + crop.w) * image_rect.width(), image_rect.top() + (crop.y + crop.h) * image_rect.height()),
            );

            // Darken outside the box
            let shade = Color32::from_black_alpha(170);
            if box_rect.top() > image_rect.top() {
                painter.rect_filled(Rect::from_min_max(image_rect.min, Pos2::new(image_rect.right(), box_rect.top())), 0.0, shade);
            }
            if box_rect.bottom() < image_rect.bottom() {
                painter.rect_filled(Rect::from_min_max(Pos2::new(image_rect.left(), box_rect.bottom()), image_rect.max), 0.0, shade);
            }
            if box_rect.left() > image_rect.left() {
                painter.rect_filled(Rect::from_min_max(Pos2::new(image_rect.left(), box_rect.top()), Pos2::new(box_rect.left(), box_rect.bottom())), 0.0, shade);
            }
            if box_rect.right() < image_rect.right() {
                painter.rect_filled(Rect::from_min_max(Pos2::new(box_rect.right(), box_rect.top()), Pos2::new(image_rect.right(), box_rect.bottom())), 0.0, shade);
            }

            // Draw rule-of-thirds grid while dragging
            if active != ActiveCropDrag::None {
                let grid_stroke = Stroke::new(1.0_f32, Color32::from_white_alpha(90));
                let x1 = box_rect.left() + box_rect.width() / 3.0;
                let x2 = box_rect.left() + 2.0 * box_rect.width() / 3.0;
                let y1 = box_rect.top() + box_rect.height() / 3.0;
                let y2 = box_rect.top() + 2.0 * box_rect.height() / 3.0;
                painter.line_segment([Pos2::new(x1, box_rect.top()), Pos2::new(x1, box_rect.bottom())], grid_stroke);
                painter.line_segment([Pos2::new(x2, box_rect.top()), Pos2::new(x2, box_rect.bottom())], grid_stroke);
                painter.line_segment([Pos2::new(box_rect.left(), y1), Pos2::new(box_rect.right(), y1)], grid_stroke);
                painter.line_segment([Pos2::new(box_rect.left(), y2), Pos2::new(box_rect.right(), y2)], grid_stroke);
            }

            // Box border
            painter.rect_stroke(box_rect, 0.0, Stroke::new(2.0_f32, theme.primary));

            // 8 handles (4 corners + 4 edges)
            let updated_handles = [
                box_rect.left_top(),
                Pos2::new(box_rect.center().x, box_rect.top()),
                box_rect.right_top(),
                Pos2::new(box_rect.right(), box_rect.center().y),
                box_rect.right_bottom(),
                Pos2::new(box_rect.center().x, box_rect.bottom()),
                box_rect.left_bottom(),
                Pos2::new(box_rect.left(), box_rect.center().y),
            ];
            for hp in updated_handles {
                let hr = Rect::from_center_size(hp, Vec2::splat(8.0));
                painter.rect_filled(hr, 2.0, Color32::WHITE);
                painter.rect_stroke(hr, 2.0, Stroke::new(1.0_f32, theme.primary));
            }
        }
    } else {
        ui.put(Rect::from_center_size(rect.center(), Vec2::splat(36.0)), egui::Spinner::new().size(36.0));
    }

    // Play overlay fades in when paused and grows a little on hover.
    let paused = ease_out(ui.ctx().animate_bool_with_time(resp.id.with("paused"), !state.is_playing, FAST * 2.0));
    // Hidden while an overlay is selected: new overlays start at the centre, right under it.
    if paused > 0.01 && !crop_interacted && state.export.selected.is_none() {
        let h = hover_t(ui, &resp);
        let r = 34.0 + 4.0 * h;
        let painter = ui.painter();
        painter.circle_filled(rect.center(), r, Color32::from_black_alpha((160.0 * paused) as u8));
        painter.circle_stroke(rect.center(), r, Stroke::new(2.0_f32, theme.primary.gamma_multiply(paused)));
        draw_vector_icon(painter, rect.center() + Vec2::new(2.0, 0.0), 26.0, IconKind::Play, Color32::WHITE.gamma_multiply(paused));
    }
    if resp.clicked() && !crop_interacted {
        state.export.selected = None;
        state.toggle_play();
    }
    if resp.double_clicked() && !crop_interacted {
        state.fullscreen = !state.fullscreen;
    }
}

/// Circular icon button with hover/press animation.
pub(super) fn round_button(ui: &mut egui::Ui, theme: &MonetTheme, icon: IconKind, size: f32, filled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let h = hover_t(ui, &resp);
    let press = ui.ctx().animate_bool_with_time(resp.id.with("press"), resp.is_pointer_button_down_on(), 0.06);
    let r = size / 2.0 - press * 1.5;
    let (bg, fg) = if filled {
        (lerp_color(theme.primary, theme.on_primary_container, h * 0.15), theme.on_primary)
    } else {
        (lerp_color(theme.surface_container_high, theme.surface_container_highest, h), lerp_color(theme.on_surface_variant, theme.on_surface, h))
    };
    ui.painter().circle_filled(rect.center(), r, bg);
    draw_vector_icon(ui.painter(), rect.center(), size * 0.42, icon, fg);
    resp
}

fn transport(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState) -> Option<Duration> {
    let mut seek = None;
    let frame = state.frame_duration();
    ui.horizontal(|ui| {
        // Everything on this row is centered on the big play button.
        ui.set_min_height(48.0);
        ui.label(
            RichText::new(format!("{}  /  {}", format_time(state.current_time), format_time(state.duration)))
                .monospace()
                .size(13.0)
                .color(theme.on_surface),
        );

        // Center the main controls.
        let controls_w = 5.0 * 36.0 + 48.0 + 6.0 * 6.0;
        let pad = ((ui.available_width() - controls_w) / 2.0 - 150.0).max(8.0);
        ui.add_space(pad);
        ui.spacing_mut().item_spacing.x = 6.0;

        if round_button(ui, theme, IconKind::Rewind, 36.0, false).on_hover_text(t("Back 5 s (Left, J: 10 s)")).clicked() {
            seek = Some(state.current_time.saturating_sub(Duration::from_secs(5)));
        }
        if round_button(ui, theme, IconKind::StepBack, 36.0, false).on_hover_text(t("Previous frame (,)")).clicked() {
            state.pause();
            seek = Some(state.current_time.saturating_sub(frame));
        }
        let play_icon = if state.is_playing { IconKind::Pause } else { IconKind::Play };
        if round_button(ui, theme, play_icon, 48.0, true).on_hover_text(t("Play / pause (Space)")).clicked() {
            state.toggle_play();
        }
        if round_button(ui, theme, IconKind::StepForward, 36.0, false).on_hover_text(t("Next frame (.)")).clicked() {
            state.pause();
            seek = Some((state.current_time + frame).min(state.duration));
        }
        if round_button(ui, theme, IconKind::FastForward, 36.0, false).on_hover_text(t("Forward 5 s (Right, L: 10 s)")).clicked() {
            seek = Some((state.current_time + Duration::from_secs(5)).min(state.duration));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if round_button(ui, theme, IconKind::Fullscreen, 30.0, false).on_hover_text(t("Fullscreen (F)")).clicked() {
                state.fullscreen = true;
            }
            ui.add_space(6.0);
            let mut vol = if state.muted { 0.0 } else { state.volume as f64 };
            let opts = crate::gui::components::slider::SliderOpts::default();
            if crate::gui::components::slider::m3_slider(ui, theme, &mut vol, 0.0..=1.0, 90.0, opts).changed() {
                state.volume = vol as _;
                state.muted = vol == 0.0;
                state.set_audio_volume(state.volume);
            }
            let icon = if state.muted || state.volume == 0.0 { IconKind::Muted } else { IconKind::Audio };
            if round_button(ui, theme, icon, 30.0, false).on_hover_text(t("Mute (M)")).clicked() {
                state.muted = !state.muted;
                state.set_audio_volume(state.volume);
            }
            ui.add_space(6.0);
            let loop_btn = round_button(ui, theme, IconKind::Loop, 30.0, state.loop_cut_preview);
            if loop_btn.on_hover_text(t("Loop the selection")).clicked() {
                state.loop_cut_preview = !state.loop_cut_preview;
                if state.loop_cut_preview {
                    seek = Some(state.trim_start);
                    state.play();
                }
            }
        });
    });
    seek
}

pub fn time_field_secs(ui: &mut egui::Ui, secs: &mut f64, min: f64, max: f64) -> bool {
    let mut val = *secs as f32;
    let changed = ui
        .add(
            egui::DragValue::new(&mut val)
                .speed(0.05)
                .clamp_range(min as f32..=max as f32)
                .custom_formatter(|v, _| format_time(Duration::from_secs_f64(v.max(0.0))))
                .custom_parser(|s| {
                    // Accept "12.5" or "1:02.5".
                    match s.split_once(':') {
                        Some((m, rest)) => Some(m.trim().parse::<f64>().ok()? * 60.0 + rest.trim().parse::<f64>().ok()?),
                        None => s.trim().parse().ok(),
                    }
                }),
        )
        .changed();
    if changed {
        *secs = val as f64;
    }
    changed
}

fn time_field(ui: &mut egui::Ui, value: &mut Duration, min: f32, max: f32) -> bool {
    let mut secs = value.as_secs_f64();
    let changed = time_field_secs(ui, &mut secs, min as f64, max as f64);
    if changed {
        *value = Duration::from_secs_f64(secs);
    }
    changed
}

/// One wrapped row: selection length, start/end fields, quick picks and loop.
fn selection(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState) -> Option<Duration> {
    let mut seek = None;
    let len = state.trim_end.saturating_sub(state.trim_start);
    let dur = state.duration.as_secs_f32();

    egui::Frame::none()
        .fill(theme.surface_container_low)
        .rounding(M3Shapes::MEDIUM)
        .inner_margin(Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                // Every item on this row is ROW tall so icons, labels, fields and buttons line up.
                const ROW: f32 = 26.0;
                m3_field_style(ui, theme);
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                ui.spacing_mut().interact_size.y = ROW;
                let (r, _) = ui.allocate_exact_size(Vec2::new(18.0, ROW), Sense::hover());
                draw_vector_icon(ui.painter(), r.center(), 17.0, IconKind::Scissors, theme.primary);
                centered_label(ui, RichText::new(t("Selection")).size(13.5).strong().color(theme.on_surface), ROW)
                    .on_hover_text(t("Tip: drag the purple handles on the timeline. Keys: Space play, , . frame step, I / O set start and end."));
                centered_label(ui, RichText::new(format!("{:.1} s", len.as_secs_f32())).size(13.5).strong().color(theme.primary), ROW);
                ui.add_space(10.0);

                centered_label(ui, RichText::new(t("Start")).size(12.5).color(theme.on_surface_variant), ROW);
                let max_in = (state.trim_end.as_secs_f32() - 0.1).max(0.0);
                if time_field(ui, &mut state.trim_start, 0.0, max_in) {
                    seek = Some(state.trim_start);
                }
                if m3_small_button(ui, theme, "I").on_hover_text(t("Set here (I)")).clicked() {
                    state.trim_start = state.current_time.min(state.trim_end.saturating_sub(Duration::from_millis(100)));
                }
                ui.add_space(6.0);
                centered_label(ui, RichText::new(t("End")).size(12.5).color(theme.on_surface_variant), ROW);
                let min_out = (state.trim_start.as_secs_f32() + 0.1).min(dur);
                if time_field(ui, &mut state.trim_end, min_out, dur) {
                    seek = Some(state.trim_end);
                }
                if m3_small_button(ui, theme, "O").on_hover_text(t("Set here (O)")).clicked() {
                    state.trim_end = state.current_time.max(state.trim_start + Duration::from_millis(100)).min(state.duration);
                }
                ui.add_space(10.0);

                // Quick picks: keep the last N seconds, the usual moment in a replay.
                for secs in [10u64, 15, 30] {
                    if m3_small_button(ui, theme, &format!("{} {} s", t("Last"), secs)).clicked() {
                        state.trim_end = state.duration;
                        state.trim_start = state.duration.saturating_sub(Duration::from_secs(secs));
                        seek = Some(state.trim_start);
                    }
                }
                if m3_small_button(ui, theme, t("Reset")).clicked() {
                    state.trim_start = Duration::ZERO;
                    state.trim_end = state.duration;
                }
            });
        });
    seek
}
