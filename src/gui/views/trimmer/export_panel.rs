//! The "Export" card of the trimmer: pick a preset, optionally drop audio, export.

use eframe::egui::{self, Margin, RichText, Stroke, Vec2};
use std::path::PathBuf;

use super::engine::{format_file_size, PlayerState};
use crate::export::{self, AudioMix, ExportJob, ExportPreset, ExportRequest, ExportState};
use crate::gui::components::button::{m3_filled_button_sized, m3_outlined_button};
use crate::gui::components::icon::{m3_vector_icon, IconKind};
use crate::gui::components::motion::{m3_switch_labeled, pulse};
use crate::gui::components::segmented::segmented;
use crate::gui::components::slider::{m3_slider, SliderOpts};
use crate::gui::components::button::m3_small_button;
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};

pub struct ExportPanel {
    pub preset: ExportPreset,
    pub audio: AudioMix,
    /// File name without extension; empty means "pick one automatically".
    pub name: String,
    pub job: Option<ExportJob>,
    /// Vertical preset: where the 9:16 frame sits (0 left .. 1 right).
    pub crop_x: f32,
    /// Send the result to Discord once the running export is done.
    pub send_to_discord: bool,
}

impl Default for ExportPanel {
    fn default() -> Self {
        Self { preset: ExportPreset::Fast, audio: AudioMix::default(), name: String::new(), job: None, crop_x: 0.5, send_to_discord: false }
    }
}

fn output_path(state: &PlayerState) -> PathBuf {
    let auto = export::default_output(&state.video_path, state.export.preset);
    let name = state.export.name.trim();
    if name.is_empty() {
        return auto;
    }
    let safe: String = name.chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect();
    auto.with_file_name(format!("{}.{}", safe, state.export.preset.extension()))
}

fn estimate(state: &PlayerState) -> String {
    let sel = state.trim_end.saturating_sub(state.trim_start).as_secs_f64();
    let ratio = (sel / state.duration.as_secs_f64().max(0.001)).clamp(0.0, 1.0);
    match state.export.preset {
        ExportPreset::Fast => format!("~{}", format_file_size((state.file_size_bytes as f64 * ratio) as u64)),
        ExportPreset::Discord => "< 10 MB".into(),
        ExportPreset::Mp3 => format!("~{}", format_file_size((sel * 190_000.0 / 8.0) as u64)),
        _ => String::new(),
    }
}

/// Sound switches: one for clips with a single track; game, microphone (+ level) when
/// the microphone was recorded separately. Plus loudness normalization.
fn audio_row(ui: &mut egui::Ui, theme: &MonetTheme, audio: &mut AudioMix, tracks: usize) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        if tracks == 0 {
            ui.label(RichText::new(t("This clip has no sound.")).size(12.0).color(theme.outline));
            return;
        }
        if tracks > 1 {
            m3_switch_labeled(ui, theme, &mut audio.game, t("Game audio"));
            m3_switch_labeled(ui, theme, &mut audio.mic, t("Microphone"));
            if audio.mic {
                ui.horizontal(|ui| {
                    ui.set_min_height(26.0);
                    let mut v = audio.mic_volume as f64;
                    let opts = crate::gui::components::slider::SliderOpts::default();
                    if crate::gui::components::slider::m3_slider(ui, theme, &mut v, 0.0..=3.0, 140.0, opts)
                        .on_hover_text(t("Microphone level in the export"))
                        .changed()
                    {
                        audio.mic_volume = v as _;
                    }
                    ui.label(RichText::new(format!("{:.0}%", audio.mic_volume * 100.0)).size(12.0).color(theme.on_surface_variant));
                });
            }
        } else {
            m3_switch_labeled(ui, theme, &mut audio.game, t("Sound"));
        }
        m3_switch_labeled(ui, theme, &mut audio.normalize, t("Even out loudness"))
            .on_hover_text(t("Normalizes the volume so every clip is about as loud (EBU R128)."));
    });
}

