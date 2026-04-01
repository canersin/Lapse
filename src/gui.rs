use eframe::egui::{self, RichText, Color32, Margin, Rounding, Vec2, Stroke};
use crate::config::Config;
use crate::recorder::{Recorder, RecordingMode};
use std::sync::{Arc, Mutex};
use std::time::{Instant, Duration};
use std::path::PathBuf;
use rfd::FileDialog;
use crate::audio::{get_audio_devices, AudioDevice};

#[derive(PartialEq, Clone, Copy)]
pub enum ActiveTab {
    Library,
    Settings,
}

#[derive(Clone)]
struct ClipMetadata {
    path: PathBuf,
    name: String,
    size_mb: f64,
    thumb_path: PathBuf,
}

pub struct LapseApp {
    config: Config,
    recorder: Arc<Mutex<Recorder>>,
    available_outputs: Vec<AudioDevice>,
    available_inputs: Vec<AudioDevice>,
    active_tab: ActiveTab,
    cached_clips: Vec<ClipMetadata>,
    last_clip_refresh: Instant,
    show_gui: Arc<Mutex<bool>>,
    icon_data: Arc<egui::IconData>,
}

impl LapseApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>, 
        config: Config, 
        recorder: Arc<Mutex<Recorder>>, 
        show_gui: Arc<Mutex<bool>>,
        icon_data: Arc<egui::IconData>
    ) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.window_rounding = Rounding::same(12.0);
        visuals.panel_fill = Color32::from_rgb(44, 44, 52); // Fresher, breathable dark gray
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(52, 52, 60);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(60, 60, 70);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(70, 70, 80);
        visuals.widgets.active.bg_fill = Color32::from_rgb(255, 209, 102); // Accent yellow
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::BLACK);
        cc.egui_ctx.set_visuals(visuals);

        let fonts = egui::FontDefinitions::default();
        cc.egui_ctx.set_fonts(fonts);

        let available_outputs = get_audio_devices(true);
        let available_inputs = get_audio_devices(false);

        Self { 
            config, 
            recorder, 
            available_outputs, 
            available_inputs, 
            active_tab: ActiveTab::Library,
            cached_clips: Vec::new(),
            last_clip_refresh: Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now),
            show_gui,
            icon_data,
        }
    }
    
    fn refresh_clips(&mut self) {
        if self.last_clip_refresh.elapsed().as_secs() < 3 {
            return; // Throttle disk IO
        }
        self.last_clip_refresh = Instant::now();
        self.cached_clips.clear();
        
        let thumb_dir = self.config.save_path.join(".thumbnails");
        if !thumb_dir.exists() {
            let _ = std::fs::create_dir_all(&thumb_dir);
        }
        
        if let Ok(entries) = std::fs::read_dir(&self.config.save_path) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        if let Some(ext) = entry.path().extension() {
                            if ext == "mp4" {
                                let name = entry.file_name().to_string_lossy().to_string();
                                let size_mb = meta.len() as f64 / 1_048_576.0;
                                let video_path = entry.path();
                                let thumb_path = thumb_dir.join(format!("{}.jpg", name));
                                
                                if !thumb_path.exists() {
                                    // Async thumbnail generation via ffmpeg
                                    let t_path = thumb_path.clone();
                                    let v_path = video_path.clone();
                                    std::thread::spawn(move || {
                                        let _ = std::process::Command::new("ffmpeg")
                                            .arg("-i").arg(&v_path).arg("-vframes").arg("1").arg("-s").arg("320x180").arg("-y").arg(&t_path).output();
                                    });
                                }
                                
                                self.cached_clips.push(ClipMetadata { path: entry.path(), name, size_mb, thumb_path });
                            }
                        }
                    }
                }
            }
        }
        
        self.cached_clips.sort_by(|a, b| b.name.cmp(&a.name));
    }
}

