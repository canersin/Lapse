use eframe::egui::{self, Align2, Color32, FontId, Margin, Pos2, Rect, RichText, Stroke, Vec2};

use crate::gui::i18n::{t, tr_any};
use crate::config::Config;
use crate::gui::components::dropdown::m3_dropdown;
use crate::gui::components::button::m3_outlined_button;
use crate::gui::components::icon::{draw_vector_icon, m3_vector_icon, IconKind};
use crate::gui::components::input::m3_setting_row;
use crate::gui::components::motion::{ease_out, lerp_color, m3_switch, MEDIUM};
use crate::gui::theme::{M3Shapes, M3Typography, MonetTheme};
use crate::hardware::gpu::{dual_gpu_report, render_env_for, GpuDevice};
use crate::ipc::StatusResponse;

pub const STEAM_LAUNCH_OPTION: &str = "Lapse run -- %command%";

/// "Advanced" fold at the bottom of the Recording tab. Closed by default: most systems
/// never need dual GPU, since recording always happens on the card the monitor is on.
pub fn render_advanced(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    config: &mut Config,
    gpus: &[GpuDevice],
    status: Option<&StatusResponse>,
) {
    let id = egui::Id::new("lapse_advanced_open");
    let mut open: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    let resp = egui::Frame::none()
        .fill(theme.surface_container_low)
        .stroke(Stroke::new(1.0_f32, theme.outline_variant))
        .rounding(M3Shapes::LARGE)
        .inner_margin(Margin::symmetric(16.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
                let turn = ui.ctx().animate_bool_with_time(id.with("chev"), open, MEDIUM);
                chevron(ui, r.center(), turn, theme.on_surface_variant);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.label(M3Typography::title_medium(t("Advanced"), theme.on_surface));
                    ui.label(M3Typography::body_medium(
                        t("Dual GPU. Not needed on most systems: Lapse records on the card your monitor is plugged into."),
                        theme.on_surface_variant,
                    ));
                });
                if config.dual_gpu {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(t("On")).size(12.0).strong().color(theme.primary));
                    });
                }
            });
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    if open {
        ui.add_space(10.0);
        render_dual_gpu_section(ui, theme, config, gpus, status);
    }
}

/// A small chevron that turns from pointing right (closed) to down (open).
fn chevron(ui: &egui::Ui, c: Pos2, turn: f32, color: Color32) {
    let a = turn * std::f32::consts::FRAC_PI_2;
    let rot = |x: f32, y: f32| c + Vec2::new(x * a.cos() - y * a.sin(), x * a.sin() + y * a.cos());
    let pts = vec![rot(-2.5, -5.0), rot(2.5, 0.0), rot(-2.5, 5.0)];
    ui.painter().add(egui::Shape::line(pts, Stroke::new(1.8_f32, color)));
}