/// Returns a path to open in the player when the user clicks "Play" on a finished export.
pub fn render_export_panel(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState) -> Option<PathBuf> {
    let mut open_result = None;
    let running = state.export.job.as_ref().map(|j| j.is_running()).unwrap_or(false);

    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        m3_vector_icon(ui, IconKind::Save, 18.0, theme.primary);
        ui.label(RichText::new(t("Export")).size(15.0).strong().color(theme.on_surface));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let est = estimate(state);
            if !est.is_empty() {
                ui.label(RichText::new(est).size(12.0).color(theme.on_surface_variant));
            }
        });
    });
    ui.add_space(8.0);

    ui.add_enabled_ui(!running, |ui| {
        ui.horizontal_wrapped(|ui| {
            let options: Vec<(ExportPreset, &str)> = ExportPreset::ALL.iter().map(|p| (*p, t(p.label()))).collect();
            segmented(ui, theme, "export_preset", &mut state.export.preset, &options, 76.0);
        });
        if state.export.preset == ExportPreset::Vertical {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.set_min_height(26.0);
                ui.label(RichText::new(t("Frame position")).size(12.5).color(theme.on_surface));
                let mut x = state.export.crop_x as f64;
                if m3_slider(ui, theme, &mut x, 0.0..=1.0, 200.0, SliderOpts::default()).changed() {
                    state.export.crop_x = x as f32;
                }
                if m3_small_button(ui, theme, t("Center")).clicked() {
                    state.export.crop_x = 0.5;
                }
            });
        }
        if state.export.preset.has_audio_option() || state.export.preset == ExportPreset::Mp3 {
            ui.add_space(6.0);
            audio_row(ui, theme, &mut state.export.audio, state.audio_tracks);
        }
        ui.add_space(4.0);
        ui.label(RichText::new(t(state.export.preset.description())).size(12.0).color(theme.on_surface_variant));
        ui.add_space(8.0);
    });

    // "Share > Send to Discord" on a big clip: send once the Discord export is done.
    if state.export.send_to_discord {
        match state.export.job.as_ref().map(|j| j.state()) {
            Some(ExportState::Done { path, .. }) => {
                state.export.send_to_discord = false;
                crate::discord::send_as(state.video_path.clone(), path, webhook_url());
            }
            Some(ExportState::Running { .. }) => {}
            Some(ExportState::Failed(e)) => {
                state.export.send_to_discord = false;
                crate::discord::mark_failed(&state.video_path, &format!("shrinking failed: {}", e));
            }
            _ => state.export.send_to_discord = false,
        }
    }

    match state.export.job.as_ref().map(|j| j.state()) {
        Some(ExportState::Running { fraction, pass }) => {
            ui.horizontal(|ui| {
                let label = if pass > 1 {
                    format!("{} {:.0}% ({} {})", t("Exporting"), fraction * 100.0, t("pass"), pass)
                } else {
                    format!("{} {:.0}%", t("Exporting"), fraction * 100.0)
                };
                let w = (ui.available_width() - 90.0).max(80.0);
                ui.add(egui::ProgressBar::new(fraction.max(0.02)).desired_width(w).text(label).animate(true));
                if m3_outlined_button(ui, theme, t("Cancel")).clicked() {
                    if let Some(job) = &state.export.job {
                        job.cancel();
                    }
                }
            });
            // Keep the progress moving smoothly.
            let _ = pulse(ui, 1.0);
        }
        other => {
            let mut export_clicked = false;
            ui.horizontal(|ui| {
                ui.set_min_height(38.0);
                crate::gui::components::button::m3_field_style(ui, theme);
                let w = (ui.available_width() - 230.0).max(120.0);
                ui.add(
                    egui::TextEdit::singleline(&mut state.export.name)
                        .hint_text(t("File name (automatic)"))
                        .desired_width(w),
                );
                ui.label(RichText::new(format!(".{}", state.export.preset.extension())).monospace().size(12.0).color(theme.outline));
                export_clicked = m3_filled_button_sized(ui, theme, t("Export clip"), Vec2::new(ui.available_width().max(150.0), 38.0)).clicked();
            });
            if export_clicked {
                state.export.send_to_discord = false;
                start_export(state);
            }

            match other {
                Some(ExportState::Done { path, bytes }) => {
                    ui.add_space(8.0);
                    egui::Frame::none()
                        .fill(theme.success.gamma_multiply(0.12))
                        .stroke(Stroke::new(1.0_f32, theme.success.gamma_multiply(0.5)))
                        .rounding(M3Shapes::SMALL)
                        .inner_margin(Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                m3_vector_icon(ui, IconKind::Check, 15.0, theme.success);
                                let name = path.file_name().unwrap_or_default().to_string_lossy();
                                ui.label(RichText::new(format!("{} ({})", name, format_file_size(bytes))).size(12.5).color(theme.on_surface));
                            });
                            ui.horizontal(|ui| {
                                if m3_small_button(ui, theme, t("Play")).clicked() {
                                    open_result = Some(path.clone());
                                }
                                if m3_small_button(ui, theme, t("Show in folder")).clicked() {
                                    let _ = open::that(path.parent().unwrap_or(&path));
                                }
                                if m3_small_button(ui, theme, t("Share link")).clicked() {
                                    crate::share::start_live_share(path.clone());
                                }
                                if m3_small_button(ui, theme, t("Copy path")).clicked() {
                                    ui.output_mut(|o| o.copied_text = path.to_string_lossy().to_string());
                                }
                                discord_button(ui, theme, &path, bytes);
                            });
                        });
                }
                Some(ExportState::Failed(msg)) => {
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("{}: {}", t("Export failed"), msg)).size(12.0).color(theme.error));
                }
                Some(ExportState::Cancelled) => {
                    ui.add_space(6.0);
                    ui.label(RichText::new(t("Export cancelled.")).size(12.0).color(theme.outline));
                }
                _ => {}
            }
        }
    }
    open_result
}

