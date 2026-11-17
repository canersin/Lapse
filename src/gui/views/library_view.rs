use eframe::egui::{self, Margin, RichText, Vec2};
use crate::gui::i18n::t;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::gui::components::button::{m3_outlined_button, m3_tonal_button};
use crate::gui::components::card::m3_card;
use crate::gui::components::chip::{m3_badge, m3_chip};
use crate::gui::components::confirm::request_delete;
use crate::gui::components::dropdown::m3_dropdown;
use crate::gui::components::header::m3_page_header;
use crate::gui::components::thumb::clip_thumbnail;
use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::components::input::m3_search_input;
use crate::gui::state::NavigationTab;
use crate::gui::theme::{M3Shapes, M3Typography, MonetTheme};
use crate::gui::views::player_view::PlayerState;

pub use crate::clip_index::ClipMetadata;

pub fn render_library(
    clips: &mut Vec<ClipMetadata>,
    last_refresh: &mut Instant,
    save_path: &PathBuf,
    search_query: &mut String,
    selected_game_filter: &mut Option<String>,
    player_state: &mut Option<PlayerState>,
    replay_hotkey_str: &str,
    theme: &MonetTheme,
    ui: &mut egui::Ui,
) {
    refresh_clips(clips, last_refresh, save_path);

    let total_size_mb: f64 = clips.iter().map(|c| c.size_mb).sum();
    let clip_count = clips.len();

    m3_page_header(
        ui,
        theme,
        t("All Clips"),
        &library_subtitle(clip_count, total_size_mb, save_path),
        |ui| {
            if m3_tonal_button(ui, theme, t("Open Folder")).clicked() {
                let _ = open::that(save_path);
            }
            ui.add_space(8.0);
            if m3_outlined_button(ui, theme, t("Refresh")).clicked() {
                *last_refresh = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
            }
        },
    );

    let fav_id = egui::Id::new("lapse_library_favorites_only");
    let mut favorites_only: bool = ui.data(|d| d.get_temp(fav_id)).unwrap_or(false);

    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(260.0, 36.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                m3_search_input(ui, theme, search_query, t("Search clips..."));
            },
        );

        ui.add_space(8.0);

        ui.horizontal_wrapped(|ui| {
            let is_all = selected_game_filter.is_none() && !favorites_only;
            if m3_chip(ui, theme, t("All Games"), is_all).clicked() {
                *selected_game_filter = None;
                favorites_only = false;
            }
            if m3_chip(ui, theme, t("Favorites"), favorites_only).clicked() {
                favorites_only = !favorites_only;
            }

            let mut unique_games: Vec<String> = clips.iter().map(|c| c.game_tag.clone()).collect();
            unique_games.sort();
            unique_games.dedup();

            for game in unique_games.iter().take(6) {
                let is_sel = selected_game_filter.as_ref() == Some(game);
                if m3_chip(ui, theme, game, is_sel).clicked() {
                    if is_sel {
                        *selected_game_filter = None;
                    } else {
                        *selected_game_filter = Some(game.clone());
                    }
                }
            }
        });
    });

    ui.data_mut(|d| d.insert_temp(fav_id, favorites_only));
    ui.add_space(10.0);

    let sort_id = egui::Id::new("lapse_library_sort");
    let mut sort: SortBy = ui.data(|d| d.get_temp(sort_id)).unwrap_or_default();

    let mut filtered_clips: Vec<&ClipMetadata> = clips
        .iter()
        .filter(|c| {
            if favorites_only && !crate::favorites::is_favorite(save_path, &c.path) {
                return false;
            }
            if let Some(ref sel_game) = selected_game_filter {
                if &c.game_tag != sel_game {
                    return false;
                }
            }
            if !search_query.trim().is_empty() {
                let query = search_query.to_lowercase();
                let matches_name = c.name.to_lowercase().contains(&query);
                let matches_game = c.game_tag.to_lowercase().contains(&query);
                if !matches_name && !matches_game {
                    return false;
                }
            }
            true
        })
        .collect();
    sort.apply(&mut filtered_clips);

    ui.horizontal(|ui| {
        let selecting = super::merge_bar::is_selecting();
        super::merge_bar::render_bar(ui, theme, clips, &filtered_clips, save_path);
        if selecting {
            return;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            m3_dropdown(ui, theme, "lapse_library_sort_box", t(sort.label()), 200.0, |ui| {
                    for option in SortBy::ALL {
                        ui.selectable_value(&mut sort, option, t(option.label()));
                    }
                });
            ui.label(RichText::new(t("Sort")).size(12.5).color(theme.on_surface_variant));
        });
    });
    ui.data_mut(|d| d.insert_temp(sort_id, sort));
    ui.add_space(10.0);

    let mut to_play: Option<PathBuf> = None;

    if filtered_clips.is_empty() {
        egui::Frame::none()
            .fill(theme.surface_container_low)
            .rounding(M3Shapes::LARGE)
            .inner_margin(Margin::same(40.0))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(16.0);
                    ui.label(M3Typography::title_medium(
                        if clips.is_empty() {
                            t("No clips yet")
                        } else {
                            t("No clips match your search")
                        },
                        theme.on_surface,
                    ));
                    ui.add_space(4.0);
                    ui.label(M3Typography::body_medium(
                        &format!("{} {} {}", t("Press"), replay_hotkey_str, t("while playing to save the last moments.")),
                        theme.on_surface_variant,
                    ));
                    ui.add_space(16.0);
                });
            });
    } else {
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let item_width = 250.0;
                let spacing = 16.0;
                let available_width = ui.available_width();
                let mut columns = ((available_width + spacing) / (item_width + spacing)).floor() as usize;
                if columns == 0 {
                    columns = 1;
                }

                egui::Grid::new("clips_m3_grid")
                    .spacing(Vec2::splat(spacing))
                    .min_col_width(item_width)
                    .max_col_width(item_width)
                    .show(ui, |ui| {
                        for (idx, clip) in filtered_clips.iter().enumerate() {
                            if idx > 0 && idx % columns == 0 {
                                ui.end_row();
                            }

                            ui.allocate_ui(Vec2::new(item_width, 275.0), |ui| {
                                m3_card(ui, theme, |ui| {
                                    ui.set_width(item_width - 32.0);
                                    ui.vertical(|ui| {
                                        let thumb_w = item_width - 32.0;
                                        let thumb_height = thumb_w * 0.5625;
                                        let thumb_rect = egui::Rect::from_min_size(ui.cursor().min, Vec2::new(thumb_w, thumb_height));
                                        let clicked = clip_thumbnail(ui, theme, clip, Vec2::new(thumb_w, thumb_height), save_path);
                                        if super::merge_bar::is_selecting() {
                                            super::merge_bar::paint_selection(ui, theme, thumb_rect, &clip.path);
                                            if clicked {
                                                super::merge_bar::toggle(&clip.path);
                                            }
                                        } else if clicked {
                                            to_play = Some(clip.path.clone());
                                        }

                                        ui.add_space(8.0);

                                        super::clip_card::clip_title(ui, theme, clip, save_path);

                                        ui.add_space(2.0);

                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(&clip.created_str)
                                                    .size(11.0)
                                                    .color(theme.on_surface_variant),
                                            );
                                            ui.label(RichText::new(t("-")).size(10.0).color(theme.outline_variant));
                                            ui.label(
                                                RichText::new(format!("{:.1} MB", clip.size_mb))
                                                    .size(11.0)
                                                    .color(theme.on_surface_variant),
                                            );
                                        });

                                        ui.add_space(4.0);

                                        m3_badge(
                                            ui,
                                            theme,
                                            &clip.game_tag,
                                            Some(theme.primary_container),
                                        );

                                        ui.add_space(6.0);
                                        ui.separator();
                                        ui.add_space(4.0);

                                        super::clip_card::clip_card_actions(ui, theme, clip);
                                    });
                                });
                            });
                        }
                    });
            });

        if let Some(p) = to_play {
            *player_state = PlayerState::new(p);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Default)]
