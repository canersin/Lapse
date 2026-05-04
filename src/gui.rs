use eframe::egui::{self, RichText, Color32, Margin, Rounding, Vec2, Stroke};
use crate::config::Config;
use crate::ipc::{self};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Instant, Duration};
use std::path::PathBuf;
use rfd::FileDialog;
use crate::audio::{get_audio_devices, AudioDevice};
use crate::monitor::{get_monitors, MonitorDevice};
use crate::gpu::{get_gpus, GpuDevice};

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
    available_inputs: Vec<AudioDevice>,
    available_monitors: Vec<MonitorDevice>,
    available_gpus: Vec<GpuDevice>,
    active_tab: ActiveTab,
    cached_clips: Vec<ClipMetadata>,
    last_clip_refresh: Instant,
    last_status: Option<ipc::StatusResponse>,
    last_status_poll: Instant,
    binding_hotkey: bool, 
}

impl LapseApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.window_rounding = Rounding::same(12.0);
        visuals.panel_fill = Color32::from_rgb(25, 25, 25);
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(40, 40, 40);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(50, 50, 50);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(70, 70, 70);
        visuals.widgets.active.bg_fill = Color32::from_rgb(255, 209, 102);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::BLACK);
        cc.egui_ctx.set_visuals(visuals);

        let (outputs, inputs) = get_audio_devices();
        let monitors = get_monitors();
        let gpus = get_gpus();

        Self { 
            config, 
            available_outputs: outputs,
            available_inputs: inputs,
            available_monitors: monitors,
            available_gpus: gpus,
            active_tab: ActiveTab::Library,
            cached_clips: Vec::new(),
            last_clip_refresh: Instant::now().checked_sub(Duration::from_secs(10)).unwrap(),
            last_status: None,
            last_status_poll: Instant::now().checked_sub(Duration::from_secs(10)).unwrap(),
            binding_hotkey: false,
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

    fn format_hotkey(&self) -> String {
        let mut s = String::new();
        let m = self.config.hotkey_replay_mod;
        if m & (1 << 1) != 0 { s.push_str("Ctrl + "); }
        if m & (1 << 0) != 0 { s.push_str("Alt + "); }
        if m & (1 << 2) != 0 { s.push_str("Shift + "); }
        if m & (1 << 3) != 0 { s.push_str("Super + "); }
        s.push_str(&format!("{:?}", self.config.hotkey_replay).replace("Key", ""));
        s
    }
}