fn render_dual_gpu_section(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    config: &mut Config,
    gpus: &[GpuDevice],
    status: Option<&StatusResponse>,
) {
    let open_t = ease_out(ui.ctx().animate_bool_with_time(
        egui::Id::new("dual_gpu_open"),
        config.dual_gpu,
        MEDIUM,
    ));
    let border = lerp_color(theme.outline_variant, theme.primary, open_t * 0.7);

    egui::Frame::none()
        .fill(theme.surface_container)
        .stroke(Stroke::new(1.0_f32 + open_t * 0.5, border))
        .rounding(M3Shapes::LARGE)
        .inner_margin(Margin::same(18.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                m3_vector_icon(ui, IconKind::Chip, 22.0, theme.primary);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(M3Typography::title_medium(t("Dual GPU Recording"), theme.on_surface));
                    ui.label(M3Typography::body_medium(
                        t("Play on one graphics card and record on the other, so encoding never costs you FPS."),
                        theme.on_surface_variant,
                    ));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    m3_switch(ui, theme, &mut config.dual_gpu);
                });
            });

            if open_t <= 0.01 {
                return;
            }

            ui.add_space(14.0 * open_t);
            ui.separator();
            ui.add_space(12.0 * open_t);

            let report = dual_gpu_report(gpus, &config.game_gpu, &config.gpu, &config.monitor);
            render_pipeline(ui, theme, &report.game_gpu, &report.encode_gpu, open_t);
            ui.add_space(14.0);

            gpu_combo(ui, theme, t("Game GPU"), t("Renders your games"), "dual_game_gpu", &mut config.game_gpu, gpus);
            ui.separator();
            gpu_combo(ui, theme, t("Encoder GPU"), t("Captures and encodes the recording"), "dual_encode_gpu", &mut config.gpu, gpus);

            ui.add_space(12.0);
            ui.label(RichText::new(t("SETUP CHECKLIST")).size(11.0).strong().color(theme.outline));
            ui.add_space(6.0);

            let enc_name = report.encode_gpu.as_ref().map(|g| g.short_name()).unwrap_or_else(|| "encoder GPU".into());
            let enc_mons = report
                .encode_gpu
                .as_ref()
                .map(|g| g.connected_monitors.clone())
                .unwrap_or_default();

            check_row(ui, theme, gpus.len() >= 2, &format!("{}: {}", t("Graphics cards detected (2 needed)"), gpus.len()));
            check_row(
                ui,
                theme,
                !enc_mons.is_empty(),
                &if enc_mons.is_empty() {
                    format!("{}: {}", t("Plug your monitor into"), enc_name)
                } else {
                    format!("{} {} ({})", t("Monitor plugged into"), enc_name, enc_mons.join(", "))
                },
            );
            check_row(ui, theme, report.is_split(), t("Game GPU and encoder GPU are different cards"));

            if let Some(s) = status {
                if !s.encode_card.is_empty() {
                    let ok = report
                        .encode_gpu
                        .as_ref()
                        .map(|g| g.card_name() == s.encode_card)
                        .unwrap_or(false);
                    let live_name = gpus
                        .iter()
                        .find(|g| g.card_name() == s.encode_card)
                        .map(|g| g.short_name())
                        .unwrap_or_else(|| s.encode_card.clone());
                    check_row(ui, theme, ok, &format!("{} {}", t("Recorder is currently encoding on"), live_name));
                }
            }

            for problem in &report.problems {
                ui.add_space(4.0);
                egui::Frame::none()
                    .fill(theme.error.gamma_multiply(0.10))
                    .rounding(M3Shapes::SMALL)
                    .inner_margin(Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            m3_vector_icon(ui, IconKind::Warning, 15.0, theme.error);
                            ui.label(RichText::new(tr_any(problem)).size(12.0).color(theme.on_surface));
                        });
                    });
            }

            ui.add_space(12.0);
            ui.label(RichText::new(t("LAUNCH GAMES ON THE GAME GPU")).size(11.0).strong().color(theme.outline));
            ui.add_space(4.0);
            ui.label(
                RichText::new(t("Steam > right-click the game > Properties > Launch Options:"))
                    .size(12.0)
                    .color(theme.on_surface_variant),
            );
            ui.add_space(4.0);
            copy_box(ui, theme, STEAM_LAUNCH_OPTION, "copy_steam_launch");

            if let Some(game) = report.game_gpu.as_ref().filter(|_| report.is_split()) {
                let env: Vec<String> = render_env_for(game)
                    .into_iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect();
                if !env.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(t("Or without Lapse (Lutris, Heroic, terminal):"))
                            .size(12.0)
                            .color(theme.on_surface_variant),
                    );
                    ui.add_space(4.0);
                    copy_box(ui, theme, &format!("{} %command%", env.join(" ")), "copy_env_launch");
                }
            }

            ui.add_space(8.0);
            ui.label(
                RichText::new(t("How it works: gpu-screen-recorder always encodes on the card your monitor is plugged into. The game renders on the game GPU and its frames are passed to the encoder GPU over PCIe (PRIME), the same idea as choosing a per-app GPU on Windows."))
                .size(11.5)
                .color(theme.outline),
            );
        });
}

fn gpu_combo(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    title: &str,
    desc: &str,
    id: &str,
    value: &mut String,
    gpus: &[GpuDevice],
) {
    m3_setting_row(ui, theme, title, desc, |ui| {
        let selected = gpus
            .iter()
            .find(|g| g.matches(value))
            .map(|g| g.short_name())
            .unwrap_or_else(|| "Auto".into());
        m3_dropdown(ui, theme, id, selected, 240.0, |ui| {
                ui.selectable_value(value, "Auto".into(), t("Auto"));
                for g in gpus {
                    let label = format!(
                        "{}  ·  {}",
                        g.short_name(),
                        if g.connected_monitors.is_empty() {
                            "no monitor".to_string()
                        } else {
                            g.connected_monitors.join(", ")
                        }
                    );
                    ui.selectable_value(value, g.pci_id.clone(), label);
                }
            });
    });
}

fn check_row(ui: &mut egui::Ui, theme: &MonetTheme, ok: bool, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), egui::Sense::hover());
        let color = if ok { theme.success } else { Color32::from_rgb(240, 180, 60) };
        ui.painter().circle_filled(rect.center(), 9.0, color.gamma_multiply(0.18));
        draw_vector_icon(
            ui.painter(),
            rect.center(),
            12.0,
            if ok { IconKind::Check } else { IconKind::Warning },
            color,
        );
        ui.add_space(4.0);
        ui.label(RichText::new(text).size(12.5).color(theme.on_surface));
    });
    ui.add_space(2.0);
}

