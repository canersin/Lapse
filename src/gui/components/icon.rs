use eframe::egui::{self, Color32, Painter, Pos2, Rect, Rounding, Stroke, Vec2};

#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconKind {
    Home,
    Library,
    Scissors,
    Gear,
    Info,
    Video,
    Audio,
    Mic,
    Keyboard,
    Palette,
    Update,
    Timer,
    RecordDot,
    Save,
    Play,
    Pause,
    Share,
    Folder,
    Trash,
    Gamepad,
    Bolt,
    Check,
    Close,
    Copy,
    Globe,
    ArrowLeft,
    Chip,
    Warning,
    StepBack,
    StepForward,
    Rewind,
    FastForward,
    Loop,
    Muted,
    Star,
    StarFilled,
    Fullscreen,
    ExitFullscreen,
    Pencil,
    Crosshair,
    Sidebar,
}

pub fn draw_vector_icon(painter: &Painter, center: Pos2, size: f32, kind: IconKind, color: Color32) {
    let r = size * 0.45;
    let sw = (size * 0.11).clamp(1.3, 2.2);
    let stroke = Stroke::new(sw, color);

    match kind {
        IconKind::Home => {
            let pts = vec![
                Pos2::new(center.x, center.y - r),
                Pos2::new(center.x - r, center.y - r * 0.1),
                Pos2::new(center.x - r * 0.7, center.y - r * 0.1),
                Pos2::new(center.x - r * 0.7, center.y + r * 0.85),
                Pos2::new(center.x + r * 0.7, center.y + r * 0.85),
                Pos2::new(center.x + r * 0.7, center.y - r * 0.1),
                Pos2::new(center.x + r, center.y - r * 0.1),
                Pos2::new(center.x, center.y - r),
            ];
            painter.add(egui::Shape::line(pts, stroke));
        }
        IconKind::Library => {
            let g = r * 0.75;
            let box_sz = Vec2::splat(g * 0.82);
            let tl = Rect::from_min_size(Pos2::new(center.x - g, center.y - g), box_sz);
            let tr = Rect::from_min_size(Pos2::new(center.x + g * 0.18, center.y - g), box_sz);
            let bl = Rect::from_min_size(Pos2::new(center.x - g, center.y + g * 0.18), box_sz);
            let br = Rect::from_min_size(Pos2::new(center.x + g * 0.18, center.y + g * 0.18), box_sz);
            for rect in [tl, tr, bl, br] {
                painter.rect_stroke(rect, Rounding::same(1.5), stroke);
            }
        }
        IconKind::Scissors => {
            let c1 = Pos2::new(center.x - r * 0.55, center.y + r * 0.55);
            let c2 = Pos2::new(center.x - r * 0.55, center.y - r * 0.55);
            painter.circle_stroke(c1, r * 0.35, stroke);
            painter.circle_stroke(c2, r * 0.35, stroke);
            painter.line_segment([Pos2::new(c1.x + r * 0.25, c1.y - r * 0.2), Pos2::new(center.x + r * 0.85, center.y - r * 0.65)], stroke);
            painter.line_segment([Pos2::new(c2.x + r * 0.25, c2.y + r * 0.2), Pos2::new(center.x + r * 0.85, center.y + r * 0.65)], stroke);
        }
        IconKind::Gear => {
            painter.circle_stroke(center, r * 0.42, stroke);
            for i in 0..8 {
                let angle = (i as f32) * std::f32::consts::PI * 0.25;
                let p1 = Pos2::new(center.x + angle.cos() * r * 0.55, center.y + angle.sin() * r * 0.55);
                let p2 = Pos2::new(center.x + angle.cos() * r * 0.92, center.y + angle.sin() * r * 0.92);
                painter.line_segment([p1, p2], stroke);
            }
        }
        IconKind::Info => {
            painter.circle_stroke(center, r * 0.88, stroke);
            painter.circle_filled(Pos2::new(center.x, center.y - r * 0.42), sw * 0.75, color);
            painter.line_segment([Pos2::new(center.x, center.y - r * 0.1), Pos2::new(center.x, center.y + r * 0.5)], stroke);
        }
        IconKind::Video => {
            let body = Rect::from_min_max(
                Pos2::new(center.x - r * 0.9, center.y - r * 0.6),
                Pos2::new(center.x + r * 0.35, center.y + r * 0.6),
            );
            painter.rect_stroke(body, Rounding::same(2.0), stroke);
            let lens = vec![
                Pos2::new(center.x + r * 0.35, center.y - r * 0.25),
                Pos2::new(center.x + r * 0.9, center.y - r * 0.55),
                Pos2::new(center.x + r * 0.9, center.y + r * 0.55),
                Pos2::new(center.x + r * 0.35, center.y + r * 0.25),
            ];
            painter.add(egui::Shape::line(lens, stroke));
        }
        IconKind::Audio => {
            let spk = vec![
                Pos2::new(center.x - r * 0.85, center.y - r * 0.3),
                Pos2::new(center.x - r * 0.45, center.y - r * 0.3),
                Pos2::new(center.x, center.y - r * 0.75),
                Pos2::new(center.x, center.y + r * 0.75),
                Pos2::new(center.x - r * 0.45, center.y + r * 0.3),
                Pos2::new(center.x - r * 0.85, center.y + r * 0.3),
                Pos2::new(center.x - r * 0.85, center.y - r * 0.3),
            ];
            painter.add(egui::Shape::line(spk, stroke));
            painter.line_segment([Pos2::new(center.x + r * 0.35, center.y - r * 0.35), Pos2::new(center.x + r * 0.5, center.y)], stroke);
            painter.line_segment([Pos2::new(center.x + r * 0.5, center.y), Pos2::new(center.x + r * 0.35, center.y + r * 0.35)], stroke);
            painter.line_segment([Pos2::new(center.x + r * 0.65, center.y - r * 0.65), Pos2::new(center.x + r * 0.85, center.y)], stroke);
            painter.line_segment([Pos2::new(center.x + r * 0.85, center.y), Pos2::new(center.x + r * 0.65, center.y + r * 0.65)], stroke);
        }
        IconKind::Mic => {
            let cap = Rect::from_min_max(
                Pos2::new(center.x - r * 0.35, center.y - r * 0.85),
                Pos2::new(center.x + r * 0.35, center.y + r * 0.15),
            );
            painter.rect_stroke(cap, Rounding::same(r * 0.35), stroke);
            painter.line_segment([Pos2::new(center.x - r * 0.65, center.y - r * 0.05), Pos2::new(center.x - r * 0.45, center.y + r * 0.45)], stroke);
            painter.line_segment([Pos2::new(center.x - r * 0.45, center.y + r * 0.45), Pos2::new(center.x + r * 0.45, center.y + r * 0.45)], stroke);
            painter.line_segment([Pos2::new(center.x + r * 0.45, center.y + r * 0.45), Pos2::new(center.x + r * 0.65, center.y - r * 0.05)], stroke);
            painter.line_segment([Pos2::new(center.x, center.y + r * 0.45), Pos2::new(center.x, center.y + r * 0.85)], stroke);
            painter.line_segment([Pos2::new(center.x - r * 0.4, center.y + r * 0.85), Pos2::new(center.x + r * 0.4, center.y + r * 0.85)], stroke);
        }
        IconKind::Keyboard => {
            let kb = Rect::from_min_max(
                Pos2::new(center.x - r * 0.95, center.y - r * 0.6),
                Pos2::new(center.x + r * 0.95, center.y + r * 0.6),
            );
            painter.rect_stroke(kb, Rounding::same(2.5), stroke);
            for dx in [-0.5, 0.0, 0.5] {
                painter.circle_filled(Pos2::new(center.x + r * dx, center.y - r * 0.2), sw * 0.65, color);
            }
            painter.line_segment([Pos2::new(center.x - r * 0.45, center.y + r * 0.25), Pos2::new(center.x + r * 0.45, center.y + r * 0.25)], stroke);
        }
        IconKind::Palette => {
            painter.circle_stroke(center, r * 0.88, stroke);
            painter.circle_filled(Pos2::new(center.x - r * 0.35, center.y - r * 0.25), sw * 0.85, color);
            painter.circle_filled(Pos2::new(center.x + r * 0.1, center.y - r * 0.42), sw * 0.85, color);
            painter.circle_filled(Pos2::new(center.x + r * 0.42, center.y - r * 0.05), sw * 0.85, color);
            painter.circle_filled(Pos2::new(center.x - r * 0.15, center.y + r * 0.35), sw * 0.85, color);
        }
        IconKind::Update => {
            painter.circle_stroke(center, r * 0.85, stroke);
            painter.line_segment([Pos2::new(center.x, center.y + r * 0.45), Pos2::new(center.x, center.y - r * 0.45)], stroke);
            painter.line_segment([Pos2::new(center.x - r * 0.35, center.y - r * 0.1), Pos2::new(center.x, center.y - r * 0.45)], stroke);
            painter.line_segment([Pos2::new(center.x + r * 0.35, center.y - r * 0.1), Pos2::new(center.x, center.y - r * 0.45)], stroke);
        }
        IconKind::Timer => {
            painter.circle_stroke(Pos2::new(center.x, center.y + r * 0.08), r * 0.8, stroke);
            painter.line_segment([Pos2::new(center.x - r * 0.3, center.y - r * 0.88), Pos2::new(center.x + r * 0.3, center.y - r * 0.88)], stroke);
            painter.line_segment([Pos2::new(center.x, center.y + r * 0.08), Pos2::new(center.x, center.y - r * 0.35)], stroke);
            painter.line_segment([Pos2::new(center.x, center.y + r * 0.08), Pos2::new(center.x + r * 0.35, center.y + r * 0.25)], stroke);
        }
        IconKind::RecordDot => {
            painter.circle_stroke(center, r * 0.85, stroke);
            painter.circle_filled(center, r * 0.42, color);
        }
        IconKind::Save => {
            let box_rect = Rect::from_min_max(
                Pos2::new(center.x - r * 0.8, center.y - r * 0.8),
                Pos2::new(center.x + r * 0.8, center.y + r * 0.8),
            );
            painter.rect_stroke(box_rect, Rounding::same(2.0), stroke);
            let inner = Rect::from_min_max(
                Pos2::new(center.x - r * 0.45, center.y - r * 0.8),
                Pos2::new(center.x + r * 0.45, center.y - r * 0.2),
            );
            painter.rect_stroke(inner, Rounding::same(1.0), stroke);
        }
        IconKind::Play => {
            let pts = vec![
                Pos2::new(center.x - r * 0.45, center.y - r * 0.7),
                Pos2::new(center.x + r * 0.65, center.y),
                Pos2::new(center.x - r * 0.45, center.y + r * 0.7),
            ];
            painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        IconKind::Pause => {
            let r1 = Rect::from_min_max(
                Pos2::new(center.x - r * 0.5, center.y - r * 0.65),
                Pos2::new(center.x - r * 0.15, center.y + r * 0.65),
            );
            let r2 = Rect::from_min_max(
                Pos2::new(center.x + r * 0.15, center.y - r * 0.65),
                Pos2::new(center.x + r * 0.5, center.y + r * 0.65),
            );
            painter.rect_filled(r1, Rounding::same(1.5), color);
            painter.rect_filled(r2, Rounding::same(1.5), color);
        }
        IconKind::Share => {
            let p_left = Pos2::new(center.x - r * 0.65, center.y);
            let p_top = Pos2::new(center.x + r * 0.55, center.y - r * 0.6);
            let p_bot = Pos2::new(center.x + r * 0.55, center.y + r * 0.6);
            painter.line_segment([p_left, p_top], stroke);
            painter.line_segment([p_left, p_bot], stroke);
            for p in [p_left, p_top, p_bot] {
                painter.circle_filled(p, r * 0.25, color);
            }
        }
        IconKind::Folder => {
            let pts = vec![
                Pos2::new(center.x - r * 0.85, center.y - r * 0.65),
                Pos2::new(center.x - r * 0.2, center.y - r * 0.65),
                Pos2::new(center.x, center.y - r * 0.35),
                Pos2::new(center.x + r * 0.85, center.y - r * 0.35),
                Pos2::new(center.x + r * 0.85, center.y + r * 0.65),
                Pos2::new(center.x - r * 0.85, center.y + r * 0.65),
                Pos2::new(center.x - r * 0.85, center.y - r * 0.65),
            ];
            painter.add(egui::Shape::line(pts, stroke));
        }
        IconKind::Trash => {
            painter.line_segment([Pos2::new(center.x - r * 0.75, center.y - r * 0.55), Pos2::new(center.x + r * 0.75, center.y - r * 0.55)], stroke);
            let body = Rect::from_min_max(
                Pos2::new(center.x - r * 0.55, center.y - r * 0.55),
                Pos2::new(center.x + r * 0.55, center.y + r * 0.75),
            );
            painter.rect_stroke(body, Rounding::same(1.5), stroke);
        }
        IconKind::Gamepad => {
            let body = Rect::from_min_max(
                Pos2::new(center.x - r * 0.85, center.y - r * 0.45),
                Pos2::new(center.x + r * 0.85, center.y + r * 0.45),
            );
            painter.rect_stroke(body, Rounding::same(r * 0.4), stroke);
            painter.line_segment([Pos2::new(center.x - r * 0.55, center.y), Pos2::new(center.x - r * 0.2, center.y)], stroke);
            painter.line_segment([Pos2::new(center.x - r * 0.375, center.y - r * 0.18), Pos2::new(center.x - r * 0.375, center.y + r * 0.18)], stroke);
            painter.circle_filled(Pos2::new(center.x + r * 0.35, center.y - r * 0.1), sw * 0.65, color);
            painter.circle_filled(Pos2::new(center.x + r * 0.55, center.y + r * 0.1), sw * 0.65, color);
        }
        IconKind::Bolt => {
            let pts = vec![
                Pos2::new(center.x + r * 0.15, center.y - r * 0.85),
                Pos2::new(center.x - r * 0.55, center.y + r * 0.08),
                Pos2::new(center.x + r * 0.05, center.y + r * 0.08),
                Pos2::new(center.x - r * 0.15, center.y + r * 0.85),
                Pos2::new(center.x + r * 0.55, center.y - r * 0.08),
                Pos2::new(center.x - r * 0.05, center.y - r * 0.08),
                Pos2::new(center.x + r * 0.15, center.y - r * 0.85),
            ];
            painter.add(egui::Shape::line(pts, stroke));
        }
        IconKind::Check => {
            let pts = vec![
                Pos2::new(center.x - r * 0.65, center.y + r * 0.05),
                Pos2::new(center.x - r * 0.15, center.y + r * 0.55),
                Pos2::new(center.x + r * 0.7, center.y - r * 0.45),
            ];
            painter.add(egui::Shape::line(pts, stroke));
        }
        IconKind::Sidebar => {
            // Window with a panel on the left, like a browser's sidebar button.
            let frame = Rect::from_center_size(center, Vec2::new(r * 2.0, r * 1.7));
            painter.rect_stroke(frame, Rounding::same(2.5), stroke);
            let x = frame.left() + frame.width() * 0.36;
            painter.line_segment([Pos2::new(x, frame.top()), Pos2::new(x, frame.bottom())], stroke);
        }
        IconKind::Close => {
            painter.line_segment([Pos2::new(center.x - r * 0.6, center.y - r * 0.6), Pos2::new(center.x + r * 0.6, center.y + r * 0.6)], stroke);
            painter.line_segment([Pos2::new(center.x + r * 0.6, center.y - r * 0.6), Pos2::new(center.x - r * 0.6, center.y + r * 0.6)], stroke);
        }
        IconKind::Copy => {
            let r1 = Rect::from_min_max(
                Pos2::new(center.x - r * 0.65, center.y - r * 0.65),
                Pos2::new(center.x + r * 0.35, center.y + r * 0.35),
            );
            let r2 = Rect::from_min_max(
                Pos2::new(center.x - r * 0.35, center.y - r * 0.35),
                Pos2::new(center.x + r * 0.65, center.y + r * 0.65),
            );
            painter.rect_stroke(r1, Rounding::same(1.5), stroke);
            painter.rect_stroke(r2, Rounding::same(1.5), stroke);
        }
        IconKind::Globe => {
            painter.circle_stroke(center, r * 0.85, stroke);
            painter.line_segment([Pos2::new(center.x - r * 0.85, center.y), Pos2::new(center.x + r * 0.85, center.y)], stroke);
            painter.line_segment([Pos2::new(center.x, center.y - r * 0.85), Pos2::new(center.x, center.y + r * 0.85)], stroke);
        }
        IconKind::ArrowLeft => {
            painter.line_segment([Pos2::new(center.x + r * 0.65, center.y), Pos2::new(center.x - r * 0.65, center.y)], stroke);
            painter.line_segment([Pos2::new(center.x - r * 0.15, center.y - r * 0.5), Pos2::new(center.x - r * 0.65, center.y)], stroke);
            painter.line_segment([Pos2::new(center.x - r * 0.15, center.y + r * 0.5), Pos2::new(center.x - r * 0.65, center.y)], stroke);
        }
        IconKind::Chip => {
            // Graphics card: board with a fan and bracket pins.
            let board = Rect::from_min_max(
                Pos2::new(center.x - r * 0.9, center.y - r * 0.55),
                Pos2::new(center.x + r * 0.9, center.y + r * 0.45),
            );
            painter.rect_stroke(board, Rounding::same(2.0), stroke);
            painter.circle_stroke(Pos2::new(center.x + r * 0.2, center.y - r * 0.05), r * 0.32, stroke);
            for i in 0..3 {
                let x = center.x - r * 0.65 + i as f32 * r * 0.3;
                painter.line_segment([Pos2::new(x, center.y + r * 0.45), Pos2::new(x, center.y + r * 0.8)], stroke);
            }
        }
        IconKind::StepBack | IconKind::StepForward => {
            // Bar + triangle: |◀ or ▶|
            let dir = if kind == IconKind::StepForward { 1.0 } else { -1.0 };
            let tip = center.x + dir * r * 0.45;
            let base = center.x - dir * r * 0.55;
            painter.add(egui::Shape::convex_polygon(
                vec![Pos2::new(base, center.y - r * 0.6), Pos2::new(tip, center.y), Pos2::new(base, center.y + r * 0.6)],
                color,
                Stroke::NONE,
            ));
            let bar_x = center.x + dir * r * 0.7;
            painter.line_segment([Pos2::new(bar_x, center.y - r * 0.6), Pos2::new(bar_x, center.y + r * 0.6)], Stroke::new(sw * 1.3, color));
        }
        IconKind::Rewind | IconKind::FastForward => {
            let dir = if kind == IconKind::FastForward { 1.0 } else { -1.0 };
            for offset in [-0.45, 0.45] {
                let cx = center.x + offset * r;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(cx - dir * r * 0.45, center.y - r * 0.55),
                        Pos2::new(cx + dir * r * 0.45, center.y),
                        Pos2::new(cx - dir * r * 0.45, center.y + r * 0.55),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
        }
        IconKind::Loop => {
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = std::f32::consts::PI * (0.25 + 1.6 * i as f32 / 24.0);
                    Pos2::new(center.x + r * 0.75 * a.cos(), center.y + r * 0.75 * a.sin())
                })
                .collect();
            let end = *pts.last().unwrap();
            painter.add(egui::Shape::line(pts, stroke));
            painter.line_segment([end, end + egui::vec2(r * 0.45, -r * 0.05)], stroke);
            painter.line_segment([end, end + egui::vec2(r * 0.05, r * 0.45)], stroke);
        }
        IconKind::Pencil => {
            // A pencil lying from bottom-left (tip) to top-right.
            let d = Vec2::new(1.0, -1.0) / 2f32.sqrt();
            let n = Vec2::new(d.y, -d.x) * -1.0;
            let tip = Pos2::new(center.x - r * 0.75, center.y + r * 0.75);
            let w = r * 0.32;
            let body_start = tip + d * r * 0.45;
            let end = tip + d * r * 2.0;
            let outline = vec![tip, body_start + n * w, end + n * w, end - n * w, body_start - n * w, tip];
            painter.add(egui::Shape::line(outline, stroke));
            painter.line_segment([end - d * r * 0.4 + n * w, end - d * r * 0.4 - n * w], stroke);
        }
        IconKind::Crosshair => {
            // Ring with four ticks pointing in and a dot in the middle: a kill.
            painter.circle_stroke(center, r * 0.62, stroke);
            for d in [Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(0.0, -1.0)] {
                painter.line_segment([center + d * r * 0.3, center + d * r * 1.05], stroke);
            }
            painter.circle_filled(center, sw * 0.6, color);
        }
        IconKind::Fullscreen | IconKind::ExitFullscreen => {
            // Four corner brackets, pointing out (enter) or in (exit).
            let (a, b) = (r * 0.85, r * 0.4);
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let corner = if kind == IconKind::Fullscreen {
                    Pos2::new(center.x + sx * a, center.y + sy * a)
                } else {
                    Pos2::new(center.x + sx * b * 0.6, center.y + sy * b * 0.6)
                };
                let dir = if kind == IconKind::Fullscreen { -1.0 } else { 1.0 };
                let pts = vec![
                    Pos2::new(corner.x + dir * sx * b, corner.y),
                    corner,
                    Pos2::new(corner.x, corner.y + dir * sy * b),
                ];
                painter.add(egui::Shape::line(pts, stroke));
            }
        }
        IconKind::Muted => {
            draw_vector_icon(painter, Pos2::new(center.x - r * 0.25, center.y), size * 0.8, IconKind::Audio, color);
            painter.line_segment([Pos2::new(center.x + r * 0.35, center.y - r * 0.3), Pos2::new(center.x + r * 0.9, center.y + r * 0.3)], stroke);
            painter.line_segment([Pos2::new(center.x + r * 0.9, center.y - r * 0.3), Pos2::new(center.x + r * 0.35, center.y + r * 0.3)], stroke);
        }
        IconKind::Star | IconKind::StarFilled => {
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                    let rr = if i % 2 == 0 { r } else { r * 0.45 };
                    Pos2::new(center.x + rr * a.cos(), center.y + 0.08 * r + rr * a.sin())
                })
                .collect();
            if kind == IconKind::StarFilled {
                // A star is not convex: fill it as a fan of triangles around the center.
                for i in 0..10 {
                    let tri = vec![center + Vec2::new(0.0, 0.08 * r), pts[i], pts[(i + 1) % 10]];
                    painter.add(egui::Shape::convex_polygon(tri, color, Stroke::NONE));
                }
            }
            let mut outline = pts.clone();
            outline.push(pts[0]);
            painter.add(egui::Shape::line(outline, stroke));
        }
        IconKind::Warning => {
            let pts = vec![
                Pos2::new(center.x, center.y - r * 0.85),
                Pos2::new(center.x + r * 0.9, center.y + r * 0.75),
                Pos2::new(center.x - r * 0.9, center.y + r * 0.75),
                Pos2::new(center.x, center.y - r * 0.85),
            ];
            painter.add(egui::Shape::line(pts, stroke));
            painter.line_segment([Pos2::new(center.x, center.y - r * 0.3), Pos2::new(center.x, center.y + r * 0.2)], stroke);
            painter.circle_filled(Pos2::new(center.x, center.y + r * 0.48), sw * 0.7, color);
        }
    }
}

pub fn m3_vector_icon(ui: &mut egui::Ui, kind: IconKind, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    draw_vector_icon(ui.painter(), rect.center(), size, kind, color);
}
