//! Pieces of a clip card shared by Home and the Library: the title with a rename
//! pencil, and the Share / Folder / Delete buttons.

use eframe::egui::{self, Margin, RichText, Vec2};
use std::path::{Path, PathBuf};

use crate::clip_index::ClipMetadata;
use crate::gui::components::chip::m3_badge;
use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::components::motion::{hover_t, lerp_color};
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};

/// Clip name, cut to fit, with a pencil that turns it into a text field
/// (Enter renames, Esc cancels).
pub fn clip_title(ui: &mut egui::Ui, theme: &MonetTheme, clip: &ClipMetadata, save_path: &Path) {
    editable_name(ui, theme, &clip.path, save_path, 13.0);
}

/// Compact outlined pill reading "+ Tag" that turns primary on hover.
fn add_tag_pill(ui: &mut egui::Ui, theme: &MonetTheme) -> egui::Response {
    let text = t("+ Tag");
    let font = egui::FontId::proportional(11.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), theme.on_surface_variant);
    let size = Vec2::new(galley.size().x + 16.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let h = hover_t(ui, &resp);
    let stroke_color = lerp_color(theme.outline_variant, theme.primary, h);
    let text_color = lerp_color(theme.on_surface_variant, theme.primary, h);
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_stroke(rect, M3Shapes::FULL, egui::Stroke::new(1.0_f32, stroke_color));
        let galley = p.layout_no_wrap(text.to_string(), font, text_color);
        p.galley(rect.center() - galley.size() / 2.0, galley, text_color);
    }
    resp.on_hover_text(t("Add note or tag"))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Tags pills and note line with an inline editor.