fn copy_box(ui: &mut egui::Ui, theme: &MonetTheme, text: &str, id: &str) {
    let copied_id = egui::Id::new(id);
    let copied_at: Option<f64> = ui.data(|d| d.get_temp(copied_id));
    let now = ui.input(|i| i.time);
    let recently = copied_at.map(|t| now - t < 1.5).unwrap_or(false);

    egui::Frame::none()
        .fill(theme.surface_container_lowest)
        .stroke(Stroke::new(1.0_f32, theme.outline_variant))
        .rounding(M3Shapes::SMALL)
        .inner_margin(Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(text).monospace().size(12.5).color(theme.primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if recently { t("Copied!") } else { t("Copy") };
                    if m3_outlined_button(ui, theme, label).clicked() {
                        ui.output_mut(|o| o.copied_text = text.to_string());
                        ui.data_mut(|d| d.insert_temp(copied_id, now));
                    }
                    if recently {
                        ui.ctx().request_repaint();
                    }
                });
            });
        });
}

/// Game GPU --(animated frames)--> Encoder GPU diagram.
fn render_pipeline(
    ui: &mut egui::Ui,
    theme: &MonetTheme,
    game: &Option<GpuDevice>,
    encoder: &Option<GpuDevice>,
    open_t: f32,
) {
    let width = ui.available_width();
    let height = 92.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    let tile_w = ((width - 120.0) / 2.0).clamp(150.0, 300.0);
    let left = Rect::from_min_size(rect.min, Vec2::new(tile_w, height));
    let right = Rect::from_min_size(Pos2::new(rect.right() - tile_w, rect.top()), Vec2::new(tile_w, height));

    let tile = |r: Rect, role: &str, gpu: &Option<GpuDevice>, accent: Color32, icon: IconKind| {
        painter.rect_filled(r, M3Shapes::MEDIUM, theme.surface_container_high);
        painter.rect_stroke(r, M3Shapes::MEDIUM, Stroke::new(1.0_f32, accent.gamma_multiply(0.6)));
        let ic = Pos2::new(r.left() + 30.0, r.center().y);
        painter.circle_filled(ic, 18.0, accent.gamma_multiply(0.18));
        draw_vector_icon(&painter, ic, 18.0, icon, accent);
        painter.text(
            Pos2::new(r.left() + 58.0, r.center().y - 18.0),
            Align2::LEFT_CENTER,
            role,
            FontId::proportional(10.5),
            accent,
        );
        let name = gpu.as_ref().map(|g| g.short_name()).unwrap_or_else(|| t("Not detected").into());
        let galley = painter.layout(
            name,
            FontId::proportional(13.0),
            theme.on_surface,
            r.width() - 66.0,
        );
        painter.galley(Pos2::new(r.left() + 58.0, r.center().y - 8.0), galley, theme.on_surface);
        let sub = gpu
            .as_ref()
            .map(|g| {
                if g.connected_monitors.is_empty() {
                    format!("{} · no monitor", g.card_name())
                } else {
                    format!("{} · {}", g.card_name(), g.connected_monitors.join(", "))
                }
            })
            .unwrap_or_default();
        painter.text(
            Pos2::new(r.left() + 58.0, r.bottom() - 14.0),
            Align2::LEFT_CENTER,
            sub,
            FontId::proportional(10.5),
            theme.outline,
        );
    };
    tile(left, t("GAME"), game, theme.primary, IconKind::Gamepad);
    tile(right, t("RECORD + ENCODE"), encoder, theme.success, IconKind::RecordDot);

    // Flowing frames between the tiles.
    let a = Pos2::new(left.right() + 10.0, rect.center().y);
    let b = Pos2::new(right.left() - 10.0, rect.center().y);
    painter.line_segment([a, b], Stroke::new(2.0_f32, theme.outline_variant));
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    let t = ui.input(|i| i.time) as f32;
    let span = b.x - a.x;
    for i in 0..4 {
        let k = ((t * 0.6 + i as f32 / 4.0) % 1.0) * open_t;
        let x = a.x + span * k;
        let fade = (k * std::f32::consts::PI).sin();
        let c = lerp_color(theme.primary, theme.success, k).gamma_multiply(fade);
        painter.rect_filled(
            Rect::from_center_size(Pos2::new(x, a.y), Vec2::new(10.0, 7.0)),
            2.0,
            c,
        );
    }
    painter.text(
        Pos2::new((a.x + b.x) / 2.0, a.y - 14.0),
        Align2::CENTER_CENTER,
        "frames · PCIe",
        FontId::proportional(10.0),
        theme.outline,
    );
    // Arrow head.
    painter.line_segment([b, b + Vec2::new(-7.0, -5.0)], Stroke::new(2.0_f32, theme.success));
    painter.line_segment([b, b + Vec2::new(-7.0, 5.0)], Stroke::new(2.0_f32, theme.success));
}
