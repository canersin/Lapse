//! Material 3 text field: a rounded container with animated outline,
//! body-large typography, clear contrast hint text, password show/hide eye toggle,
//! and optional trailing clear button.

use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::gui::components::motion::{ease_out, hover_t, lerp_color, FAST};
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};

/// Options for configuring `m3_text_field`.
#[derive(Clone, Debug)]
pub struct TextFieldOpts<'a> {
    pub hint: &'a str,
    pub width: Option<f32>,
    pub password: bool,
    pub multiline: bool,
    pub rows: usize,
    pub monospace: bool,
}

impl Default for TextFieldOpts<'_> {
    fn default() -> Self {
        Self {
            hint: "",
            width: None,
            password: false,
            multiline: false,
            rows: 2,
            monospace: false,
        }
    }
}

#[allow(dead_code)]
impl<'a> TextFieldOpts<'a> {
    pub fn hint(hint: &'a str) -> Self {
        Self { hint, ..Default::default() }
    }

    pub fn password(hint: &'a str) -> Self {
        Self { hint, password: true, ..Default::default() }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn with_multiline(mut self, rows: usize) -> Self {
        self.multiline = true;
        self.rows = rows;
        self
    }

    pub fn with_monospace(mut self, monospace: bool) -> Self {
        self.monospace = monospace;
        self
    }
}

/// Computes the outer height of the field based on multiline and rows.
pub fn field_height(multiline: bool, rows: usize) -> f32 {
    if multiline {
        (rows.max(1) as f32 * 20.0 + 20.0).max(40.0)
    } else {
        40.0
    }
}

/// Checks whether a trailing button (eye toggle or clear button) should be shown.
pub fn has_trailing_button(password: bool, text_empty: bool) -> bool {
    password || !text_empty
}

/// Draws a vector eye icon for password show/hide.
pub fn draw_eye_icon(painter: &egui::Painter, center: Pos2, size: f32, revealed: bool, color: Color32) {
    let r = size * 0.5;
    let stroke = Stroke::new(1.4_f32, color);

    let p_left = Pos2::new(center.x - r, center.y);
    let p_right = Pos2::new(center.x + r, center.y);
    let p_top = Pos2::new(center.x, center.y - r * 0.55);
    let p_bottom = Pos2::new(center.x, center.y + r * 0.55);

    let upper = vec![p_left, p_top, p_right];
    let lower = vec![p_left, p_bottom, p_right];
    painter.add(egui::Shape::line(upper, stroke));
    painter.add(egui::Shape::line(lower, stroke));

    painter.circle_filled(center, r * 0.32, color);

    if !revealed {
        let slash_start = Pos2::new(center.x - r * 0.85, center.y - r * 0.75);
        let slash_end = Pos2::new(center.x + r * 0.85, center.y + r * 0.75);
        painter.line_segment([slash_start, slash_end], Stroke::new(1.5_f32, color));
    }
}

/// Draws a small vector cross for the clear button.
pub fn draw_clear_icon(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let r = size * 0.32;
    let stroke = Stroke::new(1.3_f32, color);
    painter.line_segment([center + Vec2::new(-r, -r), center + Vec2::new(r, r)], stroke);
    painter.line_segment([center + Vec2::new(r, -r), center + Vec2::new(-r, r)], stroke);
}

/// Material 3 filled/outlined text field.
pub fn m3_text_field(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    text: &mut String,
    opts: TextFieldOpts,
) -> egui::Response {
    // The frame and the inner TextEdit are placed separately; a scope keeps them one
    // cell so the field doesn't shift the columns after it in a Grid.
    ui.scope(|ui| text_field_inner(ui, theme, text, opts)).inner
}

fn text_field_inner(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    text: &mut String,
    opts: TextFieldOpts,
) -> egui::Response {
    // An explicit width is kept as is: inside a Grid the available width of a cell is
    // tiny on the first frames and would squash the field to its minimum.
    let width = opts.width.unwrap_or_else(|| 280.0f32.min(ui.available_width().max(40.0)));
    let height = field_height(opts.multiline, opts.rows);

    let id = ui.next_auto_id();
    let reveal_id = id.with("pwd_reveal");
    let mut revealed: bool = ui.data(|d| d.get_temp(reveal_id)).unwrap_or(false);

    let bg_idx = ui.painter().add(egui::Shape::Noop);

    let (outer_rect, outer_resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());

    let show_button = has_trailing_button(opts.password, text.is_empty());
    let btn_size = 24.0;
    let right_padding = if show_button { 10.0 + btn_size + 4.0 } else { 12.0 };

    let vert_pad = if opts.multiline { 10.0 } else { 10.0 };
    let inner_rect = Rect::from_min_max(
        Pos2::new(outer_rect.left() + 12.0, outer_rect.top() + vert_pad),
        Pos2::new(outer_rect.right() - right_padding, outer_rect.bottom() - vert_pad),
    );

    let font = if opts.monospace {
        FontId::monospace(14.0)
    } else {
        FontId::proportional(14.0)
    };

    let hint_color = theme.on_surface_variant.gamma_multiply(0.70);
    let hint_widget = RichText::new(opts.hint).color(hint_color).font(font.clone());

    let edit = if opts.multiline {
        egui::TextEdit::multiline(text)
            .id_source(id.with("txt"))
            .frame(false)
            .font(font)
            .text_color(theme.on_surface)
            .hint_text(hint_widget)
            .desired_rows(opts.rows.max(1))
            .desired_width(inner_rect.width())
    } else {
        let mut e = egui::TextEdit::singleline(text)
            .id_source(id.with("txt"))
            .frame(false)
            .font(font)
            .text_color(theme.on_surface)
            .hint_text(hint_widget)
            .vertical_align(egui::Align::Center)
            .desired_width(inner_rect.width());
        if opts.password {
            e = e.password(!revealed);
        }
        e
    };

    let mut edit_resp = ui.put(inner_rect, edit);

    let button_rect = Rect::from_center_size(
        Pos2::new(outer_rect.right() - 18.0, outer_rect.center().y),
        Vec2::splat(btn_size),
    );

    if show_button {
        let btn_resp = ui.interact(button_rect, id.with("btn"), Sense::click());
        let btn_hovered = btn_resp.hovered();

        if btn_resp.clicked() {
            if opts.password {
                revealed = !revealed;
                ui.data_mut(|d| d.insert_temp(reveal_id, revealed));
                ui.ctx().request_repaint();
            } else {
                text.clear();
                edit_resp.mark_changed();
                ui.ctx().request_repaint();
            }
        }

        let p = ui.painter();
        if opts.password {
            let h = hover_t(ui, &btn_resp);
            let color = lerp_color(theme.on_surface_variant, theme.primary, h);
            draw_eye_icon(p, button_rect.center(), 16.0, revealed, color);
            btn_resp.on_hover_text(if revealed { t("Hide") } else { t("Show") });
        } else {
            let h = hover_t(ui, &btn_resp);
            let color = lerp_color(theme.on_surface_variant, theme.on_surface, h);
            draw_clear_icon(p, button_rect.center(), 14.0, color);
            btn_resp.on_hover_text(t("Clear"));
        }

        if btn_hovered {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
        }
    }

    if ui.input(|i| i.pointer.primary_clicked()) {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            if outer_rect.contains(pos) && (!show_button || !button_rect.contains(pos)) {
                edit_resp.request_focus();
            }
        }
    }

