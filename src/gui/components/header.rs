use eframe::egui::{self, Margin};
use crate::gui::theme::{M3Typography, MonetTheme};

#[allow(dead_code)]

pub fn m3_page_header<R>(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    title: &str,
    subtitle: &str,
    actions: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::none()
        .inner_margin(Margin {
            left: 4.0,
            right: 4.0,
            top: 6.0,
            bottom: 16.0,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(M3Typography::headline_large(title, theme.on_surface));
                    ui.add_space(2.0);
                    ui.label(M3Typography::body_medium(subtitle, theme.on_surface_variant));
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    actions(ui)
                })
                .inner
            })
            .inner
        })
}
