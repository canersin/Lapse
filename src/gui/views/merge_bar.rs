//! Library toolbar: select clips to merge them into one video (in the order picked),
//! or to favorite / delete several at once.

use eframe::egui::{self, Align2, Color32, FontId, Margin, Rect, RichText, Stroke};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::clip_index::ClipMetadata;
use crate::export::{ExportJob, ExportState};
use crate::gui::components::button::m3_small_button;
use crate::gui::components::button::{m3_danger_button, m3_filled_button, m3_outlined_button, m3_tonal_button};
use crate::gui::components::icon::{m3_vector_icon, IconKind};
use crate::gui::components::motion::pulse;
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};

#[derive(Default, Clone, Copy, PartialEq)]
enum Mode {
    #[default]
    Merge,
    Manage,
}

#[derive(Default)]
struct MergeUi {
    selecting: bool,
    mode: Mode,
    /// In the order the user picked them; that is the order in the merged video.
    selected: Vec<PathBuf>,
    job: Option<ExportJob>,
}

static STATE: Mutex<Option<MergeUi>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut MergeUi) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(MergeUi::default))
}

pub fn is_selecting() -> bool {
    with(|s| s.selecting)
}

pub fn toggle(path: &PathBuf) {
    with(|s| match s.selected.iter().position(|p| p == path) {
        Some(i) => {
            s.selected.remove(i);
        }
        None => s.selected.push(path.clone()),
    })
}

/// Draws the pick order (1, 2, 3...) or an empty circle on a thumbnail while selecting.
pub fn paint_selection(ui: &egui::Ui, theme: &MonetTheme, thumb: Rect, path: &PathBuf) {
    let (order, mode) = with(|s| (s.selected.iter().position(|p| p == path), s.mode));
    let center = thumb.left_top() + egui::vec2(18.0, 18.0);
    let p = ui.painter();
    match order {
        Some(i) => {
            p.rect_stroke(thumb, M3Shapes::MEDIUM, Stroke::new(3.0_f32, theme.primary));
            p.circle_filled(center, 12.0, theme.primary);
            if mode == Mode::Merge {
                p.text(center, Align2::CENTER_CENTER, (i + 1).to_string(), FontId::proportional(12.5), theme.on_primary);
            } else {
                let check = vec![center + egui::vec2(-5.0, 0.0), center + egui::vec2(-1.5, 3.5), center + egui::vec2(5.0, -4.0)];
                p.add(egui::Shape::line(check, Stroke::new(2.0_f32, theme.on_primary)));
            }
        }
        None => {
            p.circle_filled(center, 12.0, Color32::from_black_alpha(140));
            p.circle_stroke(center, 11.0, Stroke::new(1.5_f32, Color32::WHITE));
        }
    }
}

