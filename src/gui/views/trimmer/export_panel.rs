//! The "Export" card of the trimmer: compact action row, share targets, and collapsible settings.

use eframe::egui::{self, Margin, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};
use std::path::PathBuf;
use std::time::Duration;

use super::engine::{format_file_size, PlayerState};
use crate::export::{self, AudioFormat, AudioMix, CropAspect, CropRect, ExportJob, ExportPreset, ExportRequest, ExportState};
use crate::gui::components::button::{m3_filled_button_sized, m3_outlined_button, m3_small_button};
use crate::gui::components::chip::m3_chip;
use crate::gui::components::dropdown::m3_dropdown_sized;
use crate::gui::components::icon::{draw_vector_icon, m3_vector_icon, IconKind};
use crate::gui::components::motion::{hover_t, lerp_color, m3_switch, m3_switch_labeled, pulse, MEDIUM};
use crate::gui::components::segmented::segmented;
use crate::gui::components::slider::{m3_slider, SliderOpts};
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};
use super::overlays::{self, Overlay};

/// Settings > Services, where the Discord webhook and Streamable accounts are set.
const SERVICES_TAB: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PostExportAction {
    None,
    SaveOnly,
    Discord,
    Streamable,
    LiveShare,
    CopyPath,
}

pub struct ExportPanel {
    pub preset: ExportPreset,
    pub audio: AudioMix,
    /// File name without extension; empty means "pick one automatically".
    pub name: String,
    pub job: Option<ExportJob>,
    pub crop: Option<CropRect>,
    pub crop_aspect: CropAspect,
    pub last_preset: ExportPreset,
    /// Send the result to Discord once the running export is done.
    pub send_to_discord: bool,
    pub post_action: PostExportAction,
    pub overlays: Vec<Overlay>,
    pub selected: Option<usize>,
    pub last_exported_path: Option<PathBuf>,
    pub last_exported_bytes: u64,
}

impl Default for ExportPanel {
    fn default() -> Self {
        Self {
            preset: ExportPreset::Fast,
            audio: AudioMix::default(),
            name: String::new(),
            job: None,
            crop: None,
            crop_aspect: CropAspect::Free,
            last_preset: ExportPreset::Fast,
            send_to_discord: false,
            post_action: PostExportAction::None,
            overlays: Vec::new(),
            selected: None,
            last_exported_path: None,
            last_exported_bytes: 0,
        }
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
        ExportPreset::Audio(fmt) => {
            let bps = match fmt {
                AudioFormat::Mp3 => 190_000.0,
                AudioFormat::Opus => 160_000.0,
                AudioFormat::Aac => 192_000.0,
                AudioFormat::Flac => 900_000.0,
                AudioFormat::Wav => 1_411_000.0,
            };
            format!("~{}", format_file_size((sel * bps / 8.0) as u64))
        }
        _ => String::new(),
    }
}

fn render_letter_badge(ui: &mut egui::Ui, theme: &MonetTheme, rect: Rect, label: &str) {
    ui.painter().rect_filled(rect, Rounding::same(3.0), theme.primary);
    let ch = label.chars().next().unwrap_or('A').to_uppercase().to_string();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        &ch,
        egui::FontId::proportional(11.0),
        theme.on_primary,
    );
}

fn render_track_icon(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    rect: Rect,
    track_name: &str,
    label: &str,
) {
    if let Some(app) = track_name.strip_prefix("app:") {
        #[cfg(not(windows))]
        let icon = crate::audio_apps::identity(app)
            .and_then(|i| [i.desktop_id, i.binary].into_iter().filter(|v| !v.is_empty()).find_map(|v| crate::app_icons::get_app_icon(&v)))
            .or_else(|| crate::app_icons::get_app_icon(app));
        #[cfg(windows)]
        let icon = crate::app_icons::get_app_icon(app);
        if let Some(icon_path) = icon {
            egui::Image::new(format!("file://{}", icon_path.display()))
                .rounding(Rounding::same(3.0))
                .paint_at(ui, rect);
            return;
        }
        render_letter_badge(ui, theme, rect, label);
        return;
    }

    match track_name {
        "game" => {
            draw_vector_icon(ui.painter(), rect.center(), 18.0, IconKind::Gamepad, theme.primary);
        }
        "mic" => {
            draw_vector_icon(ui.painter(), rect.center(), 18.0, IconKind::Mic, theme.primary);
        }
        "chat" => {
            render_letter_badge(ui, theme, rect, label);
        }
        _ => {
            draw_vector_icon(ui.painter(), rect.center(), 18.0, IconKind::Audio, theme.primary);
        }
    }
}

