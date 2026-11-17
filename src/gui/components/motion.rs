use eframe::egui::{self, Align2, Color32, FontId, Id, Margin, Pos2, Rect, Response, Stroke, Vec2};
use std::time::{Duration, Instant};

use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::theme::{M3Shapes, MonetTheme};

/// Standard durations, kept short so the UI feels snappy rather than floaty.
pub const FAST: f32 = 0.12;
pub const MEDIUM: f32 = 0.22;

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// Ease-out cubic, used on top of egui's linear animation values.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Smoothly animated hover factor (0..1) for a response.
pub fn hover_t(ui: &egui::Ui, resp: &Response) -> f32 {
    ease_out(ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered(), FAST))
}

/// 0..1 sine pulse with the given period. Keeps the UI repainting, but only smoothly
/// while the window is focused, so an idle Lapse window in the background costs ~no CPU.
pub fn pulse(ui: &egui::Ui, period_secs: f32) -> f32 {
    let (t, focused) = ui.input(|i| (i.time as f32, i.focused));
    let frame = if focused { 33 } else { 1000 };
    ui.ctx().request_repaint_after(Duration::from_millis(frame));
    0.5 + 0.5 * (t * std::f32::consts::TAU / period_secs).sin()
}

/// A Material-style switch whose knob slides and whose track fades between states.
pub fn m3_switch(ui: &mut egui::Ui, theme: &MonetTheme, on: &mut bool) -> Response {
    let size = Vec2::new(46.0, 26.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ease_out(ui.ctx().animate_bool_with_time(resp.id, *on, MEDIUM));
    let h = hover_t(ui, &resp);

    let track = lerp_color(theme.surface_container_highest, theme.primary, t);
    let border = lerp_color(theme.outline, theme.primary, t);
    let painter = ui.painter();
    painter.rect_filled(rect, M3Shapes::FULL, track);
    painter.rect_stroke(rect, M3Shapes::FULL, Stroke::new(1.5_f32, border));

    let r_off = 7.0 + h * 1.0;
    let r_on = 10.0;
    let radius = r_off + (r_on - r_off) * t;
    let x = egui::lerp((rect.left() + 13.0)..=(rect.right() - 13.0), t);
    let knob = lerp_color(theme.outline, theme.on_primary, t);
    if h > 0.0 {
        painter.circle_filled(Pos2::new(x, rect.center().y), 16.0, theme.on_surface.gamma_multiply(0.08 * h));
    }
    painter.circle_filled(Pos2::new(x, rect.center().y), radius, knob);
    resp
}

/// A switch and its label on one row, both vertically centered. Clicking the label toggles too.
pub fn m3_switch_labeled(ui: &mut egui::Ui, theme: &MonetTheme, on: &mut bool, text: &str) -> Response {
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        let mut resp = m3_switch(ui, theme, on);
        let label = ui.add(
            egui::Label::new(egui::RichText::new(text).size(12.5).color(theme.on_surface)).sense(egui::Sense::click()),
        );
        if label.clicked() {
            *on = !*on;
            resp.mark_changed();
        }
        resp
    })
    .inner
}

/// A label centered inside a row of the given height, so it lines up with taller widgets next to it.
pub fn centered_label(ui: &mut egui::Ui, text: egui::RichText, height: f32) -> Response {
    ui.allocate_ui_with_layout(Vec2::new(0.0, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_height(height);
        ui.add(egui::Label::new(text).wrap(false))
    })
    .inner
}

