//! Settings > Games: what to record (desktop or only games), games added by hand with
//! their own clip folder and recording profile, and (in its own sub-tab) automatic clips.

use eframe::egui::{self, Margin, RichText, Stroke};
use std::time::Duration;

use crate::config::{Config, GameEntry};
use crate::gui::components::dropdown::{m3_dropdown, m3_dropdown_sized};
use crate::gui::components::number_field::{m3_number_field, m3_number_field_u32, m3_text_field, NumberOpts};
use crate::hardware::game_detector::{running_apps, RunningApp};
use crate::gui::components::button::{m3_small_button, m3_small_button_colored};
use crate::gui::components::button::m3_tonal_button;
use crate::gui::components::card::m3_section_card;
use crate::gui::components::icon::{m3_vector_icon, IconKind};
use crate::gui::components::input::m3_setting_row;
use crate::gui::components::motion::{m3_switch, m3_switch_labeled};
use crate::gui::components::segmented::segmented;
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};
use crate::autoclip::Game;
use crate::ipc::StatusResponse;

pub fn render_games_tab(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config, status: Option<&StatusResponse>) {
    // Sub-tab: 0 = your games and what to record, 1 = automatic clips.
    let id = egui::Id::new("lapse_games_subtab");
    let mut tab: u8 = ui.data(|d| d.get_temp(id)).unwrap_or(0);
    let options = [(0u8, t("Games")), (1u8, t("Auto clips"))];
    segmented(ui, theme, "games_subtab", &mut tab, &options, 130.0);
    ui.data_mut(|d| d.insert_temp(id, tab));
    ui.add_space(14.0);

    if tab == 0 {
        render_capture_mode(ui, theme, config, status);
        ui.add_space(14.0);
        render_game_list(ui, theme, config);
    } else {
        render_auto_clips(ui, theme, config, status);
    }
}

fn note(ui: &mut egui::Ui, theme: &MonetTheme, icon: IconKind, color: egui::Color32, text: &str) {
    egui::Frame::none()
        .fill(color.gamma_multiply(0.10))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.4)))
        .rounding(M3Shapes::SMALL)
        .inner_margin(Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                m3_vector_icon(ui, icon, 15.0, color);
                ui.label(RichText::new(text).size(12.5).color(theme.on_surface));
            });
        });
}

fn render_capture_mode(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config, status: Option<&StatusResponse>) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Video),
        t("What to record"),
        Some(t("Record everything, or only while a game is open.")),
        |ui| {
            let options = [("desktop".to_string(), t("Desktop")), ("game".to_string(), t("Games only"))];
            segmented(ui, theme, "capture_mode", &mut config.capture_mode, &options, 130.0);
            ui.add_space(8.0);
            let text = if config.is_game_mode() {
                if cfg!(windows) {
                    t("The replay buffer starts when a game opens and stops on the desktop, so clips only contain gameplay. Lapse records the monitor the game is on.")
                } else {
                    t("The replay buffer starts when a game opens and stops on the desktop, so clips only contain gameplay. Lapse records the monitor the game is on. Alt-tabbing inside a game is still recorded: on Wayland a running buffer can't be paused.")
                }
            } else {
                t("The replay buffer always runs, so you can also clip things on the desktop.")
            };
            ui.label(RichText::new(text).size(12.5).color(theme.on_surface_variant));
            ui.add_space(8.0);

            let (icon, color, line) = match status {
                Some(s) if s.waiting_for_game => (IconKind::Timer, theme.outline, t("Waiting for a game. The buffer is off.").to_string()),
                Some(s) => match &s.game {
                    Some(g) => (IconKind::Gamepad, theme.success, format!("{} {}", t("Game detected:"), g)),
                    None => (IconKind::Info, theme.primary, t("No game detected right now.").to_string()),
                },
                None => (IconKind::Warning, theme.outline, t("The Lapse service is not running.").to_string()),
            };
            note(ui, theme, icon, color, &line);
        },
    );
}

/// Running-state of each game in the list, refreshed every few seconds (it scans /proc).
fn running_flags(ui: &egui::Ui, games: &[GameEntry]) -> Vec<bool> {
    let id = egui::Id::new("lapse_games_running");
    let now = ui.input(|i| i.time);
    let cached: Option<(f64, Vec<String>, Vec<bool>)> = ui.data(|d| d.get_temp(id));
    let keys: Vec<String> = games.iter().map(|g| g.detect_word().to_string()).collect();
    if let Some((at, k, flags)) = cached {
        if now - at < 3.0 && k == keys {
            ui.ctx().request_repaint_after(Duration::from_secs(3));
            return flags;
        }
    }
    let flags: Vec<bool> = games.iter().map(crate::hardware::game_detector::is_running).collect();
    ui.data_mut(|d| d.insert_temp(id, (now, keys, flags.clone())));
    ui.ctx().request_repaint_after(Duration::from_secs(3));
    flags
}

