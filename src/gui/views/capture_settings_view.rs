//! Settings cards added in 2.1: quality presets, replay buffer storage, audio tracks,
//! storage limit, hotkey backend and sharing privacy.

use eframe::egui::{self, Margin, RichText, Stroke};

use crate::config::{estimated_buffer_mb, total_ram_mb, Config};
use crate::gui::file_pick;
use crate::gui::components::number_field::{m3_number_field, m3_number_field_u32, NumberOpts};
use crate::gui::components::button::{m3_small_button, m3_small_button_colored};
use crate::gui::components::chip::m3_chip;
use crate::gui::components::card::m3_section_card;
use crate::gui::components::icon::{m3_vector_icon, IconKind};
use crate::gui::components::input::m3_setting_row;
use crate::gui::components::motion::m3_switch;
use crate::gui::components::segmented::segmented;
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};
use crate::hardware::monitor::MonitorDevice;

/// (id, label, quality, kbps, fps)
const PRESETS: &[(&str, &str, &str, u32, u32)] = &[
    ("light", "Light", "low", 8000, 30),
    ("balanced", "Balanced", "medium", 15000, 60),
    ("high", "High", "high", 25000, 60),
    ("ultra", "Ultra", "ultra", 50000, 60),
];

pub fn current_preset(config: &Config) -> &'static str {
    PRESETS
        .iter()
        .find(|p| p.2 == config.quality && p.3 == config.bitrate_kbps && p.4 == config.fps)
        .map(|p| p.0)
        .unwrap_or("custom")
}

pub fn apply_preset(config: &mut Config, id: &str) {
    if let Some(p) = PRESETS.iter().find(|p| p.0 == id) {
        config.quality = p.2.into();
        config.bitrate_kbps = p.3;
        config.fps = p.4;
    }
}

pub fn preset_picker(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config, id: &str) {
    let mut chosen = current_preset(config).to_string();
    let options: Vec<(String, &str)> = PRESETS
        .iter()
        .map(|p| (p.0.to_string(), t(p.1)))
        .chain(std::iter::once(("custom".to_string(), t("Custom"))))
        .collect();
    if segmented(ui, theme, id, &mut chosen, &options, 86.0).is_some() && chosen != "custom" {
        apply_preset(config, &chosen);
    }
}

fn capture_resolution(config: &Config, monitors: &[MonitorDevice]) -> (u32, u32) {
    let parse = |s: &str| {
        let (w, h) = s.split_once('x')?;
        Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
    };
    if config.resolution != "Native" {
        if let Some(r) = parse(&config.resolution) {
            return r;
        }
    }
    monitors
        .iter()
        .find(|m| m.name == config.monitor)
        .or_else(|| monitors.first())
        .and_then(|m| parse(&m.resolution))
        .unwrap_or((1920, 1080))
}

pub fn render_quality_and_buffer(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    config: &mut Config,
    monitors: &[MonitorDevice],
) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Bolt),
        t("Quick Quality"),
        Some(t("One click presets. Fine-tune everything below if you want.")),
        |ui| {
            preset_picker(ui, theme, config, "settings_preset");
            ui.add_space(14.0);

            m3_setting_row(
                ui,
                theme,
                t("Replay buffer storage"),
                t("RAM is fastest. Disk saves memory for long or 4K buffers."),
                |ui| {
                    let options = [("ram".to_string(), t("RAM")), ("disk".to_string(), t("Disk"))];
                    segmented(ui, theme, "replay_storage", &mut config.replay_storage, &options, 80.0);
                },
            );

            let (w, h) = capture_resolution(config, monitors);
            let mb = estimated_buffer_mb(config, w, h);
            let ram = total_ram_mb();
            let heavy = config.replay_storage == "ram" && ram > 0.0 && mb > ram * 0.25;
            let (color, icon) = if heavy {
                (theme.error, IconKind::Warning)
            } else {
                (theme.primary, IconKind::Info)
            };
            let where_ = if config.replay_storage == "ram" { t("of RAM") } else { t("of disk space") };
            let size = if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{:.0} MB", mb) };
            let mut text = format!(
                "{} {} {} ({}x{}, {} fps, {} s)",
                t("Replay buffer uses at most"),
                size,
                where_,
                w,
                h,
                config.fps,
                config.replay_seconds
            );
            if heavy {
                text.push_str(&format!(
                    " — {} {:.0}%. {}",
                    t("that is"),
                    mb / ram * 100.0,
                    t("Consider Disk storage or a shorter buffer.")
                ));
            }
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
        },
    );
}

