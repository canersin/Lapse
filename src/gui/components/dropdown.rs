//! Monet dropdown: a pill button showing the current choice and a turning chevron,
//! opening a rounded menu where the selected row is tinted with the accent.
//! A themed stand-in for `egui::ComboBox`; the menu takes `ui.selectable_value` rows.

use eframe::egui::{self, Align2, FontId, Id, Key, Margin, Pos2, Rect, Sense, Stroke, Vec2};

use crate::gui::components::motion::{hover_t, lerp_color, MEDIUM};
use crate::gui::theme::{M3Shapes, MonetTheme};

pub fn m3_dropdown<R>(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    id: impl std::hash::Hash,
    selected_text: impl Into<String>,
    max_width: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    m3_dropdown_sized(ui, theme, id, selected_text, max_width, 32.0, add_contents)
}

/// Same with a given height, e.g. to line up with small pill buttons in a row.
pub fn m3_dropdown_sized<R>(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    id: impl std::hash::Hash,
    selected_text: impl Into<String>,
    max_width: f32,
    height: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let id = Id::new(id).with("m3_dropdown");
    let text: String = selected_text.into();
    let font = FontId::proportional(12.5);
    let galley = ui.painter().layout_no_wrap(text.clone(), font.clone(), theme.on_surface);
    let width = (galley.size().x + 48.0).ceil().clamp(80.0_f32.min(max_width), max_width);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let open = ui.memory(|m| m.is_popup_open(id));
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }

    if ui.is_rect_visible(rect) {
        let h = hover_t(ui, &resp);
        let turn = ui.ctx().animate_bool_with_time(id.with("turn"), open, MEDIUM);
        let p = ui.painter();
        let fill = lerp_color(theme.surface_container_highest, theme.primary_container, (h * 0.35).max(turn * 0.5));
        let border = lerp_color(theme.outline_variant, theme.primary, h.max(turn));
        p.rect_filled(rect, M3Shapes::FULL, fill);
        p.rect_stroke(rect, M3Shapes::FULL, Stroke::new(1.0_f32, border));

        // Selected text, cut with "…" when it doesn't fit.
        let text_room = rect.width() - 46.0;
        let shown = if galley.size().x <= text_room {
            text.clone()
        } else {
            let mut s = text.clone();
            while !s.is_empty() && ui.painter().layout_no_wrap(format!("{}…", s), font.clone(), theme.on_surface).size().x > text_room {
                s.pop();
            }
            format!("{}…", s.trim_end())
        };
        p.text(Pos2::new(rect.left() + 14.0, rect.center().y), Align2::LEFT_CENTER, shown, font, theme.on_surface);

        // Chevron: points down, turns up while open.
        let c = Pos2::new(rect.right() - 18.0, rect.center().y);
        let dir = 1.0 - 2.0 * turn;
        let pts = vec![c + Vec2::new(-4.5, -2.0 * dir), c + Vec2::new(0.0, 2.5 * dir), c + Vec2::new(4.5, -2.0 * dir)];
        p.add(egui::Shape::line(pts, Stroke::new(1.8_f32, lerp_color(theme.on_surface_variant, theme.primary, h.max(turn)))));
    }
    if text.len() > 0 && resp.hovered() && !open {
        // Full text when it was shortened.
        let room = rect.width() - 46.0;
        if galley.size().x > room {
            resp.clone().on_hover_text(&text);
        }
    }

    if !open {
        return None;
    }
    // Open upwards when the menu (its height from the last frame) doesn't fit below.
    let last_h: f32 = ui.data(|d| d.get_temp(id.with("menu_h"))).unwrap_or(200.0);
    let room_below = ui.ctx().screen_rect().bottom() - rect.bottom();
    let (pos, pivot) = if room_below < last_h + 12.0 && rect.top() > room_below {
        (rect.left_top() - Vec2::new(0.0, 6.0), Align2::LEFT_BOTTOM)
    } else {
        (rect.left_bottom() + Vec2::new(0.0, 6.0), Align2::LEFT_TOP)
    };
    let area = egui::Area::new(id.with("menu"))
        .order(egui::Order::Foreground)
        .constrain(true)
        .pivot(pivot)
        .fixed_pos(pos)
        .show(ui.ctx(), |ui| {
            egui::Frame::none()
                .fill(theme.surface_container_high)
                .stroke(Stroke::new(1.0_f32, theme.outline_variant))
                .rounding(M3Shapes::MEDIUM)
                .shadow(egui::epaint::Shadow { extrusion: 14.0, color: egui::Color32::from_black_alpha(90) })
                .inner_margin(Margin::same(6.0))
                .show(ui, |ui| {
                    let style = ui.style_mut();
                    style.visuals.selection.bg_fill = theme.primary_container;
                    style.visuals.selection.stroke = Stroke::new(1.0_f32, theme.on_primary_container);
                    style.visuals.widgets.hovered.weak_bg_fill = theme.on_surface.gamma_multiply(0.08);
                    style.visuals.widgets.hovered.bg_fill = theme.on_surface.gamma_multiply(0.08);
                    style.visuals.widgets.hovered.bg_stroke = Stroke::NONE;
                    style.visuals.widgets.active.weak_bg_fill = theme.primary_container;
                    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, theme.on_surface);
                    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, theme.on_surface);
                    for w in [&mut style.visuals.widgets.hovered, &mut style.visuals.widgets.active, &mut style.visuals.widgets.inactive] {
                        w.rounding = M3Shapes::SMALL;
                    }
                    style.spacing.button_padding = Vec2::new(12.0, 7.0);
                    style.spacing.item_spacing.y = 2.0;
                    // Fixed width: a justified layout in an Area would grow to the whole screen.
                    let menu_w = rect.width().max(250.0) - 12.0;
                    ui.set_width(menu_w);
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        ui.set_width(menu_w);
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, add_contents).inner
                    })
                    .inner
                })
                .inner
        });

    // Close after a choice, a click outside or Escape.
    let menu: Rect = area.response.rect;
    ui.data_mut(|d| d.insert_temp(id.with("menu_h"), menu.height()));
    let clicked_at = ui.input(|i| if i.pointer.any_click() { i.pointer.interact_pos() } else { None });
    let picked = clicked_at.map(|p| menu.contains(p)).unwrap_or(false);
    let outside = clicked_at.map(|p| !menu.contains(p) && !rect.contains(p)).unwrap_or(false);
    if picked || outside || ui.input(|i| i.key_pressed(Key::Escape)) {
        ui.memory_mut(|m| m.close_popup());
    }
    Some(area.inner)
}
