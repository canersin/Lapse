use eframe::egui::{self, Align2, FontId, Rect, Response, Sense, Stroke, Vec2};

use crate::gui::components::motion::{lerp_color, MEDIUM};
use crate::gui::theme::{M3Shapes, MonetTheme};

/// A row of mutually exclusive options with a highlight that slides to the selection.
/// Returns the response of the clicked segment, if any.
pub fn segmented<T: PartialEq + Clone>(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    id: &str,
    value: &mut T,
    options: &[(T, &str)],
    segment_width: f32,
) -> Option<Response> {
    let height = 34.0;
    let size = Vec2::new(segment_width * options.len() as f32 + 8.0, height);
    let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(outer.expand(2.0));
    painter.rect_filled(outer, M3Shapes::FULL, theme.surface_container_highest);
    painter.rect_stroke(outer, M3Shapes::FULL, Stroke::new(1.0_f32, theme.outline_variant));

    let selected = options.iter().position(|(v, _)| v == value);
    let seg_rect = |i: usize| {
        Rect::from_min_size(
            outer.min + Vec2::new(4.0 + i as f32 * segment_width, 4.0),
            Vec2::new(segment_width, height - 8.0),
        )
    };

    if let Some(i) = selected {
        let target_x = seg_rect(i).left();
        let x = ui.ctx().animate_value_with_time(egui::Id::new(id).with("pill"), target_x, MEDIUM);
        let pill = Rect::from_min_size(egui::pos2(x, outer.top() + 4.0), seg_rect(i).size());
        painter.rect_filled(pill, M3Shapes::FULL, theme.primary);
    }

    let mut clicked = None;
    for (i, (v, label)) in options.iter().enumerate() {
        let r = seg_rect(i);
        let resp = ui.interact(r, egui::Id::new(id).with(i), Sense::click());
        let sel_t = ui.ctx().animate_bool_with_time(resp.id.with("sel"), selected == Some(i), MEDIUM);
        let hover_t = ui.ctx().animate_bool_with_time(resp.id.with("hov"), resp.hovered(), 0.1);
        if hover_t > 0.0 && selected != Some(i) {
            painter.rect_filled(r, M3Shapes::FULL, theme.on_surface.gamma_multiply(0.06 * hover_t));
        }
        let color = lerp_color(theme.on_surface_variant, theme.on_primary, sel_t);
        painter.text(r.center(), Align2::CENTER_CENTER, *label, FontId::proportional(12.5), color);
        if resp.clicked() {
            *value = v.clone();
            clicked = Some(resp);
        }
    }
    clicked
}
