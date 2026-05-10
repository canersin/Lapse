use eframe::egui::{self, RichText, Color32, Vec2};
use rfd::FileDialog;
use crate::ipc;
use super::app::LapseApp;

pub fn render(app: &mut LapseApp, ui: &mut egui::Ui) {
    ui.heading(RichText::new("Settings").size(24.0).strong().color(Color32::WHITE));
    ui.add_space(20.0);

    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        egui::Grid::new("sets").num_columns(2).spacing([40.0, 30.0]).show(ui, |ui| {
            ui.label(RichText::new("Save Folder").size(16.0));
            ui.horizontal(|ui| {
                ui.label(app.config.save_path.to_string_lossy().to_string());
                if ui.button("Browse").clicked() { if let Some(path) = FileDialog::new().pick_folder() { app.config.save_path = path; } }
            });
            ui.end_row();

            ui.label(RichText::new("Monitor").size(16.0));
            egui::ComboBox::from_id_source("mon").selected_text(&app.config.monitor).width(300.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut app.config.monitor, "screen".into(), "All Screens");
                for m in &app.available_monitors {
                    ui.selectable_value(&mut app.config.monitor, m.name.clone(), format!("{} ({})", m.name, m.resolution));
                }
            });
            ui.end_row();

            ui.label(RichText::new("Encoding GPU").size(16.0));
            egui::ComboBox::from_id_source("gpu").selected_text(&app.config.gpu).width(300.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut app.config.gpu, "Auto".into(), "Auto");
                for g in &app.available_gpus {
                    ui.selectable_value(&mut app.config.gpu, g.pci_id.clone(), g.name.clone());
                }
            });
            ui.end_row();
            
            ui.label(RichText::new("Resolution").size(16.0));
            egui::ComboBox::from_id_source("res").selected_text(&app.config.resolution).width(200.0).show_ui(ui, |ui| {
                for r in &["Native", "3840x2160", "2560x1440", "1920x1080", "1280x720"] {
                    ui.selectable_value(&mut app.config.resolution, r.to_string(), *r);
                }
            });
            ui.end_row();

            ui.label(RichText::new("Framerate (FPS)").size(16.0)); ui.add(egui::DragValue::new(&mut app.config.fps)); ui.end_row();
            ui.label(RichText::new("Replay (Secs)").size(16.0)); ui.add(egui::DragValue::new(&mut app.config.replay_seconds)); ui.end_row();
            
            ui.label(RichText::new("Microphone (Input)").size(16.0));
            egui::ComboBox::from_id_source("ainp").selected_text(&app.config.audio_input).width(300.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut app.config.audio_input, "None".into(), "None");
                for d in &app.available_inputs { ui.selectable_value(&mut app.config.audio_input, d.name.clone(), &d.description); }
            });
            ui.end_row();

            ui.label(RichText::new("Speaker (Output)").size(16.0));
            egui::ComboBox::from_id_source("aout").selected_text(&app.config.audio_output).width(300.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut app.config.audio_output, "None".into(), "None");
                for d in &app.available_outputs { ui.selectable_value(&mut app.config.audio_output, d.name.clone(), &d.description); }
            });
            ui.end_row();

            ui.label(RichText::new("Save Hotkey").size(16.0));
            let h_text = format_hotkey(app);
            let btn_text = if app.binding_hotkey { "🔴 Press any key..." } else { &h_text };
            if ui.add(egui::Button::new(RichText::new(btn_text).strong())).clicked() {
                app.binding_hotkey = true;
            }
            ui.end_row();
        });
        
        ui.add_space(40.0);
        if ui.add_sized(Vec2::new(200.0, 45.0), egui::Button::new(RichText::new("💾 Save Settings & Restart").size(14.0).strong().color(Color32::BLACK)).fill(Color32::from_rgb(100, 255, 100)).rounding(8.0)).clicked() {
            let _ = app.config.save();
            let _ = app.send_command(ipc::Command::ReloadConfig);
            let _ = app.send_command(ipc::Command::Stop);
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = app.send_command(ipc::Command::StartReplay);
        }
    });
}

fn format_hotkey(app: &LapseApp) -> String {
    let mut s = String::new();
    let m = app.config.hotkey_replay_mod;
    if m & (1 << 1) != 0 { s.push_str("Ctrl + "); }
    if m & (1 << 0) != 0 { s.push_str("Alt + "); }
    if m & (1 << 2) != 0 { s.push_str("Shift + "); }
    if m & (1 << 3) != 0 { s.push_str("Super + "); }
    s.push_str(&format!("{:?}", app.config.hotkey_replay).replace("Key", ""));
    s
}

pub fn handle_hotkey_binding(app: &mut LapseApp, ctx: &egui::Context) {
    if app.binding_hotkey {
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                    app.config.hotkey_replay = map_egui_to_code(*key);
                    let mut mod_bits = 0u32;
                    if modifiers.alt { mod_bits |= 1 << 0; }
                    if modifiers.ctrl { mod_bits |= 1 << 1; }
                    if modifiers.shift { mod_bits |= 1 << 2; }
                    if modifiers.mac_cmd || modifiers.command { mod_bits |= 1 << 3; }
                    app.config.hotkey_replay_mod = mod_bits;
                    app.binding_hotkey = false;
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
