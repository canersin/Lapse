use eframe::egui::{self, Color32, FontId, Margin, Response, RichText, Stroke, Vec2};
use crate::gui::theme::{M3Shapes, M3Typography, MonetTheme};

#[allow(dead_code)]

pub fn m3_status_badge(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    is_active: bool,
    active_text: &str,
    inactive_text: &str,
) {
    m3_status_badge_sized(ui, theme, is_active, active_text, inactive_text, 196.0);
}

pub fn m3_status_badge_sized(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    is_active: bool,
    active_text: &str,
    inactive_text: &str,
    width: f32,
) {
    let (bg, fg, dot_color, text) = if is_active {
        (theme.success_container, theme.on_success_container, theme.success, active_text)
    } else {
        (theme.surface_container_high, theme.on_surface_variant, theme.outline, inactive_text)
    };

    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 32.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, M3Shapes::FULL, bg);

    let text_font = FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), text_font, fg);
    let total_content_w = 8.0 + 8.0 + galley.size().x;
    let start_x = rect.center().x - (total_content_w / 2.0);

    ui.painter().circle_filled(egui::pos2(start_x + 4.0, rect.center().y), 4.0, dot_color);
    ui.painter().galley(egui::pos2(start_x + 16.0, rect.center().y - (galley.size().y / 2.0)), galley, fg);
}

pub fn m3_chip(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    text: &str,
    is_selected: bool,
) -> Response {
    let (bg, fg, stroke) = if is_selected {
        (theme.primary_container, theme.on_primary_container, Stroke::NONE)
    } else {
        (theme.surface_container_high, theme.on_surface_variant, Stroke::new(1.0_f32, theme.outline_variant))
    };

    let btn = egui::Button::new(
        RichText::new(text)
            .size(12.0)
            .strong()
            .color(fg),
    )
    .fill(bg)
    .stroke(stroke)
    .rounding(M3Shapes::FULL)
    .min_size(Vec2::new(50.0, 28.0));

    ui.add(btn)
}

pub fn m3_badge(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    text: &str,
    bg_override: Option<Color32>,
) {
    let bg = bg_override.unwrap_or(theme.surface_container_highest);
    let lum = 0.299 * (bg.r() as f32) + 0.587 * (bg.g() as f32) + 0.114 * (bg.b() as f32);
    let fg = if lum > 140.0 {
        Color32::from_rgb(18, 16, 24)
    } else {
        Color32::from_rgb(245, 242, 250)
    };

    egui::Frame::none()
        .fill(bg)
        .rounding(M3Shapes::FULL)
        .inner_margin(Margin::symmetric(10.0, 4.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).strong().color(fg));
        });
}

pub fn m3_keycap(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    text: &str,
) {
    egui::Frame::none()
        .fill(theme.primary_container)
        .stroke(Stroke::new(1.5_f32, theme.primary))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(Margin::symmetric(12.0, 6.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(13.0)
                    .monospace()
                    .strong()
                    .color(theme.on_primary_container),
            );
        });
}
