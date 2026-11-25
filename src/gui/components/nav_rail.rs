use eframe::egui::{self, Align2, Color32, FontId, Margin, Rect, RichText, Stroke, Vec2};
use crate::gui::i18n::t;
use crate::gui::components::button::{m3_danger_button_sized, m3_filled_button_sized, m3_tonal_button_sized};
use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::state::NavigationTab;
use crate::gui::components::motion::{ease_out, hover_t, lerp_color, live_pill, pulse, MEDIUM};
use crate::gui::theme::{M3Shapes, MonetTheme};
use crate::ipc;

/// Sidebar width while collapsed to icons.
pub const RAIL_COLLAPSED_WIDTH: f32 = 68.0;
/// Narrower than this, the sidebar draws only icons.
const COMPACT_BELOW: f32 = 180.0;

/// Draws the sidebar into whatever width it is given; below `COMPACT_BELOW` it shows only
/// icons. Returns true when the collapse/expand button was clicked.
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
    collapsed: bool,
    mut on_command: impl FnMut(ipc::Command),
    mut on_navigate: impl FnMut(NavigationTab),
) -> bool {
    let compact = ui.available_width() < COMPACT_BELOW;
    let mut toggled = false;
    egui::Frame::none()
        .fill(theme.surface_container_low)
        .inner_margin(if compact { Margin::symmetric(0.0, 14.0) } else { Margin::symmetric(10.0, 14.0) })
        .show(ui, |ui| {
            let toggle_hint = if collapsed { t("Expand sidebar") } else { t("Collapse sidebar") };
            if compact {
                ui.vertical_centered(|ui| {
                    if sidebar_toggle(ui, theme).on_hover_text(toggle_hint).clicked() {
                        toggled = true;
                    }
                    ui.add_space(10.0);
                    let (dot_rect, dot_resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 10.0), egui::Sense::hover());
                    status_dot(ui, theme, dot_rect.center(), is_recording);
                    dot_resp.on_hover_text(status_text(is_recording, is_installed, waiting_for_game));
                });
            } else {
                ui.horizontal(|ui| {
                    if sidebar_toggle(ui, theme).on_hover_text(toggle_hint).clicked() {
                        toggled = true;
                    }
                    ui.add_space(4.0);
                    egui::Frame::none()
                        .fill(theme.surface_container)
                        .stroke(Stroke::new(1.0_f32, theme.outline_variant))
                        .rounding(M3Shapes::FULL)
                        .inner_margin(Margin::symmetric(12.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                                status_dot(ui, theme, dot_rect.center(), is_recording);
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(t("LAPSE"))
                                        .size(15.0)
                                        .strong()
                                        .color(theme.on_surface)
                                        .extra_letter_spacing(1.0),
                                );
                            });
                        });
                });
            }

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
                let row_width = if compact { 48.0 } else { ui.available_width() - 4.0 };

                let (rect, resp) = if compact {
                    ui.vertical_centered(|ui| ui.allocate_exact_size(Vec2::new(row_width, 40.0), egui::Sense::click())).inner
                } else {
                    ui.allocate_exact_size(Vec2::new(row_width, 40.0), egui::Sense::click())
                };
                let resp = if compact { resp.on_hover_text(t(label)) } else { resp };
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

                if compact {
                    draw_vector_icon(ui.painter(), rect.center(), 16.0, icon, icon_color);
                    if let Some(count) = badge_count.filter(|c| *c > 0) {
                        let text = if count > 99 { "99+".to_string() } else { count.to_string() };
                        let galley = ui.painter().layout_no_wrap(text, FontId::proportional(9.0), theme.on_primary);
                        let badge = Rect::from_center_size(
                            egui::pos2(rect.center().x + 11.0, rect.top() + 9.0),
                            Vec2::new((galley.size().x + 8.0).max(15.0), 14.0),
                        );
                        ui.painter().rect_filled(badge, egui::Rounding::same(7.0), theme.primary);
                        ui.painter().galley(badge.center() - galley.size() / 2.0, galley, theme.on_primary);
                    }
                    if resp.clicked() {
                        *active_tab = tab;
                        on_navigate(tab);
                    }
                    ui.add_space(4.0);
                    continue;
                }

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
                let mut shapes = vec![egui::Shape::rect_filled(pill, M3Shapes::FULL, theme.primary_container)];
                if !compact {
                    shapes.push(egui::Shape::rect_filled(accent_bar, egui::Rounding::same(2.0), theme.primary));
                }
                ui.painter().set(indicator_slot, egui::Shape::Vec(shapes));
            }

            if compact {
                render_compact_controls(ui, theme, replay_hotkey_str, is_recording, is_installed, waiting_for_game, &mut on_command);
                return;
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
                    let row_width = ui.available_width() - 4.0;

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
                        let display_name = elide(ui, game, FontId::proportional(12.0), row_width - 26.0 - 40.0);
                        ui.painter().text(egui::pos2(rect.min.x + 26.0, rect.center().y), Align2::LEFT_CENTER, &display_name, FontId::proportional(12.0), fg);
                        let resp = if display_name != *game { resp.on_hover_text(game) } else { resp };
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

            let btn_w = ui.available_width() - 8.0;

            ui.vertical_centered(|ui| {
                if !cfg!(windows) && !is_installed {
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
    toggled
}

/// The collapse/expand button in the sidebar's top-left corner.
fn sidebar_toggle(ui: &mut egui::Ui, theme: &MonetTheme) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(34.0), egui::Sense::click());
    let hover = hover_t(ui, &resp);
    if hover > 0.0 {
        ui.painter().rect_filled(rect, M3Shapes::FULL, theme.surface_container_high.gamma_multiply(hover));
    }
    let color = lerp_color(theme.on_surface_variant, theme.on_surface, hover);
    draw_vector_icon(ui.painter(), rect.center(), 17.0, IconKind::Sidebar, color);
    resp
}

fn status_dot(ui: &egui::Ui, theme: &MonetTheme, center: egui::Pos2, is_recording: bool) {
    let color = if is_recording { theme.success } else { theme.primary };
    if is_recording {
        let k = pulse(ui, 1.6);
        ui.painter().circle_filled(center, 4.0 + 3.0 * k, color.gamma_multiply(0.3 * (1.0 - k)));
    }
    ui.painter().circle_filled(center, 4.0, color);
}

fn status_text(is_recording: bool, is_installed: bool, waiting_for_game: bool) -> &'static str {
    if !cfg!(windows) && !is_installed {
        t("Missing gpu-screen-recorder")
    } else if waiting_for_game {
        t("WAITING FOR GAME")
    } else if is_recording {
        t("BUFFER LIVE")
    } else {
        t("STANDBY")
    }
}

