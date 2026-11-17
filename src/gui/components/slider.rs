//! Thin Monet slider: a 4 px rail, the filled part in the accent color and a round
//! thumb that grows a soft halo while hovered or dragged.

use eframe::egui::{self, Color32, Key, Pos2, Rect, Sense, Stroke, Vec2};
use std::ops::RangeInclusive;

use crate::gui::theme::MonetTheme;

const HEIGHT: f32 = 22.0;
const RAIL: f32 = 4.0;
const THUMB: f32 = 7.0;

#[derive(Clone, Copy, Default)]
pub struct SliderOpts {
    /// Logarithmic mapping (for ranges like 10 s .. 2 h).
    pub log: bool,
    /// Snap to multiples of this; 0 = continuous.
    pub step: f64,
}

fn to_t(v: f64, lo: f64, hi: f64, log: bool) -> f32 {
    let t = if log && lo > 0.0 { (v / lo).ln() / (hi / lo).ln() } else { (v - lo) / (hi - lo) };
    t.clamp(0.0, 1.0) as f32
}

fn from_t(t: f32, lo: f64, hi: f64, log: bool) -> f64 {
    let t = t.clamp(0.0, 1.0) as f64;
    if log && lo > 0.0 { lo * (hi / lo).powf(t) } else { lo + (hi - lo) * t }
}

pub fn m3_slider(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    value: &mut f64,
    range: RangeInclusive<f64>,
    width: f32,
    opts: SliderOpts,
) -> egui::Response {
    let (lo, hi) = (*range.start(), *range.end());
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(width, HEIGHT), Sense::click_and_drag());
    let rail = Rect::from_center_size(rect.center(), Vec2::new(rect.width() - THUMB * 2.0, RAIL));
    let enabled = ui.is_enabled();

    let mut t = to_t(*value, lo, hi, opts.log);
    let mut moved = false;
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            t = ((pos.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
            moved = true;
        }
    }
    if resp.has_focus() {
        let nudge = if opts.step > 0.0 { 0.0 } else { 0.01 };
        let (left, right) = ui.input(|i| (i.key_pressed(Key::ArrowLeft), i.key_pressed(Key::ArrowRight)));
        if left || right {
            let dir = if right { 1.0 } else { -1.0 };
            if opts.step > 0.0 {
                let v = (*value + dir * opts.step).clamp(lo, hi);
                t = to_t(v, lo, hi, opts.log);
            } else {
                t = (t + (dir * nudge) as f32).clamp(0.0, 1.0);
            }
            moved = true;
        }
    }
    if moved {
        let mut v = from_t(t, lo, hi, opts.log);
        if opts.step > 0.0 {
            v = (v / opts.step).round() * opts.step;
        }
        let v = v.clamp(lo, hi);
        if v != *value {
            *value = v;
            resp.mark_changed();
        }
        t = to_t(*value, lo, hi, opts.log);
    }

    if ui.is_rect_visible(rect) {
        let active = resp.hovered() || resp.dragged() || resp.has_focus();
        let grow = ui.ctx().animate_bool_with_time(resp.id.with("grow"), active, 0.12);
        let painter = ui.painter();
        let accent = if enabled { theme.primary } else { theme.on_surface_variant.gamma_multiply(0.5) };
        let x = rail.left() + rail.width() * t;
        let center = Pos2::new(x, rail.center().y);

        painter.rect_filled(rail, RAIL / 2.0, accent.gamma_multiply(0.22));
        // Ranges around zero (like -300..300 ms) fill from the middle.
        let anchor = if lo < 0.0 && hi > 0.0 { rail.left() + rail.width() * to_t(0.0, lo, hi, opts.log) } else { rail.left() };
        let filled = Rect::from_min_max(Pos2::new(anchor.min(x), rail.top()), Pos2::new(anchor.max(x), rail.bottom()));
        painter.rect_filled(filled, RAIL / 2.0, accent);

        if opts.step > 0.0 {
            let steps = ((hi - lo) / opts.step).round() as usize;
            if (1..=12).contains(&steps) {
                for i in 0..=steps {
                    let tx = rail.left() + rail.width() * i as f32 / steps as f32;
                    let c = if tx <= x { theme.on_primary.gamma_multiply(0.7) } else { accent.gamma_multiply(0.5) };
                    painter.circle_filled(Pos2::new(tx, rail.center().y), 1.2, c);
                }
            }
        }

        if grow > 0.0 {
            painter.circle_filled(center, THUMB + 6.0 * grow, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), (40.0 * grow) as u8));
        }
        let r = THUMB + 1.0 * grow;
        painter.circle_filled(center, r, accent);
        painter.circle_stroke(center, r, Stroke::new(2.0_f32, theme.surface_container_low));
    }
    if enabled {
        resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    resp
}