/// Sound switches: game, microphone (+ 0..300% level with limiter) when recorded separately.
/// Plus loudness normalization.
fn audio_row(ui: &mut egui::Ui, theme: &MonetTheme, audio: &mut AudioMix, tracks: usize, track_names: &[String], silent: &[bool]) {
    if tracks == 0 {
        ui.label(RichText::new(t("This clip has no sound.")).size(12.0).color(theme.outline));
        return;
    }

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;

        if tracks > 1 {
            if audio.tracks.is_empty() {
                audio.init_tracks(track_names);
            }
            for (i, track) in audio.tracks.iter_mut().enumerate() {
                if silent.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let label = crate::tracks::track_label(&track.name);
                ui.horizontal(|ui| {
                    ui.set_min_height(26.0);
                    ui.spacing_mut().item_spacing.x = 8.0;

                    let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), egui::Sense::hover());
                    render_track_icon(ui, theme, icon_rect, &track.name, &label);

                    // Fixed-width, left-aligned name column so the switches line up.
                    let lbl = ui
                        .allocate_ui_with_layout(
                            Vec2::new(130.0, 26.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(130.0);
                                ui.add(
                                    egui::Label::new(RichText::new(&label).size(12.5).color(theme.on_surface))
                                        .truncate(true)
                                        .sense(egui::Sense::click()),
                                )
                            },
                        )
                        .inner;
                    if lbl.clicked() {
                        track.enabled = !track.enabled;
                    }

                    m3_switch(ui, theme, &mut track.enabled);

                    if track.enabled {
                        let mut v = track.volume as f64;
                        let opts = SliderOpts::default();
                        if m3_slider(ui, theme, &mut v, 0.0..=3.0, 110.0, opts)
                            .on_hover_text(format!("{} {}", label, t("level in the export")))
                            .changed()
                        {
                            track.volume = v as _;
                        }
                        ui.label(
                            RichText::new(format!("{:.0}%", track.volume * 100.0))
                                .size(12.0)
                                .color(theme.on_surface_variant),
                        );
                    }
                });
            }

            // Keep legacy fields synchronized
            if let Some(g) = audio.tracks.iter().find(|t| t.name == "game") {
                audio.game = g.enabled;
            }
            if let Some(m) = audio.tracks.iter().find(|t| t.name == "mic") {
                audio.mic = m.enabled;
                audio.mic_volume = m.volume;
            }
        } else {
            ui.horizontal(|ui| {
                ui.set_min_height(26.0);
                ui.spacing_mut().item_spacing.x = 8.0;

                let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), egui::Sense::hover());
                draw_vector_icon(ui.painter(), icon_rect.center(), 18.0, IconKind::Gamepad, theme.primary);

                let label = t("Sound");
                let lbl = ui.add_sized(
                    [120.0, 26.0],
                    egui::Label::new(RichText::new(label).size(12.5).color(theme.on_surface))
                        .sense(egui::Sense::click()),
                );
                if lbl.clicked() {
                    audio.game = !audio.game;
                }

                m3_switch(ui, theme, &mut audio.game);
            });
        }

        ui.add_space(2.0);
        m3_switch_labeled(ui, theme, &mut audio.normalize, t("Even out loudness"))
            .on_hover_text(t("Normalizes the volume so every clip is about as loud (EBU R128)."));
    });
}

