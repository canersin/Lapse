//! First-run setup wizard, shown once over the main window.

use eframe::egui::{self, Align2, Color32, FontId, Id, Margin, Pos2, Rect, RichText, Stroke, Vec2};

use crate::config::{estimated_buffer_mb, Config};
use crate::gui::components::button::{m3_filled_button_sized, m3_outlined_button};
use crate::gui::components::chip::m3_keycap;
use crate::gui::components::icon::{draw_vector_icon, IconKind};
use crate::gui::components::input::m3_duration_slider_input;
use crate::gui::components::motion::{ease_out, m3_switch_labeled, MEDIUM};
use crate::gui::components::segmented::segmented;
use crate::gui::i18n::{self, t};
use crate::gui::theme::{M3Shapes, MonetTheme};
use crate::gui::views::capture_settings_view::preset_picker;
use crate::gui::views::hotkey_view::format_hotkey;
use crate::hardware::gpu::GpuDevice;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Language,
    Quality,
    Hotkeys,
    DualGpu,
    Done,
}

fn steps(gpus: &[GpuDevice]) -> Vec<Step> {
    let mut s = vec![Step::Language, Step::Quality, Step::Hotkeys];
    if gpus.len() >= 2 {
        s.push(Step::DualGpu);
    }
    s.push(Step::Done);
    s
}

/// Returns true when the wizard finished this frame (config should be saved and applied).
pub fn render_welcome(ctx: &egui::Context, theme: &MonetTheme, config: &mut Config, gpus: &[GpuDevice]) -> bool {
    if config.first_run_done {
        return false;
    }
    let steps = steps(gpus);
    let id = Id::new("lapse_welcome_step");
    let mut index: usize = ctx.data(|d| d.get_temp(id)).unwrap_or(0).min(steps.len() - 1);
    let mut finished = false;

    let screen = ctx.screen_rect();
    let appear = ease_out(ctx.animate_bool_with_time(id.with("appear"), true, MEDIUM * 1.5));

    // Dim the app behind the dialog.
    egui::Area::new(id.with("scrim"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha((170.0 * appear) as u8));
        });

    let size = Vec2::new(560.0_f32.min(screen.width() - 32.0), 470.0_f32.min(screen.height() - 32.0));
    let pos = screen.center() - size / 2.0 + Vec2::new(0.0, 24.0 * (1.0 - appear));

    egui::Area::new(id.with("dialog"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(theme.surface_container_high)
                .stroke(Stroke::new(1.0_f32, theme.outline_variant))
                .rounding(M3Shapes::EXTRA_LARGE)
                .inner_margin(Margin::same(26.0))
                .show(ui, |ui| {
                    ui.set_min_size(size - Vec2::splat(52.0));
                    ui.set_max_width(size.x - 52.0);

                    progress_dots(ui, theme, index, steps.len());
                    ui.add_space(18.0);

                    // Content slides in from the side on every step change.
                    let slide = ctx.animate_value_with_time(id.with("slide"), index as f32, MEDIUM);
                    let offset = (index as f32 - slide) * 40.0;
                    let content_rect = Rect::from_min_size(ui.cursor().min + Vec2::new(offset, 0.0), Vec2::new(size.x - 52.0, size.y - 150.0));
                    ui.allocate_ui_at_rect(content_rect, |ui| match steps[index] {
                        Step::Language => language_step(ui, theme, config),
                        Step::Quality => quality_step(ui, theme, config),
                        Step::Hotkeys => hotkey_step(ui, theme, config),
                        Step::DualGpu => dual_gpu_step(ui, theme, config),
                        Step::Done => done_step(ui, theme, config),
                    });
                    ui.add_space((size.y - 150.0 - (ui.cursor().min.y - content_rect.top())).max(0.0));

                    ui.horizontal(|ui| {
                        if index > 0 {
                            if m3_outlined_button(ui, theme, t("Back")).clicked() {
                                index -= 1;
                            }
                        } else if ui.add(egui::Button::new(RichText::new(t("Skip setup")).color(theme.outline)).frame(false)).clicked() {
                            finished = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let last = index + 1 == steps.len();
                            let label = if last { t("Start using Lapse") } else { t("Next") };
                            if m3_filled_button_sized(ui, theme, label, Vec2::new(170.0, 40.0)).clicked() {
                                if last {
                                    finished = true;
                                } else {
                                    index += 1;
                                }
                            }
                        });
                    });
                });
        });

    ctx.data_mut(|d| d.insert_temp(id, index));
    if finished {
        config.first_run_done = true;
    }
    finished
}

