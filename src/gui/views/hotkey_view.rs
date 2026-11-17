use eframe::egui;
use global_hotkey::hotkey::Code;
use crate::config::Config;
use crate::hotkeys::HotkeyEvent;

#[allow(dead_code)]
pub fn format_hotkey(code: Code, modifier_bits: u32) -> String {
    let mut s = String::new();
    if modifier_bits & (1 << 1) != 0 {
        s.push_str("Ctrl + ");
    }
    if modifier_bits & (1 << 0) != 0 {
        s.push_str("Alt + ");
    }
    if modifier_bits & (1 << 2) != 0 {
        s.push_str("Shift + ");
    }
    if modifier_bits & (1 << 3) != 0 {
        s.push_str("Super + ");
    }
    s.push_str(&format!("{:?}", code).replace("Key", ""));
    s
}

pub fn handle_hotkey_binding(binding: &mut Option<HotkeyEvent>, config: &mut Config, ctx: &egui::Context) {
    if let Some(target) = *binding {
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if *key == egui::Key::Escape {
                        *binding = None;
                        return;
                    }

                    if let Some(code) = try_map_egui_to_code(*key) {
                        let mut mod_bits = 0u32;
                        if modifiers.alt {
                            mod_bits |= 1 << 0;
                        }
                        if modifiers.ctrl {
                            mod_bits |= 1 << 1;
                        }
                        if modifiers.shift {
                            mod_bits |= 1 << 2;
                        }
                        // `command` is Ctrl outside macOS, so only the real Cmd key counts as Super.
                        if modifiers.mac_cmd {
                            mod_bits |= 1 << 3;
                        }
                        let (key_field, mod_field) = config.hotkey_mut(target);
                        *key_field = code;
                        *mod_field = mod_bits;
                        *binding = None;
                        return;
                    }
                }
            }
        });
    }
}

pub fn try_map_egui_to_code(key: egui::Key) -> Option<Code> {
    use egui::Key as K;
    use global_hotkey::hotkey::Code as C;

    macro_rules! map_keys {
        ($($egui_k:ident => $ghk_k:ident),* $(,)?) => {
            match key {
                $(K::$egui_k => Some(C::$ghk_k),)*
                _ => None,
            }
        };
    }

    map_keys! {
        F1=>F1, F2=>F2, F3=>F3, F4=>F4, F5=>F5, F6=>F6, F7=>F7, F8=>F8, F9=>F9, F10=>F10, F11=>F11, F12=>F12,
        A=>KeyA, B=>KeyB, C=>KeyC, D=>KeyD, E=>KeyE, F=>KeyF, G=>KeyG, H=>KeyH, I=>KeyI, J=>KeyJ, K=>KeyK, L=>KeyL, M=>KeyM,
        N=>KeyN, O=>KeyO, P=>KeyP, Q=>KeyQ, R=>KeyR, S=>KeyS, T=>KeyT, U=>KeyU, V=>KeyV, W=>KeyW, X=>KeyX, Y=>KeyY, Z=>KeyZ,
        Num0=>Digit0, Num1=>Digit1, Num2=>Digit2, Num3=>Digit3, Num4=>Digit4, Num5=>Digit5, Num6=>Digit6, Num7=>Digit7, Num8=>Digit8, Num9=>Digit9,
        Space=>Space, Enter=>Enter, Backspace=>Backspace, Tab=>Tab, Insert=>Insert, Delete=>Delete,
        Home=>Home, End=>End, PageUp=>PageUp, PageDown=>PageDown,
        ArrowLeft=>ArrowLeft, ArrowRight=>ArrowRight, ArrowUp=>ArrowUp, ArrowDown=>ArrowDown,
    }
}

#[allow(dead_code)]
pub fn map_egui_to_code(key: egui::Key) -> Code {
    try_map_egui_to_code(key).unwrap_or(Code::F10)
}
