use eframe::egui::{self, RichText, Color32, Margin, Rounding, Vec2, Stroke};
use crate::config::Config;
use crate::ipc::{self};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
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
    available_outputs: Vec<AudioDevice>,
    active_tab: ActiveTab,
    cached_clips: Vec<ClipMetadata>,
    last_clip_refresh: Instant,
    last_status: Option<ipc::StatusResponse>,
    last_status_poll: Instant,
}

impl LapseApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>, 
        config: Config, 
    ) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.window_rounding = Rounding::same(12.0);
        visuals.panel_fill = Color32::from_rgb(44, 44, 52);
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(52, 52, 60);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(60, 60, 70);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(70, 70, 80);
        visuals.widgets.active.bg_fill = Color32::from_rgb(255, 209, 102);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::BLACK);
        cc.egui_ctx.set_visuals(visuals);

        Self { 
            config, 
            available_outputs: get_audio_devices(true), 
            active_tab: ActiveTab::Library,
            cached_clips: Vec::new(),
            last_clip_refresh: Instant::now().checked_sub(Duration::from_secs(10)).unwrap(),
            last_status: None,
            last_status_poll: Instant::now().checked_sub(Duration::from_secs(10)).unwrap(),
        }
    }
    
    fn send_command(&self, cmd: ipc::Command) -> Option<ipc::Response> {
        if let Ok(mut stream) = UnixStream::connect("/tmp/lapse.sock") {
            let _ = stream.write_all(serde_json::to_string(&cmd).unwrap().as_bytes());
            let mut buffer = [0; 1024];
            if let Ok(n) = stream.read(&mut buffer) {
                return serde_json::from_slice(&buffer[..n]).ok();
            }
        }
        None
    }

    fn refresh_clips(&mut self) {
        if self.last_clip_refresh.elapsed().as_secs() < 2 { return; }
        self.last_clip_refresh = Instant::now();
        self.cached_clips.clear();
        let thumb_dir = self.config.save_path.join(".thumbnails");
        let _ = std::fs::create_dir_all(&thumb_dir);
        if let Ok(entries) = std::fs::read_dir(&self.config.save_path) {
            for entry in entries.flatten() {
                if let Some(ext) = entry.path().extension() {
                    if ext == "mp4" {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let size_mb = entry.metadata().unwrap().len() as f64 / 1_048_576.0;
                        let thumb_path = thumb_dir.join(format!("{}.jpg", name));
                        if !thumb_path.exists() {
                            let (v_path, t_path) = (entry.path(), thumb_path.clone());
                            std::thread::spawn(move || {
                                let _ = std::process::Command::new("ffmpeg").arg("-i").arg(&v_path).arg("-vframes").arg("1").arg("-s").arg("320x180").arg("-y").arg(&t_path).output();
                            });
                        }
                        self.cached_clips.push(ClipMetadata { path: entry.path(), name, size_mb, thumb_path });
                    }
                }
            }
        }
        self.cached_clips.sort_by(|a, b| b.name.cmp(&a.name));
    }
}

