use eframe::egui::{self, Color32, Painter, Pos2, Rect, Rounding, Stroke, Vec2};

/// Paints the Lapse logo vectorially into `rect` (square).
///
/// Scaled from the SVG geometry: rounded tile, two vertical bars,
/// filled left triangle, and hollow right triangle.
pub fn draw_lapse_logo(painter: &Painter, rect: Rect, tile: Color32, mark: Color32) {
    let size = rect.width().min(rect.height());
    let x0 = rect.min.x + (rect.width() - size) * 0.5;
    let y0 = rect.min.y + (rect.height() - size) * 0.5;
    let k = size / 512.0;

    // Tile: rounded rect with corner radius 112/512 of size.
    let corner_radius = size * (112.0 / 512.0);
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(x0, y0), Vec2::splat(size)),
        Rounding::same(corner_radius),
        tile,
    );

    // Two vertical bars.
    let bar_w = 30.25 * k;
    let bar_h = 286.0 * k;
    let bar_rx = 15.125 * k;
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(x0 + 107.5 * k, y0 + 113.0 * k), Vec2::new(bar_w, bar_h)),
        Rounding::same(bar_rx),
        mark,
    );
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(x0 + 374.25 * k, y0 + 113.0 * k), Vec2::new(bar_w, bar_h)),
        Rounding::same(bar_rx),
        mark,
    );

    // Left triangle: filled mark.
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(x0 + 159.75 * k, y0 + 129.5 * k),
            Pos2::new(x0 + 159.75 * k, y0 + 382.5 * k),
            Pos2::new(x0 + 256.0 * k, y0 + 256.0 * k),
        ],
        mark,
        Stroke::NONE,
    ));

    // Right triangle: outer filled with mark, inner filled with tile.
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(x0 + 352.25 * k, y0 + 129.5 * k),
            Pos2::new(x0 + 352.25 * k, y0 + 382.5 * k),
            Pos2::new(x0 + 256.0 * k, y0 + 256.0 * k),
        ],
        mark,
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(x0 + 330.25 * k, y0 + 194.675 * k),
            Pos2::new(x0 + 330.25 * k, y0 + 317.325 * k),
            Pos2::new(x0 + 283.775 * k, y0 + 256.0 * k),
        ],
        tile,
        Stroke::NONE,
    ));
}