fn progress_dots(ui: &mut egui::Ui, theme: &MonetTheme, index: usize, count: usize) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), egui::Sense::hover());
    let mut x = rect.left();
    for i in 0..count {
        // The current step's dot stretches into a pill.
        let w = 8.0 + 18.0 * ease_out(ui.ctx().animate_bool_with_time(Id::new(("welcome_dot", i)), i == index, MEDIUM));
        let color = if i <= index { theme.primary } else { theme.outline_variant };
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w, 8.0)), M3Shapes::FULL, color);
        x += w + 6.0;
    }
}

fn title(ui: &mut egui::Ui, theme: &MonetTheme, icon: IconKind, text: &str, sub: &str) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(44.0), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 22.0, theme.primary_container);
    draw_vector_icon(ui.painter(), r.center(), 22.0, icon, theme.primary);
    ui.add_space(10.0);
    ui.label(RichText::new(text).size(22.0).strong().color(theme.on_surface));
    ui.add_space(4.0);
    ui.label(RichText::new(sub).size(13.0).color(theme.on_surface_variant));
    ui.add_space(18.0);
}

fn language_step(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    title(ui, theme, IconKind::Globe, t("Welcome to Lapse"), t("Lapse keeps the last moments of your game in a buffer, so you can save a clip after something cool happens."));
    ui.label(RichText::new(t("Language")).size(13.0).strong().color(theme.on_surface));
    ui.add_space(6.0);
    let options: Vec<(String, &str)> = i18n::LANGUAGES.iter().map(|(c, n)| (c.to_string(), *n)).collect();
    if segmented(ui, theme, "welcome_lang", &mut config.language, &options, 110.0).is_some() {
        i18n::set_language(&config.language);
    }
}

fn quality_step(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    title(ui, theme, IconKind::Video, t("Quality and buffer"), t("Higher quality looks better but uses more memory and disk space."));
    preset_picker(ui, theme, config, "welcome_preset");
    ui.add_space(14.0);
    ui.label(RichText::new(t("How many seconds to keep")).size(13.0).strong().color(theme.on_surface));
    ui.add_space(4.0);
    m3_duration_slider_input(ui, theme, &mut config.replay_seconds);
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(t("Keep the buffer in")).size(13.0).color(theme.on_surface));
        let options = [("ram".to_string(), t("RAM")), ("disk".to_string(), t("Disk"))];
        segmented(ui, theme, "welcome_storage", &mut config.replay_storage, &options, 70.0);
    });
    ui.add_space(8.0);
    let mb = estimated_buffer_mb(config, 2560, 1440);
    ui.label(
        RichText::new(format!("{} {:.0} MB {}", t("Up to"), mb, t("at 1440p")))
            .size(12.5)
            .color(theme.primary),
    );
}

fn hotkey_step(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    title(ui, theme, IconKind::Keyboard, t("Your shortcuts"), t("They work while you play. You can change them any time in Settings > Hotkeys."));
    egui::Grid::new("welcome_keys").num_columns(2).spacing(Vec2::new(16.0, 12.0)).show(ui, |ui| {
        ui.label(RichText::new(t("Save the last moments")).size(13.5).color(theme.on_surface));
        m3_keycap(ui, theme, &format_hotkey(config.hotkey_replay, config.hotkey_replay_mod));
        ui.end_row();
        ui.label(RichText::new(t("Start / stop recording")).size(13.5).color(theme.on_surface));
        m3_keycap(ui, theme, &format_hotkey(config.hotkey_record, config.hotkey_record_mod));
        ui.end_row();
    });
    ui.add_space(14.0);
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        ui.label(
            RichText::new(t("On Wayland your desktop will ask once to allow these shortcuts. Click Allow so they work in every game."))
                .size(12.5)
                .color(theme.on_surface_variant),
        );
    }
}

fn dual_gpu_step(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    title(ui, theme, IconKind::Chip, t("Two graphics cards found"), t("Lapse can record on one card while you play on the other, so recording costs no FPS."));
    m3_switch_labeled(ui, theme, &mut config.dual_gpu, t("Enable dual GPU recording"));
    ui.add_space(10.0);
    ui.label(
        RichText::new(t("Plug your monitor into the card that should record, then launch games with: Lapse run -- %command%. Settings > Recording shows a checklist."))
            .size(12.5)
            .color(theme.on_surface_variant),
    );
}

fn done_step(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config) {
    title(ui, theme, IconKind::Check, t("All set!"), t("Lapse runs in the background and starts with your computer."));
    let key = format_hotkey(config.hotkey_replay, config.hotkey_replay_mod);
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 70.0), egui::Sense::hover());
    ui.painter().rect_filled(r, M3Shapes::LARGE, theme.primary_container);
    ui.painter().text(
        r.center(),
        Align2::CENTER_CENTER,
        format!("{} {}  :  {} {} {}", t("Press"), key, t("save the last"), config.replay_seconds, t("seconds")),
        FontId::proportional(15.0),
        theme.on_primary_container,
    );
}
