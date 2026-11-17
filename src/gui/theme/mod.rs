pub mod colors;
pub mod shapes;
pub mod typography;

pub use colors::{MonetPaletteKind, MonetTheme};
pub use shapes::M3Shapes;
pub use typography::M3Typography;

use eframe::egui::{self, Stroke};

pub fn apply_theme(ctx: &egui::Context, theme: &MonetTheme) {
    let mut visuals = egui::Visuals::dark();

    visuals.dark_mode = true;
    visuals.panel_fill = theme.background;
    visuals.window_fill = theme.surface_container;
    visuals.window_rounding = M3Shapes::LARGE;
    visuals.window_stroke = Stroke::NONE;
    visuals.window_stroke = Stroke::new(1.0f32, theme.outline_variant);

    visuals.widgets.noninteractive.bg_fill = theme.surface_container_low;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0f32, theme.on_surface_variant);
    visuals.widgets.noninteractive.rounding = M3Shapes::MEDIUM;

    visuals.widgets.inactive.bg_fill = theme.surface_container_high;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0f32, theme.on_surface_variant);
    visuals.widgets.inactive.rounding = M3Shapes::FULL;

    visuals.widgets.hovered.bg_fill = theme.surface_container_highest;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0f32, theme.on_surface);
    visuals.widgets.hovered.rounding = M3Shapes::FULL;

    visuals.widgets.active.bg_fill = theme.primary_container;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0f32, theme.on_primary_container);
    visuals.widgets.active.rounding = M3Shapes::FULL;

    visuals.widgets.open.bg_fill = theme.surface_container_highest;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0f32, theme.on_surface);
    visuals.widgets.open.rounding = M3Shapes::MEDIUM;

    visuals.selection.bg_fill = theme.primary_container;
    visuals.selection.stroke = Stroke::new(1.0f32, theme.primary);

    visuals.extreme_bg_color = theme.surface_container_lowest;
    visuals.faint_bg_color = theme.surface_container_low;
    visuals.code_bg_color = theme.surface_container_highest;

    ctx.set_visuals(visuals);
}
