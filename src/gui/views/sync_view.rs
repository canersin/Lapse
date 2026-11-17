//! Settings > Audio: audio sync. Shifts the sound of saved clips when it runs ahead of
//! or behind the picture, and can measure the shift from the last clip.

use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::av_sync::Measurement;
use crate::config::Config;
use crate::gui::components::button::m3_small_button_colored;
use crate::gui::components::button::m3_outlined_button;
use crate::gui::components::card::m3_section_card;
use crate::gui::components::icon::IconKind;
use crate::gui::components::input::m3_setting_row;
use crate::gui::components::motion::pulse;
use crate::gui::components::slider::{m3_slider, SliderOpts};
use crate::gui::i18n::t;
use crate::gui::theme::MonetTheme;
use crate::ipc::StatusResponse;

/// Below this the clip had too few clear sound + picture moments to trust the result.
const MIN_CONFIDENCE: f32 = 4.0;

enum Job {
    Running,
    Done(anyhow::Result<Measurement>),
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);

fn start(path: PathBuf) {
    *JOB.lock().unwrap_or_else(|e| e.into_inner()) = Some(Job::Running);
    std::thread::spawn(move || {
        let result = crate::av_sync::measure(&path);
        *JOB.lock().unwrap_or_else(|e| e.into_inner()) = Some(Job::Done(result));
    });
}

fn describe(ms: i32) -> String {
    match ms {
        0 => t("In sync").to_string(),
        m if m > 0 => format!("+{} ms · {}", m, t("sound plays later")),
        m => format!("{} ms · {}", m, t("sound plays earlier")),
    }
}

pub fn render_audio_sync(ui: &mut egui::Ui, theme: &MonetTheme, config: &mut Config, status: Option<&StatusResponse>) {
    m3_section_card(
        ui,
        theme,
        Some(IconKind::Timer),
        t("Audio sync"),
        Some(t("If sound comes before or after the picture in your clips, shift it here. Applies to clips saved from now on.")),
        |ui| {
            m3_setting_row(ui, theme, t("Shift sound"), &describe(config.audio_offset_ms), |ui| {
                let mut v = config.audio_offset_ms as f64;
                if m3_slider(ui, theme, &mut v, -300.0..=300.0, 220.0, SliderOpts { log: false, step: 5.0 }).changed() {
                    config.audio_offset_ms = v as i32;
                }
            });
            ui.separator();

            let last = status.and_then(|s| s.last_saved.clone()).map(PathBuf::from).filter(|p| p.is_file());
            let mut apply = None;
            m3_setting_row(ui, theme, t("Measure from the last clip"), t("Compares loud sounds with sudden changes on screen. Works best with shots or explosions."), |ui| {
                let job = JOB.lock().unwrap_or_else(|e| e.into_inner());
                let running = matches!(*job, Some(Job::Running));
                drop(job);
                if running {
                    ui.label(RichText::new(t("Measuring...")).size(12.5).color(theme.primary));
                    let _ = pulse(ui, 1.0);
                } else {
                    let button = ui.add_enabled_ui(last.is_some(), |ui| m3_outlined_button(ui, theme, t("Measure"))).inner;
                    if button.clicked() {
                        if let Some(p) = &last {
                            start(p.clone());
                        }
                    }
                }
            });

            let job = JOB.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(Job::Done(result)) = &*job {
                ui.horizontal_wrapped(|ui| match result {
                    Ok(m) if m.confidence >= MIN_CONFIDENCE => {
                        // The clip already has the current shift applied.
                        let total = (config.audio_offset_ms + m.offset_ms).clamp(-300, 300);
                        ui.label(RichText::new(format!("{} {}", t("Result:"), describe(m.offset_ms))).size(12.5).color(theme.on_surface));
                        if m.offset_ms != 0 && m3_small_button_colored(ui, theme, t("Apply"), theme.primary).clicked() {
                            apply = Some(total);
                        }
                    }
                    Ok(_) => {
                        ui.label(RichText::new(t("Couldn't measure reliably: the clip has too few clear sound moments. Try a clip with gunshots.")).size(12.5).color(theme.on_surface_variant));
                    }
                    Err(e) => {
                        ui.label(RichText::new(format!("{} {}", t("Measuring failed:"), e)).size(12.5).color(theme.error));
                    }
                });
            }
            drop(job);
            if let Some(total) = apply {
                config.audio_offset_ms = total;
                *JOB.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
        },
    );
}