enum SortBy {
    #[default]
    Newest,
    Oldest,
    Largest,
    Longest,
    Name,
}

impl SortBy {
    const ALL: [SortBy; 5] = [SortBy::Newest, SortBy::Oldest, SortBy::Largest, SortBy::Longest, SortBy::Name];

    fn label(self) -> &'static str {
        match self {
            SortBy::Newest => "Newest first",
            SortBy::Oldest => "Oldest first",
            SortBy::Largest => "Largest first",
            SortBy::Longest => "Longest first",
            SortBy::Name => "Name",
        }
    }

    fn apply(self, clips: &mut [&ClipMetadata]) {
        match self {
            SortBy::Newest => clips.sort_by(|a, b| b.modified_time.cmp(&a.modified_time)),
            SortBy::Oldest => clips.sort_by(|a, b| a.modified_time.cmp(&b.modified_time)),
            SortBy::Largest => clips.sort_by(|a, b| b.size_mb.total_cmp(&a.size_mb)),
            SortBy::Longest => clips.sort_by(|a, b| b.duration_secs.total_cmp(&a.duration_secs)),
            SortBy::Name => clips.sort_by_key(|c| c.name.to_lowercase()),
        }
    }
}

fn library_subtitle(count: usize, total_mb: f64, save_path: &PathBuf) -> String {
    let gb = |b: f64| format!("{:.1} GB", b);
    let mut text = format!("{} {} - {} {}", count, t("clips"), gb(total_mb / 1024.0), t("used"));
    if let Some(free) = crate::utils::free_space_bytes(save_path) {
        text.push_str(&format!(", {} {}", gb(free as f64 / 1024.0 / 1024.0 / 1024.0), t("free on disk")));
    }
    text
}

/// Copies the latest clip list from the background index (cheap; never touches the disk).
/// Setting `last_refresh` into the past asks the index to rescan right away.
pub fn refresh_clips(clips: &mut Vec<ClipMetadata>, last_refresh: &mut Instant, save_path: &PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEEN: AtomicU64 = AtomicU64::new(u64::MAX);

    let index = crate::clip_index::index();
    if last_refresh.elapsed() > Duration::from_secs(5) {
        *last_refresh = Instant::now();
        index.refresh();
    }
    if let Some((generation, list)) = index.poll(save_path, SEEN.load(Ordering::Relaxed)) {
        SEEN.store(generation, Ordering::Relaxed);
        *clips = list;
    }
}
