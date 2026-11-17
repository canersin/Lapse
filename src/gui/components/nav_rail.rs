use eframe::egui::{self, Align2, Color32, FontId, Margin, Rect, RichText, Stroke, Vec2};
use crate::gui::i18n::t;
use crate::gui::components::button::{m3_danger_button_sized, m3_filled_button_sized, m3_tonal_button_sized};
use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::state::NavigationTab;
use crate::gui::components::motion::{ease_out, hover_t, lerp_color, live_pill, pulse, MEDIUM};
use crate::gui::theme::{M3Shapes, MonetTheme};
use crate::ipc;

pub fn render_nav_rail(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    active_tab: &mut NavigationTab,
    replay_hotkey_str: &str,
    is_recording: bool,
    is_installed: bool,
    waiting_for_game: bool,
    clip_count: usize,
    game_counts: &[(String, usize)],
    selected_game_filter: &mut Option<String>,
    mut on_command: impl FnMut(ipc::Command),
    mut on_navigate: impl FnMut(NavigationTab),
) {
    egui::Frame::none()
        .fill(theme.surface_container_low)
        .inner_margin(Margin::symmetric(10.0, 14.0))
        .show(ui, |ui| {
            ui.set_width(220.0);

            egui::Frame::none()
                .fill(theme.surface_container)
                .stroke(Stroke::new(1.0_f32, theme.outline_variant))
                .rounding(M3Shapes::FULL)
                .inner_margin(Margin::symmetric(12.0, 6.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                        let dot_color = if is_recording { theme.success } else { theme.primary };
                        if is_recording {
                            let k = pulse(ui, 1.6);
                            ui.painter().circle_filled(dot_rect.center(), 4.0 + 3.0 * k, dot_color.gamma_multiply(0.3 * (1.0 - k)));
                        }
                        ui.painter().circle_filled(dot_rect.center(), 4.0, dot_color);
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(t("LAPSE"))
                                .size(15.0)
                                .strong()
                                .color(theme.on_surface)
                                .extra_letter_spacing(1.0),
                        );
                        ui.label(
                            RichText::new(t("REPLAY"))
                                .size(10.0)
                                .strong()
                                .color(theme.primary),
                        );
                    });
                });

            ui.add_space(14.0);

            let tabs = [
                (NavigationTab::Home, IconKind::Home, "Home", None),
                (NavigationTab::Library, IconKind::Library, "Library", Some(clip_count)),
                (NavigationTab::Settings, IconKind::Gear, "Settings", None),
                (NavigationTab::About, IconKind::Info, "About", None),
            ];

            // The selection pill is painted into a reserved slot behind the rows so it can
            // slide smoothly to the selected tab instead of jumping.
            let indicator_slot = ui.painter().add(egui::Shape::Noop);
            let mut selected_rect: Option<Rect> = None;

            for (tab, icon, label, badge_count) in tabs {
                let is_selected = *active_tab == tab;
                let row_width = (ui.available_width() - 4.0).max(180.0);

                let (rect, resp) = ui.allocate_exact_size(Vec2::new(row_width, 40.0), egui::Sense::click());
                let hover = hover_t(ui, &resp);
                let sel = ease_out(ui.ctx().animate_bool_with_time(resp.id.with("sel"), is_selected, MEDIUM));

                if is_selected {
                    selected_rect = Some(rect);
                }
                if hover > 0.0 && !is_selected {
                    ui.painter().rect_filled(
                        rect,
                        M3Shapes::FULL,
                        theme.surface_container_high.gamma_multiply(hover),
                    );
                }

                let fg = lerp_color(
                    lerp_color(theme.on_surface_variant, theme.on_surface, hover),
                    theme.on_primary_container,
                    sel,
                );
                let icon_color = lerp_color(
                    lerp_color(theme.outline, theme.on_surface_variant, hover),
                    theme.primary,
                    sel,
                );

                // Icon nudges right a little on hover/selection.
                let nudge = 2.0 * hover.max(sel);
                let icon_pos = egui::pos2(rect.min.x + 24.0 + nudge, rect.center().y);
                draw_vector_icon(ui.painter(), icon_pos, 15.0, icon, icon_color);

                ui.painter().text(
                    egui::pos2(rect.min.x + 42.0 + nudge, rect.center().y),
                    Align2::LEFT_CENTER,
                    t(label),
                    FontId::proportional(13.5),
                    fg,
                );

                if let Some(count) = badge_count {
                    let badge_rect = Rect::from_center_size(
                        egui::pos2(rect.max.x - 22.0, rect.center().y),
                        Vec2::new(26.0, 18.0),
                    );
                    let badge_bg = lerp_color(theme.surface_container_highest, theme.primary, sel);
                    let badge_fg = lerp_color(theme.on_surface, theme.on_primary, sel);
                    ui.painter().rect_filled(badge_rect, egui::Rounding::same(9.0), badge_bg);
                    ui.painter().text(
                        badge_rect.center(),
                        Align2::CENTER_CENTER,
                        count.to_string(),
                        FontId::proportional(10.5),
                        badge_fg,
                    );
                }

                if resp.clicked() {
                    *active_tab = tab;
                    on_navigate(tab);
                }

                ui.add_space(4.0);
            }

            if let Some(target) = selected_rect {
                let ctx = ui.ctx().clone();
                let y = ctx.animate_value_with_time(egui::Id::new("nav_indicator_y"), target.top(), MEDIUM);
                let pill = Rect::from_min_size(egui::pos2(target.left(), y), target.size());
                let accent_bar = Rect::from_min_size(
                    egui::pos2(pill.left() + 6.0, pill.top() + 11.0),
                    Vec2::new(3.0, pill.height() - 22.0),
                );
                ui.painter().set(
                    indicator_slot,
                    egui::Shape::Vec(vec![
                        egui::Shape::rect_filled(pill, M3Shapes::FULL, theme.primary_container),
                        egui::Shape::rect_filled(accent_bar, egui::Rounding::same(2.0), theme.primary),
                    ]),
                );
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(t("PLAYLISTS")).size(11.0).strong().color(theme.outline));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if selected_game_filter.is_some() {
                        let clear_btn = ui.add(egui::Button::new(RichText::new(t("Reset")).size(10.0).color(theme.primary)).frame(false));
                        if clear_btn.clicked() {
                            *selected_game_filter = None;
                        }
                    }
                });
            });

            ui.add_space(6.0);

            let scroll_h = (ui.available_height() - 150.0).clamp(50.0, 160.0);

            egui::ScrollArea::vertical()
                .max_height(scroll_h)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    let row_width = (ui.available_width() - 4.0).max(150.0);

                    let is_all_selected = selected_game_filter.is_none();
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(row_width, 30.0), egui::Sense::click());
                    let (bg, fg) = if is_all_selected {
                        (theme.surface_container_high, theme.on_surface)
                    } else if resp.hovered() {
                        (theme.surface_container, theme.on_surface)
                    } else {
                        (Color32::TRANSPARENT, theme.on_surface_variant)
                    };
                    ui.painter().rect_filled(rect, M3Shapes::FULL, bg);
                    ui.painter().circle_filled(egui::pos2(rect.min.x + 14.0, rect.center().y), 3.5, theme.primary);
                    ui.painter().text(egui::pos2(rect.min.x + 26.0, rect.center().y), Align2::LEFT_CENTER, t("All Replays"), FontId::proportional(12.0), fg);
                    ui.painter().text(egui::pos2(rect.max.x - 12.0, rect.center().y), Align2::RIGHT_CENTER, clip_count.to_string(), FontId::proportional(10.5), theme.outline);
                    if resp.clicked() {
                        *selected_game_filter = None;
                        *active_tab = NavigationTab::Library;
                        on_navigate(NavigationTab::Library);
                    }

                    ui.add_space(2.0);

                    for (game, count) in game_counts {
                        let is_game_selected = selected_game_filter.as_ref() == Some(game);
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(row_width, 30.0), egui::Sense::click());
                        let (bg, fg) = if is_game_selected {
                            (theme.surface_container_highest, theme.primary)
                        } else if resp.hovered() {
                            (theme.surface_container, theme.on_surface)
                        } else {
                            (Color32::TRANSPARENT, theme.on_surface_variant)
                        };
                        ui.painter().rect_filled(rect, M3Shapes::FULL, bg);

                        let dot_color = match game.to_lowercase().as_str() {
                            g if g.contains("cs") || g.contains("counter") => Color32::from_rgb(225, 160, 85),
                            g if g.contains("half") || g.contains("hl") => Color32::from_rgb(230, 160, 40),
                            g if g.contains("black") || g.contains("mesa") => Color32::from_rgb(170, 80, 255),
                            g if g.contains("outer") || g.contains("wilds") => Color32::from_rgb(0, 168, 255),
                            _ => theme.primary,
                        };

                        ui.painter().circle_filled(egui::pos2(rect.min.x + 14.0, rect.center().y), 3.5, dot_color);
                        let display_name = if game.len() > 14 { format!("{}...", &game[..12]) } else { game.clone() };
                        ui.painter().text(egui::pos2(rect.min.x + 26.0, rect.center().y), Align2::LEFT_CENTER, display_name, FontId::proportional(12.0), fg);
                        ui.painter().text(egui::pos2(rect.max.x - 12.0, rect.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(10.5), theme.outline);

                        if resp.clicked() {
                            *selected_game_filter = Some(game.clone());
                            *active_tab = NavigationTab::Library;
                            on_navigate(NavigationTab::Library);
                        }

                        ui.add_space(2.0);
                    }
                });

            let bottom_deck_needed = if is_recording { 135.0 } else { 95.0 };
            let flex_space = ui.available_height() - bottom_deck_needed - 14.0;
            if flex_space > 0.0 {
                ui.add_space(flex_space);
            }

            ui.separator();
            ui.add_space(8.0);

            let btn_w = (ui.available_width() - 8.0).max(180.0);

            ui.vertical_centered(|ui| {
                if !is_installed {
                    ui.label(RichText::new(t("Missing gpu-screen-recorder")).size(11.0).color(theme.error));
                } else if waiting_for_game {
                    // Game mode on the desktop: the buffer starts by itself when a game opens.
                    live_pill(ui, theme, t("WAITING FOR GAME"), theme.outline, false);
                } else if !is_recording {
                    if m3_filled_button_sized(ui, theme, t("Start Engine"), Vec2::new(btn_w, 36.0)).clicked() {
                        on_command(ipc::Command::StartReplay);
                    }
                    ui.add_space(8.0);
                    live_pill(ui, theme, t("STANDBY"), theme.outline, false);
                } else {
                    if m3_danger_button_sized(ui, theme, t("Stop Engine"), Vec2::new(btn_w, 34.0)).clicked() {
                        on_command(ipc::Command::Stop);
                    }
                    ui.add_space(6.0);

                    let save_label = format!("{} ({})", t("Save Replay"), replay_hotkey_str);
                    if m3_tonal_button_sized(ui, theme, &save_label, Vec2::new(btn_w, 34.0)).clicked() {
                        on_command(ipc::Command::SaveReplay);
                    }
                    ui.add_space(8.0);

                    live_pill(ui, theme, t("BUFFER LIVE"), theme.success, true);
                }
            });
            ui.add_space(14.0);
        });
}
