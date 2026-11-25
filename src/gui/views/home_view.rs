use eframe::egui::{self, Align2, FontId, Margin, Rect, RichText, Stroke, Vec2};
use crate::gui::components::button::{m3_small_button, m3_small_button_colored};
use crate::gui::components::confirm::request_delete;
use crate::gui::components::logo::draw_lapse_logo;
use crate::gui::components::thumb::clip_thumbnail;
use crate::gui::i18n::t;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::gui::components::button::{m3_filled_button_sized, m3_outlined_button};
use crate::gui::components::card::{m3_card, m3_game_capsule};
use crate::gui::components::chip::m3_badge;
use crate::gui::components::icon::{draw_vector_icon, m3_vector_icon, IconKind};
use crate::gui::state::NavigationTab;
use crate::gui::theme::{M3Shapes, MonetTheme};
use crate::gui::views::hotkey_view::format_hotkey;
use crate::gui::views::library_view::{refresh_clips, ClipMetadata};
use crate::gui::views::player_view::PlayerState;
use crate::hardware::gpu::GpuDevice;
use crate::gui::components::motion::{ease_out, hover_t, lerp_color, live_pill, FAST};
use crate::ipc;
use crate::update::{get_update_state, UpdateStatus};

pub fn render_home(
    config: &mut Config,
    clips: &mut Vec<ClipMetadata>,
    last_refresh: &mut Instant,
    active_tab: &mut NavigationTab,
    selected_game_filter: &mut Option<String>,
    mic_enabled: &mut bool,
    desktop_audio_enabled: &mut bool,
    player_state: &mut Option<PlayerState>,
    status: Option<&ipc::StatusResponse>,
    gpus: &[GpuDevice],
    theme: &MonetTheme,
    ui: &mut egui::Ui,
    on_command: impl Fn(ipc::Command),
) {
    refresh_clips(clips, last_refresh, &config.save_path);

    let is_recording = status.map(|s| s.recording).unwrap_or(false);
    let is_continuous = status.map(|s| s.mode == "Continuous").unwrap_or(false);
    let is_replay = status.map(|s| s.mode == "Replay").unwrap_or(false);

    let hotkey_str = format_hotkey(config.hotkey_replay, config.hotkey_replay_mod);

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let update_banner = {
                get_update_state().lock().ok().and_then(|g| match &*g {
                    UpdateStatus::Available { version, url } => Some((version.clone(), url.clone())),
                    _ => None,
                })
            };
            if let Some((version, url)) = update_banner {
                egui::Frame::none()
                    .fill(theme.primary_container)
                    .rounding(M3Shapes::MEDIUM)
                    .inner_margin(Margin::symmetric(16.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            m3_vector_icon(ui, IconKind::Update, 15.0, theme.on_primary_container);
                            ui.add_space(6.0);
                            ui.label(RichText::new(format!("{} ({})", t("A new version of Lapse is available"), version)).size(13.0).strong().color(theme.on_primary_container));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if m3_small_button_colored(ui, theme, t("View Release ->"), theme.primary).clicked() {
                                    let _ = open::that(url);
                                }
                            });
                        });
                    });
                ui.add_space(10.0);
            }

            egui::Frame::none()
                .fill(theme.surface_container)
                .stroke(Stroke::new(1.0_f32, theme.outline_variant))
                .rounding(M3Shapes::LARGE)
                .inner_margin(Margin::same(20.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (logo_slot, _) = ui.allocate_exact_size(Vec2::splat(56.0), egui::Sense::hover());
                        ui.add_space(16.0);
                        let title_block = ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(t("Gameplay Replay Studio")).size(24.0).strong().color(theme.on_surface));
                                ui.add_space(8.0);
                                let waiting = status.map(|s| s.waiting_for_game).unwrap_or(false);
                                if is_continuous {
                                    live_pill(ui, theme, t("RECORDING"), theme.error, true);
                                } else if is_recording {
                                    live_pill(ui, theme, t("REPLAY BUFFER LIVE"), theme.success, true);
                                } else if waiting {
                                    live_pill(ui, theme, t("WAITING FOR GAME"), theme.outline, false);
                                } else {
                                    live_pill(ui, theme, t("STANDBY"), theme.outline, false);
                                }
                                if let Some(game) = status.and_then(|s| s.game.as_ref()) {
                                    ui.add_space(4.0);
                                    live_pill(ui, theme, game, theme.primary, false);
                                }
                            });

                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} {} {}: {}",
                                    t("Save the last"),
                                    config.replay_seconds,
                                    t("seconds with"),
                                    hotkey_str
                                ))
                                .size(13.0)
                                .color(theme.on_surface_variant),
                            );

                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if is_recording {
                                    if m3_filled_button_sized(ui, theme, t("Save Instant Replay"), Vec2::new(190.0, 38.0)).clicked() {
                                        on_command(ipc::Command::SaveReplay);
                                    }
                                    ui.add_space(8.0);
                                    if m3_outlined_button(ui, theme, t("Stop Replay Buffer")).clicked() {
                                        on_command(ipc::Command::Stop);
                                    }
                                } else if m3_filled_button_sized(ui, theme, t("Start Replay Buffer"), Vec2::new(190.0, 38.0)).clicked() {
                                    on_command(ipc::Command::StartReplay);
                                }
                            });
                        });
                        let rect = Rect::from_center_size(
                            egui::pos2(logo_slot.center().x, title_block.response.rect.center().y),
                            Vec2::splat(56.0),
                        );
                        draw_lapse_logo(ui.painter(), rect, theme.surface_container_highest, theme.on_surface);
                    });
                });

            if let Some(st) = status {
                if let Some(err) = &st.last_error {
                    ui.add_space(10.0);
                    notice_banner(ui, theme, theme.error, IconKind::Warning, &format!("{}: {}", t("Recorder problem"), err), true);
                }
                for w in &st.warnings {
                    ui.add_space(8.0);
                    notice_banner(ui, theme, egui::Color32::from_rgb(240, 180, 60), IconKind::Warning, w, false);
                }
            }

            if let Some(err) = crate::hotkeys::last_hotkey_error() {
                ui.add_space(8.0);
                notice_banner(ui, theme, egui::Color32::from_rgb(240, 180, 60), IconKind::Warning, &err, false);
            }

            if !cfg!(windows) && config.dual_gpu {
                ui.add_space(10.0);
                render_dual_gpu_strip(ui, theme, config, gpus, status);
            }

            ui.add_space(14.0);

            let available_w = ui.available_width();
            let card_spacing = 10.0;
            let card_width = ((available_w - (card_spacing * 3.0)) / 4.0).max(140.0);

            let mut toggle_mic = false;
            let mut toggle_desktop = false;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = card_spacing;

                let (rep_st, rep_active) = if is_replay { (t("BUFFER ACTIVE"), true) } else { (t("OFF"), false) };
                render_expressive_stat_card(
                    ui,
                    theme,
                    IconKind::Timer,
                    t("Replay Buffer"),
                    rep_st,
                    rep_active,
                    card_width,
                    || {
                        if is_replay {
                            on_command(ipc::Command::Stop);
                        } else {
                            on_command(ipc::Command::StartReplay);
                        }
                    },
                );

                let (rec_st, rec_active) = if is_continuous { (t("RECORDING"), true) } else { (t("READY"), false) };
                render_expressive_stat_card(
                    ui,
                    theme,
                    IconKind::RecordDot,
                    t("Continuous Rec"),
                    rec_st,
                    rec_active,
                    card_width,
                    || {
                        if is_continuous {
                            on_command(ipc::Command::Stop);
                        } else {
                            on_command(ipc::Command::StartRecording);
                        }
                    },
                );

                let (mic_st, mic_active) = if *mic_enabled { (t("ACTIVE"), true) } else { (t("MUTED"), false) };
                render_expressive_stat_card(
                    ui,
                    theme,
                    IconKind::Mic,
                    t("Microphone"),
                    mic_st,
                    mic_active,
                    card_width,
                    || {
                        toggle_mic = true;
                    },
                );

                let (aud_st, aud_active) = if *desktop_audio_enabled { (t("ACTIVE"), true) } else { (t("MUTED"), false) };
                render_expressive_stat_card(
                    ui,
                    theme,
                    IconKind::Audio,
                    t("Desktop Audio"),
                    aud_st,
                    aud_active,
                    card_width,
                    || {
                        toggle_desktop = true;
                    },
                );
            });

            if toggle_mic {
                *mic_enabled = !*mic_enabled;
                toggle_audio_source(ui, config, true, *mic_enabled, &on_command);
            }
            if toggle_desktop {
                *desktop_audio_enabled = !*desktop_audio_enabled;
                toggle_audio_source(ui, config, false, *desktop_audio_enabled, &on_command);
            }

            ui.add_space(20.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Recent Clips")).size(18.0).strong().color(theme.on_surface));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if m3_small_button_colored(ui, theme, t("View Library ->"), theme.primary).clicked() {
                        *active_tab = NavigationTab::Library;
                    }
                });
            });

            ui.add_space(10.0);

            let card_outer_width = 256.0_f32;
            let grid_spacing = 14.0_f32;
            let avail_w = ui.available_width();
            let columns = ((avail_w + grid_spacing) / (card_outer_width + grid_spacing))
                .floor()
                .max(1.0) as usize;
            let max_recent = (columns * 2).max(6);

            let recent_clips: Vec<_> = clips.iter().take(max_recent).cloned().collect();
            if recent_clips.is_empty() {
                m3_card(ui, theme, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(28.0);
                        ui.label(RichText::new(t("No gameplay clips recorded yet.")).size(15.0).color(theme.on_surface_variant));
                        ui.add_space(6.0);
                        ui.label(RichText::new(format!("{} {} {}", t("Press"), hotkey_str, t("while playing to save your first clip!"))).size(13.0).color(theme.outline));
                        ui.add_space(28.0);
                    });
                });
            } else {
                let inner_w = card_outer_width - 20.0;
                let aspect_height = inner_w * 0.5625;

                egui::Grid::new("home_recent_grid_v3")
                    .spacing(Vec2::new(grid_spacing, grid_spacing))
                    .min_col_width(card_outer_width)
                    .max_col_width(card_outer_width)
                    .show(ui, |ui| {
                        for (i, clip) in recent_clips.iter().enumerate() {
                            if i > 0 && i % columns == 0 {
                                ui.end_row();
                            }

                            ui.allocate_ui(Vec2::new(card_outer_width, 255.0), |ui| {
                                egui::Frame::none()
                                    .fill(theme.surface_container_low)
                                    .stroke(Stroke::new(1.0_f32, theme.outline_variant))
                                    .rounding(M3Shapes::LARGE)
                                    .inner_margin(Margin::same(10.0))
                                    .show(ui, |ui| {
                                        ui.set_width(inner_w);
                                        ui.vertical(|ui| {
                                            if clip_thumbnail(ui, theme, clip, Vec2::new(inner_w, aspect_height), &config.save_path) {
                                                *player_state = PlayerState::new(clip.path.clone());
                                            }

                                            ui.add_space(8.0);
                                            super::clip_card::clip_title(ui, theme, clip, &config.save_path);

                                            ui.add_space(4.0);
                                            ui.horizontal(|ui| {
                                                m3_badge(ui, theme, &clip.game_tag, Some(theme.primary_container));
                                                ui.add_space(4.0);
                                                ui.label(RichText::new(format!("{:.1} MB", clip.size_mb)).size(11.0).color(theme.on_surface_variant));
                                            });

                                            ui.add_space(8.0);
                                            super::clip_card::clip_card_actions(ui, theme, clip);
                                        });
                                    });
                            });
                        }
                    });
            }

            ui.add_space(20.0);

            let mut game_list: Vec<String> = clips.iter().map(|c| c.game_tag.clone()).collect();
            game_list.sort();
            game_list.dedup();

            if !game_list.is_empty() {
                ui.label(RichText::new(t("Game Playlists")).size(16.0).strong().color(theme.on_surface));
                ui.add_space(8.0);

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);
                    for game in &game_list {
                        let count = clips.iter().filter(|c| c.game_tag == *game).count();
                        if m3_game_capsule(ui, theme, IconKind::Gamepad, game, count, false, None).clicked() {
                            *selected_game_filter = Some(game.clone());
                            *active_tab = NavigationTab::Library;
                        }
                    }
                });
            }

            ui.add_space(20.0);
        });
}

