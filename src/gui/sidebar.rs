use eframe::egui::{self, RichText, Color32, Margin, Vec2};
use crate::ipc;
use super::app::{LapseApp, ActiveTab};

pub fn render(app: &mut LapseApp, ctx: &egui::Context) {
    let is_installed = app.last_status.as_ref().map(|s| s.is_installed).unwrap_or(true);
    let recording = app.last_status.as_ref().map(|s| s.recording).unwrap_or(false);

    egui::SidePanel::left("nav_panel")
        .frame(egui::Frame::default().fill(Color32::from_rgb(18, 18, 18)).inner_margin(Margin::same(10.0)))
        .exact_width(180.0)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.heading(RichText::new("LAPSE").size(32.0).color(Color32::from_rgb(255, 209, 102)).strong());
                ui.add_space(40.0);
            });

            ui.vertical(|ui| {
                let mut draw_nav_button = |text: &str, tab: ActiveTab| {
                    let is_active = app.active_tab == tab;
                    let text_color = if is_active { Color32::BLACK } else { Color32::LIGHT_GRAY };
                    let bg_color = if is_active { Color32::from_rgb(255, 209, 102) } else { Color32::TRANSPARENT };
                    let btn = egui::Button::new(RichText::new(text).size(16.0).strong().color(text_color))
                        .fill(bg_color)
                        .rounding(8.0)
                        .min_size(Vec2::new(ui.available_width(), 40.0));
                    if ui.add(btn).clicked() {
                        app.active_tab = tab;
                    }
                    ui.add_space(10.0);
                };

                draw_nav_button("🎬 Library", ActiveTab::Library);
                draw_nav_button("⚙ Settings", ActiveTab::Settings);
            });

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(20.0);
                if !is_installed {
                    ui.label(RichText::new("⚠️ Missing Dep").color(Color32::RED));
                } else {
                    if !recording {
                        if ui.add_sized(Vec2::new(160.0, 40.0), egui::Button::new(RichText::new("🔴 Start Engine").size(14.0).strong().color(Color32::BLACK)).fill(Color32::from_rgb(255, 209, 102)).rounding(8.0)).clicked() {
                            let _ = app.send_command(ipc::Command::StartReplay);
                        }
                    } else {
                        if ui.add_sized(Vec2::new(160.0, 40.0), egui::Button::new(RichText::new("📸 Save Clip").size(14.0).strong().color(Color32::WHITE)).fill(Color32::from_rgb(120, 80, 255)).rounding(8.0)).clicked() {
                            let _ = app.send_command(ipc::Command::SaveReplay);
                        }
                        ui.add_space(10.0);
                        if ui.add_sized(Vec2::new(160.0, 40.0), egui::Button::new(RichText::new("⏹ Stop Engine").size(14.0).strong().color(Color32::WHITE)).fill(Color32::from_rgb(200, 50, 50)).rounding(8.0)).clicked() {
                            let _ = app.send_command(ipc::Command::Stop);
                        }
                    }
                }
            });
        });
}