/// Collapsible section container: closed by default, open state remembered per session in egui temp data.
fn collapsible_section<R>(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    id: egui::Id,
    icon: IconKind,
    title: &str,
    badge: Option<&str>,
    force_open: bool,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let mut is_open: bool = ui.ctx().data_mut(|d| d.get_temp(id)).unwrap_or(false);
    if force_open {
        is_open = true;
        ui.ctx().data_mut(|d| d.insert_temp(id, true));
    }

    egui::Frame::none()
        .fill(theme.surface_container_low)
        .stroke(Stroke::new(1.0_f32, theme.outline_variant))
        .rounding(M3Shapes::MEDIUM)
        .inner_margin(Margin::symmetric(14.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let header_w = ui.available_width();
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(header_w, 24.0), Sense::click());
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                is_open = !is_open;
                ui.ctx().data_mut(|d| d.insert_temp(id, is_open));
            }
            let h = hover_t(ui, &resp);
            let painter = ui.painter();
            let title_color = lerp_color(theme.on_surface, theme.primary, h);

            let mut x = rect.left();
            draw_vector_icon(painter, Pos2::new(x + 9.0, rect.center().y), 16.0, icon, theme.primary);
            x += 24.0;
            let title_galley = painter.layout_no_wrap(title.to_string(), egui::FontId::proportional(13.0), title_color);
            let title_w = title_galley.size().x;
            painter.galley(Pos2::new(x, rect.center().y - title_galley.size().y / 2.0), title_galley, title_color);
            x += title_w + 12.0;

            if let Some(b) = badge {
                let badge_galley = painter.layout_no_wrap(b.to_string(), egui::FontId::proportional(11.5), theme.on_surface_variant);
                painter.galley(Pos2::new(x, rect.center().y - badge_galley.size().y / 2.0), badge_galley, theme.on_surface_variant);
            }

            let turn = ui.ctx().animate_bool_with_time(id.with("turn"), is_open, MEDIUM);
            let c = Pos2::new(rect.right() - 10.0, rect.center().y);
            let dir = 1.0 - 2.0 * turn;
            let pts = vec![c + Vec2::new(-4.0, -2.0 * dir), c + Vec2::new(0.0, 2.0 * dir), c + Vec2::new(4.0, -2.0 * dir)];
            painter.add(egui::Shape::line(pts, Stroke::new(1.8_f32, lerp_color(theme.on_surface_variant, theme.primary, h.max(turn)))));

            if is_open {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
                Some(content(ui))
            } else {
                None
            }
        })
        .inner
}

/// Returns a path to open in the player when the user clicks "Play" on a finished export.
pub fn render_export_panel(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState) -> Option<PathBuf> {
    let mut open_result = None;

    // Handle completed export and background share tasks
    handle_export_events(ui, state);

    // One compact action row: Save clip button, Share dropdown, inline progress and results
    action_row(ui, theme, state, &mut open_result);
    ui.add_space(8.0);

    // Collapsible sections: Format, Crop, Text & media, Audio
    render_collapsible_sections(ui, theme, state);

    open_result
}

fn handle_export_events(ui: &mut egui::Ui, state: &mut PlayerState) {
    match state.export.job.as_ref().map(|j| j.state()) {
        Some(ExportState::Done { path, bytes }) => {
            let action = state.export.post_action;
            if action != PostExportAction::None {
                state.export.post_action = PostExportAction::None;
                state.export.last_exported_path = Some(path.clone());
                state.export.last_exported_bytes = bytes;
                match action {
                    PostExportAction::Discord => {
                        crate::discord::send_as(state.video_path.clone(), path, webhook_url());
                    }
                    PostExportAction::Streamable => {
                        crate::streamable::upload(path);
                    }
                    PostExportAction::LiveShare => {
                        crate::share::start_live_share(path);
                    }
                    PostExportAction::CopyPath => {
                        let path_str = path.to_string_lossy().to_string();
                        ui.output_mut(|o| o.copied_text = path_str.clone());
                        crate::streamable::copy_to_clipboard(&path_str);
                        crate::utils::send_notification("Lapse", &t("Path copied"), "emblem-shared");
                    }
                    PostExportAction::SaveOnly => {}
                    PostExportAction::None => {}
                }
            } else if state.export.send_to_discord {
                state.export.send_to_discord = false;
                crate::discord::send_as(state.video_path.clone(), path, webhook_url());
            }
        }
        Some(ExportState::Failed(e)) => {
            if state.export.post_action == PostExportAction::Discord || state.export.send_to_discord {
                crate::discord::mark_failed(&state.video_path, &format!("shrinking failed: {}", e));
            }
            state.export.post_action = PostExportAction::None;
            state.export.send_to_discord = false;
        }
        Some(ExportState::Cancelled) => {
            state.export.post_action = PostExportAction::None;
            state.export.send_to_discord = false;
        }
        _ => {}
    }
}

