use std::sync::mpsc::{self, Sender};
use crate::gui::i18n::t;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Margin, RichText, Stroke, Vec2};

use crate::config::Config;
use crate::hardware::audio::{get_audio_devices, AudioDevice};
use crate::hardware::gpu::{get_gpus, GpuDevice};
use crate::hardware::monitor::{get_monitors, MonitorDevice};
use crate::ipc;

use super::components::{render_nav_rail, RAIL_COLLAPSED_WIDTH};
use super::components::motion::{ease_out, page_fade_in, ToastKind, Toasts, MEDIUM};
use super::state::{GuiState, NavigationTab};
use super::theme::{apply_theme, M3Shapes, MonetPaletteKind, MonetTheme};
use super::views::about_view::render_about;
use super::views::home_view::render_home;
use super::views::hotkey_view::handle_hotkey_binding;
use super::views::library_view::{render_library, ClipMetadata};
use super::views::player_view::{render_fullscreen, render_player, PlayerState};
use super::views::settings_view::{render_settings, SettingsHardwareContext};

pub struct LapseApp {
    pub config: Config,
    pub theme: MonetTheme,
    pub palette_kind: MonetPaletteKind,
    pub state: GuiState,

    pub available_outputs: Vec<AudioDevice>,
    pub available_inputs: Vec<AudioDevice>,
    pub available_monitors: Vec<MonitorDevice>,
    pub available_gpus: Vec<GpuDevice>,

    pub cached_clips: Vec<ClipMetadata>,
    pub last_clip_refresh: Instant,
    pub last_status: Option<ipc::StatusResponse>,
    pub last_status_poll: Instant,
    /// The shortcut waiting for a key press in Settings > Hotkeys.
    pub binding_hotkey: Option<crate::hotkeys::HotkeyEvent>,
    pub player_state: Option<PlayerState>,
    last_open_request_check: Instant,
    /// Whether we put the window in fullscreen for the player.
    window_fullscreen: bool,
    /// Set when another launch asked this window to come to the front.
    requests: Arc<crate::gui::single_instance::Requests>,
    pub shared_status: Arc<Mutex<Option<ipc::StatusResponse>>>,
    pub toasts: Toasts,
    pub daemon_seen: bool,
}

static IPC_SENDER: std::sync::OnceLock<Mutex<Sender<ipc::Command>>> = std::sync::OnceLock::new();

fn spawn_ipc_worker(
    shared: Arc<Mutex<Option<ipc::StatusResponse>>>,
    ctx: egui::Context,
) -> Sender<ipc::Command> {
    let (tx, rx) = mpsc::channel::<ipc::Command>();
    std::thread::spawn(move || loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(cmd) => {
                let _ = LapseApp::send_ipc_command(cmd);
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if let Some(ipc::Response::Status(s)) = LapseApp::send_ipc_command(ipc::Command::GetStatus)
        {
            let changed = {
                let mut guard = match shared.lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                let changed = guard
                    .as_ref()
                    .map(|o| o.recording != s.recording || o.mode != s.mode)
                    .unwrap_or(true);
                *guard = Some(s);
                changed
            };
            if changed {
                ctx.request_repaint();
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    });
    tx
}

pub fn dispatch_ipc(cmd: ipc::Command) {
    if let Some(sender) = IPC_SENDER.get() {
        if let Ok(s) = sender.lock() {
            let _ = s.send(cmd);
        }
    }
}

impl LapseApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        if config.check_updates {
            crate::update::check_for_updates();
        }

        let palette_kind = MonetPaletteKind::from_config_name(&config.theme_palette);
        let theme = MonetTheme::from_kind(palette_kind);
        apply_theme(&cc.egui_ctx, &theme);

        let (outputs, inputs) = get_audio_devices();
        let monitors = get_monitors();
        let gpus = get_gpus();

        let shared_status = Arc::new(Mutex::new(None));
        let sender = spawn_ipc_worker(Arc::clone(&shared_status), cc.egui_ctx.clone());
        let _ = IPC_SENDER.set(Mutex::new(sender));

        let mut state = GuiState::default();
        state.mic_enabled = config.audio_input != "None" && !config.audio_input.is_empty();
        state.desktop_audio_enabled = config.audio_output != "None" && !config.audio_output.is_empty();

        Self {
            config,
            theme,
            palette_kind,
            state,
            available_outputs: outputs,
            available_inputs: inputs,
            available_monitors: monitors,
            available_gpus: gpus,
            cached_clips: Vec::new(),
            last_clip_refresh: Instant::now().checked_sub(Duration::from_secs(10)).unwrap(),
            last_status: None,
            last_status_poll: Instant::now().checked_sub(Duration::from_secs(10)).unwrap(),
            binding_hotkey: None,
            player_state: None,
            last_open_request_check: Instant::now(),
            window_fullscreen: false,
            requests: crate::gui::single_instance::listen(cc.egui_ctx.clone()),
            shared_status,
            toasts: Toasts::default(),
            daemon_seen: false,
        }
    }

    pub fn send_ipc_command(cmd: ipc::Command) -> Option<ipc::Response> {
        ipc::send(cmd)
    }
}

