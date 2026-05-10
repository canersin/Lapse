use eframe::egui::{self, Color32, Rounding, Stroke};
use crate::config::Config;
use crate::ipc;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Instant, Duration};

use crate::hardware::audio::{get_audio_devices, AudioDevice};
use crate::hardware::monitor::{get_monitors, MonitorDevice};
use crate::hardware::gpu::{get_gpus, GpuDevice};

use super::library::ClipMetadata;
use super::{sidebar, library, settings};

#[derive(PartialEq, Clone, Copy)]
pub enum ActiveTab {
    Library,
    Settings,
}

pub struct LapseApp {
    pub config: Config,
    pub available_outputs: Vec<AudioDevice>,
    pub available_inputs: Vec<AudioDevice>,
    pub available_monitors: Vec<MonitorDevice>,
    pub available_gpus: Vec<GpuDevice>,
    pub active_tab: ActiveTab,
    pub cached_clips: Vec<ClipMetadata>,
    pub last_clip_refresh: Instant,
    pub last_status: Option<ipc::StatusResponse>,
    pub last_status_poll: Instant,
    pub binding_hotkey: bool, 
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
    
    pub fn send_command(&self, cmd: ipc::Command) -> Option<ipc::Response> {
        if let Ok(mut stream) = UnixStream::connect("/tmp/lapse.sock") {
            let _ = stream.write_all(serde_json::to_string(&cmd).unwrap().as_bytes());
            let mut buffer = [0; 1024];
            if let Ok(n) = stream.read(&mut buffer) {
                return serde_json::from_slice(&buffer[..n]).ok();
            }
        }
        None
    }
}

impl eframe::App for LapseApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        settings::handle_hotkey_binding(self, ctx);

        if self.last_status_poll.elapsed().as_millis() > 500 {
            self.last_status_poll = Instant::now();
            if let Some(ipc::Response::Status(s)) = self.send_command(ipc::Command::GetStatus) {
                self.last_status = Some(s);
            }
        }
        ctx.request_repaint_after(Duration::from_millis(500));

        sidebar::render(self, ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.active_tab {
                ActiveTab::Library => library::render(self, ui),
                ActiveTab::Settings => settings::render(self, ui),
            }
        });
    }
}
