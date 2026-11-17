//! Pieces of a clip card shared by Home and the Library: the title with a rename
//! pencil, and the Share / Folder / Delete buttons.

use eframe::egui::{self, RichText, Vec2};
use std::path::{Path, PathBuf};

use crate::clip_index::ClipMetadata;
use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::components::motion::{hover_t, lerp_color};
use crate::gui::i18n::t;
use crate::gui::theme::MonetTheme;

/// Clip name, cut to fit, with a pencil that turns it into a text field
/// (Enter renames, Esc cancels).
pub fn clip_title(ui: &mut egui::Ui, theme: &MonetTheme, clip: &ClipMetadata, save_path: &Path) {
    editable_name(ui, theme, &clip.path, save_path, 13.0);
}

/// The file name with a rename pencil. Returns the new path after a rename.
pub fn editable_name(ui: &mut egui::Ui, theme: &MonetTheme, path: &Path, save_path: &Path, size: f32) -> Option<PathBuf> {
    let edit_id = egui::Id::new(("lapse_rename", path));
    let editing: Option<String> = ui.data(|d| d.get_temp(edit_id));
    let error_id = edit_id.with("error");
    let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let shown_name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let mut renamed = None;

    match editing {
        Some(mut text) => {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(ui.available_width())
                    .font(egui::FontId::proportional(size)),
            );
            if !resp.has_focus() && !resp.lost_focus() {
                resp.request_focus();
            }
            let (enter, escape) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            if escape {
                ui.data_mut(|d| {
                    d.remove::<String>(edit_id);
                    d.remove::<String>(error_id);
                });
            } else if enter || (resp.lost_focus() && !escape) {
                match crate::rename::rename_clip(save_path, path, &text) {
                    Ok(target) => {
                        ui.data_mut(|d| {
                            d.remove::<String>(edit_id);
                            d.remove::<String>(error_id);
                        });
                        renamed = (target != path).then_some(target);
                    }
                    Err(e) => ui.data_mut(|d| {
                        d.insert_temp(edit_id, text.clone());
                        d.insert_temp(error_id, e);
                    }),
                }
            } else {
                ui.data_mut(|d| d.insert_temp(edit_id, text));
            }
            if let Some(err) = ui.data(|d| d.get_temp::<String>(error_id)) {
                ui.label(RichText::new(crate::gui::i18n::tr_any(&err)).size(11.0).color(theme.error));
            }
        }
        None => {
            ui.horizontal(|ui| {
                let pencil = 22.0;
                let room = (ui.available_width() - pencil - 4.0).max(40.0);
                let text_w = ui.painter().layout_no_wrap(shown_name.clone(), egui::FontId::proportional(size), theme.on_surface).size().x + 2.0;
                ui.add_sized(
                    Vec2::new(text_w.min(room), size + 5.0),
                    egui::Label::new(RichText::new(&shown_name).size(size).strong().color(theme.on_surface)).truncate(true),
                )
                .on_hover_text(&file_name);
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(pencil), egui::Sense::click());
                let h = hover_t(ui, &resp);
                if h > 0.01 {
                    ui.painter().circle_filled(rect.center(), 11.0, theme.primary_container.gamma_multiply(h));
                }
                draw_vector_icon(ui.painter(), rect.center(), 14.0, IconKind::Pencil, lerp_color(theme.on_surface_variant, theme.primary, h));
                if resp.on_hover_text(t("Rename")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    ui.data_mut(|d| d.insert_temp(edit_id, shown_name.clone()));
                }
            });
        }
    }
    renamed
}

/// Buttons under a clip card: Share (link / Discord), Folder, and Delete on the right.
pub fn clip_card_actions(ui: &mut egui::Ui, theme: &MonetTheme, clip: &ClipMetadata) {
    use crate::gui::components::button::{m3_small_button, m3_small_button_colored};
    use crate::gui::views::share_menu::{discord_status, share_menu, webhook_url, ShareChoice};
    const H: f32 = 28.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.spacing_mut().interact_size.y = H;
        let bytes = (clip.size_mb * 1024.0 * 1024.0) as u64;
        let hint = t("Over 10 MB: the clip is shrunk with the Discord preset, then sent.");
        match share_menu(ui, theme, ("card_share", &clip.path), bytes, H, hint) {
            Some(ShareChoice::Link) => crate::share::start_live_share(clip.path.clone()),
            Some(ShareChoice::Discord) => crate::discord::send_or_shrink(clip.path.clone(), webhook_url(), clip.duration_secs),
            None => {}
        }
        if m3_small_button(ui, theme, t("Folder")).on_hover_text(t("Show in folder")).clicked() {
            let _ = open::that(clip.path.parent().unwrap_or(&clip.path));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if m3_small_button_colored(ui, theme, t("Delete"), theme.error).clicked() {
                crate::gui::components::confirm::request_delete(ui.ctx(), &clip.path, &clip.thumb_path);
            }
        });
    });
    discord_status(ui, theme, &clip.path);
}