pub fn clip_notes(ui: &mut egui::Ui, theme: &MonetTheme, clip: &ClipMetadata, save_path: &Path) {
    let edit_id = egui::Id::new(("lapse_notes_edit", &clip.path));
    let editing: bool = ui.data(|d| d.get_temp(edit_id)).unwrap_or(false);
    let notes = crate::notes::get(save_path, &clip.path);

    if editing {
        if !notes.tags.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                for tag in &notes.tags {
                    egui::Frame::none()
                        .fill(theme.surface_container_highest)
                        .rounding(M3Shapes::FULL)
                        .inner_margin(Margin::symmetric(6.0, 2.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                ui.label(RichText::new(format!("#{}", tag)).size(11.0).color(theme.on_surface));
                                if ui
                                    .add(
                                        egui::Label::new(
                                            RichText::new("×").size(11.0).strong().color(theme.error),
                                        )
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_text(t("Remove tag"))
                                    .clicked()
                                {
                                    crate::notes::remove_tag(save_path, &clip.path, tag);
                                }
                            });
                        });
                }
            });
            ui.add_space(2.0);
        }

        let tag_buf_id = edit_id.with("tag_input");
        let mut tag_input: String = ui.data(|d| d.get_temp(tag_buf_id)).unwrap_or_default();
        let tag_resp = crate::gui::components::text_field::m3_text_field(
            ui,
            theme,
            &mut tag_input,
            crate::gui::components::text_field::TextFieldOpts {
                hint: t("Add tag..."),
                width: Some(ui.available_width()),
                ..Default::default()
            },
        );
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if enter && tag_resp.has_focus() {
            if !tag_input.trim().is_empty() {
                crate::notes::add_tag(save_path, &clip.path, &tag_input);
                tag_input.clear();
                tag_resp.request_focus();
            }
        }
        ui.data_mut(|d| d.insert_temp(tag_buf_id, tag_input));
        ui.add_space(2.0);

        let note_buf_id = edit_id.with("note_input");
        let mut note_text: String = ui.data(|d| d.get_temp(note_buf_id)).unwrap_or_else(|| notes.note.clone());
        ui.horizontal(|ui| {
            let note_resp = crate::gui::components::text_field::m3_text_field(
                ui,
                theme,
                &mut note_text,
                crate::gui::components::text_field::TextFieldOpts {
                    hint: t("Note..."),
                    width: Some(ui.available_width() - 24.0),
                    ..Default::default()
                },
            );
            if note_resp.changed() {
                crate::notes::set_note(save_path, &clip.path, &note_text);
            }
            if enter && note_resp.has_focus() {
                crate::notes::set_note(save_path, &clip.path, &note_text);
                ui.data_mut(|d| d.remove::<bool>(edit_id));
            }
            if ui
                .add(
                    egui::Label::new(
                        RichText::new("✓").size(13.0).strong().color(theme.primary),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_text(t("Done"))
                .clicked()
            {
                crate::notes::set_note(save_path, &clip.path, &note_text);
                ui.data_mut(|d| d.remove::<bool>(edit_id));
            }
        });
        ui.data_mut(|d| d.insert_temp(note_buf_id, note_text));

        if escape {
            ui.data_mut(|d| d.remove::<bool>(edit_id));
        }
    } else {
        let mut open = false;

        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), 20.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for tag in &notes.tags {
                    m3_badge(ui, theme, &format!("#{}", tag), None);
                }
                if add_tag_pill(ui, theme).clicked() {
                    open = true;
                }
            },
        );

        if !notes.note.trim().is_empty() {
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), 20.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let pencil = 20.0;
                    let text_w = (ui.available_width() - pencil - 4.0).max(20.0);
                    let label = ui.add_sized(
                        Vec2::new(text_w, 18.0),
                        egui::Label::new(
                            RichText::new(&notes.note)
                                .size(12.0)
                                .color(theme.on_surface_variant),
                        )
                        .truncate(true)
                        .sense(egui::Sense::click()),
                    );
                    open |= label
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(&notes.note)
                        .clicked();
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(pencil), egui::Sense::click());
                    let h = hover_t(ui, &resp);
                    if h > 0.01 {
                        ui.painter().circle_filled(
                            rect.center(),
                            10.0,
                            theme.primary_container.gamma_multiply(h),
                        );
                    }
                    draw_vector_icon(
                        ui.painter(),
                        rect.center(),
                        12.0,
                        IconKind::Pencil,
                        lerp_color(theme.on_surface_variant, theme.primary, h),
                    );
                    open |= resp
                        .on_hover_text(t("Notes"))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked();
                },
            );
        }

        if open {
            ui.data_mut(|d| {
                d.insert_temp(edit_id, true);
                d.insert_temp(edit_id.with("note_input"), notes.note.clone());
            });
        }
    }
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
            let resp = crate::gui::components::text_field::m3_text_field(
                ui,
                theme,
                &mut text,
                crate::gui::components::text_field::TextFieldOpts {
                    width: Some(ui.available_width()),
                    ..Default::default()
                },
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
    use crate::gui::views::share_menu::{discord_status, share_menu, streamable_status, webhook_url, ShareChoice};
    const H: f32 = 28.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.spacing_mut().interact_size.y = H;
        let bytes = (clip.size_mb * 1024.0 * 1024.0) as u64;
        let hint = t("Over 10 MB: the clip is shrunk with the Discord preset, then sent.");
        match share_menu(ui, theme, ("card_share", &clip.path), bytes, clip.duration_secs, H, hint) {
            Some(ShareChoice::Link) => crate::share::start_live_share(clip.path.clone()),
            Some(ShareChoice::Discord) => crate::discord::send_or_shrink(clip.path.clone(), webhook_url(), clip.duration_secs),
            Some(ShareChoice::Streamable) => crate::streamable::upload_or_shrink(clip.path.clone(), clip.duration_secs),
            Some(ShareChoice::StreamableBrowser) => {
                let _ = open::that("https://streamable.com/");
                let _ = open::that(clip.path.parent().unwrap_or(&clip.path));
            }
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
    streamable_status(ui, theme, &clip.path);
}
