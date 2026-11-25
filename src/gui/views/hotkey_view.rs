use eframe::egui;
use global_hotkey::hotkey::Code;
use crate::config::Config;
use crate::hotkeys::HotkeyEvent;
use crate::gui::i18n::t;

#[allow(dead_code)]
pub fn format_hotkey(code: Code, modifier_bits: u32) -> String {
    if code == Code::Unidentified {
        return t("Not set").to_string();
    }
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
        s.push_str(if cfg!(windows) { "Win + " } else { "Super + " });
    }
    let key_name = match code {
        Code::Minus => "-".into(),
        Code::Equal => "=".into(),
        Code::BracketLeft => "[".into(),
        Code::BracketRight => "]".into(),
        Code::Backslash => "\\".into(),
        Code::Semicolon => ";".into(),
        Code::Quote => "'".into(),
        Code::Comma => ",".into(),
        Code::Period => ".".into(),
        Code::Slash => "/".into(),
        Code::Backquote => "`".into(),
        _ => format!("{:?}", code).replace("Key", ""),
    };
    s.push_str(&key_name);
    s
}

/// Assigns code and modifier bits to target. Clears every other shortcut that had the
/// same key and modifier combo, and returns the list of cleared shortcuts.
pub fn bind_hotkey(
    config: &mut Config,
    target: HotkeyEvent,
    code: Code,
    mods: u32,
) -> Vec<HotkeyEvent> {
    // One key, one action: a clashing shortcut is left unset.
    let mut cleared = Vec::new();
    for other in HotkeyEvent::ALL {
        if other == target {
            continue;
        }
        let (key_field, mod_field) = config.hotkey_mut(other);
        if *key_field == code && *mod_field == mods {
            *key_field = Code::Unidentified;
            *mod_field = 0;
            cleared.push(other);
        }
    }
    let (key_field, mod_field) = config.hotkey_mut(target);
    *key_field = code;
    *mod_field = mods;
    cleared
}

/// Handles the key press while a shortcut is being rebound. Returns the shortcuts that
/// lost their key because they used the same one (empty when nothing clashed), or None
/// when nothing was bound this frame.
pub fn handle_hotkey_binding(binding: &mut Option<HotkeyEvent>, config: &mut Config, ctx: &egui::Context) -> Option<Vec<HotkeyEvent>> {
    let mut bound = None;
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
                        let cleared = bind_hotkey(config, target, code, mod_bits);
                        *binding = None;
                        bound = Some(cleared);
                        return;
                    }
                }
            }
        });
    }
    bound
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
        F13=>F13, F14=>F14, F15=>F15, F16=>F16, F17=>F17, F18=>F18, F19=>F19, F20=>F20,
        A=>KeyA, B=>KeyB, C=>KeyC, D=>KeyD, E=>KeyE, F=>KeyF, G=>KeyG, H=>KeyH, I=>KeyI, J=>KeyJ, K=>KeyK, L=>KeyL, M=>KeyM,
        N=>KeyN, O=>KeyO, P=>KeyP, Q=>KeyQ, R=>KeyR, S=>KeyS, T=>KeyT, U=>KeyU, V=>KeyV, W=>KeyW, X=>KeyX, Y=>KeyY, Z=>KeyZ,
        Num0=>Digit0, Num1=>Digit1, Num2=>Digit2, Num3=>Digit3, Num4=>Digit4, Num5=>Digit5, Num6=>Digit6, Num7=>Digit7, Num8=>Digit8, Num9=>Digit9,
        Space=>Space, Enter=>Enter, Backspace=>Backspace, Tab=>Tab, Insert=>Insert, Delete=>Delete,
        Home=>Home, End=>End, PageUp=>PageUp, PageDown=>PageDown,
        ArrowLeft=>ArrowLeft, ArrowRight=>ArrowRight, ArrowUp=>ArrowUp, ArrowDown=>ArrowDown,
        Minus=>Minus, Plus=>Equal, Equals=>Equal,
        OpenBracket=>BracketLeft, CloseBracket=>BracketRight,
        Backslash=>Backslash, Slash=>Slash,
        Semicolon=>Semicolon, Colon=>Semicolon,
        Backtick=>Backquote,
        Comma=>Comma, Period=>Period,
        Pipe=>Backslash, Questionmark=>Slash,
    }
}

#[allow(dead_code)]
pub fn map_egui_to_code(key: egui::Key) -> Code {
    try_map_egui_to_code(key).unwrap_or(Code::F10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_hotkey_clears_clash() {
        let mut config = Config::default();
        let (k, m) = config.hotkey_mut(HotkeyEvent::SaveReplay);
        *k = Code::KeyK;
        *m = 1 << 0;

        let cleared = bind_hotkey(&mut config, HotkeyEvent::ToggleRecord, Code::KeyK, 1 << 0);
        assert_eq!(cleared, vec![HotkeyEvent::SaveReplay]);

        let (k_prev, m_prev) = config.hotkey_mut(HotkeyEvent::SaveReplay);
        assert_eq!(*k_prev, Code::Unidentified);
        assert_eq!(*m_prev, 0);

        let (k_target, m_target) = config.hotkey_mut(HotkeyEvent::ToggleRecord);
        assert_eq!(*k_target, Code::KeyK);
        assert_eq!(*m_target, 1 << 0);
    }
}