/// Mutes/unmutes the microphone or desktop audio by editing the recorder's audio sources.
/// The previous device is remembered so unmuting restores it.
fn toggle_audio_source(
    ui: &egui::Ui,
    config: &mut Config,
    is_mic: bool,
    enable: bool,
    on_command: &impl Fn(ipc::Command),
) {
    let mem_id = egui::Id::new(if is_mic { "last_mic_device" } else { "last_output_device" });
    let field = if is_mic { &mut config.audio_input } else { &mut config.audio_output };
    if enable {
        let fallback = if is_mic {
            if cfg!(windows) { "" } else { "default_input" }
        } else {
            if cfg!(windows) { "" } else { "default_output" }
        };
        let restored: String = ui.data(|d| d.get_temp(mem_id)).unwrap_or_else(|| fallback.to_string());
        *field = restored;
    } else {
        if field.as_str() != "None" && !field.is_empty() {
            let prev = field.clone();
            ui.data_mut(|d| d.insert_temp(mem_id, prev));
        }
        *field = "None".into();
    }
    let _ = config.save();
    on_command(ipc::Command::ReloadConfig);
}

fn notice_banner(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    color: egui::Color32,
    icon: IconKind,
    text: &str,
    show_log: bool,
) {
    egui::Frame::none()
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.5)))
        .rounding(M3Shapes::MEDIUM)
        .inner_margin(Margin::symmetric(14.0, 10.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                m3_vector_icon(ui, icon, 16.0, color);
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(text).size(12.5).color(theme.on_surface)).wrap(true));
                if show_log {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if m3_small_button(ui, theme, t("Open Log")).clicked() {
                            let _ = open::that(crate::recorder::log_path());
                        }
                    });
                }
            });
        });
}

