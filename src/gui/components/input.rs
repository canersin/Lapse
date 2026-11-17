use eframe::egui::{self, Margin, RichText, Stroke, Vec2};
use crate::gui::components::number_field::{m3_number_field, m3_number_field_u32, NumberOpts};
use crate::gui::components::slider::{m3_slider, SliderOpts};
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, M3Typography, MonetTheme};

#[allow(dead_code)]

pub fn m3_setting_row<R>(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    title: &str,
    description: &str,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let row_response = egui::Frame::none()
        .inner_margin(Margin::symmetric(4.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(280.0);
                    ui.label(M3Typography::title_medium(title, theme.on_surface));
                    if !description.is_empty() {
                        ui.label(M3Typography::body_medium(description, theme.on_surface_variant));
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    control(ui)
                })
                .inner
            })
            .inner
        });

    ui.add_space(4.0);
    row_response.inner
}

pub fn m3_duration_slider_input(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    current_val: &mut u32,
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        let mut val_f64 = *current_val as f64;
        let opts = SliderOpts { log: true, step: 0.0 };
        if m3_slider(ui, theme, &mut val_f64, 10.0..=7200.0, 170.0, opts).changed() {
            *current_val = val_f64.round() as u32;
            changed = true;
        }

        ui.add_space(8.0);

        let opts = NumberOpts { suffix: " s", speed: 2.0, width: 76.0, ..Default::default() };
        if m3_number_field_u32(ui, theme, "replay_seconds", current_val, 5..=14400, opts) {
            changed = true;
        }

        ui.add_space(8.0);

        let v = *current_val;
        let formatted_str = if v < 60 {
            format!("{} s", v)
        } else if v < 3600 {
            format!("{}:{:02} min", v / 60, v % 60)
        } else {
            format!("{}h {:02}m", v / 3600, (v % 3600) / 60)
        };

        ui.label(RichText::new(formatted_str).color(theme.primary).size(12.5).strong());
    });

    changed
}

pub fn m3_fps_slider_input(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    current_val: &mut u32,
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        let mut val_f64 = *current_val as f64;
        if m3_slider(ui, theme, &mut val_f64, 15.0..=360.0, 170.0, SliderOpts { log: false, step: 1.0 }).changed() {
            *current_val = val_f64.round() as u32;
            changed = true;
        }

        ui.add_space(8.0);

        let opts = NumberOpts { suffix: " FPS", speed: 0.5, width: 76.0, ..Default::default() };
        if m3_number_field_u32(ui, theme, "fps", current_val, 10..=500, opts) {
            changed = true;
        }

        ui.add_space(8.0);

        ui.label(RichText::new(format!("{} FPS", current_val)).color(theme.primary).size(12.5).strong());
    });

    changed
}

pub fn m3_bitrate_slider_input(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    bitrate_kbps: &mut u32,
    replay_seconds: u32,
    typical_kbps: Option<f64>,
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        let mut mbps_f64 = (*bitrate_kbps as f64) / 1000.0;
        if m3_slider(ui, theme, &mut mbps_f64, 2.0..=150.0, 150.0, SliderOpts { log: false, step: 0.5 }).changed() {
            *bitrate_kbps = (mbps_f64 * 1000.0).round() as u32;
            changed = true;
        }

        ui.add_space(6.0);

        let mut mbps = (*bitrate_kbps as f64) / 1000.0;
        let opts = NumberOpts { suffix: " Mbps", decimals: 1, speed: 0.1, width: 92.0, ..Default::default() };
        if m3_number_field(ui, theme, "bitrate_mbps", &mut mbps, 1.0..=250.0, opts) {
            *bitrate_kbps = (mbps * 1000.0).round() as u32;
            changed = true;
        }

        ui.add_space(6.0);

        // The setting is a ceiling: the encoder spends less on calm scenes.
        let size = |kbps: f64| {
            let mb = kbps * replay_seconds as f64 / 8.0 / 1024.0;
            if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{:.0} MB", mb) }
        };
        // Fixed width, right-aligned, so the text stays next to the bitrate fields.
        ui.allocate_ui_with_layout(Vec2::new(230.0, 34.0), egui::Layout::top_down(egui::Align::Max), |ui| {
            ui.label(
                RichText::new(format!("{} {} {}", t("Up to"), size(*bitrate_kbps as f64), t("per clip")))
                    .color(theme.primary)
                    .size(11.5)
                    .strong(),
            );
            let typical = match typical_kbps {
                Some(k) => format!("{} ~{:.1} Mbps, ~{} {}", t("Your recent clips:"), k / 1000.0, size(k), t("per clip")),
                None => t("Usually less: calm scenes need fewer bits.").to_string(),
            };
            ui.label(RichText::new(typical).color(theme.on_surface_variant).size(11.0));
        });
    });

    changed
}

#[allow(dead_code)]
pub fn m3_discrete_slider(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    current_val: &mut u32,
    steps: &[(u32, &str)],
) -> bool {
    let mut changed = false;
    let current_idx = steps
        .iter()
        .position(|(val, _)| *val == *current_val)
        .unwrap_or(0);

    let max_idx = steps.len().saturating_sub(1);
    let mut slider_idx = current_idx as f64;

    ui.horizontal(|ui| {
        let label_str = steps.get(current_idx).map(|(_, s)| *s).unwrap_or("");

        let opts = SliderOpts { log: false, step: 1.0 };
        m3_slider(ui, theme, &mut slider_idx, 0.0..=(max_idx as f64).max(1.0), 170.0, opts);
        ui.add_space(8.0);

        egui::Frame::none()
            .fill(theme.surface_container_highest)
            .rounding(M3Shapes::FULL)
            .inner_margin(Margin::symmetric(12.0, 5.0))
            .show(ui, |ui| {
                ui.label(RichText::new(label_str).color(theme.primary).size(12.0).strong());
            });
    });

    let new_idx = slider_idx.round() as usize;
    if new_idx != current_idx && new_idx < steps.len() {
        *current_val = steps[new_idx].0;
        changed = true;
    }
    changed
}

pub fn m3_search_input(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    query: &mut String,
    placeholder: &str,
) {
    egui::Frame::none()
        .fill(theme.surface_container_high)
        .rounding(M3Shapes::FULL)
        .inner_margin(Margin::symmetric(14.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Search")).color(theme.on_surface_variant).size(12.0).strong());
                ui.add_space(4.0);

                let edit = egui::TextEdit::singleline(query)
                    .hint_text(RichText::new(placeholder).color(theme.on_surface_variant))
                    .text_color(theme.on_surface)
                    .frame(false)
                    .min_size(Vec2::new(ui.available_width() - 30.0, 20.0));

                ui.add(edit);

                if !query.is_empty() {
                    if ui.small_button("X").clicked() {
                        query.clear();
                    }
                }
            });
        });
}