impl eframe::App for LapseApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.binding_hotkey {
            ctx.input(|i| {
                for event in &i.events {
                    if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                        self.config.hotkey_replay = map_egui_to_code(*key);
                        let mut mod_bits = 0u32;
                        if modifiers.alt { mod_bits |= 1 << 0; }
                        if modifiers.ctrl { mod_bits |= 1 << 1; }
                        if modifiers.shift { mod_bits |= 1 << 2; }
                        if modifiers.mac_cmd || modifiers.command { mod_bits |= 1 << 3; }
                        self.config.hotkey_replay_mod = mod_bits;
                        self.binding_hotkey = false;
                    }
                }
            });
        }

        if self.last_status_poll.elapsed().as_millis() > 500 {
            self.last_status_poll = Instant::now();
            if let Some(ipc::Response::Status(s)) = self.send_command(ipc::Command::GetStatus) {
                self.last_status = Some(s);
            }
        }
        ctx.request_repaint_after(Duration::from_millis(500));

        let is_installed = self.last_status.as_ref().map(|s| s.is_installed).unwrap_or(true);
        let recording = self.last_status.as_ref().map(|s| s.recording).unwrap_or(false);
        

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
                        let is_active = self.active_tab == tab;
                        let text_color = if is_active { Color32::BLACK } else { Color32::LIGHT_GRAY };
                        let bg_color = if is_active { Color32::from_rgb(255, 209, 102) } else { Color32::TRANSPARENT };
                        let btn = egui::Button::new(RichText::new(text).size(16.0).strong().color(text_color))
                            .fill(bg_color)
                            .rounding(8.0)
                            .min_size(Vec2::new(ui.available_width(), 40.0));
                        if ui.add(btn).clicked() {
                            self.active_tab = tab;
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
                                let _ = self.send_command(ipc::Command::StartReplay);
                            }
                        } else {
                            if ui.add_sized(Vec2::new(160.0, 40.0), egui::Button::new(RichText::new("📸 Save Clip").size(14.0).strong().color(Color32::WHITE)).fill(Color32::from_rgb(120, 80, 255)).rounding(8.0)).clicked() {
                                let _ = self.send_command(ipc::Command::SaveReplay);
                            }
                            ui.add_space(10.0);
                            if ui.add_sized(Vec2::new(160.0, 40.0), egui::Button::new(RichText::new("⏹ Stop Engine").size(14.0).strong().color(Color32::WHITE)).fill(Color32::from_rgb(200, 50, 50)).rounding(8.0)).clicked() {
                                let _ = self.send_command(ipc::Command::Stop);
                            }
                        }
                    }
                });
            });


        egui::CentralPanel::default().show(ctx, |ui| {
            match self.active_tab {
                ActiveTab::Library => {
                    ui.heading(RichText::new("Your Clips").size(24.0).strong().color(Color32::WHITE));
                    ui.add_space(10.0);
                    self.refresh_clips();
                    
                    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                        let item_width = 240.0;
                        let spacing = 15.0;
                        let available_width = ui.available_width();
                        let mut columns = ((available_width + spacing) / (item_width + spacing)).floor() as usize;
                        if columns == 0 { columns = 1; }

                        let mut to_delete = None;

                        egui::Grid::new("clips_grid").num_columns(columns).spacing([spacing, spacing]).min_col_width(item_width).max_col_width(item_width).show(ui, |ui| {
                            for (i, clip) in self.cached_clips.iter().enumerate() {
                                if i > 0 && i % columns == 0 {
                                    ui.end_row();
                                }
                                
                                egui::Frame::none().fill(Color32::from_rgb(35, 35, 35)).rounding(12.0).inner_margin(Margin::same(12.0)).show(ui, |ui| {
                                    ui.set_width(item_width);
                                    ui.vertical(|ui| {
                                        if clip.thumb_path.exists() {
                                            let response = ui.add(egui::Image::new(format!("file://{}", clip.thumb_path.display())).fit_to_exact_size(Vec2::new(item_width, item_width * 9.0 / 16.0)).rounding(8.0).sense(egui::Sense::click()));
                                            if response.clicked() {
                                                let _ = open::that(&clip.path);
                                            }
                                            if response.hovered() {
                                                response.on_hover_cursor(egui::CursorIcon::PointingHand);
                                            }
                                        } else {
                                            ui.allocate_space(Vec2::new(item_width, item_width * 9.0 / 16.0));
                                        }
                                        ui.add_space(10.0);

                                        ui.label(RichText::new(&clip.name).size(14.0).strong().color(Color32::WHITE));
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(format!("{:.1} MB", clip.size_mb)).color(Color32::GRAY));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.button(RichText::new("🗑").color(Color32::RED)).clicked() {
                                                    to_delete = Some(clip.clone());
                                                }
                                            });
                                        });
                                    });
                                });
                            }
                        });

                        if let Some(target) = to_delete {
                            let _ = std::fs::remove_file(&target.path);
                            let _ = std::fs::remove_file(&target.thumb_path);
                            self.last_clip_refresh = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
                        }
                    });
                }
                ActiveTab::Settings => {
                    ui.heading(RichText::new("Settings").size(24.0).strong().color(Color32::WHITE));
                    ui.add_space(20.0);

                    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                        egui::Grid::new("sets").num_columns(2).spacing([40.0, 30.0]).show(ui, |ui| {
                            ui.label(RichText::new("Save Folder").size(16.0));
                            ui.horizontal(|ui| {
                                ui.label(self.config.save_path.to_string_lossy().to_string());
                                if ui.button("Browse").clicked() { if let Some(path) = FileDialog::new().pick_folder() { self.config.save_path = path; } }
                            });
                            ui.end_row();

                            ui.label(RichText::new("Monitor").size(16.0));
                            egui::ComboBox::from_id_source("mon").selected_text(&self.config.monitor).width(300.0).show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.config.monitor, "screen".into(), "All Screens");
                                for m in &self.available_monitors {
                                    ui.selectable_value(&mut self.config.monitor, m.name.clone(), format!("{} ({})", m.name, m.resolution));
                                }
                            });
                            ui.end_row();

                            ui.label(RichText::new("Encoding GPU").size(16.0));
                            egui::ComboBox::from_id_source("gpu").selected_text(&self.config.gpu).width(300.0).show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.config.gpu, "Auto".into(), "Auto");
                                for g in &self.available_gpus {
                                    ui.selectable_value(&mut self.config.gpu, g.pci_id.clone(), g.name.clone());
                                }
                            });
                            ui.end_row();
                            
                            ui.label(RichText::new("Resolution").size(16.0));
                            egui::ComboBox::from_id_source("res").selected_text(&self.config.resolution).width(200.0).show_ui(ui, |ui| {
                                for r in &["Native", "3840x2160", "2560x1440", "1920x1080", "1280x720"] {
                                    ui.selectable_value(&mut self.config.resolution, r.to_string(), *r);
                                }
                            });
                            ui.end_row();

                            ui.label(RichText::new("Framerate (FPS)").size(16.0)); ui.add(egui::DragValue::new(&mut self.config.fps)); ui.end_row();
                            ui.label(RichText::new("Replay (Secs)").size(16.0)); ui.add(egui::DragValue::new(&mut self.config.replay_seconds)); ui.end_row();
                            
                            ui.label(RichText::new("Microphone (Input)").size(16.0));
                            egui::ComboBox::from_id_source("ainp").selected_text(&self.config.audio_input).width(300.0).show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.config.audio_input, "None".into(), "None");
                                for d in &self.available_inputs { ui.selectable_value(&mut self.config.audio_input, d.name.clone(), &d.description); }
                            });
                            ui.end_row();

                            ui.label(RichText::new("Speaker (Output)").size(16.0));
                            egui::ComboBox::from_id_source("aout").selected_text(&self.config.audio_output).width(300.0).show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.config.audio_output, "None".into(), "None");
                                for d in &self.available_outputs { ui.selectable_value(&mut self.config.audio_output, d.name.clone(), &d.description); }
                            });
                            ui.end_row();

                            ui.label(RichText::new("Save Hotkey").size(16.0));
                            let h_text = self.format_hotkey();
                            let btn_text = if self.binding_hotkey { "🔴 Press any key..." } else { &h_text };
                            if ui.add(egui::Button::new(RichText::new(btn_text).strong())).clicked() {
                                self.binding_hotkey = true;
                            }
                            ui.end_row();
                        });
                        
                        ui.add_space(40.0);
                        if ui.add_sized(Vec2::new(200.0, 45.0), egui::Button::new(RichText::new("💾 Save Settings & Restart").size(14.0).strong().color(Color32::BLACK)).fill(Color32::from_rgb(100, 255, 100)).rounding(8.0)).clicked() {
                            let _ = self.config.save();
                            let _ = self.send_command(ipc::Command::ReloadConfig);
                            let _ = self.send_command(ipc::Command::Stop);
                            std::thread::sleep(std::time::Duration::from_millis(200));
                            let _ = self.send_command(ipc::Command::StartReplay);
                        }
                    });
                }
            }
        });
    }
}

