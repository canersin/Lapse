use eframe::egui::{self, Align2, Color32, FontId, Margin, Rect, RichText, Stroke, Vec2};
use crate::gui::i18n::t;
use rfd::FileDialog;

use crate::config::Config;
use crate::gui::components::dropdown::{m3_dropdown, m3_dropdown_sized};
use crate::gui::components::motion::{ease_out, hover_t, lerp_color, m3_switch, pulse, FAST, MEDIUM};
use crate::hotkeys::HotkeyEvent;
use crate::hardware::audio::AudioDevice;
use crate::hardware::gpu::GpuDevice;
use crate::hardware::monitor::MonitorDevice;
use crate::ipc;

use crate::gui::components::button::{
    m3_filled_button_sized, m3_outlined_button, m3_small_button_colored, m3_tonal_button_sized,
};
use crate::gui::components::card::m3_section_card;
use crate::gui::components::chip::{m3_badge, m3_keycap};
use crate::gui::components::header::m3_page_header;
use crate::gui::components::icon::{m3_vector_icon, IconKind};
use crate::gui::components::input::{
    m3_bitrate_slider_input, m3_duration_slider_input, m3_fps_slider_input, m3_setting_row,
};
use crate::gui::components::text_field::{m3_text_field, TextFieldOpts};
use crate::gui::theme::{M3Shapes, M3Typography, MonetPaletteKind, MonetTheme};
use crate::gui::views::capture_settings_view as capture;
use crate::gui::views::hotkey_view::format_hotkey;
use crate::update::{check_for_updates, get_update_state, UpdateStatus};

pub struct SettingsHardwareContext<'a> {
    pub available_outputs: &'a [AudioDevice],
    pub available_inputs: &'a [AudioDevice],
    pub available_monitors: &'a [MonitorDevice],
    pub available_gpus: &'a [GpuDevice],
    pub status: Option<&'a ipc::StatusResponse>,
}

