//! The animated "not in the game" card that replaces the picture while the player is
//! alt-tabbed out in Games only mode. Pure drawing (BGRA8), no Windows API.

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use image::RgbaImage;

/// #141218 as BGRA.
pub const BACKGROUND: [u8; 4] = [0x18, 0x12, 0x14, 0xFF];
const TITLE: [u8; 3] = [0xE9, 0xE0, 0xE6];
const ACCENT: [u8; 3] = [0xC8, 0xB1, 0xFF];
const FADE_SECS: f32 = 0.35;

/// A rectangle inside the card's content box: x, y, width, height.
type Area = (usize, usize, usize, usize);

pub struct FocusCard {
    /// Where the content box sits in the full frame.
    rect: (u32, u32, u32, u32),
    /// Content box with only the title drawn.
    base: Vec<u8>,
    buf: Vec<u8>,
    icon: RgbaImage,
    icon_area: Area,
    dots_area: Area,
    dot_radius: f32,
    /// The fade-in is over and `buf` holds the title, so only the moving parts are redrawn.
    settled: bool,
}

impl FocusCard {
    pub fn new(width: u32, height: u32, title: &str) -> anyhow::Result<Self> {
        let fonts = eframe::egui::FontDefinitions::default();
        let data = fonts.font_data.get("Ubuntu-Light").ok_or_else(|| anyhow::anyhow!("bundled font missing"))?;
        let font = FontRef::try_from_slice(&data.font)?;

        let unit = height.min(width) as f32;
        let icon_size = ((unit * 0.09) as u32).max(16);
        let mut scale = PxScale::from((unit * 0.04).max(10.0));
        let text_width = |s: PxScale| {
            let f = font.as_scaled(s);
            title.chars().map(|c| f.h_advance(f.glyph_id(c))).sum::<f32>()
        };
        let max_text = width as f32 * 0.9;
        if text_width(scale) > max_text {
            scale = PxScale::from(scale.x * max_text / text_width(scale));
        }
        let text_w = text_width(scale);
        let scaled = font.as_scaled(scale);

        let gap = (unit * 0.028) as u32;
        let dots_h = ((unit * 0.03) as u32).max(8);
        let even = |v: u32| v & !1;
        let w = even((text_w.ceil() as u32 + 2 * gap).max(icon_size * 2).min(width));
        let h = even((icon_size + gap + scaled.height().ceil() as u32 + gap + dots_h + 2).min(height));
        let rect = (even((width - w) / 2), even((height - h) / 2), w, h);

        let mut base: Vec<u8> = BACKGROUND.iter().copied().cycle().take((w * h * 4) as usize).collect();
        let mut x = (w as f32 - text_w) / 2.0;
        let baseline = (icon_size + gap) as f32 + scaled.ascent();
        for c in title.chars() {
            let id = scaled.glyph_id(c);
            if let Some(outline) = font.outline_glyph(id.with_scale_and_position(scale, point(x, baseline))) {
                let b = outline.px_bounds();
                outline.draw(|gx, gy, cover| {
                    let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                        blend(&mut base, w as usize, px as usize, py as usize, TITLE, cover);
                    }
                });
            }
            x += scaled.h_advance(id);
        }

        let icon = image::load_from_memory(include_bytes!("../../assets/icon.png"))?
            .resize_exact(icon_size, icon_size, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let dots_w = (dots_h * 4).min(w);
        Ok(Self {
            rect,
            buf: base.clone(),
            base,
            icon,
            icon_area: (((w - icon_size) / 2) as usize, 0, icon_size as usize, icon_size as usize),
            dots_area: (((w - dots_w) / 2) as usize, (h - dots_h) as usize, dots_w as usize, dots_h as usize),
            dot_radius: dots_h as f32 * 0.36,
            settled: false,
        })
    }

    /// The part of the frame `draw` fills; the rest is `BACKGROUND`.
    pub fn content_rect(&self) -> (u32, u32, u32, u32) {
        self.rect
    }

    /// The content box `seconds` after the card appeared, as tightly packed BGRA.
    pub fn draw(&mut self, seconds: f32) -> &[u8] {
        let fade = 1.0 - (1.0 - (seconds / FADE_SECS).clamp(0.0, 1.0)).powi(3);
        if fade < 1.0 || !self.settled {
            self.buf.copy_from_slice(&self.base);
        }
        self.settled = fade >= 1.0;
        self.draw_icon(seconds);
        self.draw_dots(seconds);
        if fade < 1.0 {
            for px in self.buf.chunks_exact_mut(4) {
                for (v, bg) in px.iter_mut().zip(BACKGROUND) {
                    *v = (bg as f32 + (*v as f32 - bg as f32) * fade).round() as u8;
                }
            }
        }
        &self.buf
    }

