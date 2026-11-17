use eframe::egui::{self, Align2, Color32, FontId, Margin, Response, Stroke, Vec2};
use crate::gui::components::icon::{draw_vector_icon, m3_vector_icon, IconKind};
use crate::gui::theme::{M3Shapes, M3Typography, MonetTheme};

#[allow(dead_code)]

pub fn m3_card<R>(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::none()
        .fill(theme.surface_container)
        .stroke(Stroke::new(1.0_f32, theme.outline_variant))
        .rounding(M3Shapes::LARGE)
        .inner_margin(Margin::same(16.0))
        .show(ui, add_contents)
}

#[allow(dead_code)]
pub fn m3_elevated_card<R>(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::none()
        .fill(theme.surface_container_high)
        .stroke(Stroke::new(1.0_f32, theme.outline_variant))
        .rounding(M3Shapes::LARGE)
        .inner_margin(Margin::same(16.0))
        .show(ui, add_contents)
}

pub fn m3_section_card<R>(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    icon: Option<IconKind>,
    title: &str,
    description: Option<&str>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::none()
        .fill(theme.surface_container)
        .stroke(Stroke::new(1.0_f32, theme.outline_variant))
        .rounding(M3Shapes::LARGE)
        .inner_margin(Margin::same(18.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(kind) = icon {
                    m3_vector_icon(ui, kind, 22.0, theme.primary);
                    ui.add_space(6.0);
                }
                ui.vertical(|ui| {
                    ui.label(M3Typography::title_medium(title, theme.on_surface));
                    if let Some(desc) = description {
                        ui.label(M3Typography::body_medium(desc, theme.on_surface_variant));
                    }
                });
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);

            add_contents(ui)
        })
}

#[allow(dead_code)]
pub fn m3_status_card(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    icon: IconKind,
    title: &str,
    status_text: &str,
    is_active: bool,
    width: f32,
) -> Response {
    let height = 56.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());

    let is_hovered = response.hovered();
    let bg = if is_hovered {
        theme.surface_container_highest
    } else {
        theme.surface_container_high
    };

    let stroke = if is_active {
        Stroke::new(1.5_f32, theme.primary)
    } else {
        Stroke::new(1.0_f32, theme.outline_variant)
    };

    ui.painter().rect_filled(rect, M3Shapes::LARGE, bg);
    ui.painter().rect_stroke(rect, M3Shapes::LARGE, stroke);

    let icon_bg = if is_active { theme.primary } else { theme.surface_container_lowest };
    let icon_fg = if is_active { theme.on_primary } else { theme.on_surface_variant };
    let icon_center = egui::pos2(rect.min.x + 28.0, rect.center().y);
    ui.painter().circle_filled(icon_center, 18.0, icon_bg);
    draw_vector_icon(ui.painter(), icon_center, 16.0, icon, icon_fg);

    let text_x = rect.min.x + 56.0;
    ui.painter().text(
        egui::pos2(text_x, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.0),
        theme.on_surface,
    );

    let status_color = if is_active { theme.primary } else { theme.on_surface_variant };
    ui.painter().text(
        egui::pos2(text_x, rect.center().y + 9.0),
        Align2::LEFT_CENTER,
        status_text,
        FontId::proportional(11.0),
        status_color,
    );

    response
}

pub fn m3_game_capsule(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    icon: IconKind,
    game_name: &str,
    clip_count: usize,
    is_selected: bool,
    color_accent: Option<Color32>,
) -> Response {
    let width = 175.0;
    let height = 46.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());
    let is_hovered = resp.hovered();

    let accent = color_accent.unwrap_or(theme.primary);
    let bg = if is_selected {
        accent.gamma_multiply(0.25)
    } else if is_hovered {
        theme.surface_container_highest
    } else {
        theme.surface_container_high
    };

    let stroke = if is_selected {
        Stroke::new(1.5_f32, accent)
    } else {
        Stroke::new(1.0_f32, theme.outline_variant)
    };

    ui.painter().rect_filled(rect, M3Shapes::FULL, bg);
    ui.painter().rect_stroke(rect, M3Shapes::FULL, stroke);

    let icon_center = egui::pos2(rect.min.x + 24.0, rect.center().y);
    if let Some(cover) = crate::hardware::steam::cover_for_name(game_name) {
        // Steam header art, cropped to a rounded square.
        let cover_rect = egui::Rect::from_center_size(icon_center, Vec2::splat(32.0));
        egui::Image::new(format!("file://{}", cover.display()))
            .uv(egui::Rect::from_min_max(egui::pos2(0.28, 0.0), egui::pos2(0.72, 1.0)))
            .rounding(egui::Rounding::same(16.0))
            .paint_at(ui, cover_rect);
    } else {
    ui.painter().circle_filled(icon_center, 15.0, accent);
    let lum = 0.299 * (accent.r() as f32) + 0.587 * (accent.g() as f32) + 0.114 * (accent.b() as f32);
    let icon_fg = if lum > 140.0 { Color32::from_rgb(18, 16, 24) } else { Color32::WHITE };
    draw_vector_icon(ui.painter(), icon_center, 14.0, icon, icon_fg);
    }

    let text_x = rect.min.x + 48.0;
    ui.painter().text(
        egui::pos2(text_x, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        game_name,
        FontId::proportional(12.5),
        theme.on_surface,
    );

    ui.painter().text(
        egui::pos2(text_x, rect.center().y + 8.0),
        Align2::LEFT_CENTER,
        format!("{} {}", clip_count, crate::gui::i18n::t("clips")),
        FontId::proportional(10.5),
        theme.on_surface_variant,
    );

    resp
}