fn action_row(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    state: &mut PlayerState,
    open_result: &mut Option<PathBuf>,
) {
    let running = state.export.job.as_ref().map(|j| j.is_running()).unwrap_or(false);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
        ui.set_min_height(34.0);

        if running {
            ui.add_enabled_ui(false, |ui| {
                m3_filled_button_sized(ui, theme, t("Save clip"), Vec2::new(105.0, 34.0))
            });
            ui.add_enabled_ui(false, |ui| {
                m3_outlined_button(ui, theme, t("Share"))
            });

            if let Some(ExportState::Running { fraction, pass }) = state.export.job.as_ref().map(|j| j.state()) {
                let label = if pass > 1 {
                    format!("{} {:.0}% ({} {})", t("Exporting"), fraction * 100.0, t("pass"), pass)
                } else {
                    format!("{} {:.0}%", t("Exporting"), fraction * 100.0)
                };
                let w = (ui.available_width() - 90.0).clamp(120.0, 240.0);
                ui.add(egui::ProgressBar::new(fraction.max(0.02)).desired_width(w).text(label).animate(true));
                if m3_outlined_button(ui, theme, t("Cancel")).clicked() {
                    if let Some(job) = &state.export.job {
                        job.cancel();
                    }
                }
            }
            let _ = pulse(ui, 1.0);
        } else {
            // Primary button "Save clip"
            if m3_filled_button_sized(ui, theme, t("Save clip"), Vec2::new(105.0, 34.0))
                .on_hover_text(t("Export the current selection"))
                .clicked()
            {
                state.export.post_action = PostExportAction::SaveOnly;
                start_export(state);
            }

            // Share dropdown
            render_share_dropdown(ui, theme, state);

            // Inline results / status
            render_inline_status(ui, theme, state, open_result);
        }
    });
}

fn render_share_dropdown(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState) {
    let webhook = webhook_url();
    let has_webhook = !webhook.is_empty();
    let has_streamable = crate::streamable::is_configured();

    m3_dropdown_sized(ui, theme, "trimmer_action_share", t("Share"), 105.0, 34.0, |ui| {
        // Send to Discord
        let discord_hint = if !has_webhook {
            t("Set a webhook in Settings > Services first.")
        } else {
            t("Exports the selection to fit Discord, then sends it.")
        };
        if ui.selectable_label(false, t("Send to Discord")).on_hover_text(discord_hint).clicked() {
            if has_webhook {
                export_for_discord(state);
            } else {
                crate::gui::components::notice::request_setup(
                    ui.ctx(),
                    t("Discord isn't set up yet"),
                    t("To send clips to a Discord channel, paste the channel's webhook link in Settings > Services > Discord."),
                    SERVICES_TAB,
                );
            }
        }

        // Upload to Streamable
        let streamable_hint = if !has_streamable {
            t("Configure your Streamable account in Settings > Services first.")
        } else {
            t("Exports the selection, then uploads to Streamable.")
        };
        if ui.selectable_label(false, t("Upload to Streamable")).on_hover_text(streamable_hint).clicked() {
            if has_streamable {
                export_for_streamable(state);
            } else {
                crate::gui::components::notice::request_setup(
                    ui.ctx(),
                    t("Streamable isn't set up yet"),
                    t("To upload clips to Streamable, enter your account email and password in Settings > Services > Streamable."),
                    SERVICES_TAB,
                );
            }
        }

        // Share link
        if ui
            .selectable_label(false, t("Share link"))
            .on_hover_text(t("Streams the clip through a Cloudflare proxy. Anyone with the link can watch."))
            .clicked()
        {
            export_for_live_share(state);
        }

        // Copy path
        if ui
            .selectable_label(false, t("Copy path"))
            .on_hover_text(t("Exports the clip and copies the file path to clipboard."))
            .clicked()
        {
            export_for_copy_path(state);
        }
    });
}