pub fn render_settings(
    config: &mut Config,
    hardware: SettingsHardwareContext<'_>,
    binding_hotkey: &mut Option<HotkeyEvent>,
    palette_kind: &mut MonetPaletteKind,
    theme: &MonetTheme,
    status_banner: &mut Option<(String, std::time::Instant)>,
    ui: &mut egui::Ui,
    on_command: impl Fn(ipc::Command),
) {
    let initial_config = config.clone();

    m3_page_header(
        ui,
        theme,
        t("Settings"),
        t("Tune hardware encoders, custom replay buffer durations, audio channels and autostart"),
        |ui| {
            if m3_filled_button_sized(ui, theme, t("Save & Apply"), Vec2::new(140.0, 36.0)).clicked() {
                save_and_reload(config, &on_command, status_banner);
            }
        },
    );

    if let Some((msg, time)) = status_banner {
        if time.elapsed().as_secs() < 4 {
            egui::Frame::none()
                .fill(theme.primary_container)
                .rounding(M3Shapes::MEDIUM)
                .inner_margin(Margin::symmetric(16.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        m3_vector_icon(ui, IconKind::Check, 16.0, theme.on_primary_container);
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(msg.as_str())
                                .color(theme.on_primary_container)
                                .strong(),
                        );
                    });
                });
            ui.add_space(12.0);
        }
    }

    let tab_id = egui::Id::new("m3_settings_tab_idx");
    let mut current_tab: usize = ui.data_mut(|d| d.get_temp(tab_id).unwrap_or(0));

    let tabs = [
        (IconKind::Video, "Recording"),
        (IconKind::Gamepad, "Games"),
        (IconKind::Audio, "Audio"),
        (IconKind::Keyboard, "Hotkeys"),
        (IconKind::Gear, "Services"),
        (IconKind::Palette, "Appearance"),
        (IconKind::Update, "Updates"),
        (IconKind::Chip, "Advanced"),
    ];

    egui::Frame::none()
        .fill(theme.surface_container_low)
        .stroke(Stroke::new(1.0_f32, theme.outline_variant))
        .rounding(M3Shapes::LARGE)
        .inner_margin(Margin::same(6.0))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.spacing_mut().item_spacing.y = 4.0;
                for (idx, (icon_kind, title)) in tabs.iter().enumerate() {
                    let is_active = current_tab == idx;
                    let (bg, fg, border) = if is_active {
                        (
                            theme.primary_container,
                            theme.on_primary_container,
                            Stroke::new(1.0_f32, theme.primary),
                        )
                    } else {
                        (
                            theme.surface_container,
                            theme.on_surface_variant,
                            Stroke::NONE,
                        )
                    };

                    let resp = egui::Frame::none()
                        .fill(bg)
                        .stroke(border)
                        .rounding(M3Shapes::FULL)
                        .inner_margin(Margin::symmetric(14.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                m3_vector_icon(ui, *icon_kind, 15.0, fg);
                                ui.add_space(5.0);
                                ui.label(RichText::new(t(title)).size(13.0).strong().color(fg));
                            });
                        });

                    if resp.response.interact(egui::Sense::click()).clicked() {
                        current_tab = idx;
                        ui.data_mut(|d| d.insert_temp(tab_id, idx));
                    }
                }
            });
        });

    ui.add_space(16.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            match current_tab {
                0 => {
                    capture::render_quality_and_buffer(ui, theme, config, hardware.available_monitors);
                    if !cfg!(windows) {
                        ui.add_space(14.0);
                        capture::render_hdr(ui, theme, config);
                    }
                    ui.add_space(14.0);
                    render_recording_tab(ui, theme, config, hardware);
                    ui.add_space(14.0);
                    capture::render_save_behavior(ui, theme, config);
                }
                1 => crate::gui::views::games_view::render_games_tab(ui, theme, config, hardware.status),
                2 => {
                    let status = hardware.status;
                    render_audio_tab(ui, theme, config, hardware);
                    if !cfg!(windows) {
                        ui.add_space(14.0);
                        capture::render_audio_tracks(ui, theme, config);
                    }
                    ui.add_space(14.0);
                    crate::gui::views::sync_view::render_audio_sync(ui, theme, config, status);
                }
                3 => {
                    render_hotkeys_tab(ui, theme, config, binding_hotkey);
                    ui.add_space(14.0);
                    render_voice_trigger_card(ui, theme, config);
                }
                4 => {
                    render_services_tab(ui, theme, config);
                    ui.add_space(14.0);
                    capture::render_storage_limit(ui, theme, config);
                    ui.add_space(14.0);
                    capture::render_discord(ui, theme, config);
                    ui.add_space(14.0);
                    render_streamable(ui, theme);
                }
                5 => {
                    capture::render_language(ui, theme, config);
                    ui.add_space(14.0);
                    render_appearance_tab(ui, theme, palette_kind);
                }
                6 => render_updates_tab(ui, theme, config),
                _ => render_advanced_tab(ui, theme, config, hardware),
            }
            let palette_name = format!("{:?}", palette_kind);
            if config.theme_palette != palette_name {
                config.theme_palette = palette_name;
            }

            ui.add_space(24.0);

            ui.vertical_centered(|ui| {
                if m3_filled_button_sized(
                    ui,
                    theme,
                    t("Save Settings & Reload Engine"),
                    Vec2::new(300.0, 44.0),
                )
                .clicked()
                {
                    save_and_reload(config, &on_command, status_banner);
                }
                ui.add_space(20.0);
            });
        });

    // Debounced autosave: applying a change restarts the recorder, so wait until the user
    // stops dragging/typing instead of restarting it on every slider tick.
    let pending_id = egui::Id::new("lapse_settings_pending_save");
    let now = ui.input(|i| i.time);
    if *config != initial_config {
        ui.data_mut(|d| d.insert_temp(pending_id, now));
    }
    if let Some(changed_at) = ui.data(|d| d.get_temp::<f64>(pending_id)) {
        let pointer_down = ui.input(|i| i.pointer.any_down());
        if now - changed_at > 0.8 && !pointer_down {
            ui.data_mut(|d| d.remove::<f64>(pending_id));
            let _ = config.save();
            Config::sync_autostart(config.autostart);
            on_command(ipc::Command::ReloadConfig);
            *status_banner = Some((
                t("Settings saved & applied").into(),
                std::time::Instant::now(),
            ));
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}

fn render_recording_tab(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    config: &mut Config,
    hardware: SettingsHardwareContext<'_>,
) {
    let subtitle = if cfg!(windows) {
        t("Hardware-accelerated video encoding via Media Foundation (H.264, H.265, AV1)")
    } else {
        t("Select GPU encoder codec (AV1, H.265/HEVC, H.264) according to your graphics card")
    };
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Video),
        t("Video Encoder & GPU Hardware Acceleration"),
        Some(subtitle),
        |ui| {
            if cfg!(windows) {
                #[cfg(windows)]
                let (hevc_supported, av1_supported) = (
                    crate::recorder_windows::capture::hevc_supported(),
                    crate::recorder_windows::capture::av1_supported(),
                );
                #[cfg(not(windows))]
                let (hevc_supported, av1_supported) = (false, false);

                egui::Frame::none()
                    .fill(theme.surface_container_highest)
                    .rounding(M3Shapes::MEDIUM)
                    .inner_margin(Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            m3_badge(ui, theme, t("Encoder"), Some(theme.primary));
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(t("Windows Media Foundation"))
                                        .size(13.0)
                                        .strong()
                                        .color(theme.on_surface),
                                );
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    m3_badge(ui, theme, t("H.264 HW Supported"), Some(theme.success));
                                    if hevc_supported {
                                        ui.add_space(4.0);
                                        m3_badge(
                                            ui,
                                            theme,
                                            t("HEVC / H.265 HW Supported"),
                                            Some(theme.success),
                                        );
                                    }
                                    if av1_supported {
                                        ui.add_space(4.0);
                                        m3_badge(
                                            ui,
                                            theme,
                                            "AV1 Next-Gen HW Supported",
                                            Some(Color32::from_rgb(0, 200, 255)),
                                        );
                                    }
                                });
                            });
                        });
                    });

                ui.add_space(10.0);

                m3_setting_row(
                    ui,
                    theme,
                    t("Video Encoder Codec"),
                    t("H.265 and AV1 make smaller files at the same quality; H.264 plays everywhere. Auto uses H.264."),
                    |ui| {
                        let codec_label = match config.video_codec.as_str() {
                            "auto" | "" => t("Auto (Recommended)"),
                            "hevc" => t("HEVC / H.265 (Efficient)"),
                            "av1" => t("AV1 (Next-Gen High Quality)"),
                            "h264" => t("H.264 (Standard Compatibility)"),
                            other => other,
                        }
                        .to_string();
                        m3_dropdown(ui, theme, "win_codec_selector", codec_label, 320.0, |ui| {
                            ui.selectable_value(&mut config.video_codec, "auto".into(), t("Auto (Recommended)"));
                            ui.selectable_value(&mut config.video_codec, "h264".into(), t("H.264 (Standard Compatibility)"));
                            let choices = [
                                ("hevc", t("HEVC / H.265 (Efficient)"), hevc_supported, t("Your graphics card has no hardware H.265 encoder")),
                                ("av1", t("AV1 (Next-Gen High Quality)"), av1_supported, t("Needs a graphics card with an AV1 encoder (RTX 40, RX 7000, Intel Arc or newer) and Windows 11 24H2")),
                            ];
                            for (value, label, supported, why_not) in choices {
                                if supported {
                                    ui.selectable_value(&mut config.video_codec, value.into(), label);
                                } else {
                                    ui.add_enabled(false, egui::SelectableLabel::new(config.video_codec == value, label))
                                        .on_disabled_hover_text(why_not);
                                }
                            }
                        });
                    },
                );

                #[cfg(windows)]
                if !crate::recorder_windows::capture::border_removable() {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(t("Windows 10 shows a yellow border around the screen while recording. It is not in your clips, and Windows 11 doesn't show it."))
                            .size(11.5)
                            .color(theme.on_surface_variant),
                    );
                }

                ui.separator();
            } else {
                let active_gpu = if config.gpu.eq_ignore_ascii_case("auto") {
                    hardware
                        .available_gpus
                        .iter()
                        .find(|g| g.is_primary)
                        .or_else(|| hardware.available_gpus.first())
                } else {
                    hardware
                        .available_gpus
                        .iter()
                        .find(|g| g.matches(&config.gpu))
                        .or_else(|| hardware.available_gpus.first())
                };
                let gpu_name = active_gpu
                    .map(|g| g.display_name())
                    .unwrap_or_else(|| "Generic Linux GPU".into());
                let supports_av1 = active_gpu.map(|g| g.supports_av1()).unwrap_or(false);
                let supports_hevc = active_gpu.map(|g| g.supports_hevc()).unwrap_or(true);

                egui::Frame::none()
                    .fill(theme.surface_container_highest)
                    .rounding(M3Shapes::MEDIUM)
                    .inner_margin(Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            m3_badge(ui, theme, t("GPU"), Some(theme.primary));
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(format!("{}: {}", t("Active GPU"), gpu_name))
                                        .size(13.0)
                                        .strong()
                                        .color(theme.on_surface),
                                );
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    m3_badge(ui, theme, t("H.264 HW Supported"), Some(theme.success));
                                    ui.add_space(4.0);
                                    if supports_hevc {
                                        m3_badge(
                                            ui,
                                            theme,
                                            "HEVC / H.265 HW Supported",
                                            Some(theme.success),
                                        );
                                        ui.add_space(4.0);
                                    }
                                    if supports_av1 {
                                        m3_badge(
                                            ui,
                                            theme,
                                            "AV1 Next-Gen HW Supported",
                                            Some(Color32::from_rgb(0, 200, 255)),
                                        );
                                    }
                                });
                            });
                        });
                    });

                ui.add_space(10.0);

                if !config.dual_gpu {
                    m3_setting_row(
                        ui,
                        theme,
                        t("Target Graphics Card (GPU)"),
                        t("Select which graphics card handles hardware encoding and replay buffer capture"),
                        |ui| {
                            let current_text = if config.gpu.eq_ignore_ascii_case("auto") {
                                if let Some(gpu) = active_gpu {
                                    format!("Auto (Active: {})", gpu.display_name())
                                } else {
                                    "Auto (Recommended)".to_string()
                                }
                            } else if let Some(gpu) = active_gpu {
                                gpu.display_name()
                            } else {
                                config.gpu.clone()
                            };

                            m3_dropdown(ui, theme, "gpu_selector", current_text, 320.0, |ui| {
                                ui.selectable_value(
                                    &mut config.gpu,
                                    "Auto".into(),
                                    t("Auto (Recommended - Active Display GPU)"),
                                );
                                for g in hardware.available_gpus {
                                    ui.selectable_value(
                                        &mut config.gpu,
                                        g.pci_id.clone(),
                                        g.display_name(),
                                    );
                                }
                            });
                        },
                    );

                    ui.separator();
                }

                m3_setting_row(
                    ui,
                    theme,
                    t("Video Encoder Codec"),
                    t("AV1 offers best quality at smaller size; HEVC is widely compatible; H.264 is legacy"),
                    |ui| {
                        let codec_label = match config.video_codec.as_str() {
                            "auto" | "" => t("Auto (Recommended)"),
                            "av1" => "AV1",
                            "hevc" => "HEVC / H.265",
                            "h264" => "H.264",
                            "hevc_hdr" => "HEVC HDR",
                            "av1_hdr" => "AV1 HDR",
                            "hevc_10bit" => "HEVC 10-bit",
                            "av1_10bit" => "AV1 10-bit",
                            other => other,
                        }
                        .to_string();
                        m3_dropdown(ui, theme, "codec_selector", codec_label, 320.0, |ui| {
                            ui.selectable_value(&mut config.video_codec, "auto".into(), t("Auto (Recommended)"));
                            ui.selectable_value(&mut config.video_codec, "av1".into(), t("AV1 (Next-Gen High Quality)"));
                            ui.selectable_value(&mut config.video_codec, "hevc".into(), t("HEVC / H.265 (Efficient)"));
                            ui.selectable_value(&mut config.video_codec, "h264".into(), t("H.264 (Standard Compatibility)"));
                            ui.separator();
                            ui.selectable_value(&mut config.video_codec, "hevc_hdr".into(), t("HEVC HDR (needs HDR monitor)"));
                            ui.selectable_value(&mut config.video_codec, "av1_hdr".into(), t("AV1 HDR (needs HDR monitor)"));
                            ui.selectable_value(&mut config.video_codec, "hevc_10bit".into(), t("HEVC 10-bit (less banding)"));
                            ui.selectable_value(&mut config.video_codec, "av1_10bit".into(), t("AV1 10-bit (less banding)"));
                        });
                    },
                );

                ui.separator();
            }

            m3_setting_row(
                ui,
                theme,
                t("Encoding Quality & Bitrate Profile"),
                t("Select a standard bitrate profile or custom CBR / constant-quality (QP) mode"),
                |ui| {
                    let preset_label = t(match config.quality.as_str() {
                        "lossless_80" => "Extreme 4K60 (80 Mbps - ~600 MB/min)",
                        "ultra" => "Ultra 4K/1440p (50 Mbps - ~375 MB/min)",
                        "high" => "High Default (25 Mbps - ~188 MB/min)",
                        "medium" => "Medium Balanced (15 Mbps - ~112 MB/min)",
                        "low" => "Compact Share (8 Mbps - ~60 MB/min)",
                        "qp_ultra" => "Variable Bitrate QP (Ultra Constant Quality)",
                        "qp_very_high" => "Variable Bitrate QP (Very High Constant Quality)",
                        _ => "Custom Bitrate (Manual Mbps / kbps)",
                    });

                    let prev_q = config.quality.clone();
                    m3_dropdown(ui, theme, "quality_selector", preset_label, 320.0, |ui| {
                            if ui
                                .selectable_value(
                                    &mut config.quality,
                                    "lossless_80".into(),
                                    t("Extreme 4K60 (80 Mbps - ~600 MB/min)"),
                                )
                                .clicked()
                            {
                                config.bitrate_kbps = 80000;
                            }
                            if ui
                                .selectable_value(
                                    &mut config.quality,
                                    "ultra".into(),
                                    t("Ultra 4K/1440p (50 Mbps - ~375 MB/min)"),
                                )
                                .clicked()
                            {
                                config.bitrate_kbps = 50000;
                            }
                            if ui
                                .selectable_value(
                                    &mut config.quality,
                                    "high".into(),
                                    t("High Default (25 Mbps - ~188 MB/min)"),
                                )
                                .clicked()
                            {
                                config.bitrate_kbps = 25000;
                            }
                            if ui
                                .selectable_value(
                                    &mut config.quality,
                                    "medium".into(),
                                    t("Medium Balanced (15 Mbps - ~112 MB/min)"),
                                )
                                .clicked()
                            {
                                config.bitrate_kbps = 15000;
                            }
                            if ui
                                .selectable_value(
                                    &mut config.quality,
                                    "low".into(),
                                    t("Compact Share (8 Mbps - ~60 MB/min)"),
                                )
                                .clicked()
                            {
                                config.bitrate_kbps = 8000;
                            }
                            ui.separator();
                            ui.selectable_value(
                                &mut config.quality,
                                "custom".into(),
                                t("Custom Bitrate (Manual Mbps / kbps)"),
                            );
                            ui.selectable_value(
                                &mut config.quality,
                                "qp_ultra".into(),
                                t("Variable Bitrate QP (Ultra Constant Quality)"),
                            );
                            ui.selectable_value(
                                &mut config.quality,
                                "qp_very_high".into(),
                                t("Variable Bitrate QP (Very High Constant Quality)"),
                            );
                        });
                    let _ = prev_q;
                },
            );

            ui.separator();

            m3_setting_row(
                ui,
                theme,
                t("Maximum Video Bitrate"),
                t("The encoder never goes above this. Calm scenes use less, so clips are usually smaller."),
                |ui| {
                    if m3_bitrate_slider_input(
                        ui,
                        theme,
                        &mut config.bitrate_kbps,
                        config.replay_seconds,
                        crate::clip_index::index().recent_average_kbps(),
                    ) {
                        config.quality = match config.bitrate_kbps {
                            80000 => "lossless_80".into(),
                            50000 => "ultra".into(),
                            25000 => "high".into(),
                            15000 => "medium".into(),
                            8000 => "low".into(),
                            _ => "custom".into(),
                        };
                    }
                },
            );
        },
    );

    ui.add_space(16.0);

    m3_section_card(
        ui,
        theme,
        Some(IconKind::Timer),
        t("Buffer Duration & Capture Fidelity"),
        Some(t("Set custom duration in seconds, framerate, and target display")),
        |ui| {
            m3_setting_row(
                ui,
                theme,
                t("Buffer Duration"),
                t("Drag slider or type exact seconds (e.g. 600 for 10 min, 3600 for 1 hour)"),
                |ui| {
                    m3_duration_slider_input(ui, theme, &mut config.replay_seconds);
                },
            );

            ui.separator();

            m3_setting_row(
                ui,
                theme,
                t("Frame Rate"),
                t("Drag slider or type custom FPS (up to 500 FPS)"),
                |ui| {
                    m3_fps_slider_input(ui, theme, &mut config.fps);
                },
            );

            ui.separator();

            m3_setting_row(
                ui,
                theme,
                t("Target Display"),
                t("Screen to capture (Auto captures active screen from target GPU)"),
                |ui| {
                    let current_text = if config.monitor.is_empty()
                        || config.monitor.to_lowercase() == "focused"
                        || config.monitor.to_lowercase() == "auto"
                        || config.monitor.to_lowercase() == "screen"
                    {
                        "Auto (Primary Active Display)".to_string()
                    } else if let Some(m) = hardware
                        .available_monitors
                        .iter()
                        .find(|m| m.name == config.monitor)
                    {
                        format!("{} ({}) - {}", m.name, m.card_name, m.resolution)
                    } else {
                        config.monitor.clone()
                    };

                    m3_dropdown(ui, theme, "monitor_selector", current_text, 320.0, |ui| {
                            ui.selectable_value(
                                &mut config.monitor,
                                "focused".into(),
                                t("Auto (Primary Active Display)"),
                            );
                            for m in hardware.available_monitors {
                                ui.selectable_value(
                                    &mut config.monitor,
                                    m.name.clone(),
                                    format!("{} ({}) - {}", m.name, m.card_name, m.resolution),
                                );
                            }
                        });
                },
            );
        },
    );
}


