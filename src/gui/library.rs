use eframe::egui::{self, RichText, Color32, Margin, Vec2};
use std::time::{Instant, Duration};
use std::path::PathBuf;
use super::app::LapseApp;

#[derive(Clone)]
pub struct ClipMetadata {
    pub path: PathBuf,
    pub name: String,
    pub size_mb: f64,
    pub thumb_path: PathBuf,
}

pub fn render(app: &mut LapseApp, ui: &mut egui::Ui) {
    ui.heading(RichText::new("Your Clips").size(24.0).strong().color(Color32::WHITE));
    ui.add_space(10.0);
    refresh_clips(app);
    
    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        let item_width = 240.0;
        let spacing = 15.0;
        let available_width = ui.available_width();
        let mut columns = ((available_width + spacing) / (item_width + spacing)).floor() as usize;
        if columns == 0 { columns = 1; }

        let mut to_delete = None;

        egui::Grid::new("clips_grid").num_columns(columns).spacing([spacing, spacing]).min_col_width(item_width).max_col_width(item_width).show(ui, |ui| {
            for (i, clip) in app.cached_clips.iter().enumerate() {
                if i > 0 && i % columns == 0 {
                    ui.end_row();
                }
                
                egui::Frame::none().fill(Color32::from_rgb(35, 35, 35)).rounding(12.0).inner_margin(Margin::same(12.0)).show(ui, |ui| {
                    ui.set_width(item_width);
                    ui.vertical(|ui| {
                        if clip.thumb_path.exists() {
                            let response = ui.add(egui::Image::new(format!("file://{}", clip.thumb_path.display())).fit_to_exact_size(Vec2::new(item_width, item_width * 9.0 / 16.0)).rounding(8.0).sense(egui::Sense::click()));
                            if response.clicked() {
                                app.player_state = crate::gui::player::PlayerState::new(clip.path.clone());
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
            app.last_clip_refresh = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
        }
    });
}

fn refresh_clips(app: &mut LapseApp) {
    if app.last_clip_refresh.elapsed().as_secs() < 2 { return; }
    app.last_clip_refresh = Instant::now();
    app.cached_clips.clear();
    let thumb_dir = app.config.save_path.join(".thumbnails");
    let _ = std::fs::create_dir_all(&thumb_dir);
    if let Ok(entries) = std::fs::read_dir(&app.config.save_path) {
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
                    app.cached_clips.push(ClipMetadata { path: entry.path(), name, size_mb, thumb_path });
                }
            }
        }
    }
    app.cached_clips.sort_by(|a, b| b.name.cmp(&a.name));
}
