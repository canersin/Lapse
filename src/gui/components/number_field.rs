//! Number field: a pill showing the value with its unit. Drag it sideways to change
//! the value, click it to type one. A themed stand-in for `egui::DragValue`.

use std::ops::RangeInclusive;

use eframe::egui::{self, Align2, CursorIcon, FontId, Id, Key, Pos2, Sense, Stroke, Vec2};

use crate::gui::components::motion::{hover_t, lerp_color};
use crate::gui::theme::{M3Shapes, MonetTheme};

pub struct NumberOpts<'a> {
    /// Unit after the number, e.g. " FPS".
    pub suffix: &'a str,
    pub decimals: usize,
    /// Change per pixel of dragging.
    pub speed: f64,
    pub width: f32,
    /// Shown instead of 0, e.g. "Default" when 0 means "use the global setting".
    pub zero_text: Option<&'a str>,
}

impl Default for NumberOpts<'_> {
    fn default() -> Self {
        Self { suffix: "", decimals: 0, speed: 1.0, width: 84.0, zero_text: None }
    }
}

fn round_to(v: f64, decimals: usize) -> f64 {
    let f = 10f64.powi(decimals as i32);
    (v * f).round() / f
}

/// "12.5 Mbps", "1,5", "default" -> a number; None when it isn't one.
fn parse(text: &str, opts: &NumberOpts) -> Option<f64> {
    let t = text.trim().to_lowercase();
    if t.is_empty() || opts.zero_text.is_some_and(|z| t == z.to_lowercase()) {
        return opts.zero_text.map(|_| 0.0);
    }
    let number: String = t.chars().take_while(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-')).collect();
    number.replace(',', ".").parse().ok()
}

/// Returns true when the value changed.
pub fn m3_number_field(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    id_source: impl std::hash::Hash,
    value: &mut f64,
    range: RangeInclusive<f64>,
    opts: NumberOpts,
) -> bool {
    let id = Id::new(id_source).with("m3_number");
    let edit_id = id.with("edit");
    let drag_id = id.with("drag");
    let height = ui.spacing().interact_size.y.clamp(26.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(opts.width, height), Sense::click_and_drag());
    let font = FontId::proportional(12.5);
    let mut changed = false;

    let editing: Option<String> = ui.data(|d| d.get_temp(edit_id));
    if let Some(mut text) = editing {
        let p = ui.painter();
        p.rect_filled(rect, M3Shapes::FULL, theme.surface_container_highest);
        p.rect_stroke(rect, M3Shapes::FULL, Stroke::new(1.5_f32, theme.primary));
        let inner = rect.shrink2(Vec2::new(10.0, 3.0));
        let edit = ui.put(
            inner,
            egui::TextEdit::singleline(&mut text)
                .frame(false)
                .font(font)
                .text_color(theme.on_surface)
                .desired_width(inner.width()),
        );
        let started = ui.data(|d| d.get_temp::<bool>(edit_id.with("focused"))).unwrap_or(false);
        if !started {
            edit.request_focus();
            ui.data_mut(|d| d.insert_temp(edit_id.with("focused"), true));
        }
        let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        if escape || enter || (started && !edit.has_focus()) {
            if !escape {
                if let Some(v) = parse(&text, &opts) {
                    let v = round_to(v.clamp(*range.start(), *range.end()), opts.decimals);
                    changed = v != *value;
                    *value = v;
                }
            }
            ui.data_mut(|d| {
                d.remove::<String>(edit_id);
                d.remove::<bool>(edit_id.with("focused"));
            });
        } else {
            ui.data_mut(|d| d.insert_temp(edit_id, text));
        }
        return changed;
    }

    // Dragging: the value follows the pointer from where the drag began.
    if resp.drag_started() {
        ui.data_mut(|d| d.insert_temp(drag_id, (*value, 0.0f32)));
    }
    if resp.dragged() {
        let (start, moved): (f64, f32) = ui.data(|d| d.get_temp(drag_id)).unwrap_or((*value, 0.0));
        let moved = moved + resp.drag_delta().x;
        ui.data_mut(|d| d.insert_temp(drag_id, (start, moved)));
        let v = round_to((start + moved as f64 * opts.speed).clamp(*range.start(), *range.end()), opts.decimals);
        if v != *value {
            *value = v;
            changed = true;
        }
    }
    if resp.clicked() {
        let start = if *value == 0.0 && opts.zero_text.is_some() { String::new() } else { format!("{:.*}", opts.decimals, value) };
        ui.data_mut(|d| d.insert_temp(edit_id, start));
    }

    if ui.is_rect_visible(rect) {
        let h = hover_t(ui, &resp).max(if resp.dragged() { 1.0 } else { 0.0 });
        let p = ui.painter();
        p.rect_filled(rect, M3Shapes::FULL, lerp_color(theme.surface_container_highest, theme.primary_container, h * 0.35));
        p.rect_stroke(rect, M3Shapes::FULL, Stroke::new(1.0_f32, lerp_color(theme.outline_variant, theme.primary, h)));
        let (text, color) = match opts.zero_text {
            Some(z) if *value == 0.0 => (z.to_string(), theme.on_surface_variant),
            _ => (format!("{:.*}{}", opts.decimals, value, opts.suffix), theme.on_surface),
        };
        p.text(rect.center(), Align2::CENTER_CENTER, text, font, color);
        // Small arrows at the ends while hovered: this can be dragged sideways.
        if h > 0.01 {
            let c = theme.primary.gamma_multiply(h);
            for (x, dir) in [(rect.left() + 9.0, -1.0f32), (rect.right() - 9.0, 1.0)] {
                let tip = Pos2::new(x + dir * 2.0, rect.center().y);
                p.line_segment([tip, tip + Vec2::new(-dir * 3.5, -3.5)], Stroke::new(1.3_f32, c));
                p.line_segment([tip, tip + Vec2::new(-dir * 3.5, 3.5)], Stroke::new(1.3_f32, c));
            }
        }
    }
    resp.on_hover_cursor(CursorIcon::ResizeHorizontal);
    changed
}

/// The same for whole numbers.
pub fn m3_number_field_u32(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    id_source: impl std::hash::Hash,
    value: &mut u32,
    range: RangeInclusive<u32>,
    opts: NumberOpts,
) -> bool {
    let mut v = *value as f64;
    let changed = m3_number_field(ui, theme, id_source, &mut v, *range.start() as f64..=*range.end() as f64, NumberOpts { decimals: 0, ..opts });
    if changed {
        *value = v.round() as u32;
    }
    changed
}

/// Text box in the same pill style as the number field.
pub fn m3_text_field(ui: &mut egui::Ui, theme: &MonetTheme, text: &mut String, hint: &str, width: f32) -> egui::Response {
    let height = ui.spacing().interact_size.y.clamp(26.0, 30.0);
    // The pill goes under the text, so keep its place before the text box is drawn.
    let bg = ui.painter().add(egui::Shape::Noop);
    let edit = egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(12.0, 0.0))
        .show(ui, |ui| {
            ui.add_sized(
                [width - 24.0, height],
                egui::TextEdit::singleline(text)
                    .frame(false)
                    .font(FontId::proportional(12.5))
                    .text_color(theme.on_surface)
                    .hint_text(egui::RichText::new(hint).color(theme.outline))
                    .vertical_align(egui::Align::Center),
            )
        });
    let (rect, edit) = (edit.response.rect, edit.inner);
    let focused = edit.has_focus();
    let h = if focused { 1.0 } else { hover_t(ui, &edit) };
    let fill = lerp_color(theme.surface_container_highest, theme.primary_container, h * 0.2);
    let stroke = Stroke::new(if focused { 1.5_f32 } else { 1.0 }, lerp_color(theme.outline_variant, theme.primary, h));
    ui.painter().set(
        bg,
        egui::Shape::Vec(vec![egui::Shape::rect_filled(rect, M3Shapes::FULL, fill), egui::Shape::rect_stroke(rect, M3Shapes::FULL, stroke)]),
    );
    edit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_values_are_read() {
        let o = NumberOpts { zero_text: Some("Default"), ..Default::default() };
        assert_eq!(parse("12,5 Mbps", &o), Some(12.5));
        assert_eq!(parse("  60 fps", &o), Some(60.0));
        assert_eq!(parse("default", &o), Some(0.0));
        assert_eq!(parse("", &o), Some(0.0));
        assert_eq!(parse("abc", &NumberOpts::default()), None);
    }
}