/// Cached `gpu-screen-recorder --list-application-audio` output.
fn app_audio_list(ui: &egui::Ui, refresh: bool) -> Vec<String> {
    let id = egui::Id::new("lapse_app_audio_list");
    if !refresh {
        if let Some(list) = ui.data(|d| d.get_temp::<Vec<String>>(id)) {
            return list;
        }
    }
    let list: Vec<String> = std::process::Command::new("gpu-screen-recorder")
        .arg("--list-application-audio")
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && !l.contains("[lapse]"))
                .collect()
        })
        .unwrap_or_default();
    ui.data_mut(|d| d.insert_temp(id, list.clone()));
    list
}

pub fn render_audio_tracks(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Audio),
        t("Audio Tracks & Apps"),
        Some(t("Keep your voice editable and record only the apps you want.")),
        |ui| {
            m3_setting_row(
                ui,
                theme,
                t("Separate tracks"),
                t("Desktop audio and microphone are saved as two audio tracks, so you can mute your voice later."),
                |ui| {
                    m3_switch(ui, theme, &mut config.separate_audio_tracks);
                },
            );
            ui.separator();

            let mut refresh = false;
            m3_setting_row(
                ui,
                theme,
                t("Record only these apps"),
                t("Leave empty to record all desktop audio. Tip: pick your game and Discord."),
                |ui| {
                    if m3_small_button(ui, theme, t("Refresh")).clicked() {
                        refresh = true;
                    }
                },
            );
            let apps = app_audio_list(ui, refresh);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                // Show running apps plus any configured app that isn't running right now.
                let mut all = apps.clone();
                for a in &config.audio_apps {
                    if !all.contains(a) {
                        all.push(a.clone());
                    }
                }
                if all.is_empty() {
                    ui.label(RichText::new(t("No apps are playing audio right now.")).size(12.0).color(theme.outline));
                }
                for app in all {
                    let mut on = config.audio_apps.contains(&app);
                    let resp = m3_chip(ui, theme, &app, on);
                    if resp.clicked() {
                        on = !on;
                        if on {
                            config.audio_apps.push(app.clone());
                        } else {
                            config.audio_apps.retain(|a| a != &app);
                        }
                    }
                }
            });
        },
    );
}

pub fn render_save_behavior(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Save),
        t("When a clip is saved"),
        None,
        |ui| {
            m3_setting_row(ui, theme, t("Play a sound"), t("A short shutter sound, so you know the clip was saved while playing."), |ui| {
                m3_switch(ui, theme, &mut config.save_sound);
            });
            if config.save_sound {
                render_sound_file(ui, theme, config);
            }
            ui.separator();
            m3_setting_row(ui, theme, t("Show a notification"), t("Shows the clip name and the game folder it went into."), |ui| {
                m3_switch(ui, theme, &mut config.save_notification);
            });
            ui.separator();
            m3_setting_row(
                ui,
                theme,
                t("Clear the buffer after saving"),
                t("Off: two saves close together share footage, nothing is lost. On: each clip only has what happened since the last save."),
                |ui| {
                    m3_switch(ui, theme, &mut config.restart_replay_on_save);
                },
            );
        },
    );
}

pub fn render_hdr(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    m3_section_card(ui, theme, Some(IconKind::Video), t("HDR"), None, |ui| {
        m3_setting_row(
            ui,
            theme,
            t("Record in HDR"),
            t("10-bit HEVC or AV1. Only useful when HDR is turned on for your monitor; HDR clips look washed out on SDR screens and many sites."),
            |ui| {
                m3_switch(ui, theme, &mut config.hdr);
            },
        );
    });
}

