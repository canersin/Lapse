use eframe::egui::{self, RichText, Color32, Margin};
use crate::config::Config;
use crate::recorder::Recorder;
use std::sync::{Arc, Mutex};
use rfd::FileDialog;

pub struct LapseApp {
    config: Config,
    #[allow(dead_code)]
    recorder: Arc<Mutex<Recorder>>,
}

impl LapseApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config, recorder: Arc<Mutex<Recorder>>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.window_rounding = 12.0.into();
        visuals.panel_fill = Color32::from_rgb(25, 25, 30); // Dark sleek background
        cc.egui_ctx.set_visuals(visuals);

        let fonts = egui::FontDefinitions::default();
        // Here we could load custom fonts, but default is fine if scaled up
        cc.egui_ctx.set_fonts(fonts);

        Self { config, recorder }
    }
}

impl eframe::App for LapseApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(ctx.style().visuals.panel_fill).inner_margin(Margin::same(20.0)))
            .show(ctx, |ui| {
            
            ui.vertical_centered(|ui| {
                ui.heading(RichText::new("Lapse").size(32.0).color(Color32::from_rgb(180, 150, 255)).strong());
                ui.label(RichText::new("High-Performance Game Clipper").size(14.0).color(Color32::GRAY));
            });
            
            ui.add_space(20.0);
            ui.separator();
            ui.add_space(20.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Status:").size(16.0).strong());
                ui.label(RichText::new("Active (Replay Mode)").size(16.0).color(Color32::GREEN));
            });

            ui.add_space(20.0);

            egui::Frame::group(ui.style())
                .fill(Color32::from_rgb(35, 35, 40))
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add_space(10.0);
                    ui.heading(RichText::new("Configuration").size(20.0).color(Color32::WHITE));
                    ui.add_space(15.0);

                    egui::Grid::new("settings_grid").num_columns(2).spacing([20.0, 15.0]).show(ui, |ui| {
                        // Save Folder
                        ui.label(RichText::new("Save Folder:").size(14.0));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(self.config.save_path.to_string_lossy().to_string()).color(Color32::LIGHT_GRAY));
                            if ui.button("Browse...").clicked() {
                                if let Some(path) = FileDialog::new().pick_folder() {
                                    self.config.save_path = path;
                                }
                            }
                        });
                        ui.end_row();

                        // Replay Hotkey
                        ui.label(RichText::new("Replay Hotkey:").size(14.0));
                        ui.text_edit_singleline(&mut self.config.hotkey_replay);
                        ui.end_row();

                        // Record Hotkey
                        ui.label(RichText::new("Record Hotkey:").size(14.0));
                        ui.text_edit_singleline(&mut self.config.hotkey_record);
                        ui.end_row();

                        // Audio Source
                        ui.label(RichText::new("Audio Source:").size(14.0));
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_source("audio_combo")
                                .selected_text(match self.config.audio_source.as_str() {
                                    "None" => "None",
                                    "default_output" => "Default Output (Speakers)",
                                    "default_input" => "Default Input (Microphone)",
                                    "default_output|default_input" => "Speakers + Mic",
                                    _ => &self.config.audio_source,
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.config.audio_source, "None".to_string(), "None");
                                    ui.selectable_value(&mut self.config.audio_source, "default_output".to_string(), "Default Output (Speakers)");
                                    ui.selectable_value(&mut self.config.audio_source, "default_input".to_string(), "Default Input (Microphone)");
                                    ui.selectable_value(&mut self.config.audio_source, "default_output|default_input".to_string(), "Speakers + Mic");
                                });
                        });
                        ui.end_row();

                        // Replay Seconds
                        ui.label(RichText::new("Replay Buffer:").size(14.0));
                        ui.add(egui::Slider::new(&mut self.config.replay_seconds, 15..=300).text("seconds"));
                        ui.end_row();
                    });
                    ui.add_space(10.0);
            });

            ui.add_space(30.0);

            ui.vertical_centered(|ui| {
                if ui.add_sized([150.0, 40.0], egui::Button::new(RichText::new("Save Settings").size(16.0))).clicked() {
                    let _ = self.config.save();
                }
                
                ui.add_space(15.0);
                ui.label(RichText::new(format!("Press {} to save a replay.", self.config.hotkey_replay)).color(Color32::GRAY));
            });
        });
    }
}