    let focused = edit_resp.has_focus();
    let hovered = outer_resp.hovered() || edit_resp.hovered();
    let h = ease_out(ui.ctx().animate_bool_with_time(id.with("hov"), hovered, FAST));
    let f = ease_out(ui.ctx().animate_bool_with_time(id.with("foc"), focused, FAST));

    let border_idle_hover = lerp_color(theme.outline_variant, theme.outline, h);
    let border_color = lerp_color(border_idle_hover, theme.primary, f);
    let border_width = 1.0_f32 + 1.0_f32 * f;

    let fill = theme.surface_container_highest;
    let rounding = if opts.multiline { M3Shapes::MEDIUM } else { M3Shapes::FULL };

    ui.painter().set(
        bg_idx,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(outer_rect, rounding, fill),
            egui::Shape::rect_stroke(outer_rect, rounding, Stroke::new(border_width, border_color)),
        ]),
    );

    if hovered && (!show_button || !ui.input(|i| i.pointer.hover_pos().map(|p| button_rect.contains(p)).unwrap_or(false))) {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Text);
    }

    edit_resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opts_defaults() {
        let opts = TextFieldOpts::default();
        assert_eq!(opts.hint, "");
        assert_eq!(opts.width, None);
        assert!(!opts.password);
        assert!(!opts.multiline);
        assert_eq!(opts.rows, 2);
        assert!(!opts.monospace);
    }

    #[test]
    fn test_opts_builders() {
        let opts = TextFieldOpts::hint("Search").with_width(200.0).with_monospace(true);
        assert_eq!(opts.hint, "Search");
        assert_eq!(opts.width, Some(200.0));
        assert!(opts.monospace);
        assert!(!opts.password);

        let pwd = TextFieldOpts::password("Pass").with_multiline(4);
        assert!(pwd.password);
        assert!(pwd.multiline);
        assert_eq!(pwd.rows, 4);
    }

    #[test]
    fn test_field_height() {
        assert_eq!(field_height(false, 1), 40.0);
        assert_eq!(field_height(false, 5), 40.0);
        assert_eq!(field_height(true, 1), 40.0);
        assert_eq!(field_height(true, 2), 60.0);
        assert_eq!(field_height(true, 3), 80.0);
    }

    #[test]
    fn test_trailing_button_visibility() {
        assert!(has_trailing_button(true, true));
        assert!(has_trailing_button(true, false));
        assert!(!has_trailing_button(false, true));
        assert!(has_trailing_button(false, false));
    }
}