fn render_audio_tab(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    config: &mut Config,
    hardware: SettingsHardwareContext<'_>,
) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Audio),
        t("Desktop Audio & Game Sound"),
        Some(t("Capture system sound effects, Discord, and game audio")),
        |ui| {
            m3_setting_row(
                ui,
                theme,
                t("Desktop Output Sink"),
                if cfg!(windows) {
                    t("Target audio playback device")
                } else {
                    t("Target audio playback device from PipeWire / PulseAudio")
                },
                |ui| {
                    if cfg!(windows) {
                        let is_default = ["", "default", "default_output"].contains(&config.audio_output.as_str());
                        let cur = if is_default { t("Default output").to_string() } else { config.audio_output.clone() };
                        m3_dropdown(ui, theme, "audio_desktop_sink", cur, 320.0, |ui| {
                            ui.selectable_value(
                                &mut config.audio_output,
                                "".into(),
                                t("Default output"),
                            );
                            for dev in hardware.available_outputs {
                                ui.selectable_value(&mut config.audio_output, dev.name.clone(), &dev.description);
                            }
                        });
                    } else {
                        let cur_desc = if config.audio_output.is_empty() {
                            "Default System Sink".to_string()
                        } else {
                            hardware
                                .available_outputs
                                .iter()
                                .find(|d| d.name == config.audio_output)
                                .map(|d| d.description.clone())
                                .unwrap_or_else(|| config.audio_output.clone())
                        };

                        m3_dropdown(ui, theme, "audio_desktop_sink", cur_desc, 320.0, |ui| {
                            ui.selectable_value(
                                &mut config.audio_output,
                                "".into(),
                                t("Default System Sink"),
                            );
                            for dev in hardware.available_outputs {
                                ui.selectable_value(
                                    &mut config.audio_output,
                                    dev.name.clone(),
                                    &dev.description,
                                );
                            }
                        });
                    }
                },
            );
        },
    );

    ui.add_space(16.0);

    m3_section_card(
        ui,
        theme,
        Some(IconKind::Mic),
        t("Microphone & Voice Input"),
        Some(t("Mix microphone voice track into recorded gameplay clips")),
        |ui| {
            m3_setting_row(
                ui,
                theme,
                t("Microphone Source"),
                if cfg!(windows) {
                    t("Input microphone device")
                } else {
                    t("Input microphone device from PipeWire / PulseAudio")
                },
                |ui| {
                    if cfg!(windows) {
                        let cur_desc = if config.audio_input.eq_ignore_ascii_case("none") {
                            t("None").to_string()
                        } else if ["", "default", "default_input"].contains(&config.audio_input.as_str()) {
                            t("Default microphone").to_string()
                        } else {
                            config.audio_input.clone()
                        };
                        m3_dropdown(ui, theme, "audio_mic_source", cur_desc, 320.0, |ui| {
                            ui.selectable_value(
                                &mut config.audio_input,
                                "".into(),
                                t("Default microphone"),
                            );
                            ui.selectable_value(
                                &mut config.audio_input,
                                "None".into(),
                                t("None"),
                            );
                            for dev in hardware.available_inputs {
                                ui.selectable_value(&mut config.audio_input, dev.name.clone(), &dev.description);
                            }
                        });
                    } else {
                        let cur_desc = if config.audio_input.is_empty() {
                            "Default System Microphone".to_string()
                        } else {
                            hardware
                                .available_inputs
                                .iter()
                                .find(|d| d.name == config.audio_input)
                                .map(|d| d.description.clone())
                                .unwrap_or_else(|| config.audio_input.clone())
                        };

                        m3_dropdown(ui, theme, "audio_mic_source", cur_desc, 320.0, |ui| {
                            ui.selectable_value(
                                &mut config.audio_input,
                                "".into(),
                                t("Default System Microphone"),
                            );
                            ui.selectable_value(
                                &mut config.audio_input,
                                "None".into(),
                                t("Disabled (Mute Microphone)"),
                            );
                            for dev in hardware.available_inputs {
                                ui.selectable_value(
                                    &mut config.audio_input,
                                    dev.name.clone(),
                                    &dev.description,
                                );
                            }
                        });
                    }
                },
            );

            if !cfg!(windows) {
                ui.separator();

                m3_setting_row(
                    ui,
                    theme,
                    t("Noise suppression"),
                    t("Adds a microphone named “Virtual-Microphone”. Pick it in Discord, games or as Lapse's mic."),
                    |ui| {
                        m3_switch(ui, theme, &mut config.noise_suppression);
                    },
                );

                if config.noise_suppression {
                    ui.separator();

                    m3_setting_row(
                        ui,
                        theme,
                        t("Use as default microphone"),
                        t("Other apps like Discord will hear the cleaned-up microphone."),
                        |ui| {
                            m3_switch(ui, theme, &mut config.noise_as_default);
                        },
                    );

                    ui.separator();

                    m3_setting_row(
                        ui,
                        theme,
                        t("Source microphone"),
                        "",
                        |ui| {
                            let cur_desc = if config.noise_source.is_empty() || config.noise_source == "default_input" {
                                t("Default System Microphone").to_string()
                            } else {
                                hardware
                                    .available_inputs
                                    .iter()
                                    .find(|d| d.name == config.noise_source)
                                    .map(|d| d.description.clone())
                                    .unwrap_or_else(|| config.noise_source.clone())
                            };

                            m3_dropdown(ui, theme, "audio_noise_source", cur_desc, 320.0, |ui| {
                                ui.selectable_value(
                                    &mut config.noise_source,
                                    "default_input".into(),
                                    t("Default System Microphone"),
                                );
                                for dev in hardware.available_inputs.iter().filter(|d| d.name != "lapse_denoise") {
                                    ui.selectable_value(
                                        &mut config.noise_source,
                                        dev.name.clone(),
                                        &dev.description,
                                    );
                                }
                            });
                        },
                    );
                }
            }

            ui.separator();

            m3_setting_row(
                ui,
                theme,
                t("Microphone boost"),
                t("Increase microphone recording volume with an automatic soft limiter to prevent clipping."),
                |ui| {
                    let mut pct = (config.mic_gain * 100.0).round() as f64;
                    let opts = crate::gui::components::slider::SliderOpts {
                        log: false,
                        step: 5.0,
                    };
                    ui.horizontal(|ui| {
                        if crate::gui::components::slider::m3_slider(
                            ui,
                            theme,
                            &mut pct,
                            100.0..=300.0,
                            170.0,
                            opts,
                        )
                        .changed()
                        {
                            config.mic_gain = ((pct / 100.0) as f32).clamp(1.0, 3.0);
                            let _ = config.save();
                            reload_daemon_config();
                        }
                        ui.label(
                            RichText::new(format!("{:.0}%", pct))
                                .size(12.5)
                                .monospace()
                                .color(theme.on_surface),
                        );
                    });
                },
            );
        },
    );

    ui.add_space(16.0);

    m3_section_card(
        ui,
        theme,
        Some(IconKind::Audio),
        t("Separate app audio"),
        Some(t("Pick the apps you want on their own audio track, so you can mute them or change their volume while editing. Applies to new clips.")),
        |ui| {
            m3_setting_row(
                ui,
                theme,
                t("Record apps on their own tracks"),
                t("Each app below gets its own audio track, so you can mute it or change its volume when editing a clip. Applies to new clips."),
                |ui| {
                    m3_switch(ui, theme, &mut config.separate_app_audio);
                },
            );

            if config.separate_app_audio {
                #[cfg(target_os = "linux")]
                {
                    ui.separator();
                    m3_setting_row(
                        ui,
                        theme,
                        t("Find apps automatically"),
                        t("Every app that plays sound gets its own track. An app heard for the first time joins right after your next saved clip."),
                        |ui| {
                            m3_switch(ui, theme, &mut config.auto_detect_apps);
                        },
                    );
                    if config.auto_detect_apps {
                        render_heard_apps(ui, theme);
                    }
                }
                ui.separator();

                ui.vertical(|ui| {
                    ui.label(RichText::new(t("Apps on their own track")).size(13.0).strong().color(theme.on_surface));
                    ui.add_space(4.0);

                    render_editable_chip_list(
                        ui,
                        theme,
                        &mut config.separate_apps,
                        &t("No apps separated yet."),
                    );

                    ui.add_space(6.0);
                    render_app_adder(
                        ui,
                        theme,
                        "pick_separate_app",
                        &mut config.separate_apps,
                    );
                });
            }

            ui.separator();

            ui.vertical(|ui| {
                ui.label(RichText::new(t("Never record")).size(13.0).strong().color(theme.on_surface));
                ui.label(
                    RichText::new(t("Apps in this list are completely excluded and will never be recorded in any audio track."))
                        .size(11.5)
                        .color(theme.on_surface_variant),
                );
                ui.add_space(4.0);

                render_editable_chip_list(
                    ui,
                    theme,
                    &mut config.excluded_apps,
                    &t("No apps excluded."),
                );

                ui.add_space(6.0);
                render_app_adder(
                    ui,
                    theme,
                    "pick_excluded_app",
                    &mut config.excluded_apps,
                );
            });
        },
    );
}