pub fn render_discord(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Share),
        t("Discord"),
        Some(t("Show what you play on your profile, and send clips straight to a channel.")),
        |ui| {
            m3_setting_row(ui, theme, t("Show on my profile"), t("While a game is open your Discord status reads \"<game> with Lapse\", with the game's cover. Needs the Discord app running on this PC."), |ui| {
                m3_switch(ui, theme, &mut config.discord_rpc);
            });
            if config.discord_rpc {
                m3_setting_row(ui, theme, t("\"Get Lapse\" button"), t("Others see a button on your status that opens the Lapse GitHub page."), |ui| {
                    m3_switch(ui, theme, &mut config.discord_rpc_button);
                });
            }
            ui.separator();
            let show_id = egui::Id::new("lapse_webhook_visible");
            let mut show: bool = ui.data(|d| d.get_temp(show_id)).unwrap_or(false);
            m3_setting_row(ui, theme, t("Webhook link"), t("For sending clips. In Discord: Channel settings > Integrations > Webhooks > New webhook > Copy URL. Kept only in your config file."), |ui| {
                if m3_small_button(ui, theme, if show { t("Hide") } else { t("Show") }).clicked() {
                    show = !show;
                }
                ui.add(
                    egui::TextEdit::singleline(&mut config.discord_webhook)
                        .password(!show)
                        .hint_text("https://discord.com/api/webhooks/...")
                        .desired_width(300.0),
                );
            });
            ui.data_mut(|d| d.insert_temp(show_id, show));
            let url = config.discord_webhook.trim();
            if !url.is_empty() && !crate::discord::is_webhook(url) {
                ui.label(RichText::new(t("This doesn't look like a Discord webhook link.")).size(12.0).color(theme.error));
            } else if !url.is_empty() {
                ui.label(RichText::new(t("Share links are posted to this channel too. \"Send to Discord\" appears after exporting a clip in the trimmer.")).size(12.0).color(theme.on_surface_variant));
            }
        },
    );
}

const SOUND_DIALOG: &str = "save_sound";

/// Which sound plays: the shutter or a file the user picked (Reset goes back).
fn render_sound_file(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    let error_id = egui::Id::new("lapse_sound_error");
    let custom = !config.save_sound_path.trim().is_empty();
    let name = if custom {
        std::path::Path::new(&config.save_sound_path).file_name().unwrap_or_default().to_string_lossy().to_string()
    } else {
        t("Built-in shutter").to_string()
    };
    m3_setting_row(ui, theme, t("Sound"), &name, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if custom && m3_small_button_colored(ui, theme, t("Reset"), theme.primary).on_hover_text(t("Back to the built-in shutter")).clicked() {
            config.save_sound_path.clear();
            ui.data_mut(|d| d.remove::<String>(error_id));
        }
        if m3_small_button(ui, theme, t("Test")).clicked() {
            crate::sound::play_save_sound(&config.save_sound_path);
        }
        let picking = file_pick::is_open(SOUND_DIALOG);
        if ui.add_enabled_ui(!picking, |ui| m3_small_button(ui, theme, t("Choose file..."))).inner.clicked() {
            let current = config.save_sound_path.clone();
            file_pick::open(ui.ctx(), SOUND_DIALOG, move || pick_sound_file(&current));
        }
        if let Some(path) = file_pick::take(SOUND_DIALOG) {
            match crate::sound::check(&path) {
                Ok(()) => {
                    config.save_sound_path = path.to_string_lossy().to_string();
                    ui.data_mut(|d| d.remove::<String>(error_id));
                    crate::sound::play_save_sound(&config.save_sound_path);
                }
                Err(e) => ui.data_mut(|d| d.insert_temp(error_id, e)),
            }
        }
    });
    if let Some(err) = ui.data(|d| d.get_temp::<String>(error_id)) {
        ui.label(RichText::new(format!("{} {}", t("Can't play this file:"), err)).size(12.0).color(theme.error));
    }
    if custom {
        ui.label(RichText::new(t("Long files are cut after 4 seconds.")).size(11.5).color(theme.on_surface_variant));
    }
}

