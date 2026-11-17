//! Clip thumbnail used by the library and home grids: image (or placeholder),
//! a moving preview while hovered (move the mouse to scrub), a duration badge and a
//! clickable favorite star.

use eframe::egui::{self, Color32, FontId, Rect, Sense, Stroke, Vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::clip_index::ClipMetadata;
use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::components::motion::{hover_t, lerp_color};
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};

/// Returns true when the thumbnail itself was clicked (open the clip).
pub fn clip_thumbnail(ui: &mut egui::Ui, theme: &MonetTheme, clip: &ClipMetadata, size: Vec2, save_path: &Path) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let h = hover_t(ui, &resp);

    if clip.thumb_path.exists() {
        let uri = format!("file://{}", clip.thumb_path.display());
        egui::Image::new(uri).rounding(M3Shapes::MEDIUM).paint_at(ui, rect);
    } else {
        ui.painter().rect_filled(rect, M3Shapes::MEDIUM, theme.surface_container_highest);
        ui.painter().rect_stroke(rect, M3Shapes::MEDIUM, Stroke::new(1.0_f32, theme.outline_variant));
    }

    let preview = hover_preview(ui, &resp, clip, rect);
    let painter = ui.painter();
    if preview {
        // The preview is the hint; only a thin progress line on top.
    } else if h > 0.01 {
        painter.rect_filled(rect, M3Shapes::MEDIUM, Color32::from_black_alpha((70.0 * h) as u8));
        painter.circle_filled(rect.center(), 22.0, Color32::from_black_alpha((150.0 * h) as u8));
        draw_vector_icon(painter, rect.center() + Vec2::new(1.5, 0.0), 20.0, IconKind::Play, Color32::WHITE.gamma_multiply(h));
    } else if !clip.thumb_path.exists() {
        draw_vector_icon(painter, rect.center(), 24.0, IconKind::Play, theme.outline);
    }

    // Duration badge, bottom right.
    if !clip.duration_str.is_empty() {
        let galley = painter.layout_no_wrap(clip.duration_str.clone(), FontId::proportional(11.0), Color32::WHITE);
        let badge = Rect::from_min_size(
            rect.right_bottom() - galley.size() - Vec2::new(14.0, 10.0),
            galley.size() + Vec2::new(8.0, 4.0),
        );
        painter.rect_filled(badge, M3Shapes::SMALL, Color32::from_black_alpha(185));
        painter.galley(badge.min + Vec2::new(4.0, 2.0), galley, Color32::WHITE);
    }

    // Favorite star, top right. Always visible when starred, otherwise only on hover.
    let starred = crate::favorites::is_favorite(save_path, &clip.path);
    let star_rect = Rect::from_center_size(rect.right_top() + Vec2::new(-18.0, 18.0), Vec2::splat(28.0));
    let star_resp = ui.interact(star_rect, resp.id.with("star"), Sense::click());
    let star_h = hover_t(ui, &star_resp);
    let show = if starred { 1.0 } else { h.max(star_h) };
    if show > 0.01 {
        let painter = ui.painter();
        painter.circle_filled(star_rect.center(), 13.0, Color32::from_black_alpha((150.0 * show) as u8));
        let color = if starred {
            lerp_color(Color32::from_rgb(255, 200, 60), Color32::WHITE, star_h * 0.3)
        } else {
            Color32::WHITE.gamma_multiply(show)
        };
        let icon = if starred { IconKind::StarFilled } else { IconKind::Star };
        draw_vector_icon(painter, star_rect.center(), 15.0, icon, color);
    }
    let hint = if starred { t("Remove from favorites") } else { t("Add to favorites (kept by the storage limit)") };
    if star_resp.on_hover_text(hint).clicked() {
        crate::favorites::toggle(save_path, &clip.path);
        return false;
    }

    resp.clicked()
}

/// Preview frames of hovered clips (the trimmer's filmstrip, cached on disk).
static PREVIEWS: Mutex<Option<HashMap<PathBuf, crate::gui::views::trimmer::timeline::Filmstrip>>> = Mutex::new(None);

/// While hovered for a moment, shows the frame under the mouse. Returns true if it drew one.
fn hover_preview(ui: &egui::Ui, resp: &egui::Response, clip: &ClipMetadata, rect: Rect) -> bool {
    use crate::gui::views::trimmer::timeline::{Filmstrip, FRAMES};
    let since_id = resp.id.with("hover_since");
    let Some(pos) = resp.hover_pos() else {
        ui.data_mut(|d| d.remove::<f64>(since_id));
        return false;
    };
    let now = ui.input(|i| i.time);
    let since: f64 = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(since_id, || now));
    if now - since < 0.35 || clip.duration_secs <= 0.0 {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        return false;
    }
    let frames = {
        let mut guard = PREVIEWS.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.get_or_insert_with(HashMap::new);
        if map.len() > 40 {
            map.clear();
        }
        map.entry(clip.path.clone())
            .or_insert_with(|| Filmstrip::start(&clip.path, clip.duration_secs))
            .frames()
    };
    let frac = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 0.999);
    let index = (frac * FRAMES as f32) as usize;
    let Some(Some(frame)) = frames.get(index) else {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
        return false;
    };
    egui::Image::new(format!("file://{}", frame.display())).rounding(M3Shapes::MEDIUM).paint_at(ui, rect);
    let bar = Rect::from_min_size(rect.left_bottom() - Vec2::new(0.0, 3.0), Vec2::new(rect.width() * (index + 1) as f32 / FRAMES as f32, 3.0));
    ui.painter().rect_filled(bar, 1.5, Color32::WHITE.gamma_multiply(0.85));
    true
}