/// Compact "Game GPU → Encoder GPU" strip shown on the home page when dual GPU is on.
fn render_dual_gpu_strip(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    config: &Config,
    gpus: &[GpuDevice],
    status: Option<&ipc::StatusResponse>,
) {
    let report = crate::hardware::gpu::dual_gpu_report(gpus, &config.game_gpu, &config.gpu, &config.monitor);
    let live_card = status.map(|s| s.encode_card.clone()).unwrap_or_default();
    let ok = report.problems.is_empty();
    let accent = if ok { theme.success } else { egui::Color32::from_rgb(240, 180, 60) };

    egui::Frame::none()
        .fill(theme.surface_container)
        .stroke(Stroke::new(1.0_f32, theme.outline_variant))
        .rounding(M3Shapes::MEDIUM)
        .inner_margin(Margin::symmetric(14.0, 10.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                m3_vector_icon(ui, IconKind::Chip, 18.0, theme.primary);
                ui.add_space(4.0);
                ui.label(RichText::new(t("DUAL GPU")).size(11.0).strong().color(theme.outline));
                ui.add_space(8.0);
                let game = report.game_gpu.as_ref().map(|g| g.short_name()).unwrap_or_else(|| "not set".into());
                let enc = report.encode_gpu.as_ref().map(|g| g.short_name()).unwrap_or_else(|| "not set".into());
                ui.label(RichText::new(format!("{}: {}", t("Game"), game)).size(12.5).strong().color(theme.on_surface));
                ui.label(RichText::new(t(">")).size(14.0).strong().color(accent));
                ui.label(RichText::new(format!("{}: {}", t("Record"), enc)).size(12.5).strong().color(theme.on_surface));
                if !live_card.is_empty() {
                    ui.label(RichText::new(format!("({} {})", t("encoding on"), live_card)).size(11.0).color(theme.outline));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if ok { t("READY") } else { t("CHECK SETTINGS") };
                    live_pill(ui, theme, label, accent, false);
                });
            });
        });
}