/// "Send to Discord" through the webhook set in Settings > Services.
fn discord_button(ui: &mut egui::Ui, theme: &MonetTheme, path: &std::path::Path, bytes: u64) {
    let webhook = webhook_url();
    if webhook.is_empty() {
        return;
    }
    match crate::discord::state_for(path) {
        Some(crate::discord::SendState::Shrinking) => {
            ui.label(RichText::new(t("Shrinking for Discord...")).size(12.0).color(theme.primary));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
        Some(crate::discord::SendState::Sending) => {
            ui.label(RichText::new(t("Sending to Discord...")).size(12.0).color(theme.primary));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
        Some(crate::discord::SendState::Sent) => {
            ui.label(RichText::new(t("Sent to Discord")).size(12.0).color(theme.success));
        }
        Some(crate::discord::SendState::Failed(e)) => {
            ui.label(RichText::new(format!("{} {}", t("Discord:"), e)).size(12.0).color(theme.error));
        }
        None if bytes > crate::discord::MAX_BYTES => {
            ui.add_enabled_ui(false, |ui| m3_small_button(ui, theme, t("Send to Discord"))).inner
                .on_disabled_hover_text(t("Over 10 MB: export with the Discord preset first."));
        }
        None => {
            if m3_small_button(ui, theme, t("Send to Discord")).clicked() {
                crate::discord::send(path.to_path_buf(), webhook);
            }
        }
    }
}

pub(super) use crate::gui::views::share_menu::webhook_url;

fn start_export(state: &mut PlayerState) {
    state.pause();
    let req = ExportRequest {
        input: state.video_path.clone(),
        start: state.trim_start.as_secs_f64(),
        end: state.trim_end.as_secs_f64(),
        preset: state.export.preset,
        audio: if state.export.preset == ExportPreset::Gif { AudioMix::muted() } else { state.export.audio.clone() },
        output: output_path(state),
        crop_x: state.export.crop_x,
    };
    state.export.job = Some(export::start(req));
}

/// Exports the selection with the Discord preset (under 10 MB) and sends it when done.
pub(super) fn export_for_discord(state: &mut PlayerState) {
    if state.export.job.as_ref().is_some_and(|j| j.is_running()) {
        return;
    }
    state.export.preset = ExportPreset::Discord;
    state.export.send_to_discord = true;
    crate::discord::mark_shrinking(&state.video_path);
    start_export(state);
}