/// Apps found automatically (audio_apps.json). Removing one only forgets it until it plays again.
#[cfg(target_os = "linux")]
fn render_heard_apps(ui: &mut egui::Ui, theme: &MonetTheme) {
    let id = egui::Id::new("heard_audio_apps");
    let now = ui.input(|i| i.time);
    let mut apps = match ui.data(|d| d.get_temp::<(f64, Vec<String>)>(id)) {
        Some((at, apps)) if now - at < 3.0 => apps,
        _ => {
            let apps = crate::audio_apps::load();
            ui.data_mut(|d| d.insert_temp(id, (now, apps.clone())));
            apps
        }
    };
    ui.vertical(|ui| {
        ui.label(RichText::new(t("Found so far")).size(13.0).strong().color(theme.on_surface));
        ui.add_space(4.0);
        let before = apps.clone();
        render_editable_chip_list(ui, theme, &mut apps, &t("Nothing yet. Play something and it shows up here."));
        if let Some(gone) = before.iter().find(|a| !apps.contains(a)) {
            crate::audio_apps::forget(gone);
            ui.data_mut(|d| d.insert_temp(id, (now, apps.clone())));
        }
    });
}

/// What to call an app in the lists: its real name when Lapse learned one
/// ("Serein" rather than its audio name "cpal-pulseaudio-2").
fn app_label(app: &str) -> String {
    #[cfg(target_os = "linux")]
    {
        crate::audio_apps::display_name(&crate::audio_apps::resolve_key(app))
    }
    #[cfg(not(target_os = "linux"))]
    {
        app.to_string()
    }
}

fn render_editable_chip_list(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    apps: &mut Vec<String>,
    empty_msg: &str,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        if apps.is_empty() {
            ui.label(RichText::new(empty_msg).size(12.0).color(theme.outline));
            return;
        }
        let mut remove_idx = None;
        for (i, app) in apps.iter().enumerate() {
            let text = format!("{}  ×", app_label(app));
            let resp = ui.add(
                egui::Button::new(RichText::new(text).size(12.0).strong().color(theme.on_secondary_container))
                    .fill(theme.secondary_container)
                    .rounding(crate::gui::theme::M3Shapes::FULL)
            ).on_hover_text(t("Click to remove"));
            if resp.clicked() {
                remove_idx = Some(i);
            }
        }
        if let Some(i) = remove_idx {
            apps.remove(i);
        }
    });
}

fn render_app_adder(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    id_str: &str,
    target_list: &mut Vec<String>,
) {
    let mut picked = None;
    ui.push_id(id_str, |ui| {
        ui.horizontal(|ui| {
            m3_dropdown_sized(ui, theme, id_str, format!("+  {}", t("Add from playing apps")), 260.0, 40.0, |ui| {
                let id = egui::Id::new(id_str).with("list");
                let now = ui.input(|i| i.time);
                // (label, value to store): apps playing sound now, then every open window.
                type Lists = (Vec<(String, String)>, Vec<(String, String)>);
                let cached: Option<(f64, Lists)> = ui.data(|d| d.get_temp(id));
                let (playing, windows) = match cached {
                    Some((at, lists)) if now - at < 3.0 => lists,
                    _ => {
                        #[cfg(target_os = "linux")]
                        let playing: Vec<(String, String)> = crate::recorder::list_running_audio_apps()
                            .into_iter()
                            .filter(|a| !crate::audio_apps::is_ignored(a))
                            .map(|a| (app_label(&a), a))
                            .collect();
                        #[cfg(windows)]
                        let playing = crate::recorder_windows::audio::playing_apps();
                        #[cfg(not(any(target_os = "linux", windows)))]
                        let playing: Vec<(String, String)> = Vec::new();
                        let open = crate::hardware::game_detector::open_programs();
                        let windows: Vec<(String, String)> = open.into_iter().map(|a| (a.name, a.matcher)).collect();
                        let lists = (playing, windows);
                        ui.data_mut(|d| d.insert_temp(id, (now, lists.clone())));
                        lists
                    }
                };

                let mut section = |ui: &mut egui::Ui, title: &str, items: &[(String, String)], empty: &str| {
                    ui.label(RichText::new(title).size(11.0).strong().color(theme.on_surface_variant));
                    if items.is_empty() {
                        ui.label(RichText::new(empty).size(12.0).color(theme.outline));
                    }
                    for (label, value) in items {
                        let added = target_list.iter().any(|e| e.eq_ignore_ascii_case(value) || app_label(e) == *label);
                        let text = if added { format!("{}  ({})", label, t("added")) } else { label.clone() };
                        if ui.add_enabled(!added, egui::SelectableLabel::new(false, text)).clicked() {
                            picked = Some(value.to_lowercase());
                        }
                    }
                };
                if cfg!(target_os = "linux") {
                    section(ui, t("Playing audio right now"), &playing, t("No apps are playing audio right now."));
                    ui.add_space(6.0);
                }
                section(ui, t("Open windows"), &windows, t("No other programs are open."));
            });

            let text_id = egui::Id::new(id_str).with("type_input");
            let mut typed = ui.data(|d| d.get_temp::<String>(text_id)).unwrap_or_default();
            let resp = m3_text_field(
                ui,
                theme,
                &mut typed,
                TextFieldOpts::hint(t("or type app name...")).with_width(200.0),
            );
            let add_btn = m3_tonal_button_sized(ui, theme, t("Add"), egui::vec2(90.0, 40.0)).clicked();
            if add_btn || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                let clean = typed.trim().to_lowercase();
                if !clean.is_empty() && !target_list.iter().any(|e| e.eq_ignore_ascii_case(&clean)) {
                    picked = Some(clean);
                    typed.clear();
                }
            }
            ui.data_mut(|d| d.insert_temp(text_id, typed));
        });
    });

    if let Some(app) = picked {
        if !target_list.iter().any(|e| e.eq_ignore_ascii_case(&app)) {
            target_list.push(app);
        }
    }
}


