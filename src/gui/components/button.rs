use eframe::egui::{self, Response, RichText, Stroke, Vec2};
use crate::gui::theme::{M3Shapes, MonetTheme};

#[allow(dead_code)]

pub fn m3_filled_button(ui: &mut egui::Ui, theme: &MonetTheme, text: &str) -> Response {
    let btn = egui::Button::new(
        RichText::new(text)
            .size(14.0)
            .strong()
            .color(theme.on_primary),
    )
    .fill(theme.primary)
    .rounding(M3Shapes::FULL)
    .min_size(Vec2::new(120.0, 38.0));

    ui.add(btn)
}

pub fn m3_filled_button_sized(ui: &mut egui::Ui, theme: &MonetTheme, text: &str, size: Vec2) -> Response {
    let btn = egui::Button::new(
        RichText::new(text)
            .size(14.0)
            .strong()
            .color(theme.on_primary),
    )
    .fill(theme.primary)
    .rounding(M3Shapes::FULL)
    .min_size(size);

    ui.add(btn)
}

pub fn m3_tonal_button(ui: &mut egui::Ui, theme: &MonetTheme, text: &str) -> Response {
    let btn = egui::Button::new(
        RichText::new(text)
            .size(13.0)
            .strong()
            .color(theme.on_primary_container),
    )
    .fill(theme.primary_container)
    .rounding(M3Shapes::FULL)
    .min_size(Vec2::new(100.0, 36.0));

    ui.add(btn)
}

pub fn m3_tonal_button_sized(ui: &mut egui::Ui, theme: &MonetTheme, text: &str, size: Vec2) -> Response {
    let btn = egui::Button::new(
        RichText::new(text)
            .size(13.0)
            .strong()
            .color(theme.on_primary_container),
    )
    .fill(theme.primary_container)
    .rounding(M3Shapes::FULL)
    .min_size(size);

    ui.add(btn)
}

pub fn m3_outlined_button(ui: &mut egui::Ui, theme: &MonetTheme, text: &str) -> Response {
    let btn = egui::Button::new(
        RichText::new(text)
            .size(13.0)
            .strong()
            .color(theme.on_surface),
    )
    .stroke(Stroke::new(1.0f32, theme.outline_variant))
    .fill(egui::Color32::TRANSPARENT)
    .rounding(M3Shapes::FULL)
    .min_size(Vec2::new(90.0, 34.0));

    ui.add(btn)
}

#[allow(dead_code)]
pub fn m3_danger_button(ui: &mut egui::Ui, theme: &MonetTheme, text: &str) -> Response {
    let btn = egui::Button::new(
        RichText::new(text)
            .size(13.0)
            .strong()
            .color(theme.on_error),
    )
    .fill(theme.error)
    .rounding(M3Shapes::FULL)
    .min_size(Vec2::new(100.0, 36.0));

    ui.add(btn)
}

pub fn m3_danger_button_sized(ui: &mut egui::Ui, theme: &MonetTheme, text: &str, size: Vec2) -> Response {
    let btn = egui::Button::new(
        RichText::new(text)
            .size(13.0)
            .strong()
            .color(theme.on_error),
    )
    .fill(theme.error)
    .rounding(M3Shapes::FULL)
    .min_size(size);

    ui.add(btn)
}

/// Small pill button for toolbars and rows (trimmer, library cards): quiet surface fill
/// that takes on the accent while hovered.
pub fn m3_small_button(ui: &mut egui::Ui, theme: &MonetTheme, text: &str) -> Response {
    m3_small_button_colored(ui, theme, text, theme.on_surface)
}

pub fn m3_small_button_colored(ui: &mut egui::Ui, theme: &MonetTheme, text: &str, color: egui::Color32) -> Response {
    use crate::gui::components::motion::{hover_t, lerp_color};
    let font = egui::FontId::proportional(12.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let height = ui.spacing().interact_size.y.clamp(26.0, 30.0);
    let size = Vec2::new(galley.size().x + 24.0, height);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let h = if enabled { hover_t(ui, &resp) } else { 0.0 };
        let press = if resp.is_pointer_button_down_on() { 1.0 } else { 0.0 };
        let fill = lerp_color(theme.surface_container_highest, theme.primary_container, h * 0.45 + press * 0.3);
        let border = lerp_color(theme.outline_variant, theme.primary, h);
        let p = ui.painter();
        p.rect_filled(rect, M3Shapes::FULL, fill);
        p.rect_stroke(rect, M3Shapes::FULL, Stroke::new(1.0_f32, border));
        let fg = if enabled { color } else { color.gamma_multiply(0.4) };
        p.galley(rect.center() - galley.size() / 2.0, galley, fg);
    }
    if ui.is_enabled() {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Makes DragValues and text fields in `ui` match the pill buttons.
pub fn m3_field_style(ui: &mut egui::Ui, theme: &MonetTheme) {
    let v = &mut ui.style_mut().visuals;
    v.extreme_bg_color = theme.surface_container_highest;
    for (w, stroke) in [
        (&mut v.widgets.inactive, theme.outline_variant),
        (&mut v.widgets.hovered, theme.primary),
        (&mut v.widgets.active, theme.primary),
    ] {
        w.bg_fill = theme.surface_container_highest;
        w.weak_bg_fill = theme.surface_container_highest;
        w.bg_stroke = Stroke::new(1.0_f32, stroke);
        w.rounding = M3Shapes::FULL;
        w.fg_stroke = Stroke::new(1.0_f32, theme.on_surface);
        w.expansion = 0.0;
    }
    v.selection.stroke = Stroke::new(1.0_f32, theme.primary);
    ui.spacing_mut().button_padding = Vec2::new(10.0, 4.0);
}