    fn clear(&mut self, (ax, ay, aw, ah): Area) {
        let w = self.rect.2 as usize;
        for y in ay..ay + ah {
            for px in self.buf[(y * w + ax) * 4..(y * w + ax + aw) * 4].chunks_exact_mut(4) {
                px.copy_from_slice(&BACKGROUND);
            }
        }
    }

    fn draw_icon(&mut self, t: f32) {
        let breath = 0.9 + 0.1 * (t * std::f32::consts::TAU / 2.4).sin();
        self.clear(self.icon_area);
        let (ax, ay, ..) = self.icon_area;
        for (x, y, p) in self.icon.enumerate_pixels() {
            let alpha = p[3] as f32 / 255.0 * breath;
            blend(&mut self.buf, self.rect.2 as usize, ax + x as usize, ay + y as usize, [p[2], p[1], p[0]], alpha);
        }
    }

    fn draw_dots(&mut self, t: f32) {
        self.clear(self.dots_area);
        let (ax, ay, aw, ah) = self.dots_area;
        for i in 0..3 {
            let pulse = 0.5 + 0.5 * ((t / 1.2 - i as f32 * 0.15) * std::f32::consts::TAU).sin();
            let radius = self.dot_radius * (0.65 + 0.35 * pulse);
            let (cx, cy) = (aw as f32 * (i as f32 + 0.5) / 3.0, ah as f32 / 2.0);
            for y in 0..ah {
                for x in 0..aw {
                    let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                    let cover = (radius + 0.5 - d).clamp(0.0, 1.0) * (0.35 + 0.65 * pulse);
                    blend(&mut self.buf, self.rect.2 as usize, ax + x, ay + y, ACCENT, cover);
                }
            }
        }
    }
}

/// Blends a BGR colour over one pixel of a `width`-wide BGRA image.
fn blend(buf: &mut [u8], width: usize, x: usize, y: usize, color: [u8; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let i = (y * width + x) * 4;
    for (v, c) in buf[i..i + 3].iter_mut().zip(color) {
        *v = (*v as f32 + (c as f32 - *v as f32) * alpha.min(1.0)).round() as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TITLE_TR: &str = "Oyun ekranında değil";

    #[test]
    fn test_content_stays_inside_the_frame() {
        for (w, h) in [(640, 360), (1920, 1080), (3840, 2160), (1080, 1920), (3440, 1440), (320, 240)] {
            let mut card = FocusCard::new(w, h, TITLE_TR).unwrap();
            let (x, y, cw, ch) = card.content_rect();
            assert!(x + cw <= w && y + ch <= h, "{w}x{h}");
            assert_eq!([x % 2, y % 2, cw % 2, ch % 2], [0; 4]);
            assert_eq!(card.draw(1.0).len(), (cw * ch * 4) as usize);
        }
    }

    #[test]
    fn test_fades_in_and_moves() {
        let mut card = FocusCard::new(1280, 720, TITLE_TR).unwrap();
        assert!(card.draw(0.0).chunks_exact(4).all(|p| p == BACKGROUND));
        let half = card.draw(0.1).to_vec();
        let one = card.draw(1.0).to_vec();
        let later = card.draw(1.3).to_vec();
        assert!(half != one && one != later);
        assert!(later.chunks_exact(4).all(|p| p[3] == 255));
        // Leaving again starts over from the background.
        assert!(card.draw(0.0).chunks_exact(4).all(|p| p == BACKGROUND));
        assert_eq!(card.draw(1.0), &one[..]);
    }

    #[test]
    fn test_title_is_drawn_with_turkish_letters() {
        let with = FocusCard::new(1280, 720, "ığşİĞŞçö").unwrap();
        assert!(with.base.chunks_exact(4).any(|p| p[..3] == TITLE));
        let fonts = eframe::egui::FontDefinitions::default();
        let font = FontRef::try_from_slice(&fonts.font_data["Ubuntu-Light"].font).unwrap();
        assert!("ığşİĞŞçöüÜ".chars().all(|c| font.glyph_id(c).0 != 0));
    }

    /// Writes previews: cargo test focus_card -- --ignored
    #[test]
    #[ignore]
    fn write_previews() {
        let dir = std::env::temp_dir().join("lapse_card");
        std::fs::create_dir_all(&dir).unwrap();
        let mut card = FocusCard::new(1920, 1080, TITLE_TR).unwrap();
        let (x, y, w, _) = card.content_rect();
        for i in 0..72 {
            let mut img = RgbaImage::from_pixel(1920, 1080, image::Rgba([0x14, 0x12, 0x18, 255]));
            for (n, p) in card.draw(i as f32 / 30.0).chunks_exact(4).enumerate() {
                img.put_pixel(x + n as u32 % w, y + n as u32 / w, image::Rgba([p[2], p[1], p[0], p[3]]));
            }
            img.save(dir.join(format!("{i:03}.png"))).unwrap();
        }
    }
}