/// Narrowest the expanded sidebar can be dragged.
const SIDEBAR_MIN_WIDTH: f32 = 200.0;
/// Dragging the edge narrower than this collapses the sidebar to icons.
const SIDEBAR_SNAP_WIDTH: f32 = 140.0;
/// Room the pages need beside the sidebar.
const CONTENT_MIN_WIDTH: f32 = 640.0;

fn sidebar_max_width(window_width: f32) -> f32 {
    (window_width - CONTENT_MIN_WIDTH).clamp(SIDEBAR_MIN_WIDTH, 420.0)
}

impl LapseApp {
    /// "Trim" in a clip notification: open that clip in the trimmer.
    fn take_open_request(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if now.duration_since(self.last_open_request_check) < Duration::from_millis(700) {
            ctx.request_repaint_after(Duration::from_millis(1000));
            return;
        }
        self.last_open_request_check = now;
        let req = crate::utils::open_request_path();
        let Ok(text) = std::fs::read_to_string(&req) else { return };
        let _ = std::fs::remove_file(&req);
        let path = PathBuf::from(text.trim());
        if path.is_file() {
            if let Some(mut old) = self.player_state.take() {
                old.stop();
            }
            self.player_state = PlayerState::new(path);
            self.state.active_tab = NavigationTab::Library;
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    /// Grip on the sidebar's right edge. Dragging it sets the width; dragging it almost
    /// shut collapses the sidebar, and dragging a collapsed one out expands it again.
    fn sidebar_resize_handle(&mut self, ctx: &egui::Context, sidebar: egui::Rect, max_width: f32) {
        let grip = egui::Rect::from_x_y_ranges(sidebar.right() - 4.0..=sidebar.right() + 4.0, sidebar.y_range());
        let resp = egui::Area::new(egui::Id::new("sidebar_resize_grip"))
            .order(egui::Order::Foreground)
            .fixed_pos(grip.min)
            .show(ctx, |ui| ui.allocate_exact_size(grip.size(), egui::Sense::drag()).1)
            .inner;

        if resp.hovered() || resp.dragged() {
            ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            let color = if resp.dragged() { self.theme.primary } else { self.theme.outline };
            ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("sidebar_resize_line")))
                .line_segment(
                    [egui::pos2(sidebar.right(), sidebar.top()), egui::pos2(sidebar.right(), sidebar.bottom())],
                    Stroke::new(2.0_f32, color),
                );
        }
        if resp.dragged() {
            if let Some(pointer) = ctx.pointer_latest_pos() {
                let width = pointer.x - sidebar.left();
                if self.config.sidebar_collapsed {
                    if width > SIDEBAR_SNAP_WIDTH {
                        self.config.sidebar_collapsed = false;
                        self.config.sidebar_width = width.clamp(SIDEBAR_MIN_WIDTH, max_width);
                    }
                } else if width < SIDEBAR_SNAP_WIDTH {
                    self.config.sidebar_collapsed = true;
                } else {
                    self.config.sidebar_width = width.clamp(SIDEBAR_MIN_WIDTH, max_width);
                }
            }
        }
        if resp.drag_released() {
            let _ = self.config.save();
        }
    }
}