fn render_hotkeys_tab(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    config: &mut Config,
    binding_hotkey: &mut Option<HotkeyEvent>,
) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Keyboard),
        t("Global Shortcuts & Keybindings"),
        Some(t("Assign global hotkeys to save replays or toggle continuous recording")),
        |ui| {
            if let Some(err) = crate::hotkeys::last_hotkey_error() {
                egui::Frame::none()
                    .fill(theme.error_container)
                    .rounding(M3Shapes::MEDIUM)
                    .inner_margin(Margin::symmetric(16.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            m3_vector_icon(ui, IconKind::Warning, 16.0, theme.error);
                            ui.add_space(6.0);
                            ui.label(RichText::new(&err).color(theme.on_error_container).size(13.0));
                        });
                    });
                ui.add_space(8.0);
            }
            hotkey_row(ui, theme, config, binding_hotkey, HotkeyEvent::SaveReplay,
                t("Save Instant Replay Buffer"), t("Saves recent gameplay buffer to your clips library directory"));
            ui.separator();
            hotkey_row(ui, theme, config, binding_hotkey, HotkeyEvent::ToggleRecord,
                t("Toggle Manual Recording"), t("Starts or stops manual continuous gameplay video capture"));
            ui.separator();

            m3_setting_row(ui, theme, t("More shortcuts"), t("Save only the last seconds, take a screenshot, or mark a moment."), |ui| {
                m3_switch(ui, theme, &mut config.extra_hotkeys);
            });
            if config.extra_hotkeys {
                ui.separator();
                hotkey_row(ui, theme, config, binding_hotkey, HotkeyEvent::SaveShort,
                    t("Save the last seconds"), t("Keeps the buffer running; good for quick moments."));
                m3_setting_row(ui, theme, t("Length"), "", |ui| {
                    let options = [(300u32, "5 min"), (60u32, "60 s"), (30u32, "30 s"), (10u32, "10 s")];
                    crate::gui::components::segmented::segmented(ui, theme, "short_save_seconds", &mut config.short_save_seconds, &options, 62.0);
                });
                ui.separator();
                hotkey_row(ui, theme, config, binding_hotkey, HotkeyEvent::Screenshot,
                    t("Screenshot"), t("Saves a PNG of the recorded monitor next to your clips."));
                ui.separator();
                hotkey_row(ui, theme, config, binding_hotkey, HotkeyEvent::Marker,
                    t("Mark this moment"), t("The mark shows on the trimmer timeline of the saved clip."));
            }

            ui.separator();
            render_gamepad_rows(ui, theme, config);
            ui.separator();

            let defaults = Config::default();
            let fields = |c: &Config| c.hotkey_bindings().into_iter().map(|(_, k, m)| (k, m)).collect::<Vec<_>>();
            let is_default = fields(config) == fields(&Config { extra_hotkeys: config.extra_hotkeys, ..defaults.clone() });
            m3_setting_row(ui, theme, t("Default shortcuts"), t("F10 saves the replay, F9 starts or stops recording."), |ui| {
                let reset = ui.add_enabled_ui(!is_default, |ui| m3_outlined_button(ui, theme, t("Reset to defaults"))).inner;
                if reset.clicked() {
                    for ev in [HotkeyEvent::SaveReplay, HotkeyEvent::ToggleRecord, HotkeyEvent::SaveShort, HotkeyEvent::Screenshot, HotkeyEvent::Marker] {
                        let mut d = defaults.clone();
                        let (dk, dm) = d.hotkey_mut(ev);
                        let (dk, dm) = (*dk, *dm);
                        let (k, m) = config.hotkey_mut(ev);
                        *k = dk;
                        *m = dm;
                    }
                    *binding_hotkey = None;
                    let _ = config.save();
                    Config::sync_autostart(config.autostart);
                    crate::gui::platform::sync_autostart_windows(config.autostart);
                }
            });

            ui.separator();

            let save_path_str = config.save_path.to_string_lossy().to_string();
            m3_setting_row(ui, theme, t("Clips Storage Folder"), &save_path_str, |ui| {
                let picking = crate::gui::file_pick::is_open("save_path");
                if ui.add_enabled_ui(!picking, |ui| m3_outlined_button(ui, theme, t("Browse..."))).inner.clicked() {
                    let start = config.save_path.clone();
                    crate::gui::file_pick::open(ui.ctx(), "save_path", move || {
                        FileDialog::new().set_title(t("Choose where clips are saved")).set_directory(start).pick_folder()
                    });
                }
                if let Some(path) = crate::gui::file_pick::take("save_path") {
                    config.save_path = path;
                }
            });
        },
    );
}


#[derive(Clone)]
struct VoiceWizardState {
    is_open: bool,
    testing: bool,
    takes_done: [bool; 6],
    current_take: usize,
    error_msg: Option<String>,
    last_test: Option<(bool, f32, f32, std::time::Instant)>,
    smoothed_level: f32,
    controller: std::sync::Arc<crate::voice::VoiceWizardController>,
}

impl Default for VoiceWizardState {
    fn default() -> Self {
        Self {
            is_open: false,
            testing: false,
            takes_done: [false; 6],
            current_take: 0,
            error_msg: None,
            last_test: None,
            smoothed_level: 0.0,
            controller: std::sync::Arc::new(crate::voice::VoiceWizardController::new()),
        }
    }
}

fn draw_level_meter(ui: &mut egui::Ui, theme: &MonetTheme, level: f32) {
    let desired_size = Vec2::new(ui.available_width().min(240.0), 8.0);
    let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, M3Shapes::FULL, theme.surface_container_highest);

    let db = 20.0 * level.max(1e-6).log10();
    let progress = ((db - (-75.0)) / 60.0).clamp(0.0, 1.0);
    if progress > 0.01 {
        let filled_rect = Rect::from_min_size(
            rect.min,
            Vec2::new(rect.width() * progress, rect.height()),
        );
        let color = lerp_color(theme.primary.gamma_multiply(0.6), theme.primary, progress);
        painter.rect_filled(filled_rect, M3Shapes::FULL, color);
    }
}

fn voice_row<R>(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    row_key: &str,
    title: &str,
    description: &str,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let id = ui.id().with(row_key);
    let prev_h: f32 = ui.data(|d| d.get_temp(id.with("hov"))).unwrap_or(0.0);
    let bg = theme.surface_container_high.gamma_multiply(prev_h * 0.4);
    let frame_res = egui::Frame::none()
        .fill(bg)
        .rounding(M3Shapes::MEDIUM)
        .inner_margin(Margin::symmetric(6.0, 2.0))
        .show(ui, |ui| {
            m3_setting_row(ui, theme, title, description, control)
        });
    let resp = ui.interact(frame_res.response.rect, id, egui::Sense::hover());
    let h = hover_t(ui, &resp);
    ui.data_mut(|d| d.insert_temp(id.with("hov"), h));
    if h > 0.001 && h < 0.999 {
        ui.ctx().request_repaint();
    }
    frame_res.inner
}