impl eframe::App for LapseApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll status from Daemon
        if self.last_status_poll.elapsed().as_millis() > 500 {
            self.last_status_poll = Instant::now();
            if let Some(ipc::Response::Status(s)) = self.send_command(ipc::Command::GetStatus) {
                self.last_status = Some(s);
            }
        }
        ctx.request_repaint_after(Duration::from_millis(500));

        let is_installed = self.last_status.as_ref().map(|s| s.is_installed).unwrap_or(true);
        let recording = self.last_status.as_ref().map(|s| s.recording).unwrap_or(false);
        
        egui::TopBottomPanel::top("nav_bar")
            .frame(egui::Frame::default().fill(Color32::from_rgb(38, 38, 46)).inner_margin(Margin::same(15.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("LAPSE").size(24.0).color(Color32::from_rgb(255, 209, 102)).strong());
                    ui.add_space(20.0);
                    if ui.add(egui::Button::new(RichText::new("🎬 Library").size(16.0).color(if self.active_tab == ActiveTab::Library { Color32::WHITE } else { Color32::GRAY })).fill(Color32::TRANSPARENT)).clicked() {
                        self.active_tab = ActiveTab::Library;
                    }
                    ui.add_space(10.0);
                    if ui.add(egui::Button::new(RichText::new("⚙ Capture Settings").size(16.0).color(if self.active_tab == ActiveTab::Settings { Color32::WHITE } else { Color32::GRAY })).fill(Color32::TRANSPARENT)).clicked() {
                        self.active_tab = ActiveTab::Settings;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !is_installed {
                            ui.label(RichText::new("⚠️ Hata: gpu-screen-recorder eksik").color(Color32::RED));
                        } else {
                            if !recording {
                                if ui.add_sized(Vec2::new(140.0, 30.0), egui::Button::new(RichText::new("🔴 Start Recording").size(14.0).strong()).fill(Color32::from_rgb(255, 209, 102)).rounding(6.0)).clicked() {
                                    let _ = self.send_command(ipc::Command::StartReplay);
                                }
                            } else {
                                if ui.add_sized(Vec2::new(30.0, 30.0), egui::Button::new(RichText::new("⏹").size(14.0).strong().color(Color32::WHITE)).fill(Color32::from_rgb(200, 50, 50)).rounding(6.0)).clicked() {
                                    let _ = self.send_command(ipc::Command::Stop);
                                }
                                ui.add_space(5.0);
                                if ui.add_sized(Vec2::new(140.0, 30.0), egui::Button::new(RichText::new("📸 Save Clip").size(14.0).strong().color(Color32::WHITE)).fill(Color32::from_rgb(120, 80, 255)).rounding(6.0)).clicked() {
                                    let _ = self.send_command(ipc::Command::SaveReplay);
                                }
                                ui.add_space(10.0);
                                ui.label(RichText::new("🔴 ACTIVE").color(Color32::from_rgb(100, 255, 100)).strong());
                            }
                        }
                    });
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.active_tab {
                ActiveTab::Library => {
                    self.refresh_clips();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let mut to_delete = None;
                            for clip in &self.cached_clips {
                                egui::Frame::group(ui.style()).fill(Color32::from_rgb(52, 52, 60)).rounding(8.0).inner_margin(Margin::same(10.0)).show(ui, |ui| {
                                    ui.set_width(180.0);
                                    ui.vertical_centered(|ui| {
                                        if clip.thumb_path.exists() {
                                            ui.add(egui::Image::new(format!("file://{}", clip.thumb_path.display())).fit_to_exact_size(Vec2::new(160.0, 90.0)).rounding(6.0));
                                        }
                                        ui.add_space(8.0);
                                        ui.label(RichText::new(&clip.name).size(14.0).strong());
                                        ui.label(RichText::new(format!("{:.1} MB", clip.size_mb)).color(Color32::LIGHT_GRAY));
                                        ui.horizontal(|ui| {
                                            if ui.button("▶ Play").clicked() { let _ = open::that(&clip.path); }
                                            if ui.button("🗑 Del").clicked() { to_delete = Some(clip.clone()); }
                                        });
                                    });
                                });
                            }
                            if let Some(target) = to_delete {
                                let _ = std::fs::remove_file(&target.path);
                                let _ = std::fs::remove_file(&target.thumb_path);
                                self.last_clip_refresh = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
                            }
                        });
                    });
                }
                ActiveTab::Settings => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        egui::Grid::new("sets").num_columns(2).spacing([30.0, 20.0]).show(ui, |ui| {
                            ui.label("Save Folder:");
                            ui.horizontal(|ui| {
                                ui.label(self.config.save_path.to_string_lossy().to_string());
                                if ui.button("Browse").clicked() { if let Some(path) = FileDialog::new().pick_folder() { self.config.save_path = path; } }
                            });
                            ui.end_row();
                            
                            ui.label("Resolution:");
                            egui::ComboBox::from_id_source("res").selected_text(&self.config.resolution).show_ui(ui, |ui| {
                                for r in &["Native", "3840x2160", "2560x1440", "1920x1080", "1280x720"] {
                                    ui.selectable_value(&mut self.config.resolution, r.to_string(), *r);
                                }
                            });
                            ui.end_row();

                            ui.label("Framerate (FPS):"); ui.add(egui::DragValue::new(&mut self.config.fps)); ui.end_row();
                            ui.label("Replay (Secs):"); ui.add(egui::DragValue::new(&mut self.config.replay_seconds)); ui.end_row();
                            
                            ui.label("Audio Output:");
                            egui::ComboBox::from_id_source("aout").selected_text(&self.config.audio_output).show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.config.audio_output, "default_output".into(), "Default");
                                for d in &self.available_outputs { ui.selectable_value(&mut self.config.audio_output, d.name.clone(), &d.description); }
                            });
                            ui.end_row();

                            ui.label("Save Hotkey:"); ui.text_edit_singleline(&mut self.config.hotkey_replay); ui.end_row();
                        });
                        
                        if ui.button("Apply Settings").clicked() {
                            let _ = self.config.save();
                            let _ = self.send_command(ipc::Command::Stop);
                            let _ = self.send_command(ipc::Command::StartReplay);
                        }
                    });
                }
            }
        });
    }
}