fn render_expressive_stat_card(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    icon: IconKind,
    title: &str,
    status_text: &str,
    is_active: bool,
    width: f32,
    on_click: impl FnOnce(),
) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 68.0), egui::Sense::click());
    let hover = hover_t(ui, &resp);
    let active = ease_out(ui.ctx().animate_bool_with_time(resp.id.with("active"), is_active, FAST * 2.0));
    let press = ui.ctx().animate_bool_with_time(resp.id.with("press"), resp.is_pointer_button_down_on(), 0.08);

    // Slight lift on hover, slight sink on press.
    let rect = rect.translate(Vec2::new(0.0, -2.0 * hover + 1.5 * press)).shrink(press * 1.5);

    let idle_bg = lerp_color(theme.surface_container_high, theme.surface_container_highest, hover);
    let bg = lerp_color(idle_bg, theme.primary_container, active);
    let border_color = lerp_color(
        lerp_color(theme.outline_variant, theme.outline, hover),
        theme.primary,
        active,
    );

    if hover > 0.01 {
        ui.painter().rect_filled(
            rect.translate(Vec2::new(0.0, 3.0)),
            M3Shapes::MEDIUM,
            egui::Color32::from_black_alpha((40.0 * hover) as u8),
        );
    }
    ui.painter().rect_filled(rect, M3Shapes::MEDIUM, bg);
    ui.painter().rect_stroke(rect, M3Shapes::MEDIUM, Stroke::new(1.0_f32 + 0.5 * active, border_color));

    let icon_fg = lerp_color(theme.primary, theme.on_primary_container, active);
    draw_vector_icon(
        ui.painter(),
        egui::pos2(rect.min.x + 26.0, rect.center().y),
        20.0 + 1.5 * hover,
        icon,
        icon_fg,
    );

    let fg_title = lerp_color(theme.on_surface, theme.on_primary_container, active);
    ui.painter().text(
        egui::pos2(rect.min.x + 52.0, rect.center().y - 10.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.0),
        fg_title,
    );

    let fg_status = lerp_color(theme.outline, theme.primary, active);
    ui.painter().text(
        egui::pos2(rect.min.x + 52.0, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        status_text,
        FontId::proportional(11.0),
        fg_status,
    );

    if resp.clicked() {
        on_click();
    }
}