fn render_inline_status(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    state: &mut PlayerState,
    open_result: &mut Option<PathBuf>,
) {
    let mut shown_status = false;

    // Discord status
    let discord_state = crate::discord::state_for(&state.video_path)
        .or_else(|| state.export.last_exported_path.as_ref().and_then(|p| crate::discord::state_for(p)));

    if let Some(ds) = discord_state {
        match ds {
            crate::discord::SendState::Shrinking => {
                ui.label(RichText::new(t("Shrinking for Discord...")).size(12.0).color(theme.primary));
                ui.ctx().request_repaint_after(Duration::from_millis(250));
                shown_status = true;
            }
            crate::discord::SendState::Sending => {
                ui.label(RichText::new(t("Sending to Discord...")).size(12.0).color(theme.primary));
                ui.ctx().request_repaint_after(Duration::from_millis(250));
                shown_status = true;
            }
            crate::discord::SendState::Sent => {
                ui.horizontal(|ui| {
                    m3_vector_icon(ui, IconKind::Check, 14.0, theme.success);
                    ui.label(RichText::new(t("Sent to Discord")).size(12.0).color(theme.success));
                });
                shown_status = true;
            }
            crate::discord::SendState::Failed(e) => {
                ui.label(RichText::new(format!("{} {}", t("Discord:"), e)).size(12.0).color(theme.error));
                shown_status = true;
            }
        }
    }

    // Streamable status
    let streamable_state = state
        .export
        .last_exported_path
        .as_ref()
        .and_then(|p| crate::streamable::state_for(p))
        .or_else(|| crate::streamable::state_for(&state.video_path));

    if let Some(ss) = streamable_state {
        match ss {
            crate::streamable::UploadState::Uploading => {
                ui.label(RichText::new(t("Uploading to Streamable...")).size(12.0).color(theme.primary));
                ui.ctx().request_repaint_after(Duration::from_millis(250));
                shown_status = true;
            }
            crate::streamable::UploadState::Done(url) => {
                ui.horizontal(|ui| {
                    m3_vector_icon(ui, IconKind::Check, 14.0, theme.success);
                    ui.add(egui::Label::new(RichText::new(&url).size(11.5).color(theme.success)).truncate(true));
                    if m3_small_button(ui, theme, t("Copy")).clicked() {
                        crate::streamable::copy_to_clipboard(&url);
                        crate::utils::send_notification("Lapse", "Streamable link copied", "emblem-shared");
                    }
                });
                shown_status = true;
            }
            crate::streamable::UploadState::Failed(e) => {
                ui.label(RichText::new(format!("{} {}", t("Streamable:"), e)).size(12.0).color(theme.error));
                shown_status = true;
            }
        }
    }

    // If export is done and no upload in progress taking precedence
    if !shown_status {
        if let Some(ExportState::Done { path, bytes }) = state.export.job.as_ref().map(|j| j.state()) {
            ui.horizontal(|ui| {
                m3_vector_icon(ui, IconKind::Check, 14.0, theme.success);
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                ui.label(RichText::new(format!("{} ({})", name, format_file_size(bytes))).size(12.0).color(theme.on_surface));
                if m3_small_button(ui, theme, t("Play")).clicked() {
                    *open_result = Some(path.clone());
                }
                if m3_small_button(ui, theme, t("Show in folder")).clicked() {
                    let _ = open::that(path.parent().unwrap_or(&path));
                }
                if m3_small_button(ui, theme, t("Copy path")).clicked() {
                    let p_str = path.to_string_lossy().to_string();
                    ui.output_mut(|o| o.copied_text = p_str.clone());
                    crate::streamable::copy_to_clipboard(&p_str);
                    crate::utils::send_notification("Lapse", &t("Path copied"), "emblem-shared");
                }
                let duration = (state.trim_end - state.trim_start).as_secs_f64();
                discord_button(ui, theme, &path, bytes, duration);
            });
            shown_status = true;
        } else if let Some(ExportState::Failed(msg)) = state.export.job.as_ref().map(|j| j.state()) {
            ui.label(RichText::new(format!("{}: {}", t("Export failed"), msg)).size(12.0).color(theme.error));
            shown_status = true;
        }
    }

    // If completely idle
    if !shown_status {
        let est = estimate(state);
        if !est.is_empty() {
            ui.label(RichText::new(est).size(12.0).color(theme.on_surface_variant));
        }
    }
}