pub fn render_voice_trigger_card(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    config: &mut Config,
) {
    let open_t = ease_out(ui.ctx().animate_bool_with_time(
        ui.id().with("voice_trigger_open"),
        config.voice_trigger_enabled,
        MEDIUM,
    ));
    let border = lerp_color(theme.outline_variant, theme.primary, open_t * 0.7);

    egui::Frame::none()
        .fill(theme.surface_container)
        .stroke(Stroke::new(1.0_f32 + open_t * 0.5, border))
        .rounding(M3Shapes::LARGE)
        .inner_margin(Margin::same(18.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                m3_vector_icon(ui, IconKind::Mic, 22.0, theme.primary);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(M3Typography::title_medium(t("Voice trigger"), theme.on_surface));
                    ui.label(M3Typography::body_medium(
                        t("Save a clip when you say a trigger word"),
                        theme.on_surface_variant,
                    ));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if m3_switch(ui, theme, &mut config.voice_trigger_enabled).changed() {
                        let _ = config.save();
                        reload_daemon_config();
                    }
                });
            });

            if open_t <= 0.01 {
                return;
            }

            if open_t < 0.999 {
                ui.ctx().request_repaint();
            }

            ui.add_space(14.0 * open_t);
            ui.separator();
            ui.add_space(12.0 * open_t);

            let wizard_id = ui.id().with("voice_wizard_state");
            let mut wizard: VoiceWizardState = ui.data_mut(|d| d.get_temp(wizard_id).unwrap_or_default());

            // Poll worker events without blocking the frame
            while let Some(evt) = wizard.controller.try_recv() {
                match evt {
                    crate::voice::WizardEvent::CaptureReady { .. } => {
                        wizard.error_msg = None;
                    }
                    crate::voice::WizardEvent::CaptureError(err) => {
                        wizard.error_msg = Some(err);
                    }
                    crate::voice::WizardEvent::TakeRecorded { index, next_take } => {
                        wizard.takes_done[index] = true;
                        wizard.current_take = next_take;
                        wizard.error_msg = None;
                    }
                    crate::voice::WizardEvent::EnrollmentSuccess => {
                        wizard.takes_done = [true; 6];
                        wizard.testing = true;
                        wizard.error_msg = None;
                        config.voice_trigger_enabled = true;
                        let _ = config.save();
                        reload_daemon_config();
                    }
                    crate::voice::WizardEvent::EnrollmentOutlier { take_index } => {
                        wizard.takes_done[take_index] = false;
                        wizard.current_take = take_index;
                        wizard.error_msg =
                            Some(t("A take was inconsistent with the others. Please repeat it.").into());
                    }
                    crate::voice::WizardEvent::EnrollmentFailed(err) => {
                        wizard.error_msg =
                            Some(format!("{}: {}", t("Enrollment failed. Please try again."), err));
                    }
                    crate::voice::WizardEvent::TestResult {
                        matched,
                        distance,
                        threshold,
                    } => {
                        wizard.last_test =
                            Some((matched, distance, threshold, std::time::Instant::now()));
                    }
                }
            }

            // Smoothed attack/release level tracking
            let raw_level = wizard.controller.current_level();
            let prev_smooth = wizard.smoothed_level;
            let smoothed = if raw_level > prev_smooth {
                prev_smooth + (raw_level - prev_smooth) * 0.45
            } else {
                prev_smooth + (raw_level - prev_smooth) * 0.12
            };
            wizard.smoothed_level = smoothed;

            let has_templates = Config::get_voice_templates_path()
                .map(|p| p.exists())
                .unwrap_or(false);

            let setup_text = if wizard.is_open {
                t("Close setup")
            } else {
                t("Set up")
            };

            voice_row(
                ui,
                theme,
                "voice_setup",
                t("Trigger word setup"),
                if has_templates {
                    t("Trigger word is configured. You can re-record it anytime.")
                } else {
                    t("Record your trigger word to enable voice-activated clipping.")
                },
                |ui| {
                    if m3_outlined_button(ui, theme, setup_text).clicked() {
                        wizard.is_open = !wizard.is_open;
                        if wizard.is_open {
                            wizard.testing = false;
                            wizard.takes_done = [false; 6];
                            wizard.current_take = 0;
                            wizard.error_msg = None;
                            wizard.last_test = None;
                            wizard.controller.send(crate::voice::WizardCmd::Start {
                                device_hint: config.audio_input.clone(),
                                sensitivity: config.voice_sensitivity,
                            });
                        } else {
                            wizard.controller.send(crate::voice::WizardCmd::Stop);
                        }
                    }
                },
            );

            // Animate wizard opening/closing with height & opacity ease
            let wizard_open_t = ease_out(ui.ctx().animate_bool_with_time(
                ui.id().with("voice_wizard_open"),
                wizard.is_open,
                MEDIUM,
            ));

            if wizard.is_open {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
            } else if wizard_open_t > 0.001 {
                ui.ctx().request_repaint();
            }

            if wizard_open_t > 0.001 {
                ui.add_space(8.0 * wizard_open_t);

                let card_fill = theme.surface_container_low.gamma_multiply(wizard_open_t);
                let card_stroke =
                    Stroke::new(1.0f32, theme.outline_variant.gamma_multiply(wizard_open_t));

                egui::Frame::none()
                    .fill(card_fill)
                    .rounding(M3Shapes::MEDIUM)
                    .inner_margin(Margin::same(16.0))
                    .stroke(card_stroke)
                    .show(ui, |ui| {
                        if !wizard.testing {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(t("Say your trigger word"))
                                        .size(15.0)
                                        .strong()
                                        .color(theme.on_surface.gamma_multiply(wizard_open_t)),
                                );
                                ui.add_space(8.0);
                                draw_level_meter(ui, theme, wizard.smoothed_level);
                            });

                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(t(
                                    "Say your trigger word clearly. Recording auto-stops when you stop speaking.",
                                ))
                                .size(12.5)
                                .color(theme.on_surface_variant.gamma_multiply(wizard_open_t)),
                            );

                            if let Some(err) = &wizard.error_msg {
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    m3_vector_icon(ui, IconKind::Warning, 14.0, theme.error);
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(err).color(theme.error).size(12.5));
                                });
                            }

                            ui.add_space(10.0);

                            // Take indicators with animated filling dots and ✓ appearing
                            ui.horizontal(|ui| {
                                for i in 0..6 {
                                    let is_done = wizard.takes_done[i];
                                    let is_curr = i == wizard.current_take;
                                    let fill_t = ease_out(ui.ctx().animate_bool_with_time(
                                        ui.id().with(("take_dot_fill", i)),
                                        is_done,
                                        FAST * 2.0,
                                    ));
                                    let check_t = ease_out(ui.ctx().animate_bool_with_time(
                                        ui.id().with(("take_dot_chk", i)),
                                        is_done,
                                        FAST * 2.0,
                                    ));

                                    let dot_size = Vec2::splat(22.0);
                                    let (r, _) = ui.allocate_exact_size(dot_size, egui::Sense::hover());
                                    let p = ui.painter();
                                    let bg = lerp_color(
                                        theme.surface_container_highest,
                                        theme.primary,
                                        fill_t,
                                    );
                                    let border =
                                        lerp_color(theme.outline_variant, theme.primary, fill_t);
                                    p.circle_filled(r.center(), 10.0, bg);
                                    p.circle_stroke(r.center(), 10.0, Stroke::new(1.0_f32, border));

                                    if is_curr && !is_done {
                                        let k = pulse(ui, 1.4);
                                        p.circle_stroke(
                                            r.center(),
                                            10.0 + 3.0 * k,
                                            Stroke::new(
                                                1.5_f32,
                                                theme.primary.gamma_multiply(0.6 * (1.0 - k)),
                                            ),
                                        );
                                    }

                                    if check_t > 0.05 {
                                        crate::gui::components::icon::draw_vector_icon(
                                            p,
                                            r.center(),
                                            12.0 * check_t,
                                            IconKind::Check,
                                            theme.on_primary.gamma_multiply(check_t),
                                        );
                                    } else {
                                        p.text(
                                            r.center(),
                                            Align2::CENTER_CENTER,
                                            (i + 1).to_string(),
                                            FontId::proportional(11.0),
                                            if is_curr {
                                                theme.primary
                                            } else {
                                                theme.on_surface_variant
                                            },
                                        );
                                    }
                                    ui.add_space(4.0);
                                }
                            });

                            ui.add_space(10.0);
                            for i in 0..6 {
                                let row_id = ui.id().with(("take_row", i));
                                let prev_h: f32 = ui.data(|d| d.get_temp(row_id.with("hov"))).unwrap_or(0.0);
                                let bg = theme.surface_container_high.gamma_multiply(prev_h * 0.4);
                                let row_res = egui::Frame::none()
                                    .fill(bg)
                                    .rounding(M3Shapes::SMALL)
                                    .inner_margin(Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            let is_done = wizard.takes_done[i];
                                            let is_curr = i == wizard.current_take;
                                            let check_t = ease_out(ui.ctx().animate_bool_with_time(
                                                ui.id().with(("take_row_chk", i)),
                                                is_done,
                                                FAST * 2.0,
                                            ));

                                            if is_done {
                                                ui.label(
                                                    RichText::new(format!("{} {}", t("Take"), i + 1))
                                                        .color(theme.primary)
                                                        .strong()
                                                        .size(13.0),
                                                );
                                                if check_t > 0.05 {
                                                    m3_vector_icon(
                                                        ui,
                                                        IconKind::Check,
                                                        14.0 * check_t,
                                                        theme.primary.gamma_multiply(check_t),
                                                    );
                                                }
                                                ui.add_space(8.0);
                                                if crate::gui::components::button::m3_small_button(
                                                    ui,
                                                    theme,
                                                    t("Redo"),
                                                )
                                                .clicked()
                                                {
                                                    wizard.takes_done[i] = false;
                                                    wizard.current_take = i;
                                                    wizard.error_msg = None;
                                                    wizard.controller.send(crate::voice::WizardCmd::RedoTake(i));
                                                }
                                            } else if is_curr {
                                                let k = pulse(ui, 1.4);
                                                let dot_col = lerp_color(theme.primary, theme.primary_container, k);
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{} {}: {}",
                                                        t("Take"),
                                                        i + 1,
                                                        t("Listening...")
                                                    ))
                                                    .color(dot_col)
                                                    .strong()
                                                    .size(13.0),
                                                );
                                            } else {
                                                ui.label(
                                                    RichText::new(format!("{} {}: —", t("Take"), i + 1))
                                                        .color(theme.on_surface_variant)
                                                        .size(13.0),
                                                );
                                            }
                                        });
                                    });
                                let resp = ui.interact(row_res.response.rect, row_id, egui::Sense::hover());
                                let h = hover_t(ui, &resp);
                                ui.data_mut(|d| d.insert_temp(row_id.with("hov"), h));
                                if h > 0.001 && h < 0.999 {
                                    ui.ctx().request_repaint();
                                }
                            }

                            ui.add_space(10.0);
                            if m3_outlined_button(ui, theme, t("Cancel")).clicked() {
                                wizard.is_open = false;
                                wizard.controller.send(crate::voice::WizardCmd::Stop);
                            }
                        } else {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(t("Test your trigger word"))
                                        .size(15.0)
                                        .strong()
                                        .color(theme.on_surface.gamma_multiply(wizard_open_t)),
                                );
                                ui.add_space(8.0);
                                draw_level_meter(ui, theme, wizard.smoothed_level);
                            });

                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(t("Say your trigger word to test if Lapse recognizes it."))
                                    .size(12.5)
                                    .color(theme.on_surface_variant.gamma_multiply(wizard_open_t)),
                            );

                            ui.add_space(10.0);

                            // Animated flashing & fading match result
                            if let Some((matched, dist, thresh, at)) = wizard.last_test {
                                let elapsed = at.elapsed().as_secs_f32();
                                let display_dur = 2.8f32;
                                if elapsed < display_dur {
                                    ui.ctx().request_repaint();
                                    let flash = ease_out((elapsed / 0.12).min(1.0));
                                    let fade = ((display_dur - elapsed) / 1.0).clamp(0.0, 1.0);
                                    let alpha = (flash * fade).clamp(0.0, 1.0);

                                    let (color, icon, text) = if matched {
                                        (theme.success, IconKind::Check, t("Matched!"))
                                    } else {
                                        (theme.error, IconKind::Close, t("Not matched"))
                                    };

                                    let pill_fill = color.gamma_multiply(0.18 * alpha);
                                    let pill_stroke = Stroke::new(1.5_f32, color.gamma_multiply(0.7 * alpha));
                                    let fg = color.gamma_multiply(alpha);

                                    egui::Frame::none()
                                        .fill(pill_fill)
                                        .stroke(pill_stroke)
                                        .rounding(M3Shapes::MEDIUM)
                                        .inner_margin(Margin::symmetric(14.0, 10.0))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                m3_vector_icon(ui, icon, 20.0, fg);
                                                ui.add_space(8.0);
                                                ui.vertical(|ui| {
                                                    ui.label(
                                                        RichText::new(text).color(fg).strong().size(15.0),
                                                    );
                                                    ui.label(
                                                        RichText::new(format!("{:.3} / {:.3}", dist, thresh))
                                                            .color(theme.on_surface_variant.gamma_multiply(alpha))
                                                            .size(11.5),
                                                    );
                                                });
                                            });
                                        });
                                } else {
                                    ui.label(
                                        RichText::new(t("Waiting for trigger word..."))
                                            .size(13.0)
                                            .color(theme.on_surface_variant),
                                    );
                                }
                            } else {
                                ui.label(
                                    RichText::new(t("Waiting for trigger word..."))
                                        .size(13.0)
                                        .color(theme.on_surface_variant),
                                );
                            }

                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if m3_outlined_button(ui, theme, t("Done")).clicked() {
                                    wizard.is_open = false;
                                    wizard.controller.send(crate::voice::WizardCmd::Stop);
                                }
                                ui.add_space(8.0);
                                if m3_outlined_button(ui, theme, t("Re-record")).clicked() {
                                    wizard.testing = false;
                                    wizard.takes_done = [false; 6];
                                    wizard.current_take = 0;
                                    wizard.error_msg = None;
                                    wizard.last_test = None;
                                    wizard.controller.send(crate::voice::WizardCmd::ResetEnrollment);
                                }
                            });
                        }
                    });
            }

            ui.data_mut(|d| d.insert_temp(wizard_id, wizard.clone()));

            ui.separator();

            voice_row(
                ui,
                theme,
                "voice_sensitivity",
                t("Sensitivity"),
                t("Adjust detection threshold (lower = stricter, higher = easier to trigger)."),
                |ui| {
                    let mut sens_val = config.voice_sensitivity as f64;
                    let opts = crate::gui::components::slider::SliderOpts {
                        log: false,
                        step: 0.05,
                    };
                    ui.horizontal(|ui| {
                        if crate::gui::components::slider::m3_slider(
                            ui,
                            theme,
                            &mut sens_val,
                            0.70..=1.30,
                            170.0,
                            opts,
                        )
                        .changed()
                        {
                            config.voice_sensitivity = (sens_val as f32).clamp(0.70, 1.30);
                            let _ = config.save();
                            reload_daemon_config();
                            wizard.controller.send(crate::voice::WizardCmd::UpdateSensitivity(config.voice_sensitivity));
                        }
                        ui.add_space(8.0);
                        egui::Frame::none()
                            .fill(theme.surface_container_highest)
                            .rounding(M3Shapes::FULL)
                            .inner_margin(Margin::symmetric(12.0, 5.0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("{:.2}x", config.voice_sensitivity))
                                        .color(theme.primary)
                                        .size(12.0)
                                        .strong(),
                                );
                            });
                    });
                },
            );

            ui.separator();

            voice_row(
                ui,
                theme,
                "voice_beep",
                t("Beep when a clip is saved"),
                t("Play a sound notification when a clip is triggered by your voice."),
                |ui| {
                    if m3_switch(ui, theme, &mut config.voice_beep).changed() {
                        let _ = config.save();
                        reload_daemon_config();
                    }
                },
            );

            ui.separator();

            ui.horizontal(|ui| {
                m3_vector_icon(ui, IconKind::Info, 14.0, theme.on_surface_variant);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(t("Works best with a headset. Pick a word you don't say in normal conversation."))
                        .size(12.0)
                        .color(theme.on_surface_variant),
                );
            });
        },
    );
}