impl eframe::App for LapseApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        crate::gui::single_instance::cleanup();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::gui::i18n::set_language(&self.config.language);
        let target_theme = MonetTheme::from_kind(self.palette_kind);
        if self.theme != target_theme {
            self.theme = target_theme;
            apply_theme(ctx, &self.theme);
        }

        if self.requests.quit.load(std::sync::atomic::Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.requests.raise.swap(false, std::sync::atomic::Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            // Wayland may refuse to move focus; then at least flag the window.
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        }
        handle_hotkey_binding(&mut self.binding_hotkey, &mut self.config, ctx);
        self.take_open_request(ctx);
        // Keep the clip list (and the nav counters) current on every page.
        crate::gui::views::library_view::refresh_clips(&mut self.cached_clips, &mut self.last_clip_refresh, &self.config.save_path);

        if self.last_status_poll.elapsed().as_millis() > 100 {
            self.last_status_poll = Instant::now();
            let latest = self
                .shared_status
                .try_lock()
                .ok()
                .and_then(|g| g.as_ref().cloned());
            if let Some(s) = latest {
                self.announce_status_change(&s);
                self.last_status = Some(s);
            }
        }
        ctx.request_repaint_after(Duration::from_millis(500));

        if let Ok(st) = crate::share::GLOBAL_SHARE_STATE.lock() {
            if st.is_sharing && st.public_url.is_none() {
                ctx.request_repaint_after(Duration::from_millis(80));
            }
        }

        let mut game_counts: Vec<(String, usize)> = Vec::new();
        for clip in &self.cached_clips {
            if let Some(entry) = game_counts.iter_mut().find(|(g, _)| *g == clip.game_tag) {
                entry.1 += 1;
            } else {
                game_counts.push((clip.game_tag.clone(), 1));
            }
        }

        let previous_tab = self.state.active_tab;

        let replay_hk_str = crate::gui::views::hotkey_view::format_hotkey(
            self.config.hotkey_replay,
            self.config.hotkey_replay_mod,
        );
        let record_hk_str = crate::gui::views::hotkey_view::format_hotkey(
            self.config.hotkey_record,
            self.config.hotkey_record_mod,
        );

        // Fullscreen viewing: only the video, and the window follows the player's state.
        let fullscreen = self.player_state.as_ref().is_some_and(|p| p.fullscreen);
        if fullscreen != self.window_fullscreen {
            self.window_fullscreen = fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(fullscreen));
        }
        if fullscreen {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(egui::Color32::BLACK))
                .show(ctx, |ui| render_fullscreen(&mut self.player_state, &self.theme, ui, ctx));
            return;
        }

        // Sidebar: dragged wider or narrower by its edge, or collapsed to icons.
        let max_sidebar = sidebar_max_width(ctx.screen_rect().width());
        let expanded_w = self.config.sidebar_width.clamp(SIDEBAR_MIN_WIDTH, max_sidebar);
        let collapse = ease_out(ctx.animate_bool_with_time(
            egui::Id::new("sidebar_collapse"),
            self.config.sidebar_collapsed,
            MEDIUM,
        ));
        let sidebar_w = expanded_w + (RAIL_COLLAPSED_WIDTH - expanded_w) * collapse;
        let mut sidebar_toggled = false;
        let sidebar = egui::SidePanel::left("m3_nav_rail")
            .resizable(false)
            .exact_width(sidebar_w)
            .frame(
                egui::Frame::default()
                    .fill(self.theme.surface_container_low)
                    .stroke(Stroke::new(1.0_f32, self.theme.outline_variant))
                    .inner_margin(Margin::symmetric(0.0, 12.0)),
            )
            .show(ctx, |ui| {
                // While it animates, nothing may spill past the edge.
                ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                let theme = self.theme;
                let status = self.last_status.as_ref();
                let is_recording = status.map(|s| s.recording).unwrap_or(false);
                let is_installed = status.map(|s| s.is_installed).unwrap_or(true);
                let waiting_for_game = status.map(|s| s.waiting_for_game).unwrap_or(false);
                sidebar_toggled = render_nav_rail(
                    ui,
                    &theme,
                    &mut self.state.active_tab,
                    &replay_hk_str,
                    is_recording,
                    is_installed,
                    waiting_for_game,
                    self.cached_clips.len(),
                    &game_counts,
                    &mut self.state.selected_game_filter,
                    self.config.sidebar_collapsed,
                    |cmd| {
                        dispatch_ipc(cmd);
                    },
                    |_new_tab| {
                        if let Some(mut player) = self.player_state.take() {
                            player.stop();
                        }
                    },
                );
            });
        if sidebar_toggled {
            self.config.sidebar_collapsed = !self.config.sidebar_collapsed;
            let _ = self.config.save();
        }
        self.sidebar_resize_handle(ctx, sidebar.response.rect, max_sidebar);

        if self.state.active_tab != previous_tab {
            if previous_tab == NavigationTab::Settings {
                let _ = self.config.save();
                Config::sync_autostart(self.config.autostart);
                dispatch_ipc(ipc::Command::ReloadConfig);
            }
            if let Some(mut player) = self.player_state.take() {
                player.stop();
            }
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(self.theme.background)
                    .inner_margin(Margin::same(20.0)),
            )
            .show(ctx, |ui| {
                let share_snapshot = crate::share::GLOBAL_SHARE_STATE
                    .lock()
                    .ok()
                    .map(|st| st.clone());

                // A share that stopped with a message failed; show it once as a toast.
                if let Some(ref st) = share_snapshot {
                    if !st.is_sharing {
                        if let Some(msg) = &st.status_message {
                            self.toasts.push(ToastKind::Error, msg.clone());
                            if let Ok(mut g) = crate::share::GLOBAL_SHARE_STATE.lock() {
                                g.status_message = None;
                            }
                        }
                    }
                }

                if let Some(ref share_state) = share_snapshot {
                    if share_state.is_sharing {
                        egui::Frame::none()
                            .fill(self.theme.surface_container_highest)
                            .stroke(Stroke::new(1.5_f32, self.theme.primary))
                            .rounding(M3Shapes::LARGE)
                            .inner_margin(Margin::same(12.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let (dot_rect, _) = ui.allocate_exact_size(
                                        Vec2::splat(12.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().circle_filled(
                                        dot_rect.center(),
                                        5.0,
                                        self.theme.success,
                                    );
                                    ui.add_space(6.0);

                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t("LIVE STREAM PROXY ACTIVE"))
                                                    .size(12.0)
                                                    .strong()
                                                    .color(self.theme.success),
                                            );
                                            if !share_state.filename.is_empty() {
                                                let meta_label =
                                                    if share_state.file_size_str.is_empty() {
                                                        format!("({})", share_state.filename)
                                                    } else {
                                                        format!(
                                                            "({} • {})",
                                                            share_state.filename,
                                                            share_state.file_size_str
                                                        )
                                                    };
                                                ui.label(
                                                    RichText::new(meta_label)
                                                        .size(12.0)
                                                        .color(self.theme.on_surface_variant),
                                                );
                                            }
                                        });

                                        if let Some(ref url) = share_state.public_url {
                                            ui.horizontal(|ui| {
                                                ui.hyperlink_to(
                                                    RichText::new(url)
                                                        .size(13.5)
                                                        .strong()
                                                        .color(self.theme.primary)
                                                        .underline(),
                                                    url,
                                                );
                                                ui.add_space(10.0);
                                                if ui
                                                    .button(RichText::new(t("Copy Link")).size(12.0))
                                                    .clicked()
                                                {
                                                    ui.output_mut(|o| {
                                                        o.copied_text = url.to_string()
                                                    });
                                                }
                                                if ui
                                                    .button(
                                                        RichText::new(t("Open in Browser")).size(12.0),
                                                    )
                                                    .clicked()
                                                {
                                                    let _ = open::that(url);
                                                }
                                            });
                                        } else {
                                            ui.horizontal(|ui| {
                                                ui.spinner();
                                                ui.label(
                                                    RichText::new(
                                                        t("Connecting secure public live tunnel..."),
                                                    )
                                                    .size(12.0)
                                                    .color(self.theme.outline),
                                                );
                                            });
                                        }
                                    });

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .button(
                                                    RichText::new(t("Stop Sharing"))
                                                        .size(12.0)
                                                        .color(self.theme.error),
                                                )
                                                .clicked()
                                            {
                                                crate::share::stop_live_share();
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(12.0);
                    }
                }

                if self.player_state.is_some() {
                    render_player(&mut self.player_state, &self.theme, &self.config.save_path, ui, ctx);
                } else {
                    match self.state.active_tab {
                        NavigationTab::Home => {
                            let status = self.last_status.as_ref();
                            render_home(
                                &mut self.config,
                                &mut self.cached_clips,
                                &mut self.last_clip_refresh,
                                &mut self.state.active_tab,
                                &mut self.state.selected_game_filter,
                                &mut self.state.mic_enabled,
                                &mut self.state.desktop_audio_enabled,
                                &mut self.player_state,
                                status,
                                &self.available_gpus,
                                &self.theme,
                                ui,
                                |cmd| {
                                    dispatch_ipc(cmd);
                                },
                            );
                        }
                        NavigationTab::Library => {
                            render_library(
                                &mut self.cached_clips,
                                &mut self.last_clip_refresh,
                                &self.config.save_path,
                                &mut self.state.search_query,
                                &mut self.state.selected_game_filter,
                                &mut self.player_state,
                                &replay_hk_str,
                                &self.theme,
                                ui,
                            );
                        }
                        NavigationTab::Settings => {
                            if self.available_monitors.is_empty() {
                                self.available_monitors = get_monitors();
                            }
                            let hardware = SettingsHardwareContext {
                                available_outputs: &self.available_outputs,
                                available_inputs: &self.available_inputs,
                                available_monitors: &self.available_monitors,
                                available_gpus: &self.available_gpus,
                                status: self.last_status.as_ref(),
                            };

                            let theme = self.theme;
                            render_settings(
                                &mut self.config,
                                hardware,
                                &mut self.binding_hotkey,
                                &mut self.palette_kind,
                                &theme,
                                &mut self.state.status_banner,
                                ui,
                                |cmd| {
                                    dispatch_ipc(cmd);
                                },
                            );
                        }
                        NavigationTab::About => {
                            render_about(&self.theme, &replay_hk_str, &record_hk_str, ui);
                        }
                    }
                }

                let page_rect = ui.clip_rect();
                page_fade_in(
                    ui,
                    &self.theme,
                    page_rect,
                    (self.state.active_tab, self.player_state.is_some()),
                );
            });

        use crate::gui::components::confirm::{render_delete_dialog, DeleteOutcome};
        match render_delete_dialog(ctx, &self.theme, &self.config.save_path) {
            Some(DeleteOutcome::Removed(name)) => {
                self.toasts.push(ToastKind::Info, format!("{} {}", t("Deleted"), name));
            }
            Some(DeleteOutcome::Failed(name)) => {
                self.toasts.push(ToastKind::Error, format!("{} {}", t("Could not delete"), name));
            }
            None => {}
        }

        if let Some(tab) = crate::gui::components::notice::render_setup_dialog(ctx, &self.theme) {
            if let Some(mut player) = self.player_state.take() {
                player.stop();
            }
            self.state.active_tab = NavigationTab::Settings;
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("m3_settings_tab_idx"), tab));
        }

        self.toasts.show(ctx, &self.theme);

        if crate::gui::views::welcome_view::render_welcome(ctx, &self.theme, &mut self.config, &self.available_gpus) {
            let _ = self.config.save();
            dispatch_ipc(ipc::Command::ReloadConfig);
        }
    }
}