impl eframe::App for LapseApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let is_visible = *self.show_gui.lock().unwrap();

        if is_visible {
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("lapse_gui_window"),
                egui::ViewportBuilder::default()
                    .with_title("Lapse")
                    .with_icon(self.icon_data.clone())
                    .with_inner_size([500.0, 480.0])
                    .with_active(true),
                |ctx, _class| {
                    if ctx.input(|i| i.viewport().close_requested()) {
                        *self.show_gui.lock().unwrap() = false;
                        return;
                    }
                    
                    let mut is_installed = true;
                    let mut current_mode = RecordingMode::None;
                    if let Ok(rec) = self.recorder.lock() {
                        is_installed = rec.is_installed();
                        current_mode = rec.current_mode();
                    }
                    
                    egui::TopBottomPanel::top("nav_bar")
                        .frame(egui::Frame::default().fill(Color32::from_rgb(38, 38, 46)).inner_margin(Margin::same(15.0)))
                        .show(ctx, |ui| {
                            ui.horizontal(|ui| {
                                ui.heading(RichText::new("LAPSE")
                                    .size(24.0)
                                    .color(Color32::from_rgb(255, 209, 102))
                                    .strong());
                                ui.add_space(20.0);
                                
                                let lib_col = if self.active_tab == ActiveTab::Library { Color32::WHITE } else { Color32::GRAY };
                                if ui.add(egui::Button::new(RichText::new("🎬 Library").size(16.0).color(lib_col)).fill(Color32::TRANSPARENT)).clicked() {
                                    self.active_tab = ActiveTab::Library;
                                }
                                
                                ui.add_space(10.0);
                                
                                let set_col = if self.active_tab == ActiveTab::Settings { Color32::WHITE } else { Color32::GRAY };
                                if ui.add(egui::Button::new(RichText::new("⚙ Capture Settings").size(16.0).color(set_col)).fill(Color32::TRANSPARENT)).clicked() {
                                    self.active_tab = ActiveTab::Settings;
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if !is_installed {
                                        ui.label(RichText::new("⚠️ Hata: gpu-screen-recorder eksik").color(Color32::RED));
                                    } else {
                                        if current_mode == RecordingMode::None {
                                            let start_btn = egui::Button::new(RichText::new("🔴 Start Recording").size(14.0).strong())
                                                .fill(Color32::from_rgb(255, 209, 102)).rounding(6.0);
                                            if ui.add_sized(Vec2::new(140.0, 30.0), start_btn).clicked() {
                                                if let Ok(mut rec) = self.recorder.lock() { let _ = rec.start_replay(); }
                                            }
                                        } else {
                                            let stop_btn = egui::Button::new(RichText::new("⏹").size(14.0).strong().color(Color32::WHITE))
                                                .fill(Color32::from_rgb(200, 50, 50)).rounding(6.0);
                                            if ui.add_sized(Vec2::new(30.0, 30.0), stop_btn).clicked() {
                                                if let Ok(mut rec) = self.recorder.lock() { let _ = rec.stop(); }
                                            }

                                            ui.add_space(5.0);

                                            let save_btn = egui::Button::new(RichText::new("📸 Save Clip").size(14.0).strong().color(Color32::WHITE))
                                                .fill(Color32::from_rgb(120, 80, 255)).rounding(6.0);
                                            if ui.add_sized(Vec2::new(140.0, 30.0), save_btn).clicked() {
                                                if let Ok(rec) = self.recorder.lock() { let _ = rec.save_replay(); }
                                            }

                                            ui.add_space(10.0);
                                            ui.label(RichText::new("🔴 ACTIVE").color(Color32::from_rgb(100, 255, 100)).strong());
                                        }
                                    }
                                });
                            });
                    });

                    egui::CentralPanel::default()
                        .frame(egui::Frame::default().fill(ctx.style().visuals.panel_fill).inner_margin(Margin::same(24.0)))
                        .show(ctx, |ui| {
                            match self.active_tab {
                                ActiveTab::Library => {
                                    self.refresh_clips();
                                    if self.cached_clips.is_empty() {
                                        ui.vertical_centered(|ui| {
                                            ui.add_space(80.0);
                                            ui.label(RichText::new("No captures found.").size(22.0).color(Color32::GRAY));
                                            ui.label(RichText::new("Start recording from the top bar!").size(16.0).color(Color32::DARK_GRAY));
                                        });
                                    } else {
                                        egui::ScrollArea::vertical().show(ui, |ui| {
                                            ui.horizontal_wrapped(|ui| {
                                                let mut clip_to_delete = None;
                                                for clip in &self.cached_clips {
                                                    egui::Frame::group(ui.style())
                                                        .fill(Color32::from_rgb(52, 52, 60))
                                                        .rounding(8.0)
                                                        .inner_margin(Margin::same(10.0))
                                                        .show(ui, |ui| {
                                                            ui.set_width(180.0);
                                                            ui.vertical_centered(|ui| {
                                                                if clip.thumb_path.exists() {
                                                                    ui.add(egui::Image::new(format!("file://{}", clip.thumb_path.display()))
                                                                        .fit_to_exact_size(Vec2::new(160.0, 90.0))
                                                                        .rounding(6.0));
                                                                } else {
                                                                    ui.label(RichText::new("🎬").size(48.0));
                                                                }
                                                                ui.add_space(8.0);
                                                                let display_name = if clip.name.len() > 20 {
                                                                    format!("{}...", &clip.name[..17])
                                                                } else {
                                                                    clip.name.clone()
                                                                };
                                                                ui.label(RichText::new(display_name).size(14.0).strong());
                                                                ui.label(RichText::new(format!("{:.1} MB", clip.size_mb)).color(Color32::LIGHT_GRAY));
                                                                ui.add_space(5.0);
                                                                ui.horizontal(|ui| {
                                                                    if ui.button("▶ Play Clip").clicked() {
                                                                        let _ = open::that(&clip.path);
                                                                    }
                                                                    if ui.button("🗑 Delete").clicked() {
                                                                        clip_to_delete = Some(clip.clone());
                                                                    }
                                                                });
                                                            });
                                                        });
                                                }
                                                if let Some(target) = clip_to_delete {
                                                    let _ = std::fs::remove_file(&target.path);
                                                    let _ = std::fs::remove_file(&target.thumb_path);
                                                    self.last_clip_refresh = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
                                                }
                                            });
                                        });
                                    }
                                }
                                ActiveTab::Settings => {
                                    if !is_installed {
                                        egui::Frame::group(ui.style())
                                            .fill(Color32::from_rgb(180, 80, 80))
                                            .rounding(10.0)
                                            .inner_margin(Margin::same(15.0))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.heading(RichText::new("⚠️ gpu-screen-recorder not found!").size(18.0).color(Color32::WHITE));
                                                ui.add_space(8.0);
                                                ui.horizontal(|ui| { ui.label("Arch: "); ui.code("sudo pacman -S gpu-screen-recorder"); });
                                                ui.horizontal(|ui| { ui.label("Flatpak: "); ui.code("flatpak install flathub com.dec05eba.gpu_screen_recorder"); });
                                            });
                                        ui.add_space(20.0);
                                    }

                                    egui::Frame::none()
                                        .fill(Color32::from_rgb(52, 52, 60))
                                        .rounding(12.0)
                                        .inner_margin(Margin::same(20.0))
                                        .show(ui, |ui| {
                                            ui.heading(RichText::new("Preferences").size(20.0).color(Color32::WHITE));
                                            ui.add_space(15.0);
                                            
                                            egui::ScrollArea::vertical().show(ui, |ui| {
                                                egui::Grid::new("settings_grid").num_columns(2).spacing([30.0, 20.0]).show(ui, |ui| {
                                                    
                                                    ui.label(RichText::new("Save Folder:").size(14.0));
                                                    ui.horizontal(|ui| {
                                                        ui.label(RichText::new(self.config.save_path.to_string_lossy().to_string()).color(Color32::GRAY));
                                                        if ui.button("Browse").clicked() {
                                                            if let Some(path) = FileDialog::new().pick_folder() {
                                                                self.config.save_path = path;
                                                            }
                                                        }
                                                    });
                                                    ui.end_row();
                                                    
                                                    ui.label(RichText::new("Resolution:").size(14.0));
                                                    egui::ComboBox::from_id_source("res_combo")
                                                        .selected_text(&self.config.resolution)
                                                        .show_ui(ui, |ui| {
                                                            ui.selectable_value(&mut self.config.resolution, "Native".to_string(), "Native");
                                                            ui.selectable_value(&mut self.config.resolution, "3840x2160".to_string(), "4K (3840x2160)");
                                                            ui.selectable_value(&mut self.config.resolution, "2560x1440".to_string(), "1440p (2560x1440)");
                                                            ui.selectable_value(&mut self.config.resolution, "1920x1080".to_string(), "1080p (1920x1080)");
                                                            ui.selectable_value(&mut self.config.resolution, "1280x720".to_string(), "720p (1280x720)");
                                                        });
                                                    ui.end_row();

                                                    ui.label(RichText::new("Framerate (FPS):").size(14.0));
                                                    ui.add(egui::DragValue::new(&mut self.config.fps).speed(1));
                                                    ui.end_row();

                                                    ui.label(RichText::new("Replay Buffer:").size(14.0));
                                                    ui.add(egui::DragValue::new(&mut self.config.replay_seconds).speed(1).suffix(" Seconds"));
                                                    ui.end_row();
                                                    
                                                    ui.label(RichText::new("Audio Output:").size(14.0));
                                                    egui::ComboBox::from_id_source("audio_out_combo")
                                                        .selected_text(get_device_desc(&self.config.audio_output, &self.available_outputs, "Speakers"))
                                                        .show_ui(ui, |ui| {
                                                            ui.selectable_value(&mut self.config.audio_output, "None".to_string(), "None");
                                                            ui.selectable_value(&mut self.config.audio_output, "default_output".to_string(), "Default Output");
                                                            for dev in &self.available_outputs {
                                                                ui.selectable_value(&mut self.config.audio_output, dev.name.clone(), &dev.description);
                                                            }
                                                        });
                                                    ui.end_row();

                                                    ui.label(RichText::new("Audio Input:").size(14.0));
                                                    egui::ComboBox::from_id_source("audio_in_combo")
                                                        .selected_text(get_device_desc(&self.config.audio_input, &self.available_inputs, "Microphone"))
                                                        .show_ui(ui, |ui| {
                                                            ui.selectable_value(&mut self.config.audio_input, "None".to_string(), "None");
                                                            ui.selectable_value(&mut self.config.audio_input, "default_input".to_string(), "Default Input");
                                                            for dev in &self.available_inputs {
                                                                ui.selectable_value(&mut self.config.audio_input, dev.name.clone(), &dev.description);
                                                            }
                                                        });
                                                    ui.end_row();

                                                    ui.label(RichText::new("Save Hotkey:").size(14.0));
                                                    ui.text_edit_singleline(&mut self.config.hotkey_replay);
                                                    ui.end_row();
                                                });
                                                
                                                ui.add_space(20.0);
                                                if ui.add_sized([120.0, 35.0], egui::Button::new(RichText::new("Apply Settings").size(14.0).strong())).clicked() {
                                                    let _ = self.config.save();
                                                    // Auto restart logic so resolution changes take effect if recording
                                                    if current_mode != RecordingMode::None {
                                                        if let Ok(mut rec) = self.recorder.lock() { 
                                                            let _ = rec.stop();
                                                            let _ = rec.start_replay();
                                                        }
                                                    }
                                                }
                                            });
                                        });
                                }
                            }
                    });
                }
            );
        } else {
            // Invisible sleep layout, no window gets rendered, eframe effectively idles
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

fn get_device_desc<'a>(name: &'a str, devices: &'a [AudioDevice], default_label: &str) -> String {
    if name == "None" {
        "None".to_string()
    } else if name == "default_output" || name == "default_input" {
        format!("Default ({})", default_label)
    } else {
        devices.iter().find(|d| d.name == name).map(|d| d.description.clone()).unwrap_or_else(|| name.to_string())
    }
}