/// Gamepad: save a replay with a button combination.
fn render_gamepad_rows(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    m3_setting_row(ui, theme, t("Gamepad"), t("Save a replay by holding a button combination on your controller."), |ui| {
        m3_switch(ui, theme, &mut config.gamepad_hotkey);
    });
    if !config.gamepad_hotkey {
        return;
    }
    const COMBOS: [(&str, &[u16]); 4] = [
        ("Select + R3", &[314, 318]),
        ("Select + L3", &[314, 317]),
        ("L3 + R3", &[317, 318]),
        ("Select + Start", &[314, 315]),
    ];
    m3_setting_row(ui, theme, t("Buttons"), t("Select is Back / View / Share, R3 is pressing the right stick."), |ui| {
        let current = COMBOS.iter().find(|(_, b)| *b == config.gamepad_buttons.as_slice()).map(|(n, _)| *n).unwrap_or("Custom");
        m3_dropdown(ui, theme, "lapse_gamepad_combo", current, 200.0, |ui| {
                for (name, buttons) in COMBOS {
                    if ui.selectable_label(current == name, name).clicked() {
                        config.gamepad_buttons = buttons.to_vec();
                    }
                }
            });
    });
    // Scanning /proc is cheap, but not every frame.
    let id = egui::Id::new("lapse_gamepads_seen");
    let now = ui.input(|i| i.time);
    let cached: Option<(f64, Vec<String>)> = ui.data(|d| d.get_temp(id));
    let names = match cached {
        Some((at, names)) if now - at < 3.0 => names,
        _ => {
            let names = crate::hotkeys::connected_gamepads();
            ui.data_mut(|d| d.insert_temp(id, (now, names.clone())));
            names
        }
    };
    let text = if names.is_empty() {
        t("No controller with buttons found. Plug it in (or start Steam for Steam Input pads); you need to be in the \"input\" group.").to_string()
    } else {
        format!("{} {}", t("Found:"), names.join(", "))
    };
    ui.label(RichText::new(text).size(12.0).color(theme.on_surface_variant));
}

/// One shortcut: title, description, its key and a Change button.
fn hotkey_row(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    config: &mut Config,
    binding: &mut Option<HotkeyEvent>,
    event: HotkeyEvent,
    title: &str,
    description: &str,
) {
    let (code, mods) = config.hotkey_mut(event);
    let label = format_hotkey(*code, *mods);
    let waiting = *binding == Some(event);
    m3_setting_row(ui, theme, title, description, |ui| {
        ui.horizontal(|ui| {
            let text = if waiting { t("Press any key...") } else { t("Change Key") };
            if m3_outlined_button(ui, theme, text).clicked() {
                *binding = if waiting { None } else { Some(event) };
            }
            ui.add_space(10.0);
            m3_keycap(ui, theme, &label);
        });
    });
}

fn render_services_tab(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    let (title, desc) = if cfg!(windows) {
        (
            t("Start Lapse when Windows starts"),
            t("Starts the replay buffer when Windows starts"),
        )
    } else {
        (
            t("Launch on Login"),
            t("Starts the replay buffer when your desktop session starts"),
        )
    };

    m3_section_card(
        ui,
        theme,
        Some(IconKind::Gear),
        t("Autostart"),
        if cfg!(windows) {
            Some(t("Run Lapse in the background when Windows starts"))
        } else {
            Some(t("Run Lapse in the background when you log in"))
        },
        |ui| {
            m3_setting_row(
                ui,
                theme,
                title,
                desc,
                |ui| {
                    let mut autostart = config.autostart;
                    if m3_switch(ui, theme, &mut autostart).changed() {
                        config.autostart = autostart;
                        Config::sync_autostart(autostart);
                        crate::gui::platform::sync_autostart_windows(autostart);
                    }
                },
            );

        },
    );
}

fn render_streamable(ui: &mut egui::Ui, theme: &MonetTheme) {
    let email_id = egui::Id::new("streamable_email_input");
    let pass_id = egui::Id::new("streamable_pass_input");
    let loaded_id = egui::Id::new("streamable_loaded");
    let status_id = egui::Id::new("streamable_status_msg");

    let is_loaded: bool = ui.data(|d| d.get_temp(loaded_id)).unwrap_or(false);
    let mut email: String = ui.data(|d| d.get_temp(email_id)).unwrap_or_default();
    let mut password: String = ui.data(|d| d.get_temp(pass_id)).unwrap_or_default();
    let mut status: Option<(String, bool)> = ui.data(|d| d.get_temp(status_id));

    if !is_loaded {
        if let Some(creds) = crate::streamable::load_credentials() {
            email = creds.email;
            password = creds.password;
        }
        ui.data_mut(|d| d.insert_temp(loaded_id, true));
    }

    m3_section_card(
        ui,
        theme,
        Some(IconKind::Share),
        t("Streamable"),
        Some(t("Uploads go to your own Streamable account.")),
        |ui| {
            m3_setting_row(ui, theme, t("Email"), t("Streamable account email"), |ui| {
                crate::gui::components::text_field::m3_text_field(
                    ui,
                    theme,
                    &mut email,
                    crate::gui::components::text_field::TextFieldOpts {
                        hint: "user@example.com",
                        width: Some(280.0),
                        ..Default::default()
                    },
                );
            });
            m3_setting_row(ui, theme, t("Password"), t("Streamable account password"), |ui| {
                crate::gui::components::text_field::m3_text_field(
                    ui,
                    theme,
                    &mut password,
                    crate::gui::components::text_field::TextFieldOpts {
                        width: Some(280.0),
                        password: true,
                        ..Default::default()
                    },
                );
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(t("Signed up with Google or Facebook? Streamable only accepts an email and password here. If your account has no password, set one on streamable.com (try \"Forgot password\" on the log-in page with the same email), or use \"Upload in browser\" from the Share menu."))
                    .size(12.0)
                    .color(theme.on_surface_variant),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if crate::gui::components::button::m3_small_button(ui, theme, t("Save")).clicked() {
                    if let Err(e) = crate::streamable::save_credentials(&email, &password) {
                        crate::streamable::clear_check_state();
                        status = Some((e.to_string(), true));
                    } else {
                        status = None;
                        crate::streamable::start_check(&email, &password);
                    }
                }
                if crate::gui::components::button::m3_small_button(ui, theme, t("Check")).clicked() {
                    status = None;
                    crate::streamable::start_check(&email, &password);
                }
                if crate::gui::components::button::m3_small_button_colored(ui, theme, t("Remove"), theme.error).clicked() {
                    let _ = crate::streamable::remove_credentials();
                    email.clear();
                    password.clear();
                    crate::streamable::clear_check_state();
                    status = Some((t("Credentials removed").to_string(), false));
                }
                if let Some(check) = crate::streamable::check_state() {
                    match check {
                        crate::streamable::CheckState::Checking => {
                            ui.label(RichText::new(t("Checking account...")).size(12.0).color(theme.primary));
                            ui.ctx().request_repaint();
                        }
                        crate::streamable::CheckState::Ok => {
                            ui.label(RichText::new(t("Signed in to Streamable")).size(12.0).color(theme.success));
                        }
                        crate::streamable::CheckState::Failed(e) => {
                            ui.label(RichText::new(e).size(12.0).color(theme.error));
                        }
                    }
                } else if let Some((ref msg, is_err)) = status {
                    let color = if is_err { theme.error } else { theme.success };
                    ui.label(RichText::new(msg).size(12.0).color(color));
                }
            });
        },
    );

    ui.data_mut(|d| {
        d.insert_temp(email_id, email);
        d.insert_temp(pass_id, password);
        d.insert_temp(status_id, status);
    });
}