pub fn render_bar(ui: &mut egui::Ui, theme: &MonetTheme, clips: &[ClipMetadata], visible: &[&ClipMetadata], save_path: &std::path::Path) {
    let (selecting, mode, count, job_state) = with(|s| (s.selecting, s.mode, s.selected.len(), s.job.as_ref().map(|j| j.state())));

    if let Some(ExportState::Running { fraction, .. }) = &job_state {
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 110.0).max(120.0);
            ui.add(egui::ProgressBar::new(fraction.max(0.02)).desired_width(w).text(format!("{} {:.0}%", t("Merging"), fraction * 100.0)));
            if m3_outlined_button(ui, theme, t("Cancel")).clicked() {
                with(|s| s.job.as_ref().map(|j| j.cancel()));
            }
        });
        let _ = pulse(ui, 1.0);
        return;
    }

    if !selecting {
        ui.horizontal(|ui| {
            if m3_tonal_button(ui, theme, t("Select")).on_hover_text(t("Pick several clips to favorite or delete them")).clicked() {
                with(|s| {
                    s.selecting = true;
                    s.mode = Mode::Manage;
                    s.selected.clear();
                });
            }
            if m3_tonal_button(ui, theme, t("Merge clips")).on_hover_text(t("Join several clips into one video")).clicked() {
                with(|s| {
                    s.selecting = true;
                    s.mode = Mode::Merge;
                    s.selected.clear();
                    s.job = None;
                });
            }
            match &job_state {
                Some(ExportState::Done { path, .. }) => {
                    m3_vector_icon(ui, IconKind::Check, 15.0, theme.success);
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    ui.label(RichText::new(format!("{} {}", t("Merged into"), name)).size(12.5).color(theme.on_surface));
                    if m3_small_button(ui, theme, t("Show in folder")).clicked() {
                        let _ = open::that(path.parent().unwrap_or(path));
                    }
                    // The app polls this request file, so the merged video opens in the trimmer
                    // where text, images, videos and sound can be added on top.
                    if m3_small_button(ui, theme, t("Open in editor")).clicked() {
                        let _ = std::fs::write(crate::utils::open_request_path(), path.to_string_lossy().as_bytes());
                    }
                }
                Some(ExportState::Failed(msg)) => {
                    ui.label(RichText::new(format!("{}: {}", t("Merge failed"), msg)).size(12.0).color(theme.error));
                }
                _ => {}
            }
        });
        return;
    }

    egui::Frame::none()
        .fill(theme.primary_container.gamma_multiply(0.5))
        .stroke(Stroke::new(1.0_f32, theme.primary.gamma_multiply(0.5)))
        .rounding(M3Shapes::MEDIUM)
        .inner_margin(Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(36.0);
                    m3_vector_icon(ui, IconKind::Scissors, 16.0, theme.primary);
                    let text = if count == 0 && mode == Mode::Merge {
                        t("Click clips in the order you want them.").to_string()
                    } else if count == 0 {
                        t("Click clips to select them.").to_string()
                    } else {
                        format!("{} {}", count, t("clips selected"))
                    };
                    ui.label(RichText::new(text).size(13.0).color(theme.on_surface));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if m3_outlined_button(ui, theme, t("Cancel")).clicked() {
                            with(|s| {
                                s.selecting = false;
                                s.selected.clear();
                            });
                        }
                        if mode == Mode::Merge {
                            ui.add_enabled_ui(count >= 2, |ui| {
                                if m3_filled_button(ui, theme, t("Merge")).clicked() {
                                    start_merge(clips);
                                }
                            });
                            return;
                        }
                        let selected = with(|s| s.selected.clone());
                    ui.add_enabled_ui(count > 0, |ui| {
                        if m3_danger_button(ui, theme, t("Delete")).clicked() {
                            let pairs = selected
                                .iter()
                                .map(|p| (p.clone(), crate::clip_index::thumb_path_for(save_path, p)))
                                .collect();
                            crate::gui::components::confirm::request_delete_many(ui.ctx(), pairs);
                            with(|s| {
                                s.selecting = false;
                                s.selected.clear();
                            });
                        }
                        if m3_outlined_button(ui, theme, t("Favorite")).on_hover_text(t("Star all selected clips")).clicked() {
                            for p in &selected {
                                if !crate::favorites::is_favorite(save_path, p) {
                                    crate::favorites::toggle(save_path, p);
                                }
                            }
                        }
                        if m3_outlined_button(ui, theme, t("Unfavorite")).on_hover_text(t("Unstar all selected clips")).clicked() {
                            for p in &selected {
                                if crate::favorites::is_favorite(save_path, p) {
                                    crate::favorites::toggle(save_path, p);
                                }
                            }
                        }

                        let mut games: Vec<String> = clips.iter().map(|c| c.game_tag.clone()).collect();
                        games.sort();
                        games.dedup();
                        if !games.iter().any(|g| g == "Gameplay") {
                            games.insert(0, "Gameplay".to_string());
                        }

                        let mut chosen_game = None;
                        crate::gui::components::dropdown::m3_dropdown_sized(
                            ui,
                            theme,
                            "lapse_merge_move_to",
                            t("Move to…"),
                            140.0,
                            32.0,
                            |ui| {
                                for game in &games {
                                    if ui.selectable_label(false, game).clicked() {
                                        chosen_game = Some(game.clone());
                                    }
                                }
                            },
                        );
                        if let Some(target_game) = chosen_game {
                            move_clips(save_path, &selected, &target_game);
                        }

                        let tag_toggle_id = egui::Id::new("lapse_merge_show_add_tag");
                        let mut show_tag_input: bool = ui.data(|d| d.get_temp(tag_toggle_id)).unwrap_or(false);
                        if show_tag_input {
                            let tag_input_id = egui::Id::new("lapse_merge_tag_input");
                            let mut tag_input: String = ui.data(|d| d.get_temp(tag_input_id)).unwrap_or_default();
                            if m3_small_button(ui, theme, "×").on_hover_text(t("Cancel")).clicked() {
                                show_tag_input = false;
                                tag_input.clear();
                            }
                            let apply_clicked = m3_small_button(ui, theme, t("Apply")).clicked();
                            let resp = crate::gui::components::text_field::m3_text_field(
                                ui,
                                theme,
                                &mut tag_input,
                                crate::gui::components::text_field::TextFieldOpts {
                                    hint: t("Add tag..."),
                                    width: Some(100.0),
                                    ..Default::default()
                                },
                            );
                            if !resp.has_focus() && !resp.lost_focus() && tag_input.is_empty() {
                                resp.request_focus();
                            }
                            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter)) && resp.has_focus();
                            let escape_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));

                            if apply_clicked || enter_pressed {
                                let trimmed = tag_input.trim();
                                if !trimmed.is_empty() {
                                    for p in &selected {
                                        crate::notes::add_tag(save_path, p, trimmed);
                                    }
                                }
                                tag_input.clear();
                                show_tag_input = false;
                            } else if escape_pressed {
                                tag_input.clear();
                                show_tag_input = false;
                            }
                            ui.data_mut(|d| d.insert_temp(tag_input_id, tag_input));
                        } else if m3_outlined_button(ui, theme, t("Add tag…")).clicked() {
                            show_tag_input = true;
                        }
                        ui.data_mut(|d| d.insert_temp(tag_toggle_id, show_tag_input));
                    });
                    let all_visible = !visible.is_empty() && visible.iter().all(|c| selected.contains(&c.path));
                    if m3_small_button(ui, theme, if all_visible { t("Select none") } else { t("Select all") }).clicked() {
                        with(|s| {
                            if all_visible {
                                s.selected.clear();
                            } else {
                                for c in visible {
                                    if !s.selected.contains(&c.path) {
                                        s.selected.push(c.path.clone());
                                    }
                                }
                            }
                        });
                    }
                });
            });

            let (still_selecting, selected_paths) = with(|s| (s.selecting, s.selected.clone()));
            if still_selecting && mode == Mode::Merge && selected_paths.len() >= 2 {
                ui.add_space(4.0);
                egui::ScrollArea::horizontal().auto_shrink([false; 2]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let mut swap = None;
                        for (i, path) in selected_paths.iter().enumerate() {
                            let raw_name = path.file_name().unwrap_or_default().to_string_lossy();
                            let short_name = truncate_name(&raw_name);
                            ui.label(RichText::new(format!("{} {}", i + 1, short_name)).size(12.5).color(theme.on_surface));
                            ui.add_enabled_ui(i > 0, |ui| {
                                if m3_small_button(ui, theme, "◀").clicked() {
                                    swap = Some((i, i - 1));
                                }
                            });
                            ui.add_enabled_ui(i + 1 < selected_paths.len(), |ui| {
                                if m3_small_button(ui, theme, "▶").clicked() {
                                    swap = Some((i, i + 1));
                                }
                            });
                            if i + 1 < selected_paths.len() {
                                ui.add_space(6.0);
                            }
                        }
                        if let Some((a, b)) = swap {
                            with(|s| {
                                if a < s.selected.len() && b < s.selected.len() {
                                    s.selected.swap(a, b);
                                }
                            });
                        }
                    });
                });
            }
        });
    });
}

