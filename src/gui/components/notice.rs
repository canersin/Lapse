//! "Set this up first" dialog: something the user clicked needs a setting. Views call
//! `request_setup`; the app calls `render_setup_dialog` once per frame and opens the
//! settings page it returns.

use eframe::egui::{self, Color32, Id, Key, Margin, RichText, Stroke, Vec2};

use crate::gui::components::button::{m3_filled_button, m3_outlined_button};
use crate::gui::components::icon::{m3_vector_icon, IconKind};
use crate::gui::components::motion::{ease_out, MEDIUM};
use crate::gui::i18n::t;
use crate::gui::theme::{M3Shapes, MonetTheme};

#[derive(Clone)]
struct Setup {
    title: String,
    body: String,
    /// Settings tab that holds the setting.
    settings_tab: usize,
}

fn id() -> Id {
    Id::new("lapse_setup_notice")
}

pub fn request_setup(ctx: &egui::Context, title: &str, body: &str, settings_tab: usize) {
    let setup = Setup { title: title.into(), body: body.into(), settings_tab };
    ctx.data_mut(|d| d.insert_temp(id(), Some(setup)));
}

/// Returns the settings tab to open when the user chose "Open settings".
pub fn render_setup_dialog(ctx: &egui::Context, theme: &MonetTheme) -> Option<usize> {
    let setup: Setup = ctx.data(|d| d.get_temp::<Option<Setup>>(id())).flatten()?;
    let mut close = false;
    let mut open = false;

    let screen = ctx.screen_rect();
    let appear = ease_out(ctx.animate_bool_with_time(id().with(&setup.title), true, MEDIUM));

    egui::Area::new(id().with("scrim"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha((150.0 * appear) as u8));
            if resp.clicked() {
                close = true;
            }
        });

    let width = 400.0_f32.min(screen.width() - 32.0);
    let pos = egui::pos2(screen.center().x - width / 2.0, screen.center().y - 90.0 + 16.0 * (1.0 - appear));
    egui::Area::new(id().with("dialog"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(theme.surface_container_high)
                .stroke(Stroke::new(1.0_f32, theme.outline_variant))
                .rounding(M3Shapes::EXTRA_LARGE)
                .inner_margin(Margin::same(24.0))
                .show(ui, |ui| {
                    ui.set_width(width - 48.0);
                    ui.horizontal(|ui| {
                        m3_vector_icon(ui, IconKind::Gear, 22.0, theme.primary);
                        ui.label(RichText::new(&setup.title).size(18.0).strong().color(theme.on_surface));
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new(&setup.body).size(13.0).color(theme.on_surface_variant));
                    ui.add_space(18.0);
                    let row = Vec2::new(ui.available_width(), 40.0);
                    ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if m3_filled_button(ui, theme, t("Open settings")).clicked() {
                            open = true;
                        }
                        ui.add_space(8.0);
                        if m3_outlined_button(ui, theme, t("Close")).clicked() {
                            close = true;
                        }
                    });
                });
        });

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        close = true;
    }
    if ctx.input(|i| i.key_pressed(Key::Enter)) {
        open = true;
    }
    if close || open {
        ctx.data_mut(|d| d.insert_temp::<Option<Setup>>(id(), None));
    }
    open.then_some(setup.settings_tab)
}