fn render_collapsible_sections(ui: &mut egui::Ui, theme: &MonetTheme, state: &mut PlayerState) {
    let running = state.export.job.as_ref().map(|j| j.is_running()).unwrap_or(false);

    ui.add_enabled_ui(!running, |ui| {
        // 1. Format section
        let format_id = ui.id().with("section_format");
        let preset_label = t(state.export.preset.label());
        collapsible_section(ui, theme, format_id, IconKind::Video, &t("Format"), Some(preset_label), false, |ui| {
            let prev_preset = state.export.preset;
            ui.horizontal_wrapped(|ui| {
                let options: Vec<(ExportPreset, &str)> = ExportPreset::ALL.iter().map(|p| (*p, t(p.label()))).collect();
                let mut picker_preset = match state.export.preset {
                    ExportPreset::Audio(_) => ExportPreset::Audio(AudioFormat::Mp3),
                    other => other,
                };
                if segmented(ui, theme, "export_preset", &mut picker_preset, &options, 76.0).is_some() {
                    state.export.preset = match picker_preset {
                        ExportPreset::Audio(_) => match state.export.preset {
                            ExportPreset::Audio(fmt) => ExportPreset::Audio(fmt),
                            _ => ExportPreset::Audio(AudioFormat::Mp3),
                        },
                        other => other,
                    };
                }
            });
            if state.export.preset != prev_preset || state.export.preset != state.export.last_preset {
                if state.export.preset == ExportPreset::Vertical {
                    state.export.crop_aspect = CropAspect::Vertical9x16;
                    state.export.crop = Some(CropRect::default_for_aspect(
                        state.orig_width,
                        state.orig_height,
                        CropAspect::Vertical9x16,
                    ));
                }
                state.export.last_preset = state.export.preset;
            } else if state.export.preset == ExportPreset::Vertical && state.export.crop.is_none() {
                state.export.crop_aspect = CropAspect::Vertical9x16;
                state.export.crop = Some(CropRect::default_for_aspect(
                    state.orig_width,
                    state.orig_height,
                    CropAspect::Vertical9x16,
                ));
            }

            ui.add_space(4.0);
            ui.label(RichText::new(t(state.export.preset.description())).size(12.0).color(theme.on_surface_variant));

            if let ExportPreset::Audio(current_fmt) = state.export.preset {
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for &fmt in &AudioFormat::ALL {
                        let sel = current_fmt == fmt;
                        if m3_chip(ui, theme, fmt.label(), sel).clicked() {
                            state.export.preset = ExportPreset::Audio(fmt);
                        }
                    }
                });
            }

            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let w = (ui.available_width() - 60.0).clamp(140.0, 320.0);
                crate::gui::components::text_field::m3_text_field(
                    ui,
                    theme,
                    &mut state.export.name,
                    crate::gui::components::text_field::TextFieldOpts {
                        hint: t("File name (automatic)"),
                        width: Some(w),
                        ..Default::default()
                    },
                );
                ui.label(RichText::new(format!(".{}", state.export.preset.extension())).monospace().size(12.0).color(theme.outline));
            });
        });

        ui.add_space(6.0);

        // 2. Crop section
        let crop_id = ui.id().with("section_crop");
        let is_video = !matches!(state.export.preset, ExportPreset::Audio(_));
        let crop_active = is_video && state.export.crop.is_some();
        let crop_badge = if crop_active { t(state.export.crop_aspect.label()) } else { t("Off") };
        collapsible_section(ui, theme, crop_id, IconKind::Scissors, &t("Crop"), Some(crop_badge), crop_active, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let mut crop_on = is_video && state.export.crop.is_some();
                if m3_switch_labeled(ui, theme, &mut crop_on, t("Crop")).changed() {
                    if crop_on {
                        let aspect = if state.export.preset == ExportPreset::Vertical {
                            CropAspect::Vertical9x16
                        } else {
                            state.export.crop_aspect
                        };
                        state.export.crop_aspect = aspect;
                        state.export.crop = Some(CropRect::default_for_aspect(
                            state.orig_width,
                            state.orig_height,
                            aspect,
                        ));
                    } else if state.export.preset != ExportPreset::Vertical {
                        state.export.crop = None;
                    }
                }
                if is_video && state.export.crop.is_some() {
                    for aspect in CropAspect::ALL {
                        let sel = state.export.crop_aspect == aspect;
                        let chip_text = t(aspect.label());
                        if m3_chip(ui, theme, chip_text, sel).clicked() {
                            state.export.crop_aspect = aspect;
                            if let Some(c) = state.export.crop.as_mut() {
                                *c = c.adjust_to_aspect(state.orig_width, state.orig_height, aspect);
                            }
                        }
                    }
                    if m3_small_button(ui, theme, t("Reset")).clicked() {
                        state.export.crop = Some(CropRect::default_for_aspect(
                            state.orig_width,
                            state.orig_height,
                            state.export.crop_aspect,
                        ));
                    }
                }
            });
        });

        ui.add_space(6.0);

        // 3. Text & media section
        let overlays_id = ui.id().with("section_overlays");
        let overlay_count = state.export.overlays.len();
        let overlay_badge = if overlay_count > 0 { Some(format!("{}", overlay_count)) } else { None };
        let overlay_force = state.export.selected.is_some();
        collapsible_section(
            ui,
            theme,
            overlays_id,
            IconKind::Pencil,
            &t("Text & media"),
            overlay_badge.as_deref(),
            overlay_force,
            |ui| {
                overlays::render_export_panel_overlays(ui, theme, state);
            },
        );

        ui.add_space(6.0);

        // 4. Audio section
        let audio_id = ui.id().with("section_audio");
        let mic_str;
        let is_gif = !state.export.preset.has_audio_option() && !matches!(state.export.preset, ExportPreset::Audio(_));
        let audio_badge: Option<&str> = if is_gif {
            Some(t("Muted (GIF)"))
        } else if state.audio_tracks == 0 {
            Some(t("No sound"))
        } else if !state.export.audio.tracks.is_empty() {
            // Only the tracks changed from their default, e.g. "Game & desktop 50% · Chat & music off".
            mic_str = state
                .export
                .audio
                .tracks
                .iter()
                .filter(|tr| !tr.enabled || (tr.volume - 1.0).abs() >= 0.005)
                .map(|tr| {
                    let label = crate::tracks::track_label(&tr.name);
                    if tr.enabled {
                        format!("{} {:.0}%", label, tr.volume * 100.0)
                    } else {
                        format!("{} {}", label, t("off"))
                    }
                })
                .collect::<Vec<_>>()
                .join(" · ");
            (!mic_str.is_empty()).then_some(mic_str.as_str())
        } else if state.audio_tracks > 1 && state.export.audio.mic {
            mic_str = format!("Mic {:.0}%", state.export.audio.mic_volume * 100.0);
            Some(&mic_str)
        } else {
            None
        };
        collapsible_section(
            ui,
            theme,
            audio_id,
            IconKind::Audio,
            &t("Audio"),
            audio_badge,
            false,
            |ui| {
                if is_gif {
                    ui.label(RichText::new(t("GIF exports have no audio.")).size(12.0).color(theme.outline));
                } else {
                    let silent = state.silent_tracks.lock().map(|s| s.clone()).unwrap_or_default();
                    audio_row(ui, theme, &mut state.export.audio, state.audio_tracks, &state.track_names, &silent);
                }
            },
        );
    });
}