/// Fades the freshly shown page in by painting a fading cover over it.
/// Call after rendering the page content, with a key that changes when the page changes.
pub fn page_fade_in(ui: &egui::Ui, theme: &MonetTheme, rect: Rect, page_key: impl std::hash::Hash) {
    let id = Id::new("lapse_page_fade");
    let key = egui::util::hash(page_key);
    let now = ui.input(|i| i.time);
    let (last_key, start) = ui.data(|d| d.get_temp::<(u64, f64)>(id)).unwrap_or((key, now - 10.0));
    let start = if last_key != key { now } else { start };
    ui.data_mut(|d| d.insert_temp(id, (key, start)));

    let t = ((now - start) as f32 / MEDIUM).clamp(0.0, 1.0);
    if t < 1.0 {
        let alpha = 1.0 - ease_out(t);
        ui.painter()
            .rect_filled(rect, 0.0, theme.background.gamma_multiply(alpha));
        ui.ctx().request_repaint();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub created: Instant,
    pub lifetime: Duration,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, kind: ToastKind, message: impl Into<String>) {
        let message = message.into();
        // Collapse duplicates instead of stacking the same message.
        self.items.retain(|t| t.message != message);
        let lifetime = if kind == ToastKind::Error { Duration::from_secs(7) } else { Duration::from_secs(4) };
        self.items.push(Toast { message, kind, created: Instant::now(), lifetime });
        if self.items.len() > 4 {
            self.items.remove(0);
        }
    }

    /// Draws toasts stacked in the bottom-right corner; each one slides in and fades out.
    pub fn show(&mut self, ctx: &egui::Context, theme: &MonetTheme) {
        let exit = Duration::from_millis(250);
        self.items.retain(|t| t.created.elapsed() < t.lifetime + exit);
        if self.items.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(16));

        let screen = ctx.screen_rect();
        let width = 340.0_f32.min(screen.width() - 32.0);
        let mut y = screen.bottom() - 20.0;
        let mut dismiss = None;

        for (idx, toast) in self.items.iter().enumerate().rev() {
            let age = toast.created.elapsed().as_secs_f32();
            let enter = ease_out((age / MEDIUM).min(1.0));
            let leaving = (age - toast.lifetime.as_secs_f32()).max(0.0) / exit.as_secs_f32();
            let alpha = (enter * (1.0 - leaving)).clamp(0.0, 1.0);
            let slide = (1.0 - enter) * 40.0 + leaving * 20.0;

            let (accent, icon) = match toast.kind {
                ToastKind::Info => (theme.primary, IconKind::Info),
                ToastKind::Success => (theme.success, IconKind::Check),
                ToastKind::Error => (theme.error, IconKind::Close),
            };

            let height = 52.0;
            let rect = Rect::from_min_size(
                Pos2::new(screen.right() - width - 20.0 + slide, y - height),
                Vec2::new(width, height),
            );
            y -= height + 10.0;

            let area = egui::Area::new(Id::new("lapse_toast").with(idx))
                .order(egui::Order::Foreground)
                .fixed_pos(rect.min)
                .interactable(true);
            area.show(ctx, |ui| {
                let (r, resp) = ui.allocate_exact_size(rect.size(), egui::Sense::click());
                let p = ui.painter();
                p.rect_filled(
                    r.translate(Vec2::new(0.0, 4.0)),
                    M3Shapes::MEDIUM,
                    Color32::from_black_alpha((60.0 * alpha) as u8),
                );
                p.rect_filled(r, M3Shapes::MEDIUM, theme.surface_container_highest.gamma_multiply(alpha));
                p.rect_stroke(r, M3Shapes::MEDIUM, Stroke::new(1.0_f32, accent.gamma_multiply(0.6 * alpha)));
                let bar = Rect::from_min_size(r.min + Vec2::new(0.0, 10.0), Vec2::new(3.0, r.height() - 20.0));
                p.rect_filled(bar, 2.0, accent.gamma_multiply(alpha));

                let icon_c = Pos2::new(r.left() + 26.0, r.center().y);
                p.circle_filled(icon_c, 13.0, accent.gamma_multiply(0.18 * alpha));
                draw_vector_icon(p, icon_c, 14.0, icon, accent.gamma_multiply(alpha));

                let galley = p.layout(
                    toast.message.clone(),
                    FontId::proportional(12.5),
                    theme.on_surface.gamma_multiply(alpha),
                    r.width() - 64.0,
                );
                let text_pos = Pos2::new(r.left() + 48.0, r.center().y - galley.size().y / 2.0);
                p.galley(text_pos, galley, theme.on_surface);

                // Remaining lifetime as a thin progress line.
                let life = 1.0 - (age / toast.lifetime.as_secs_f32()).min(1.0);
                let line = Rect::from_min_size(
                    Pos2::new(r.left() + 12.0, r.bottom() - 3.0),
                    Vec2::new((r.width() - 24.0) * life, 1.5),
                );
                p.rect_filled(line, 1.0, accent.gamma_multiply(0.5 * alpha));

                if resp.clicked() {
                    dismiss = Some(idx);
                }
            });
        }

        if let Some(i) = dismiss {
            self.items.remove(i);
        }
    }
}

/// A small pill label with a dot, e.g. "● REC". The dot pulses when `live`.
pub fn live_pill(ui: &mut egui::Ui, theme: &MonetTheme, text: &str, color: Color32, live: bool) -> Response {
    let font = FontId::proportional(11.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), color);
    let size = Vec2::new(galley.size().x + 30.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, M3Shapes::FULL, color.gamma_multiply(0.14));
    p.rect_stroke(rect, M3Shapes::FULL, Stroke::new(1.0_f32, color.gamma_multiply(0.45)));
    let dot = Pos2::new(rect.left() + 12.0, rect.center().y);
    if live {
        let k = pulse(ui, 1.4);
        p.circle_filled(dot, 4.0 + 4.0 * k, color.gamma_multiply(0.25 * (1.0 - k)));
    }
    p.circle_filled(dot, 4.0, color);
    p.text(Pos2::new(rect.left() + 22.0, rect.center().y), Align2::LEFT_CENTER, text, font, color);
    let _ = theme;
    resp
}
