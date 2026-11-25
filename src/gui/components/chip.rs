use eframe::egui::{self, Color32, FontId, Margin, Response, RichText, Stroke, Vec2, WidgetText};
use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::components::motion::{hover_t, lerp_color};
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
    let text_color = if is_selected {
        theme.on_primary_container
    } else {
        theme.on_surface
    };
    let fg = if ui.is_enabled() {
        text_color
    } else {
        text_color.gamma_multiply(0.4)
    };

    let widget_text = WidgetText::from(RichText::new(text).size(13.0).strong().color(fg));
    let galley = widget_text.into_galley(ui, Some(false), f32::INFINITY, egui::TextStyle::Button);

    const HEIGHT: f32 = 34.0;
    const PAD: f32 = 16.0;
    const ICON_SIZE: f32 = 12.0;
    const ICON_GAP: f32 = 6.0;

    let content_w = if is_selected {
        ICON_SIZE + ICON_GAP + galley.size().x
    } else {
        galley.size().x
    };
    let size = Vec2::new(content_w + PAD * 2.0, HEIGHT);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let h = if enabled { hover_t(ui, &resp) } else { 0.0 };
        let p = ui.painter();

        if is_selected {
            p.rect_filled(rect, M3Shapes::FULL, lerp_color(theme.primary_container, theme.on_primary_container, 0.08 * h));
            let icon_center = egui::pos2(rect.left() + PAD + ICON_SIZE / 2.0, rect.center().y);
            draw_vector_icon(p, icon_center, ICON_SIZE, IconKind::Check, fg);
            let text_pos = egui::pos2(
                rect.left() + PAD + ICON_SIZE + ICON_GAP,
                rect.center().y - galley.size().y / 2.0,
            );
            p.galley(text_pos, galley, fg);
        } else {
            let fill = lerp_color(Color32::TRANSPARENT, theme.surface_container_high, h);
            p.rect_filled(rect, M3Shapes::FULL, fill);
            p.rect_stroke(rect, M3Shapes::FULL, Stroke::new(1.0_f32, theme.outline_variant));
            let text_pos = egui::pos2(
                rect.left() + PAD,
                rect.center().y - galley.size().y / 2.0,
            );
            p.galley(text_pos, galley, fg);
        }
    }

    if ui.is_enabled() {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
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
