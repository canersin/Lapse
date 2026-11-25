use eframe::egui::{self, RichText, Vec2};
use crate::gui::i18n::t;
use crate::gui::components::button::m3_small_button_colored;
use crate::gui::components::button::m3_outlined_button;
use crate::gui::components::card::m3_card;
use crate::gui::components::chip::{m3_badge, m3_keycap};
use crate::gui::components::header::m3_page_header;
use crate::gui::components::logo::draw_lapse_logo;
use crate::gui::theme::{M3Typography, MonetTheme};
use crate::update::{check_for_updates, get_update_state, UpdateStatus};

const REPO_URL: &str = "https://github.com/canersin/Lapse";

/// GitHub card: the mark and a short line; the whole card opens the repository.
fn github_card(ui: &mut egui::Ui, theme: &MonetTheme) {
    use crate::gui::components::motion::{hover_t, lerp_color};
    use crate::gui::theme::M3Shapes;
    let resp = egui::Frame::none()
        .fill(theme.surface_container)
        .stroke(egui::Stroke::new(1.0_f32, theme.outline_variant))
        .rounding(M3Shapes::LARGE)
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(48.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 24.0, theme.surface_container_highest);
                egui::Image::new(egui::include_image!("../../../assets/github-mark.svg"))
                    .tint(theme.on_surface)
                    .paint_at(ui, egui::Rect::from_center_size(r.center(), Vec2::splat(30.0)));
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("GitHub").size(16.0).strong().color(theme.on_surface));
                    ui.label(RichText::new(t("Source code, releases and bug reports.")).size(12.0).color(theme.on_surface_variant));
                });
            });
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let h = hover_t(ui, &resp);
    if h > 0.0 {
        ui.painter().rect_stroke(resp.rect, M3Shapes::LARGE, egui::Stroke::new(1.5_f32, lerp_color(theme.outline_variant, theme.primary, h)));
    }
    if resp.clicked() {
        let _ = open::that(REPO_URL);
    }
}

pub fn render_about(
    theme: &MonetTheme,
    replay_hotkey_str: &str,
    record_hotkey_str: &str,
    ui: &mut egui::Ui,
) {
    let subtitle = if cfg!(windows) {
        t("Next-generation game replay capture & lossless trimming for Windows")
    } else {
        t("Next-generation game replay capture & lossless trimming for Linux")
    };
    m3_page_header(
        ui,
        theme,
        t("About Lapse"),
        subtitle,
        |_| {},
    );

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            m3_card(ui, theme, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), egui::Sense::hover());
                    draw_lapse_logo(ui.painter(), rect, theme.surface_container_highest, theme.on_surface);

                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(t("Lapse - Replay Studio")).size(20.0).strong().color(theme.on_surface));
                        ui.add_space(2.0);
                        ui.label(RichText::new(format!("{} {}", t("Version"), env!("CARGO_PKG_VERSION"))).size(13.0).color(theme.on_surface_variant));
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            m3_badge(ui, theme, t("GPL-3.0 License"), Some(theme.primary));
                            ui.add_space(4.0);
                            m3_badge(ui, theme, t("Lossless Cut Stream-Copy"), Some(theme.success));
                            ui.add_space(4.0);
                            m3_badge(ui, theme, if cfg!(windows) { "Windows" } else { "Linux" }, Some(theme.primary));
                        });
                    });
                });
            });

            ui.add_space(16.0);
            github_card(ui, theme);
            ui.add_space(16.0);

            m3_card(ui, theme, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(M3Typography::title_medium(t("Software Updates"), theme.on_surface));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if m3_outlined_button(ui, theme, t("Check for Updates")).clicked() {
                                check_for_updates();
                            }
                        });
                    });
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    let st_opt = get_update_state().lock().ok().map(|g| (*g).clone());
                    if let Some(st) = st_opt {
                        match st {
                            UpdateStatus::Idle => {
                                ui.label(RichText::new(t("Automatic check on startup enabled. Click check button to inspect latest release.")).size(12.5).color(theme.on_surface_variant));
                            }
                            UpdateStatus::Checking => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.add_space(6.0);
                                    ui.label(RichText::new(t("Checking GitHub releases for latest Lapse build...")).size(12.5).color(theme.primary));
                                });
                            }
                            UpdateStatus::UpToDate => {
                                ui.horizontal(|ui| {
                                    m3_badge(ui, theme, t("Up to date"), Some(theme.success));
                                    ui.add_space(8.0);
                                    ui.label(RichText::new(format!("You are running the latest version (v{})", env!("CARGO_PKG_VERSION"))).size(12.5).color(theme.on_surface));
                                });
                            }
                            UpdateStatus::Available { version, url } => {
                                ui.horizontal(|ui| {
                                    m3_badge(ui, theme, t("Update Available"), Some(theme.error));
                                    ui.add_space(8.0);
                                    ui.label(RichText::new(format!("New release {} is available!", version)).size(13.0).strong().color(theme.on_surface));
                                    if m3_small_button_colored(ui, theme, t("Open GitHub Release"), theme.primary).clicked() {
                                        let _ = open::that(url);
                                    }
                                });
                            }
                            UpdateStatus::Error(err) => {
                                ui.label(RichText::new(format!("Update check notice: {}", err)).size(12.0).color(theme.outline));
                            }
                        }
                    }
                });
            });

            ui.add_space(16.0);

            m3_card(ui, theme, |ui| {
                ui.vertical(|ui| {
                    ui.label(M3Typography::title_medium(t("Keyboard Shortcuts & Workflow"), theme.on_surface));
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    let shortcuts = [
                        (replay_hotkey_str, "Instant Save Gameplay Replay Buffer"),
                        (record_hotkey_str, "Toggle Manual Recording Session"),
                        ("Space", "Play / Pause Video Clip"),
                        ("M", "Mute / Unmute Clip Audio"),
                        ("Left / Right", "Seek Backward / Forward 5 seconds"),
                        ("J / L", "Seek Backward / Forward 10 seconds"),
                        ("F", "Fullscreen (Esc to leave)"),
                        ("0 - 9", "Jump to 0-90% of the clip"),
                        ("Up / Down", "Volume"),
                        (", / .", "Previous / next frame"),
                        ("I / O", "Set selection start / end"),
                    ];

                    for (key, desc) in shortcuts {
                        ui.horizontal(|ui| {
                            m3_keycap(ui, theme, key);
                            ui.add_space(12.0);
                            ui.label(RichText::new(t(desc)).size(13.0).color(theme.on_surface));
                        });
                        ui.add_space(6.0);
                    }
                });
            });

            ui.add_space(16.0);

            m3_card(ui, theme, |ui| {
                ui.vertical(|ui| {
                    ui.label(M3Typography::title_medium(t("Under the Hood"), theme.on_surface));
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    let lines: &[&str] = if cfg!(windows) {
                        &[
                            "Capture: Windows Graphics Capture and Media Foundation hardware encoding (H.264, H.265, AV1).",
                            "Interface: Rust, eframe and egui, native on Windows.",
                            "Editing: ffmpeg, with instant stream-copy cuts or exact re-encoded cuts.",
                        ]
                    } else {
                        &[
                            "Capture: gpu-screen-recorder with hardware encoding (VAAPI, NVENC) and a replay buffer in RAM or on disk.",
                            "Interface: Rust, eframe and egui, native on Wayland and X11.",
                            "Editing: ffmpeg, with instant stream-copy cuts or exact re-encoded cuts.",
                        ]
                    };

                    for line in lines {
                        ui.label(M3Typography::body_medium(&format!("- {}", t(line)), theme.on_surface_variant));
                    }
                });
            });
        });
}
