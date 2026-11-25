//! "Delete this clip?" dialog (one clip or a selection). Views call `request_delete`; the app calls
//! `render_delete_dialog` once per frame. Clips go to the trash when possible.

use eframe::egui::{self, Color32, Id, Key, Margin, RichText, Stroke, Vec2};
use std::path::{Path, PathBuf};

use crate::gui::components::button::{m3_danger_button, m3_outlined_button};
use crate::gui::components::icon::{m3_vector_icon, IconKind};
use crate::gui::components::motion::{ease_out, MEDIUM};
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};

#[derive(Clone)]
struct Pending {
    path: PathBuf,
    thumb: PathBuf,
}

fn id() -> Id {
    Id::new("lapse_pending_delete")
}

pub fn request_delete(ctx: &egui::Context, path: &Path, thumb: &Path) {
    request_delete_many(ctx, vec![(path.to_path_buf(), thumb.to_path_buf())]);
}

/// (clip, thumbnail) pairs.
pub fn request_delete_many(ctx: &egui::Context, clips: Vec<(PathBuf, PathBuf)>) {
    let pending: Vec<Pending> = clips.into_iter().map(|(path, thumb)| Pending { path, thumb }).collect();
    if !pending.is_empty() {
        ctx.data_mut(|d| d.insert_temp(id(), Some(pending)));
    }
}

/// Moves the clip to the trash, or deletes it when there is no trash. Returns false on failure.
fn remove_clip(path: &Path) -> bool {
    trash::delete(path).is_ok() || std::fs::remove_file(path).is_ok()
}

pub enum DeleteOutcome {
    Removed(String),
    Failed(String),
}

pub fn render_delete_dialog(ctx: &egui::Context, theme: &MonetTheme, save_path: &Path) -> Option<DeleteOutcome> {
    let pending: Option<Vec<Pending>> = ctx.data(|d| d.get_temp(id())).flatten();
    let Some(pending) = pending else { return None };
    let Some(first) = pending.first().cloned() else { return None };
    let many = pending.len() > 1;
    let mut close = false;
    let mut confirmed = false;

    let screen = ctx.screen_rect();
    let appear = ease_out(ctx.animate_bool_with_time(id().with(&first.path), true, MEDIUM));

    egui::Area::new(id().with("scrim"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha((150.0 * appear) as u8));
            if resp.clicked() {
                close = true;
            }
        });

    let size = Vec2::new(400.0_f32.min(screen.width() - 32.0), 0.0);
    let pos = egui::pos2(screen.center().x - size.x / 2.0, screen.center().y - 90.0 + 16.0 * (1.0 - appear));
    let name = if many {
        format!("{} {}", pending.len(), t("clips"))
    } else {
        first.path.file_name().unwrap_or_default().to_string_lossy().to_string()
    };

    egui::Area::new(id().with("dialog"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(theme.surface_container_high)
                .stroke(Stroke::new(1.0_f32, theme.outline_variant))
                .rounding(M3Shapes::EXTRA_LARGE)
                .inner_margin(Margin::same(24.0))
                .show(ui, |ui| {
                    ui.set_width(size.x - 48.0);
                    ui.horizontal(|ui| {
                        m3_vector_icon(ui, IconKind::Trash, 22.0, theme.error);
                        let title = if many { t("Delete these clips?") } else { t("Delete this clip?") };
                        ui.label(RichText::new(title).size(18.0).strong().color(theme.on_surface));
                    });
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(RichText::new(&name).size(13.0).monospace().color(theme.on_surface)).truncate(true));
                    ui.add_space(4.0);
                    let note = t("It will be moved to the trash, so you can still restore it.");
                    ui.label(RichText::new(note).size(12.5).color(theme.on_surface_variant));
                    ui.add_space(18.0);
                    let row = Vec2::new(ui.available_width(), 40.0);
                    ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if m3_danger_button(ui, theme, t("Move to trash")).clicked() {
                            confirmed = true;
                        }
                        ui.add_space(8.0);
                        if m3_outlined_button(ui, theme, t("Cancel")).clicked() {
                            close = true;
                        }
                    });
                });
        });

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        close = true;
    }
    if ctx.input(|i| i.key_pressed(Key::Enter)) {
        confirmed = true;
    }

    let mut outcome = None;
    if confirmed {
        let mut failed = 0;
        for p in &pending {
            if remove_clip(&p.path) {
                let _ = std::fs::remove_file(&p.thumb);
                crate::favorites::forget(save_path, &p.path);
            } else {
                failed += 1;
            }
        }
        crate::clip_index::index().refresh();
        outcome = Some(if failed == 0 { DeleteOutcome::Removed(name) } else { DeleteOutcome::Failed(name) });
        close = true;
    }
    if close {
        ctx.data_mut(|d| d.insert_temp::<Option<Vec<Pending>>>(id(), None));
    }
    outcome
}