/// "Send to Discord" through the webhook set in Settings > Services.
/// Never disabled for big files: automatically uses send_or_shrink.
fn discord_button(ui: &mut egui::Ui, theme: &MonetTheme, path: &std::path::Path, bytes: u64, duration: f64) {
    let webhook = webhook_url();
    if webhook.is_empty() {
        return;
    }
    match crate::discord::state_for(path) {
        Some(crate::discord::SendState::Shrinking) => {
            ui.label(RichText::new(t("Shrinking for Discord...")).size(12.0).color(theme.primary));
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        Some(crate::discord::SendState::Sending) => {
            ui.label(RichText::new(t("Sending to Discord...")).size(12.0).color(theme.primary));
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        Some(crate::discord::SendState::Sent) => {
            ui.label(RichText::new(t("Sent to Discord")).size(12.0).color(theme.success));
        }
        Some(crate::discord::SendState::Failed(e)) => {
            ui.label(RichText::new(format!("{} {}", t("Discord:"), e)).size(12.0).color(theme.error));
        }
        None => {
            let hint = if bytes > crate::discord::MAX_BYTES {
                t("Over 10 MB: the clip is shrunk with the Discord preset, then sent.")
            } else {
                t("Uploads the clip to your Discord channel.")
            };
            if m3_small_button(ui, theme, t("Send to Discord")).on_hover_text(hint).clicked() {
                crate::discord::send_or_shrink(path.to_path_buf(), webhook, duration);
            }
        }
    }
}

pub(super) use crate::gui::views::share_menu::webhook_url;

fn start_export(state: &mut PlayerState) {
    state.pause();
    let overlays = state.export.overlays.clone();
    let req = ExportRequest {
        input: state.video_path.clone(),
        start: state.trim_start.as_secs_f64(),
        end: state.trim_end.as_secs_f64(),
        preset: state.export.preset,
        audio: if state.export.preset == ExportPreset::Gif { AudioMix::muted() } else { state.export.audio.clone() },
        output: output_path(state),
        crop: if matches!(state.export.preset, ExportPreset::Audio(_)) { None } else { state.export.crop },
        overlays,
    };
    state.export.job = Some(export::start(req));
}

/// Exports the selection with the Discord preset (under 10 MB) and sends it when done.
pub fn export_for_discord(state: &mut PlayerState) {
    if state.export.job.as_ref().is_some_and(|j| j.is_running()) {
        return;
    }
    state.export.preset = ExportPreset::Discord;
    state.export.post_action = PostExportAction::Discord;
    state.export.send_to_discord = true;
    crate::discord::mark_shrinking(&state.video_path);
    start_export(state);
}

/// Exports the selection and uploads to Streamable when done.
pub fn export_for_streamable(state: &mut PlayerState) {
    if state.export.job.as_ref().is_some_and(|j| j.is_running()) {
        return;
    }
    state.export.post_action = PostExportAction::Streamable;
    start_export(state);
}

/// Exports the selection and starts live share when done.
pub fn export_for_live_share(state: &mut PlayerState) {
    if state.export.job.as_ref().is_some_and(|j| j.is_running()) {
        return;
    }
    state.export.post_action = PostExportAction::LiveShare;
    start_export(state);
}

/// Exports the selection and copies the file path to clipboard when done.
pub fn export_for_copy_path(state: &mut PlayerState) {
    if state.export.job.as_ref().is_some_and(|j| j.is_running()) {
        return;
    }
    state.export.post_action = PostExportAction::CopyPath;
    start_export(state);
}