/// File dialog for audio only, titled for the job, opening next to the current
/// sound, else in Music, else in the system sounds.
fn pick_sound_file(current: &str) -> Option<std::path::PathBuf> {
    let current = std::path::Path::new(current);
    let start = current
        .parent()
        .filter(|p| !current.as_os_str().is_empty() && p.is_dir())
        .map(|p| p.to_path_buf())
        .or_else(|| directories::UserDirs::new().and_then(|u| u.audio_dir().map(|d| d.to_path_buf())))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| std::path::PathBuf::from("/usr/share/sounds"));
    // Globs are case-sensitive in some pickers: list both cases.
    let exts: Vec<String> = crate::sound::EXTENSIONS.iter().flat_map(|e| [e.to_string(), e.to_uppercase()]).collect();
    rfd::FileDialog::new()
        .set_title(t("Choose a sound for saved clips"))
        .set_directory(start)
        .add_filter(format!("{} (ogg, wav, mp3, flac)", t("Audio")), &exts)
        .pick_file()
}

pub fn render_storage_limit(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Trash),
        t("Storage Limit"),
        Some(t("Automatically delete the oldest clips when the clip folder gets too big.")),
        |ui| {
            m3_setting_row(ui, theme, t("Enable storage limit"), t("Off by default. Starred clips are never deleted. Deleted clips cannot be recovered."), |ui| {
                m3_switch(ui, theme, &mut config.cleanup_enabled);
            });
            if config.cleanup_enabled {
                m3_setting_row(ui, theme, t("Maximum size"), "", |ui| {
                    let opts = NumberOpts { suffix: " GB", speed: 1.0, width: 84.0, ..Default::default() };
                    m3_number_field_u32(ui, theme, "cleanup_max_gb", &mut config.cleanup_max_gb, 1..=10000, opts);
                });
                let would_delete = crate::cleanup::plan(&config.save_path, config.cleanup_max_gb as u64 * 1024 * 1024 * 1024);
                let text = if would_delete.is_empty() {
                    t("Your clips are under the limit; nothing would be deleted.").to_string()
                } else {
                    format!("{} {} {}", t("With this limit,"), would_delete.len(), t("oldest clip(s) would be deleted."))
                };
                let color = if would_delete.is_empty() { theme.on_surface_variant } else { theme.error };
                ui.label(RichText::new(text).size(12.5).color(color));
            }
        },
    );
}

pub fn render_hotkey_backend(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    m3_setting_row(
        ui,
        theme,
        t("Shortcut system"),
        t("Auto uses the Wayland shortcut portal (works in every app) and X11 as a fallback. GNOME/KDE ask once to confirm."),
        |ui| {
            let options = [
                ("auto".to_string(), t("Auto")),
                ("portal".to_string(), "Portal"),
                ("x11".to_string(), "X11"),
            ];
            segmented(ui, theme, "hotkey_backend", &mut config.hotkey_backend, &options, 70.0);
        },
    );
}

pub fn render_share_privacy(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Globe),
        t("Sharing & Privacy"),
        Some(t("Share links stream the clip from your PC through a tunnel.")),
        |ui| {
            m3_setting_row(
                ui,
                theme,
                t("Allow third-party SSH tunnels"),
                t("Fallback to localhost.run / serveo.net. Your clip passes through their servers."),
                |ui| {
                    m3_switch(ui, theme, &mut config.share_allow_ssh_fallback);
                },
            );
        },
    );
}

pub fn render_language(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    crate::gui::components::card::m3_card(ui, theme, |ui| {
        m3_setting_row(ui, theme, t("Language"), t("Language of the Lapse window."), |ui| {
            let options: Vec<(String, &str)> = crate::gui::i18n::LANGUAGES.iter().map(|(c, n)| (c.to_string(), *n)).collect();
            if segmented(ui, theme, "settings_language", &mut config.language, &options, 96.0).is_some() {
                crate::gui::i18n::set_language(&config.language);
            }
        });
    });
}