fn map_egui_to_code(key: egui::Key) -> global_hotkey::hotkey::Code {
    use egui::Key as K;
    use global_hotkey::hotkey::Code as C;

    macro_rules! map_keys {
        ($($egui_k:ident => $ghk_k:ident),* $(,)?) => {
            match key {
                $(K::$egui_k => C::$ghk_k,)*
                _ => C::F10,
            }
        };
    }

    map_keys! {
        F1=>F1, F2=>F2, F3=>F3, F4=>F4, F5=>F5, F6=>F6, F7=>F7, F8=>F8, F9=>F9, F10=>F10, F11=>F11, F12=>F12,
        A=>KeyA, B=>KeyB, C=>KeyC, D=>KeyD, E=>KeyE, F=>KeyF, G=>KeyG, H=>KeyH, I=>KeyI, J=>KeyJ, K=>KeyK, L=>KeyL, M=>KeyM,
        N=>KeyN, O=>KeyO, P=>KeyP, Q=>KeyQ, R=>KeyR, S=>KeyS, T=>KeyT, U=>KeyU, V=>KeyV, W=>KeyW, X=>KeyX, Y=>KeyY, Z=>KeyZ,
        Num0=>Digit0, Num1=>Digit1, Num2=>Digit2, Num3=>Digit3, Num4=>Digit4, Num5=>Digit5, Num6=>Digit6, Num7=>Digit7, Num8=>Digit8, Num9=>Digit9,
        Space=>Space, Enter=>Enter, Escape=>Escape, Backspace=>Backspace, Tab=>Tab, Insert=>Insert, Delete=>Delete,
        Home=>Home, End=>End, PageUp=>PageUp, PageDown=>PageDown,
        ArrowLeft=>ArrowLeft, ArrowRight=>ArrowRight, ArrowUp=>ArrowUp, ArrowDown=>ArrowDown,
    }
}