/// Round icon button for the collapsed sidebar.
fn round_icon_button(ui: &mut egui::Ui, icon: Option<IconKind>, fill: Color32, fg: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), egui::Sense::click());
    let hover = hover_t(ui, &resp);
    ui.painter().circle_filled(rect.center(), 20.0, lerp_color(fill, fg, 0.12 * hover));
    match icon {
        Some(icon) => draw_vector_icon(ui.painter(), rect.center(), 16.0, icon, fg),
        // Stop: a filled square.
        None => {
            ui.painter().rect_filled(Rect::from_center_size(rect.center(), Vec2::splat(11.0)), egui::Rounding::same(2.0), fg);
        }
    }
    resp
}

/// Start/stop and save buttons at the bottom of the collapsed sidebar.
fn render_compact_controls(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    replay_hotkey_str: &str,
    is_recording: bool,
    is_installed: bool,
    waiting_for_game: bool,
    on_command: &mut impl FnMut(ipc::Command),
) {
    let deck = if is_recording { 110.0 } else { 60.0 };
    let flex_space = ui.available_height() - deck - 14.0;
    if flex_space > 0.0 {
        ui.add_space(flex_space);
    }
    ui.vertical_centered(|ui| {
        if !cfg!(windows) && !is_installed {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), egui::Sense::hover());
            draw_vector_icon(ui.painter(), rect.center(), 18.0, IconKind::Warning, theme.error);
            resp.on_hover_text(t("Missing gpu-screen-recorder"));
        } else if waiting_for_game {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), egui::Sense::hover());
            draw_vector_icon(ui.painter(), rect.center(), 18.0, IconKind::Gamepad, theme.outline);
            resp.on_hover_text(t("WAITING FOR GAME"));
        } else if !is_recording {
            if round_icon_button(ui, Some(IconKind::Play), theme.primary, theme.on_primary)
                .on_hover_text(t("Start Engine"))
                .clicked()
            {
                on_command(ipc::Command::StartReplay);
            }
        } else {
            if round_icon_button(ui, None, theme.error, theme.on_error)
                .on_hover_text(t("Stop Engine"))
                .clicked()
            {
                on_command(ipc::Command::Stop);
            }
            ui.add_space(8.0);
            let save_label = format!("{} ({})", t("Save Replay"), replay_hotkey_str);
            if round_icon_button(ui, Some(IconKind::Save), theme.primary_container, theme.on_primary_container)
                .on_hover_text(save_label)
                .clicked()
            {
                on_command(ipc::Command::SaveReplay);
            }
        }
    });
    ui.add_space(14.0);
}

/// `text` cut with an ellipsis so it fits in `max_width`.
fn elide(ui: &egui::Ui, text: &str, font: FontId, max_width: f32) -> String {
    let fits = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x <= max_width;
    if fits(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut n = chars.len();
    while n > 1 {
        n -= 1;
        let cut: String = chars[..n].iter().collect::<String>().trim_end().to_string() + "…";
        if fits(&cut) {
            return cut;
        }
    }
    "…".to_string()
}
