use eframe::egui::{self, Margin, RichText, Vec2};
use crate::gui::i18n::t;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::gui::components::button::{m3_outlined_button, m3_tonal_button};
use crate::gui::components::card::m3_card;
use crate::gui::components::chip::m3_badge;
use crate::gui::components::confirm::request_delete;
use crate::gui::components::dropdown::m3_dropdown;
use crate::gui::components::header::m3_page_header;
use crate::gui::components::thumb::clip_thumbnail;
use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::components::input::m3_search_input;
use crate::gui::components::motion::m3_switch_labeled;
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

    let date_id = egui::Id::new("lapse_library_date_range");
    let mut date_range: DateRange = ui.data(|d| d.get_temp(date_id)).unwrap_or_default();

    let all_tags = crate::notes::all_tags(save_path);
    let tag_id = egui::Id::new("lapse_library_tag_filter");
    let mut selected_tag: Option<String> = ui.data(|d| d.get_temp(tag_id));

    let mut game_counts: Vec<(String, usize)> = Vec::new();
    for clip in clips.iter() {
        if let Some(entry) = game_counts.iter_mut().find(|(g, _)| g == &clip.game_tag) {
            entry.1 += 1;
        } else {
            game_counts.push((clip.game_tag.clone(), 1));
        }
    }
    game_counts.sort_by(|a, b| a.0.cmp(&b.0));

    let current_game_label = match selected_game_filter.as_ref() {
        Some(game) => match game_counts.iter().find(|(g, _)| g == game) {
            Some((_, count)) => format!("{} ({})", game, count),
            None => game.clone(),
        },
        None => t("All games").to_string(),
    };

    ui.horizontal(|ui| {
        ui.spacing_mut().interact_size.y = 36.0;

        ui.allocate_ui_with_layout(
            Vec2::new(260.0, 36.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                m3_search_input(ui, theme, search_query, t("Search clips..."));
            },
        );

        ui.add_space(12.0);

        ui.label(RichText::new(t("Game")).size(12.5).color(theme.on_surface_variant));
        m3_dropdown(ui, theme, "lapse_library_game_box", current_game_label, 220.0, |ui| {
            ui.selectable_value(selected_game_filter, None, t("All games"));
            for (game, count) in &game_counts {
                ui.selectable_value(
                    selected_game_filter,
                    Some(game.clone()),
                    format!("{} ({})", game, count),
                );
            }
        });

        ui.add_space(12.0);

        ui.label(RichText::new(t("Date")).size(12.5).color(theme.on_surface_variant));
        m3_dropdown(ui, theme, "lapse_library_date_box", t(date_range.label()), 140.0, |ui| {
            for range in DateRange::ALL {
                ui.selectable_value(&mut date_range, range, t(range.label()));
            }
        });

        if !all_tags.is_empty() {
            ui.add_space(12.0);
            let current_tag_label = match selected_tag.as_ref() {
                Some(tag) => format!("#{}", tag),
                None => t("Any tag").to_string(),
            };
            ui.label(RichText::new(t("Tag")).size(12.5).color(theme.on_surface_variant));
            m3_dropdown(ui, theme, "lapse_library_tag_box", current_tag_label, 140.0, |ui| {
                ui.selectable_value(&mut selected_tag, None, t("Any tag"));
                for tag in &all_tags {
                    ui.selectable_value(&mut selected_tag, Some(tag.clone()), format!("#{}", tag));
                }
            });
        }

        ui.add_space(12.0);

        m3_switch_labeled(ui, theme, &mut favorites_only, t("Favorites only"));
    });

    ui.data_mut(|d| d.insert_temp(fav_id, favorites_only));
    ui.data_mut(|d| d.insert_temp(date_id, date_range));
    ui.data_mut(|d| d.insert_temp(tag_id, selected_tag.clone()));
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
            if !date_range.contains(c.modified_time) {
                return false;
            }
            let notes = crate::notes::get(save_path, &c.path);
            if let Some(ref tag) = selected_tag {
                if !notes.tags.iter().any(|t| t == tag) {
                    return false;
                }
            }
            if !search_query.trim().is_empty() {
                let query = search_query.to_lowercase();
                let q_tag = query.trim_start_matches('#');
                let matches_name = c.name.to_lowercase().contains(&query);
                let matches_game = c.game_tag.to_lowercase().contains(&query);
                let matches_note = notes.note.to_lowercase().contains(&query);
                let matches_tags = notes.tags.iter().any(|t| {
                    t.to_lowercase().contains(&query)
                        || (!q_tag.is_empty() && t.to_lowercase().contains(q_tag))
                });
                if !matches_name && !matches_game && !matches_note && !matches_tags {
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

                            ui.allocate_ui(Vec2::new(item_width, 320.0), |ui| {
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

                                        super::clip_card::clip_notes(ui, theme, clip, save_path);

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

#[derive(Clone, Copy, PartialEq, Default, Debug)]
enum DateRange {
    #[default]
    AnyTime,
    Today,
    Last7Days,
    Last30Days,
    ThisYear,
}

impl DateRange {
    const ALL: [DateRange; 5] = [
        DateRange::AnyTime,
        DateRange::Today,
        DateRange::Last7Days,
        DateRange::Last30Days,
        DateRange::ThisYear,
    ];

    fn label(self) -> &'static str {
        match self {
            DateRange::AnyTime => "Any time",
            DateRange::Today => "Today",
            DateRange::Last7Days => "Last 7 days",
            DateRange::Last30Days => "Last 30 days",
            DateRange::ThisYear => "This year",
        }
    }

    fn contains(self, time: SystemTime) -> bool {
        use chrono::Datelike;
        let now = chrono::Local::now();
        let clip_dt: chrono::DateTime<chrono::Local> = time.into();
        match self {
            DateRange::AnyTime => true,
            DateRange::Today => clip_dt.date_naive() == now.date_naive(),
            DateRange::Last7Days => clip_dt >= now - chrono::Duration::days(7),
            DateRange::Last30Days => clip_dt >= now - chrono::Duration::days(30),
            DateRange::ThisYear => clip_dt.year() == now.year(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_range_contains() {
        let now = SystemTime::now();
        assert!(DateRange::AnyTime.contains(now));
        assert!(DateRange::Today.contains(now));
        assert!(DateRange::Last7Days.contains(now));
        assert!(DateRange::Last30Days.contains(now));
        assert!(DateRange::ThisYear.contains(now));

        let now_dt = chrono::Local::now();

        let two_days_ago: SystemTime = (now_dt - chrono::Duration::days(2)).into();
        assert!(DateRange::AnyTime.contains(two_days_ago));
        assert!(!DateRange::Today.contains(two_days_ago));
        assert!(DateRange::Last7Days.contains(two_days_ago));
        assert!(DateRange::Last30Days.contains(two_days_ago));

        let ten_days_ago: SystemTime = (now_dt - chrono::Duration::days(10)).into();
        assert!(!DateRange::Last7Days.contains(ten_days_ago));
        assert!(DateRange::Last30Days.contains(ten_days_ago));

        let forty_five_days_ago: SystemTime = (now_dt - chrono::Duration::days(45)).into();
        assert!(!DateRange::Last30Days.contains(forty_five_days_ago));

        let last_year: SystemTime = (now_dt - chrono::Duration::days(400)).into();
        assert!(!DateRange::ThisYear.contains(last_year));
    }
}
