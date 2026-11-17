use eframe::egui::{self, Color32, Margin, RichText, Stroke, Vec2};
use crate::gui::i18n::t;
use rfd::FileDialog;

use crate::config::Config;
use crate::gui::components::dropdown::m3_dropdown;
use crate::gui::components::motion::m3_switch;
use crate::hotkeys::HotkeyEvent;
use crate::hardware::audio::AudioDevice;
use crate::hardware::gpu::GpuDevice;
use crate::hardware::monitor::MonitorDevice;
use crate::ipc;

use crate::gui::components::button::{m3_filled_button_sized, m3_outlined_button};
use crate::gui::components::card::m3_section_card;
use crate::gui::components::chip::{m3_badge, m3_keycap};
use crate::gui::components::header::m3_page_header;
use crate::gui::components::icon::{m3_vector_icon, IconKind};
use crate::gui::components::input::{
    m3_bitrate_slider_input, m3_duration_slider_input, m3_fps_slider_input, m3_setting_row,
};
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
        t("Tune hardware encoders, custom replay buffer durations, audio channels, and init services"),
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
                    let (gpus, status) = (hardware.available_gpus, hardware.status);
                    capture::render_quality_and_buffer(ui, theme, config, hardware.available_monitors);
                    ui.add_space(14.0);
                    capture::render_hdr(ui, theme, config);
                    ui.add_space(14.0);
                    render_recording_tab(ui, theme, config, hardware);
                    ui.add_space(14.0);
                    capture::render_save_behavior(ui, theme, config);
                    ui.add_space(14.0);
                    crate::gui::views::dual_gpu_view::render_advanced(
                        ui,
                        theme,
                        config,
                        gpus,
                        status,
                    );
                }
                1 => crate::gui::views::games_view::render_games_tab(ui, theme, config, hardware.status),
                2 => {
                    let status = hardware.status;
                    render_audio_tab(ui, theme, config, hardware);
                    ui.add_space(14.0);
                    capture::render_audio_tracks(ui, theme, config);
                    ui.add_space(14.0);
                    crate::gui::views::sync_view::render_audio_sync(ui, theme, config, status);
                }
                3 => {
                    render_hotkeys_tab(ui, theme, config, binding_hotkey);
                    ui.add_space(14.0);
                    crate::gui::components::card::m3_card(ui, theme, |ui| {
                        capture::render_hotkey_backend(ui, theme, config);
                    });
                }
                4 => {
                    render_services_tab(ui, theme, config);
                    ui.add_space(14.0);
                    capture::render_storage_limit(ui, theme, config);
                    ui.add_space(14.0);
                    capture::render_share_privacy(ui, theme, config);
                    ui.add_space(14.0);
                    capture::render_discord(ui, theme, config);
                }
                5 => {
                    capture::render_language(ui, theme, config);
                    ui.add_space(14.0);
                    render_appearance_tab(ui, theme, palette_kind);
                }
                _ => render_updates_tab(ui, theme, config),
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
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Video),
        t("Video Encoder & GPU Hardware Acceleration"),
        Some(t("Select GPU encoder codec (AV1, H.265/HEVC, H.264) according to your graphics card")),
        |ui| {
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
                t("Target audio playback device from PipeWire / PulseAudio"),
                |ui| {
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
                t("Input microphone device from PipeWire / PulseAudio"),
                |ui| {
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
                },
            );
        },
    );
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
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Gear),
        t("Autostart"),
        Some(t("Run Lapse in the background when you log in")),
        |ui| {
            m3_setting_row(
                ui,
                theme,
                t("Launch on Login"),
                t("Starts the replay buffer when your desktop session starts"),
                |ui| {
                    let mut autostart = config.autostart;
                    if m3_switch(ui, theme, &mut autostart).changed() {
                        config.autostart = autostart;
                        Config::sync_autostart(autostart);
                    }
                },
            );

        },
    );
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
    on_command(ipc::Command::ReloadConfig);
    *status_banner = Some((
        "Settings successfully applied to engine!".into(),
        std::time::Instant::now(),
    ));
}