fn render_game_list(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Gamepad),
        t("Your games"),
        Some(t("Add games Lapse doesn't know, and give any game its own recording settings. Clips go into a folder with the game's name.")),
        |ui| {
            m3_switch_labeled(
                ui,
                theme,
                &mut config.auto_game_profiles,
                t("Use each game's own settings automatically"),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(t("When a game from this list opens, recording switches to its FPS, resolution and bitrate, and back when it closes. Switching restarts the replay buffer, so the seconds recorded just before are not kept."))
                    .size(11.5)
                    .color(theme.outline),
            );
            ui.add_space(10.0);

            let running = running_flags(ui, &config.games);
            let mut remove = None;

            if config.games.is_empty() {
                ui.label(RichText::new(t("No games added yet. Open the game, then pick it below.")).size(12.5).color(theme.outline));
            } else {
                egui::Grid::new("games_grid").num_columns(6).spacing([8.0, 8.0]).show(ui, |ui| {
                    for h in ["Name", "FPS", "Resolution", "Bitrate", "", ""] {
                        ui.label(RichText::new(t(h)).size(11.5).strong().color(theme.on_surface_variant));
                    }
                    ui.end_row();
                    for (i, game) in config.games.iter_mut().enumerate() {
                        let name = m3_text_field(ui, theme, &mut game.name, t("Game name"), 240.0);
                        if !game.matcher.trim().is_empty() {
                            name.on_hover_text(format!("{} {}", t("Found by:"), game.matcher));
                        }

                        ui.add_enabled_ui(config.auto_game_profiles, |ui| {
                            let fps_opts = NumberOpts { suffix: " FPS", speed: 0.5, width: 90.0, zero_text: Some(t("Default")), ..Default::default() };
                            m3_number_field_u32(ui, theme, ("game_fps", i), &mut game.fps, 0..=500, fps_opts);
                        });

                        ui.add_enabled_ui(config.auto_game_profiles, |ui| {
                            let res_label = if game.resolution.is_empty() {
                                t("Default").to_string()
                            } else {
                                game.resolution.clone()
                            };
                            m3_dropdown_sized(ui, theme, ("game_res", i), res_label, 120.0, 28.0, |ui| {
                                ui.selectable_value(&mut game.resolution, String::new(), t("Default"));
                                for res in ["3840x2160", "2560x1440", "1920x1080", "1600x900", "1280x720"] {
                                    ui.selectable_value(&mut game.resolution, res.to_string(), res);
                                }
                            });
                        });

                        ui.add_enabled_ui(config.auto_game_profiles, |ui| {
                            let mut mbps = game.bitrate_kbps as f64 / 1000.0;
                            let rate_opts = NumberOpts { suffix: " Mbps", speed: 0.1, width: 100.0, zero_text: Some(t("Default")), ..Default::default() };
                            if m3_number_field(ui, theme, ("game_mbps", i), &mut mbps, 0.0..=250.0, rate_opts) {
                                game.bitrate_kbps = (mbps * 1000.0).round() as u32;
                            }
                        });

                        let (color, label) = match (game.detect_word().is_empty(), running.get(i).copied().unwrap_or(false)) {
                            (true, _) => (theme.outline, t("No name")),
                            (false, true) => (theme.success, t("Running")),
                            (false, false) => (theme.outline, t("Not running")),
                        };
                        ui.label(RichText::new(label).size(11.5).color(color));
                        if m3_small_button_colored(ui, theme, t("Remove"), theme.error).clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
            }
            if let Some(i) = remove {
                config.games.remove(i);
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if let Some(pick) = pick_running_app(ui, theme, &config.games) {
                    let entry = match pick {
                        Some(app) => GameEntry { name: app.name, matcher: app.matcher, ..Default::default() },
                        None => GameEntry::default(),
                    };
                    config.games.push(entry);
                }
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(t("Open the game, then pick it from the list. Default FPS, resolution and bitrate use the Recording settings."))
                    .size(12.0)
                    .color(theme.on_surface_variant),
            );
        },
    );
}

/// Menu of programs running right now (anything drawing with the GPU): open the game,
/// pick it here. `Some(None)` = add a row to type a name.
fn pick_running_app(ui: &mut egui::Ui, theme: &MonetTheme, games: &[GameEntry]) -> Option<Option<RunningApp>> {
    let known = |a: &RunningApp| games.iter().any(|g| g.name.eq_ignore_ascii_case(&a.name));
    let mut picked = None;
    m3_dropdown(ui, theme, "pick_running_game", format!("+  {}", t("Add game")), 260.0, |ui| {
        // Only while the menu is open, and again every 3 seconds.
        let id = egui::Id::new("running_apps_list");
        let now = ui.input(|i| i.time);
        let cached: Option<(f64, Vec<RunningApp>)> = ui.data(|d| d.get_temp(id));
        let apps = match cached {
            Some((at, apps)) if now - at < 3.0 => apps,
            _ => {
                let apps = running_apps();
                ui.data_mut(|d| d.insert_temp(id, (now, apps.clone())));
                apps
            }
        };
        ui.label(RichText::new(t("Open right now")).size(11.0).strong().color(theme.on_surface_variant));
        if apps.is_empty() {
            ui.label(RichText::new(t("Nothing is running. Open the game, then come back here.")).size(12.0).color(theme.outline));
        }
        for app in &apps {
            let label = if known(app) { format!("{}  ({})", app.name, t("added")) } else { app.name.clone() };
            if ui.add_enabled(!known(app), egui::SelectableLabel::new(false, label)).clicked() {
                picked = Some(Some(app.clone()));
            }
        }
        ui.separator();
        if ui.selectable_label(false, RichText::new(t("Type a name instead")).color(theme.primary)).clicked() {
            picked = Some(None);
        }
    });
    picked
}

/// Texts and options that differ between the auto clip games.
struct AutoClipCard {
    game: Game,
    title: &'static str,
    description: &'static str,
    kills_title: &'static str,
    kills_description: &'static str,
    options: [(u32, &'static str); 4],
    /// What "Before the first kill" means for this game's clips.
    length_description: &'static str,
    /// Extra setup the game needs besides the cfg file.
    note: Option<&'static str>,
}

const CS2_CARD: AutoClipCard = AutoClipCard {
    game: Game::Cs2,
    title: "Counter-Strike 2 auto clips",
    description: "Saves a clip by itself when you get several kills in one round.",
    kills_title: "Kills in a round",
    kills_description: "Clip when you reach this many kills in one round.",
    options: [(2, "2"), (3, "3"), (4, "4"), (5, "Ace")],
    length_description: "The clip starts this much before your first kill and runs until the next round begins, at least 8 seconds past your last kill.",
    note: None,
};

const DOTA_CARD: AutoClipCard = AutoClipCard {
    game: Game::Dota2,
    title: "Dota 2 auto clips",
    description: "Saves a clip by itself for double kills, triple kills, ultra kills and rampages.",
    kills_title: "Multi-kill",
    kills_description: "Clip from this multi-kill and up.",
    options: [(2, "Double"), (3, "Triple"), (4, "Ultra"), (5, "Rampage")],
    length_description: "The clip starts this much before the first kill of the streak and runs until the fight is over.",
    note: Some("Dota 2 also needs the launch option -gamestateintegration (Steam > Dota 2 > Properties > Launch options)."),
};

const LOL_CARD: AutoClipCard = AutoClipCard {
    game: Game::Lol,
    title: "League of Legends auto clips",
    description: "Saves a clip by itself for double kills, triple kills, quadra kills, penta kills and aces.",
    kills_title: "Multi-kill",
    kills_description: "Clip from this multi-kill and up.",
    options: [(2, "Double"), (3, "Triple"), (4, "Quadra"), (5, "Penta")],
    length_description: "The clip starts this much before the first kill of the streak and runs until the fight is over.",
    note: None,
};

fn render_auto_clips(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config, status: Option<&StatusResponse>) {
    let cs2_age = status.and_then(|s| s.cs2_data_age);
    let dota_age = status.and_then(|s| s.dota_data_age);
    let lol_age = status.and_then(|s| s.lol_data_age);
    render_auto_clip(ui, theme, &CS2_CARD, &mut config.cs2_auto_clip, &mut config.cs2_min_kills, &mut config.cs2_lead_in, cs2_age, status);
    ui.add_space(14.0);
    render_auto_clip(ui, theme, &DOTA_CARD, &mut config.dota_auto_clip, &mut config.dota_min_kills, &mut config.dota_lead_in, dota_age, status);
    ui.add_space(14.0);
    render_auto_clip(ui, theme, &LOL_CARD, &mut config.lol_auto_clip, &mut config.lol_min_kills, &mut config.lol_lead_in, lol_age, status);
}

fn render_auto_clip(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    card: &AutoClipCard,
    enabled: &mut bool,
    min_kills: &mut u32,
    lead_in: &mut u32,
    data_age: Option<u64>,
    status: Option<&StatusResponse>,
) {
    let game = card.game;
    m3_section_card(ui, theme, Some(IconKind::Bolt), t(card.title), Some(t(card.description)), |ui| {
        let auto_desc = if game == Game::Lol {
            t("Uses the game's official Live Client Data API. No cheats, nothing is injected.")
        } else {
            t("Uses the game's official Game State Integration. No cheats, nothing is injected.")
        };
        m3_setting_row(ui, theme, t("Automatic clips"), auto_desc, |ui| {
            m3_switch(ui, theme, enabled);
        });
        if !*enabled {
            return;
        }
        m3_setting_row(ui, theme, t(card.kills_title), t(card.kills_description), |ui| {
            let options: Vec<(u32, &str)> = card.options.iter().map(|(k, label)| (*k, t(label))).collect();
            segmented(ui, theme, &format!("auto_clip_min_{:?}", game), min_kills, &options, 70.0);
        });
        m3_setting_row(ui, theme, t("Before the first kill"), t(card.length_description), |ui| {
            let options = [(5u32, "5 s"), (8u32, "8 s"), (15u32, "15 s"), (30u32, "30 s")];
            segmented(ui, theme, &format!("auto_clip_lead_{:?}", game), lead_in, &options, 62.0);
        });
        if let Some(note) = card.note {
            note_line(ui, theme, t(note));
        }

        let msg_id = egui::Id::new(("lapse_auto_clip_msg", game.name()));
        ui.add_space(4.0);
        if game == Game::Lol {
            let in_game = data_age.map(|age| age <= 10).unwrap_or(false)
                || status.and_then(|s| s.game.as_deref()) == Some(game.name())
                || crate::autoclip::lol::is_lol_running();
            ui.horizontal(|ui| {
                ui.set_min_height(36.0);
                let (icon, color, text) = if in_game {
                    (IconKind::Check, theme.success, t("In game"))
                } else {
                    (IconKind::Check, theme.on_surface_variant, t("Ready"))
                };
                m3_vector_icon(ui, icon, 16.0, color);
                ui.label(RichText::new(text).size(12.5).color(theme.on_surface));
            });
        } else {
            let installed = game.is_installed();
            ui.horizontal(|ui| {
                ui.set_min_height(36.0);
                if installed {
                    // Games only read the integration file when they start, so check that data really arrives.
                    let receiving = data_age.map(|age| age <= 60).unwrap_or(false);
                    let open = status.and_then(|s| s.game.as_deref()) == Some(game.name());
                    let (icon, color, text) = if receiving {
                        (IconKind::Check, theme.success, t("Receiving live data. Clips will save automatically.").to_string())
                    } else if open {
                        (IconKind::Warning, theme.error, format!("{} {}", game.name(), t("is open but sends no data. Close it and start it again to load the integration.")))
                    } else {
                        (IconKind::Check, theme.on_surface_variant, t("Ready. Start the game and play; clips save by themselves.").to_string())
                    };
                    m3_vector_icon(ui, icon, 16.0, color);
                    ui.label(RichText::new(text).size(12.5).color(theme.on_surface));
                } else if game.cfg_dir().is_some() {
                    if m3_tonal_button(ui, theme, &format!("{} {}", t("Connect"), game.name())).clicked() {
                        let msg = match game.install() {
                            Ok(_) => t("Connected. Restart the game if it is open.").to_string(),
                            Err(e) => format!("{}: {}", t("Could not connect"), e),
                        };
                        ui.data_mut(|d| d.insert_temp(msg_id, msg));
                    }
                    ui.label(RichText::new(t("Adds a small config file to the game folder.")).size(12.0).color(theme.on_surface_variant));
                } else {
                    m3_vector_icon(ui, IconKind::Warning, 16.0, theme.outline);
                    ui.label(RichText::new(format!("{} {}", game.name(), t("was not found in your Steam libraries."))).size(12.5).color(theme.outline));
                }
            });
            if installed && m3_small_button(ui, theme, t("Disconnect")).clicked() {
                game.uninstall();
            }
            if let Some(msg) = ui.data(|d| d.get_temp::<String>(msg_id)) {
                ui.label(RichText::new(msg).size(12.0).color(theme.primary));
            }
        }
    });
}

fn note_line(ui: &mut egui::Ui, theme: &MonetTheme, text: &str) {
    ui.horizontal_wrapped(|ui| {
        m3_vector_icon(ui, IconKind::Info, 14.0, theme.primary);
        ui.label(RichText::new(text).size(12.0).color(theme.on_surface_variant));
    });
}