fn truncate_name(name: &str) -> String {
    if name.chars().count() > 18 {
        let prefix: String = name.chars().take(17).collect();
        format!("{}…", prefix)
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_long_names_safely() {
        assert_eq!(truncate_name("short.mp4"), "short.mp4");
        assert_eq!(truncate_name("123456789012345678"), "123456789012345678");
        assert_eq!(truncate_name("1234567890123456789"), "12345678901234567…");
        assert_eq!(truncate_name("Replay_2026-11-16_00-47-30.mp4"), "Replay_2026-11-16…");
        assert_eq!(truncate_name("öçşğüıöçşğüıöçşğüı"), "öçşğüıöçşğüıöçşğüı");
        assert_eq!(truncate_name("öçşğüıöçşğüıöçşğüıextra"), "öçşğüıöçşğüıöçşğü…");
    }
}

fn start_merge(clips: &[ClipMetadata]) {
    with(|s| {
        let inputs: Vec<(PathBuf, f64)> = s
            .selected
            .iter()
            .map(|p| (p.clone(), clips.iter().find(|c| &c.path == p).map(|c| c.duration_secs).unwrap_or(0.0)))
            .collect();
        if let Some(first) = inputs.first() {
            let output = crate::merge::default_output(&first.0);
            s.job = Some(crate::merge::start(inputs, output));
        }
        s.selecting = false;
        s.selected.clear();
    });
}

fn move_clips(save_path: &std::path::Path, selected: &[PathBuf], target_game: &str) {
    let dest_dir = if target_game == "Gameplay" {
        save_path.to_path_buf()
    } else {
        save_path.join(target_game)
    };
    let _ = std::fs::create_dir_all(&dest_dir);

    for clip in selected {
        if !clip.exists() {
            continue;
        }
        if clip.parent() == Some(&dest_dir) {
            continue;
        }
        let ext_str = clip.extension().and_then(|e| e.to_str()).unwrap_or("");
        let stem_str = clip.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let mut candidate = if ext_str.is_empty() {
            dest_dir.join(stem_str)
        } else {
            dest_dir.join(format!("{}.{}", stem_str, ext_str))
        };
        let mut counter = 2;
        while candidate.exists() {
            candidate = if ext_str.is_empty() {
                dest_dir.join(format!("{} ({})", stem_str, counter))
            } else {
                dest_dir.join(format!("{} ({}).{}", stem_str, counter, ext_str))
            };
            counter += 1;
        }
        let new_path = candidate;

        if std::fs::rename(clip, &new_path).is_ok() {
            let old_thumb = crate::clip_index::thumb_path_for(save_path, clip);
            let new_thumb = crate::clip_index::thumb_path_for(save_path, &new_path);
            if old_thumb.exists() && old_thumb != new_thumb {
                let _ = std::fs::rename(&old_thumb, &new_thumb);
            }
            let thumbs = crate::clip_index::thumb_dir(save_path);
            let old_name = clip.file_name().unwrap_or_default().to_string_lossy().to_string();
            let new_name = new_path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let old_markers = thumbs.join(format!("{}.markers.json", old_name));
            let new_markers = thumbs.join(format!("{}.markers.json", new_name));
            if old_markers.exists() && old_markers != new_markers {
                let _ = std::fs::rename(&old_markers, &new_markers);
            }
            crate::notes::rename(save_path, clip, &new_path);
            if crate::favorites::is_favorite(save_path, clip) {
                crate::favorites::forget(save_path, clip);
                crate::favorites::toggle(save_path, &new_path);
            }
        }
    }

    with(|s| {
        s.selecting = false;
        s.selected.clear();
    });
    crate::clip_index::index().refresh();
}