fn render_appearance_tab(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    palette_kind: &mut MonetPaletteKind,
) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Palette),
        t("Material You Monet Aesthetics"),
        Some(t("Harmonious color schemes generated using Material 3 color algorithms")),
        |ui| {
            ui.label(M3Typography::title_medium(
                t("Color Palette Swatches"),
                theme.on_surface,
            ));
            ui.add_space(4.0);
            ui.label(M3Typography::body_medium(
                t("Choose your preferred dynamic Monet accent tone"),
                theme.on_surface_variant,
            ));
            ui.add_space(12.0);

            // As many columns as fit; each card's outer width (inner width + margins +
            // border) stays inside the section, so the right edge keeps its rounding.
            let spacing = 10.0;
            let avail = ui.available_width() - 4.0;
            let columns = (((avail + spacing) / (160.0 + spacing)).floor() as usize).clamp(1, 4);
            let outer_w = (avail - spacing * (columns as f32 - 1.0)) / columns as f32;
            let card_w = (outer_w - 24.0 - 4.0).max(60.0);

            egui::Grid::new("monet_palette_grid")
                .spacing(Vec2::new(spacing, spacing))
                .show(ui, |ui| {
                    for (i, kind) in MonetPaletteKind::ALL.iter().enumerate() {
                        let is_sel = *palette_kind == *kind;
                        let swatch = kind.swatch_color();

                        let (bg, border) = if is_sel {
                            (theme.primary_container, Stroke::new(2.0_f32, theme.primary))
                        } else {
                            (
                                theme.surface_container_high,
                                Stroke::new(1.0_f32, theme.outline_variant),
                            )
                        };

                        let resp = egui::Frame::none()
                            .fill(bg)
                            .stroke(border)
                            .rounding(M3Shapes::MEDIUM)
                            .inner_margin(Margin::symmetric(12.0, 10.0))
                            .show(ui, |ui| {
                                ui.set_width(card_w);
                                ui.horizontal(|ui| {
                                    let (circle_rect, _) = ui.allocate_exact_size(
                                        Vec2::splat(18.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter()
                                        .circle_filled(circle_rect.center(), 9.0, swatch);
                                    ui.painter().circle_stroke(
                                        circle_rect.center(),
                                        9.0,
                                        Stroke::new(1.0_f32, Color32::from_black_alpha(80)),
                                    );
                                    ui.add_space(6.0);
                                    let fg = if is_sel {
                                        theme.on_primary_container
                                    } else {
                                        theme.on_surface
                                    };
                                    ui.label(
                                        RichText::new(kind.display_name())
                                            .size(12.0)
                                            .strong()
                                            .color(fg),
                                    );
                                });
                            });

                        if resp.response.interact(egui::Sense::click()).clicked() {
                            *palette_kind = *kind;
                        }

                        if (i + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        },
    );
}


fn render_updates_tab(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Update),
        t("Software Updates & Maintenance"),
        Some(t("GitHub release tracking and automatic version check")),
        |ui| {
            m3_setting_row(
                ui,
                theme,
                t("Check for Updates"),
                t("Periodically check GitHub releases for new features and bugfixes"),
                |ui| {
                    m3_switch(ui, theme, &mut config.check_updates);
                },
            );

            ui.separator();

            m3_setting_row(
                ui,
                theme,
                "Current Release",
                &format!("Installed build: v{}", env!("CARGO_PKG_VERSION")),
                |ui| {
                    ui.horizontal(|ui| {
                        if m3_outlined_button(ui, theme, t("Check Now")).clicked() {
                            check_for_updates();
                        }

                        ui.add_space(8.0);

                        let st_opt = get_update_state().lock().ok().map(|g| (*g).clone());
                        if let Some(st) = st_opt {
                            match st {
                                UpdateStatus::Idle => {
                                    ui.label(RichText::new(t("Idle")).size(12.0).color(theme.outline));
                                }
                                UpdateStatus::Checking => {
                                    ui.spinner();
                                    ui.label(
                                        RichText::new(t("Checking..."))
                                            .size(12.0)
                                            .color(theme.primary),
                                    );
                                }
                                UpdateStatus::UpToDate => {
                                    m3_badge(ui, theme, t("Up to date"), Some(theme.success));
                                }
                                UpdateStatus::Available { version, url } => {
                                    m3_badge(
                                        ui,
                                        theme,
                                        &format!("v{} Available", version),
                                        Some(theme.error),
                                    );
                                    if ui
                                        .button(
                                            RichText::new(t("Download"))
                                                .size(11.0)
                                                .color(theme.primary),
                                        )
                                        .clicked()
                                    {
                                        let _ = open::that(url);
                                    }
                                }
                                UpdateStatus::Error(e) => {
                                    ui.label(
                                        RichText::new(e.as_str()).size(11.0).color(theme.outline),
                                    );
                                }
                            }
                        }
                    });
                },
            );
        },
    );
}


fn save_and_reload(
    config: &Config,
    on_command: &impl Fn(ipc::Command),
    status_banner: &mut Option<(String, std::time::Instant)>,
) {
    let _ = config.save();
    Config::sync_autostart(config.autostart);
    crate::gui::platform::sync_autostart_windows(config.autostart);
    on_command(ipc::Command::ReloadConfig);
    *status_banner = Some((
        "Settings successfully applied to engine!".into(),
        std::time::Instant::now(),
    ));
}

/// Tells the daemon to re-read the config without blocking the frame (it may take
/// a moment to restart the voice listener).
fn reload_daemon_config() {
    std::thread::spawn(|| {
        let _ = crate::ipc::send(crate::ipc::Command::ReloadConfig);
    });
}

fn render_advanced_tab(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    config: &mut Config,
    hardware: SettingsHardwareContext<'_>,
) {
    if !cfg!(windows) {
        crate::gui::views::dual_gpu_view::render_advanced(
            ui,
            theme,
            config,
            hardware.available_gpus,
            hardware.status,
        );
        ui.add_space(14.0);
    }

    m3_section_card(
        ui,
        theme,
        Some(IconKind::Play),
        t("Trimmer Video Preview"),
        Some(t("Hardware-accelerated video playback in the trimmer")),
        |ui| {
            let mpv_avail = crate::gui::views::trimmer::mpv::mpv_available();
            let status_desc = if mpv_avail {
                t("mpv found: 4K preview on the GPU").to_string()
            } else if cfg!(windows) {
                let dir = crate::gui::views::trimmer::mpv::mpv_plugin_dir();
                format!("{} {}", t("Put libmpv-2.dll in"), dir.display())
            } else {
                t("Install mpv with your package manager").to_string()
            };

            m3_setting_row(
                ui,
                theme,
                t("GPU preview (mpv)"),
                &status_desc,
                |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    m3_switch(ui, theme, &mut config.mpv_preview);
                    if !mpv_avail && cfg!(windows) {
                        if crate::gui::components::button::m3_small_button(ui, theme, t("Open folder")).clicked() {
                            let dir = crate::gui::views::trimmer::mpv::mpv_plugin_dir();
                            let _ = std::fs::create_dir_all(&dir);
                            let _ = open::that(&dir);
                        }
                    }
                },
            );
        },
    );
    ui.add_space(14.0);

    if !cfg!(windows) {
        m3_section_card(
            ui,
            theme,
            Some(IconKind::Keyboard),
            t("System Integration"),
            None,
            |ui| {
                capture::render_hotkey_backend(ui, theme, config);
            },
        );
        ui.add_space(14.0);
    }

    m3_section_card(
        ui,
        theme,
        Some(IconKind::Chip),
        t("Replay Buffer Internals"),
        None,
        |ui| {
            m3_setting_row(
                ui,
                theme,
                t("Replay buffer storage"),
                t("RAM is fastest. Disk saves memory for long or 4K buffers."),
                |ui| {
                    let options = [("ram".to_string(), t("RAM")), ("disk".to_string(), t("Disk"))];
                    crate::gui::components::segmented::segmented(ui, theme, "replay_storage_advanced", &mut config.replay_storage, &options, 80.0);
                },
            );
            if config.replay_storage == "ram" {
                ui.separator();
                m3_setting_row(
                    ui,
                    theme,
                    t("Protect against low memory"),
                    t("When free memory gets low during a game, the RAM buffer moves to disk instead of risking a crash."),
                    |ui| {
                        m3_switch(ui, theme, &mut config.low_memory_guard);
                    },
                );
            }
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
    ui.add_space(14.0);

    capture::render_share_privacy(ui, theme, config);
    ui.add_space(14.0);

    m3_section_card(
        ui,
        theme,
        Some(IconKind::Info),
        t("Diagnostics & Logs"),
        Some(t("Log files for troubleshooting and error reporting")),
        |ui| {
            m3_setting_row(
                ui,
                theme,
                t("Daemon Log"),
                &crate::report::show_path(&crate::daemon::log_path()),
                |ui| {
                    if crate::gui::components::button::m3_outlined_button(ui, theme, t("Open Log")).clicked() {
                        let _ = open::that(crate::daemon::log_path());
                    }
                },
            );
            ui.separator();
            m3_setting_row(
                ui,
                theme,
                t("Recorder Log"),
                &crate::report::show_path(&crate::recorder::log_path()),
                |ui| {
                    if crate::gui::components::button::m3_outlined_button(ui, theme, t("Open Log")).clicked() {
                        let _ = open::that(crate::recorder::log_path());
                    }
                },
            );
            ui.separator();
            let gui_log_path = crate::daemon::log_path().with_file_name("gui.log");
            m3_setting_row(
                ui,
                theme,
                t("Window Log"),
                &crate::report::show_path(&gui_log_path),
                |ui| {
                    if crate::gui::components::button::m3_outlined_button(ui, theme, t("Open Log")).clicked() {
                        let _ = open::that(&gui_log_path);
                    }
                },
            );
            ui.separator();
            let error_id = ui.id().with("report_error");
            m3_setting_row(
                ui,
                theme,
                t("Error report"),
                t("Writes lapse-report.txt with your system details and recent logs (user name removed) and opens a GitHub issue to attach it to."),
                |ui| {
                    if m3_small_button_colored(ui, theme, t("Report a bug"), theme.primary).clicked() {
                        match crate::report::open_bug_report() {
                            Ok(_) => {
                                ui.data_mut(|d| d.remove::<String>(error_id));
                            }
                            Err(e) => {
                                ui.data_mut(|d| d.insert_temp(error_id, e.to_string()));
                            }
                        }
                    }
                },
            );
            if let Some(err) = ui.data(|d| d.get_temp::<String>(error_id)) {
                ui.label(RichText::new(&err).size(12.0).color(theme.error));
                ui.add_space(4.0);
            }
        },
    );
}