impl LapseApp {
    /// Turns daemon status transitions into short toast notifications.
    fn announce_status_change(&mut self, new: &ipc::StatusResponse) {
        let Some(old) = self.last_status.as_ref() else {
            self.daemon_seen = true;
            if let Some(err) = &new.last_error {
                self.toasts.push(ToastKind::Error, format!("{}: {}", t("Recorder error"), err));
            }
            return;
        };

        if new.save_counter > old.save_counter {
            let name = new
                .last_saved
                .as_deref()
                .and_then(|p| std::path::Path::new(p).file_name())
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "clip".into());
            self.toasts.push(ToastKind::Success, format!("{} {}", t("Saved"), name));
            // Show the new clip right away.
            self.last_clip_refresh = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
        }
        if new.last_error.is_some() && new.last_error != old.last_error {
            self.toasts.push(
                ToastKind::Error,
                format!("{}: {}", t("Recorder error"), new.last_error.as_deref().unwrap_or_default()),
            );
        }
        if new.restarts > old.restarts && new.recording {
            self.toasts.push(ToastKind::Info, t("Recorder recovered automatically"));
        }
        if new.mode != old.mode {
            match new.mode.as_str() {
                "Replay" if old.mode == "None" => self.toasts.push(ToastKind::Info, t("Replay buffer started")),
                "Continuous" => self.toasts.push(ToastKind::Info, t("Recording started")),
                "None" if new.last_error.is_none() => self.toasts.push(ToastKind::Info, t("Recorder stopped")),
                _ => {}
            }
        }
    }
}
