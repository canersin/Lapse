//! Global shortcuts for Windows using RegisterHotKey and a thread message loop.
#![allow(dead_code)]

#[cfg(windows)]
use std::collections::HashMap;
#[cfg(windows)]
use std::sync::mpsc::{channel, Receiver, Sender};

use global_hotkey::hotkey::Code;
#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[cfg(windows)]
use super::HotkeyEvent;
#[cfg(windows)]
use crate::config::Config;

#[cfg(windows)]
extern "system" {
    fn GetCurrentThreadId() -> u32;
}

#[cfg(windows)]
enum Command {
    Register(Vec<(i32, u32, u32, HotkeyEvent, String)>),
    #[allow(dead_code)]
    Unregister,
    Stop,
}

#[cfg(windows)]
pub struct WindowsHotkeys {
    thread_id: u32,
    cmd_tx: Sender<Command>,
}

#[cfg(windows)]
impl WindowsHotkeys {
    pub fn new(tx: Sender<HotkeyEvent>) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Command>();
        let (ready_tx, ready_rx) = channel::<u32>();

        std::thread::Builder::new()
            .name("lapse-windows-hotkeys".into())
            .spawn(move || {
                run_message_loop(cmd_rx, ready_tx, tx);
            })
            .expect("failed to spawn hotkey thread");

        let thread_id = ready_rx.recv().unwrap_or(0);

        Self { thread_id, cmd_tx }
    }

    pub fn register_from_config(&self, cfg: &Config) {
        let mut bindings = Vec::new();
        for (i, (event, code, mods)) in cfg.hotkey_bindings().into_iter().enumerate() {
            if let Some(vk) = code_to_vk(code) {
                let win_mods = modifiers_to_win32(mods);
                let key_name = format_key(code, mods);
                bindings.push(((i + 1) as i32, win_mods, vk, event, key_name));
            }
        }
        let _ = self.cmd_tx.send(Command::Register(bindings));
        if self.thread_id != 0 {
            for _ in 0..10 {
                if unsafe { PostThreadMessageW(self.thread_id, WM_USER, 0, 0) } != 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    #[allow(dead_code)]
    pub fn unregister(&self) {
        let _ = self.cmd_tx.send(Command::Unregister);
        if self.thread_id != 0 {
            unsafe {
                PostThreadMessageW(self.thread_id, WM_USER, 0, 0);
            }
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsHotkeys {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Command::Stop);
        if self.thread_id != 0 {
            unsafe {
                PostThreadMessageW(self.thread_id, WM_USER, 0, 0);
                PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
            }
        }
    }
}

static LAST_HOTKEY_ERROR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[allow(dead_code)]
pub fn set_hotkey_error(err: Option<String>) {
    if let Ok(mut lock) = LAST_HOTKEY_ERROR.lock() {
        *lock = err.clone();
    }
    if let Some(proj) = directories::ProjectDirs::from("com", "lapse", "lapse") {
        let path = proj.config_dir().join("hotkey_error");
        if let Some(msg) = err {
            let _ = std::fs::create_dir_all(proj.config_dir());
            let _ = std::fs::write(&path, msg);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

pub fn last_hotkey_error() -> Option<String> {
    if let Ok(lock) = LAST_HOTKEY_ERROR.lock() {
        if let Some(ref err) = *lock {
            return Some(err.clone());
        }
    }
    if let Some(proj) = directories::ProjectDirs::from("com", "lapse", "lapse") {
        let path = proj.config_dir().join("hotkey_error");
        if let Ok(s) = std::fs::read_to_string(&path) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

#[allow(dead_code)]
pub fn format_key(code: Code, mods: u32) -> String {
    let mut s = String::new();
    if mods & (1 << 1) != 0 {
        s.push_str("Ctrl+");
    }
    if mods & (1 << 0) != 0 {
        s.push_str("Alt+");
    }
    if mods & (1 << 2) != 0 {
        s.push_str("Shift+");
    }
    if mods & (1 << 3) != 0 {
        s.push_str("Win+");
    }
    s.push_str(&format_code_name(code));
    s
}

pub fn format_code_name(code: Code) -> String {
    match code {
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
    }
}

#[cfg(windows)]
fn run_message_loop(cmd_rx: Receiver<Command>, ready_tx: Sender<u32>, event_tx: Sender<HotkeyEvent>) {
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    unsafe {
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }

    let tid = unsafe { GetCurrentThreadId() };
    let _ = ready_tx.send(tid);

    let mut registered: HashMap<i32, HotkeyEvent> = HashMap::new();

    let handle_commands = |registered: &mut HashMap<i32, HotkeyEvent>| -> bool {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                Command::Register(bindings) => {
                    for &id in registered.keys() {
                        unsafe {
                            UnregisterHotKey(std::ptr::null_mut(), id);
                        }
                    }
                    for id in 1..=10 {
                        unsafe {
                            UnregisterHotKey(std::ptr::null_mut(), id);
                        }
                    }
                    registered.clear();
                    let mut failure_msg = None;
                    for (id, mods, vk, event, name) in bindings {
                        let mut ok = unsafe { RegisterHotKey(std::ptr::null_mut(), id, mods, vk) } != 0;
                        if !ok && (mods & MOD_NOREPEAT != 0) {
                            ok = unsafe { RegisterHotKey(std::ptr::null_mut(), id, mods & !MOD_NOREPEAT, vk) } != 0;
                        }
                        if ok {
                            eprintln!("Hotkey registered: {}", name);
                            use std::io::Write;
                            let _ = std::io::stderr().flush();
                            registered.insert(id, event);
                        } else {
                            let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
                            let msg = if err == 1409 {
                                format!("Shortcut {} is already registered by another app (error 1409)", name)
                            } else {
                                format!("RegisterHotKey error for {}: {}", name, err)
                            };
                            eprintln!("Lapse: {}", msg);
                            use std::io::Write;
                            let _ = std::io::stderr().flush();
                            if failure_msg.is_none() {
                                failure_msg = Some(msg);
                            }
                        }
                    }
                    set_hotkey_error(failure_msg);
                }
                Command::Unregister => {
                    for &id in registered.keys() {
                        unsafe {
                            UnregisterHotKey(std::ptr::null_mut(), id);
                        }
                    }
                    for id in 1..=10 {
                        unsafe {
                            UnregisterHotKey(std::ptr::null_mut(), id);
                        }
                    }
                    registered.clear();
                    set_hotkey_error(None);
                }
                Command::Stop => {
                    for &id in registered.keys() {
                        unsafe {
                            UnregisterHotKey(std::ptr::null_mut(), id);
                        }
                    }
                    for id in 1..=10 {
                        unsafe {
                            UnregisterHotKey(std::ptr::null_mut(), id);
                        }
                    }
                    set_hotkey_error(None);
                    return false;
                }
            }
        }
        true
    };

    if !handle_commands(&mut registered) {
        return;
    }

    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        let _ = handle_commands(&mut registered);
        if msg.message == WM_HOTKEY {
            let id = msg.wParam as i32;
            if let Some(&event) = registered.get(&id) {
                let _ = event_tx.send(event);
            }
        } else if msg.message == WM_USER {
            if !handle_commands(&mut registered) {
                return;
            }
        }
    }
}

// Modifiers
pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;
pub const MOD_NOREPEAT: u32 = 0x4000;

// Standard Virtual Keys
pub const VK_BACK: u32 = 0x08;
pub const VK_TAB: u32 = 0x09;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_PAUSE: u32 = 0x13;
pub const VK_CAPITAL: u32 = 0x14;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_SPACE: u32 = 0x20;
pub const VK_PRIOR: u32 = 0x21;
pub const VK_NEXT: u32 = 0x22;
pub const VK_END: u32 = 0x23;
pub const VK_HOME: u32 = 0x24;
pub const VK_LEFT: u32 = 0x25;
pub const VK_UP: u32 = 0x26;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_DOWN: u32 = 0x28;
pub const VK_SNAPSHOT: u32 = 0x2C;
pub const VK_INSERT: u32 = 0x2D;
pub const VK_DELETE: u32 = 0x2E;

// Numpad
pub const VK_NUMPAD0: u32 = 0x60;
pub const VK_NUMPAD1: u32 = 0x61;
pub const VK_NUMPAD2: u32 = 0x62;
pub const VK_NUMPAD3: u32 = 0x63;
pub const VK_NUMPAD4: u32 = 0x64;
pub const VK_NUMPAD5: u32 = 0x65;
pub const VK_NUMPAD6: u32 = 0x66;
pub const VK_NUMPAD7: u32 = 0x67;
pub const VK_NUMPAD8: u32 = 0x68;
pub const VK_NUMPAD9: u32 = 0x69;
pub const VK_MULTIPLY: u32 = 0x6A;
pub const VK_ADD: u32 = 0x6B;
pub const VK_SEPARATOR: u32 = 0x6C;
pub const VK_SUBTRACT: u32 = 0x6D;
pub const VK_DECIMAL: u32 = 0x6E;
pub const VK_DIVIDE: u32 = 0x6F;

// Function keys F1 - F24
pub const VK_F1: u32 = 0x70;
pub const VK_F2: u32 = 0x71;
pub const VK_F3: u32 = 0x72;
pub const VK_F4: u32 = 0x73;
pub const VK_F5: u32 = 0x74;
pub const VK_F6: u32 = 0x75;
pub const VK_F7: u32 = 0x76;
pub const VK_F8: u32 = 0x77;
pub const VK_F9: u32 = 0x78;
pub const VK_F10: u32 = 0x79;
pub const VK_F11: u32 = 0x7A;
pub const VK_F12: u32 = 0x7B;
pub const VK_F13: u32 = 0x7C;
pub const VK_F14: u32 = 0x7D;
pub const VK_F15: u32 = 0x7E;
pub const VK_F16: u32 = 0x7F;
pub const VK_F17: u32 = 0x80;
pub const VK_F18: u32 = 0x81;
pub const VK_F19: u32 = 0x82;
pub const VK_F20: u32 = 0x83;
pub const VK_F21: u32 = 0x84;
pub const VK_F22: u32 = 0x85;
pub const VK_F23: u32 = 0x86;
pub const VK_F24: u32 = 0x87;

pub const VK_NUMLOCK: u32 = 0x90;
pub const VK_SCROLL: u32 = 0x91;
pub const VK_OEM_NEC_EQUAL: u32 = 0x92;

// OEM keys
pub const VK_OEM_1: u32 = 0xBA;
pub const VK_OEM_PLUS: u32 = 0xBB;
pub const VK_OEM_COMMA: u32 = 0xBC;
pub const VK_OEM_MINUS: u32 = 0xBD;
pub const VK_OEM_PERIOD: u32 = 0xBE;
pub const VK_OEM_2: u32 = 0xBF;
pub const VK_OEM_3: u32 = 0xC0;
pub const VK_OEM_4: u32 = 0xDB;
pub const VK_OEM_5: u32 = 0xDC;
pub const VK_OEM_6: u32 = 0xDD;
pub const VK_OEM_7: u32 = 0xDE;
pub const VK_OEM_102: u32 = 0xE2;

pub fn modifiers_to_win32(mods: u32) -> u32 {
    let mut flags = MOD_NOREPEAT;
    if mods & (1 << 0) != 0 {
        flags |= MOD_ALT;
    }
    if mods & (1 << 1) != 0 {
        flags |= MOD_CONTROL;
    }
    if mods & (1 << 2) != 0 {
        flags |= MOD_SHIFT;
    }
    if mods & (1 << 3) != 0 {
        flags |= MOD_WIN;
    }
    flags
}

pub fn code_to_vk(code: Code) -> Option<u32> {
    let vk = match code {
        Code::KeyA => 0x41,
        Code::KeyB => 0x42,
        Code::KeyC => 0x43,
        Code::KeyD => 0x44,
        Code::KeyE => 0x45,
        Code::KeyF => 0x46,
        Code::KeyG => 0x47,
        Code::KeyH => 0x48,
        Code::KeyI => 0x49,
        Code::KeyJ => 0x4A,
        Code::KeyK => 0x4B,
        Code::KeyL => 0x4C,
        Code::KeyM => 0x4D,
        Code::KeyN => 0x4E,
        Code::KeyO => 0x4F,
        Code::KeyP => 0x50,
        Code::KeyQ => 0x51,
        Code::KeyR => 0x52,
        Code::KeyS => 0x53,
        Code::KeyT => 0x54,
        Code::KeyU => 0x55,
        Code::KeyV => 0x56,
        Code::KeyW => 0x57,
        Code::KeyX => 0x58,
        Code::KeyY => 0x59,
        Code::KeyZ => 0x5A,
        Code::Digit0 => 0x30,
        Code::Digit1 => 0x31,
        Code::Digit2 => 0x32,
        Code::Digit3 => 0x33,
        Code::Digit4 => 0x34,
        Code::Digit5 => 0x35,
        Code::Digit6 => 0x36,
        Code::Digit7 => 0x37,
        Code::Digit8 => 0x38,
        Code::Digit9 => 0x39,
        Code::F1 => VK_F1,
        Code::F2 => VK_F2,
        Code::F3 => VK_F3,
        Code::F4 => VK_F4,
        Code::F5 => VK_F5,
        Code::F6 => VK_F6,
        Code::F7 => VK_F7,
        Code::F8 => VK_F8,
        Code::F9 => VK_F9,
        Code::F10 => VK_F10,
        Code::F11 => VK_F11,
        Code::F12 => VK_F12,
        Code::F13 => VK_F13,
        Code::F14 => VK_F14,
        Code::F15 => VK_F15,
        Code::F16 => VK_F16,
        Code::F17 => VK_F17,
        Code::F18 => VK_F18,
        Code::F19 => VK_F19,
        Code::F20 => VK_F20,
        Code::F21 => VK_F21,
        Code::F22 => VK_F22,
        Code::F23 => VK_F23,
        Code::F24 => VK_F24,
        Code::Space => VK_SPACE,
        Code::Enter => VK_RETURN,
        Code::Escape => VK_ESCAPE,
        Code::Tab => VK_TAB,
        Code::Backspace => VK_BACK,
        Code::Insert => VK_INSERT,
        Code::Delete => VK_DELETE,
        Code::Home => VK_HOME,
        Code::End => VK_END,
        Code::PageUp => VK_PRIOR,
        Code::PageDown => VK_NEXT,
        Code::PrintScreen => VK_SNAPSHOT,
        Code::ScrollLock => VK_SCROLL,
        Code::Pause => VK_PAUSE,
        Code::Minus => VK_OEM_MINUS,
        Code::Equal => VK_OEM_PLUS,
        Code::Backquote => VK_OEM_3,
        Code::BracketLeft => VK_OEM_4,
        Code::BracketRight => VK_OEM_6,
        Code::Backslash => VK_OEM_5,
        Code::IntlBackslash => VK_OEM_102,
        Code::Semicolon => VK_OEM_1,
        Code::Quote => VK_OEM_7,
        Code::Comma => VK_OEM_COMMA,
        Code::Period => VK_OEM_PERIOD,
        Code::Slash => VK_OEM_2,
        Code::ArrowUp => VK_UP,
        Code::ArrowDown => VK_DOWN,
        Code::ArrowLeft => VK_LEFT,
        Code::ArrowRight => VK_RIGHT,
        Code::Numpad0 => VK_NUMPAD0,
        Code::Numpad1 => VK_NUMPAD1,
        Code::Numpad2 => VK_NUMPAD2,
        Code::Numpad3 => VK_NUMPAD3,
        Code::Numpad4 => VK_NUMPAD4,
        Code::Numpad5 => VK_NUMPAD5,
        Code::Numpad6 => VK_NUMPAD6,
        Code::Numpad7 => VK_NUMPAD7,
        Code::Numpad8 => VK_NUMPAD8,
        Code::Numpad9 => VK_NUMPAD9,
        Code::NumpadAdd => VK_ADD,
        Code::NumpadSubtract => VK_SUBTRACT,
        Code::NumpadMultiply => VK_MULTIPLY,
        Code::NumpadDivide => VK_DIVIDE,
        Code::NumpadDecimal => VK_DECIMAL,
        Code::NumpadComma => VK_SEPARATOR,
        Code::NumpadEnter => VK_RETURN,
        Code::NumpadEqual => VK_OEM_NEC_EQUAL,
        Code::CapsLock => VK_CAPITAL,
        Code::NumLock => VK_NUMLOCK,
        _ => return None,
    };
    Some(vk)
}

pub fn key_name_to_vk(name: &str) -> Option<u32> {
    let s = name.trim();
    match s.to_ascii_uppercase().as_str() {
        "A" => Some(0x41), "B" => Some(0x42), "C" => Some(0x43), "D" => Some(0x44),
        "E" => Some(0x45), "F" => Some(0x46), "G" => Some(0x47), "H" => Some(0x48),
        "I" => Some(0x49), "J" => Some(0x4A), "K" => Some(0x4B), "L" => Some(0x4C),
        "M" => Some(0x4D), "N" => Some(0x4E), "O" => Some(0x4F), "P" => Some(0x50),
        "Q" => Some(0x51), "R" => Some(0x52), "S" => Some(0x53), "T" => Some(0x54),
        "U" => Some(0x55), "V" => Some(0x56), "W" => Some(0x57), "X" => Some(0x58),
        "Y" => Some(0x59), "Z" => Some(0x5A),
        "0" => Some(0x30), "1" => Some(0x31), "2" => Some(0x32), "3" => Some(0x33),
        "4" => Some(0x34), "5" => Some(0x35), "6" => Some(0x36), "7" => Some(0x37),
        "8" => Some(0x38), "9" => Some(0x39),
        "F1" => Some(VK_F1), "F2" => Some(VK_F2), "F3" => Some(VK_F3), "F4" => Some(VK_F4),
        "F5" => Some(VK_F5), "F6" => Some(VK_F6), "F7" => Some(VK_F7), "F8" => Some(VK_F8),
        "F9" => Some(VK_F9), "F10" => Some(VK_F10), "F11" => Some(VK_F11), "F12" => Some(VK_F12),
        "F13" => Some(VK_F13), "F14" => Some(VK_F14), "F15" => Some(VK_F15), "F16" => Some(VK_F16),
        "F17" => Some(VK_F17), "F18" => Some(VK_F18), "F19" => Some(VK_F19), "F20" => Some(VK_F20),
        "F21" => Some(VK_F21), "F22" => Some(VK_F22), "F23" => Some(VK_F23), "F24" => Some(VK_F24),
        "NUMPAD0" => Some(VK_NUMPAD0), "NUMPAD1" => Some(VK_NUMPAD1), "NUMPAD2" => Some(VK_NUMPAD2),
        "NUMPAD3" => Some(VK_NUMPAD3), "NUMPAD4" => Some(VK_NUMPAD4), "NUMPAD5" => Some(VK_NUMPAD5),
        "NUMPAD6" => Some(VK_NUMPAD6), "NUMPAD7" => Some(VK_NUMPAD7), "NUMPAD8" => Some(VK_NUMPAD8),
        "NUMPAD9" => Some(VK_NUMPAD9), "NUMPADADD" | "ADD" => Some(VK_ADD),
        "NUMPADSUBTRACT" | "SUBTRACT" => Some(VK_SUBTRACT),
        "NUMPADMULTIPLY" | "MULTIPLY" => Some(VK_MULTIPLY),
        "NUMPADDIVIDE" | "DIVIDE" => Some(VK_DIVIDE),
        "NUMPADDECIMAL" | "DECIMAL" => Some(VK_DECIMAL),
        "NUMPADCOMMA" => Some(VK_SEPARATOR),
        "NUMPADENTER" => Some(VK_RETURN),
        "NUMPADEQUAL" => Some(VK_OEM_NEC_EQUAL),
        "-" | "MINUS" => Some(VK_OEM_MINUS),
        "=" | "+" | "EQUAL" | "EQUALS" | "PLUS" => Some(VK_OEM_PLUS),
        "[" | "BRACKETLEFT" | "OPENBRACKET" => Some(VK_OEM_4),
        "]" | "BRACKETRIGHT" | "CLOSEBRACKET" => Some(VK_OEM_6),
        ";" | "SEMICOLON" => Some(VK_OEM_1),
        "'" | "QUOTE" | "APOSTROPHE" => Some(VK_OEM_7),
        "," | "COMMA" => Some(VK_OEM_COMMA),
        "." | "PERIOD" => Some(VK_OEM_PERIOD),
        "/" | "SLASH" => Some(VK_OEM_2),
        "\\" | "BACKSLASH" => Some(VK_OEM_5),
        "`" | "BACKQUOTE" | "BACKTICK" | "GRAVE" => Some(VK_OEM_3),
        "ARROWUP" | "UP" => Some(VK_UP),
        "ARROWDOWN" | "DOWN" => Some(VK_DOWN),
        "ARROWLEFT" | "LEFT" => Some(VK_LEFT),
        "ARROWRIGHT" | "RIGHT" => Some(VK_RIGHT),
        "HOME" => Some(VK_HOME), "END" => Some(VK_END),
        "PAGEUP" | "PGUP" | "PRIOR" => Some(VK_PRIOR),
        "PAGEDOWN" | "PGDN" | "NEXT" => Some(VK_NEXT),
        "INSERT" | "INS" => Some(VK_INSERT),
        "DELETE" | "DEL" => Some(VK_DELETE),
        "PRINTSCREEN" | "PRTSC" | "SNAPSHOT" => Some(VK_SNAPSHOT),
        "PAUSE" => Some(VK_PAUSE),
        "SPACE" => Some(VK_SPACE),
        "ENTER" | "RETURN" => Some(VK_RETURN),
        "ESCAPE" | "ESC" => Some(VK_ESCAPE),
        "TAB" => Some(VK_TAB),
        "BACKSPACE" => Some(VK_BACK),
        "CAPSLOCK" => Some(VK_CAPITAL),
        "SCROLLLOCK" => Some(VK_SCROLL),
        "NUMLOCK" => Some(VK_NUMLOCK),
        _ => {
            if let Some(rest) = s.strip_prefix("Key") {
                return key_name_to_vk(rest);
            }
            if let Some(rest) = s.strip_prefix("Digit") {
                return key_name_to_vk(rest);
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_letters_to_vk() {
        assert_eq!(code_to_vk(Code::KeyA), Some(0x41));
        assert_eq!(code_to_vk(Code::KeyZ), Some(0x5A));
        assert_eq!(key_name_to_vk("A"), Some(0x41));
        assert_eq!(key_name_to_vk("z"), Some(0x5A));
        assert_eq!(key_name_to_vk("KeyA"), Some(0x41));
    }

    #[test]
    fn test_digits_to_vk() {
        assert_eq!(code_to_vk(Code::Digit0), Some(0x30));
        assert_eq!(code_to_vk(Code::Digit9), Some(0x39));
        assert_eq!(key_name_to_vk("0"), Some(0x30));
        assert_eq!(key_name_to_vk("9"), Some(0x39));
        assert_eq!(key_name_to_vk("Digit5"), Some(0x35));
    }

    #[test]
    fn test_function_keys_f1_to_f24() {
        assert_eq!(code_to_vk(Code::F1), Some(0x70));
        assert_eq!(code_to_vk(Code::F12), Some(0x7B));
        assert_eq!(code_to_vk(Code::F13), Some(0x7C));
        assert_eq!(code_to_vk(Code::F24), Some(0x87));
        assert_eq!(key_name_to_vk("F1"), Some(0x70));
        assert_eq!(key_name_to_vk("F12"), Some(0x7B));
        assert_eq!(key_name_to_vk("F13"), Some(0x7C));
        assert_eq!(key_name_to_vk("F24"), Some(0x87));
    }

    #[test]
    fn test_numpad_keys() {
        assert_eq!(code_to_vk(Code::Numpad0), Some(0x60));
        assert_eq!(code_to_vk(Code::Numpad9), Some(0x69));
        assert_eq!(code_to_vk(Code::NumpadAdd), Some(0x6B));
        assert_eq!(code_to_vk(Code::NumpadSubtract), Some(0x6D));
        assert_eq!(code_to_vk(Code::NumpadMultiply), Some(0x6A));
        assert_eq!(code_to_vk(Code::NumpadDivide), Some(0x6F));
        assert_eq!(code_to_vk(Code::NumpadDecimal), Some(0x6E));
        assert_eq!(code_to_vk(Code::NumpadComma), Some(0x6C));
        assert_eq!(code_to_vk(Code::NumpadEnter), Some(0x0D));
        assert_eq!(code_to_vk(Code::NumpadEqual), Some(0x92));
        assert_eq!(key_name_to_vk("Numpad0"), Some(0x60));
        assert_eq!(key_name_to_vk("NumpadAdd"), Some(0x6B));
        assert_eq!(key_name_to_vk("NumpadSubtract"), Some(0x6D));
    }

    #[test]
    fn test_oem_keys() {
        assert_eq!(code_to_vk(Code::Minus), Some(0xBD));
        assert_eq!(code_to_vk(Code::Equal), Some(0xBB));
        assert_eq!(code_to_vk(Code::BracketLeft), Some(0xDB));
        assert_eq!(code_to_vk(Code::BracketRight), Some(0xDD));
        assert_eq!(code_to_vk(Code::Semicolon), Some(0xBA));
        assert_eq!(code_to_vk(Code::Quote), Some(0xDE));
        assert_eq!(code_to_vk(Code::Comma), Some(0xBC));
        assert_eq!(code_to_vk(Code::Period), Some(0xBE));
        assert_eq!(code_to_vk(Code::Slash), Some(0xBF));
        assert_eq!(code_to_vk(Code::Backslash), Some(0xDC));
        assert_eq!(code_to_vk(Code::IntlBackslash), Some(0xE2));
        assert_eq!(code_to_vk(Code::Backquote), Some(0xC0));

        assert_eq!(key_name_to_vk("-"), Some(0xBD));
        assert_eq!(key_name_to_vk("Minus"), Some(0xBD));
        assert_eq!(key_name_to_vk("="), Some(0xBB));
        assert_eq!(key_name_to_vk("Equal"), Some(0xBB));
        assert_eq!(key_name_to_vk("["), Some(0xDB));
        assert_eq!(key_name_to_vk("BracketLeft"), Some(0xDB));
        assert_eq!(key_name_to_vk("]"), Some(0xDD));
        assert_eq!(key_name_to_vk(";"), Some(0xBA));
        assert_eq!(key_name_to_vk("'"), Some(0xDE));
        assert_eq!(key_name_to_vk(","), Some(0xBC));
        assert_eq!(key_name_to_vk("."), Some(0xBE));
        assert_eq!(key_name_to_vk("/"), Some(0xBF));
        assert_eq!(key_name_to_vk("\\"), Some(0xDC));
        assert_eq!(key_name_to_vk("`"), Some(0xC0));
    }

    #[test]
    fn test_navigation_and_control_keys() {
        assert_eq!(code_to_vk(Code::ArrowUp), Some(0x26));
        assert_eq!(code_to_vk(Code::ArrowDown), Some(0x28));
        assert_eq!(code_to_vk(Code::ArrowLeft), Some(0x25));
        assert_eq!(code_to_vk(Code::ArrowRight), Some(0x27));
        assert_eq!(code_to_vk(Code::Home), Some(0x24));
        assert_eq!(code_to_vk(Code::End), Some(0x23));
        assert_eq!(code_to_vk(Code::PageUp), Some(0x21));
        assert_eq!(code_to_vk(Code::PageDown), Some(0x22));
        assert_eq!(code_to_vk(Code::Insert), Some(0x2D));
        assert_eq!(code_to_vk(Code::Delete), Some(0x2E));
        assert_eq!(code_to_vk(Code::PrintScreen), Some(0x2C));
        assert_eq!(code_to_vk(Code::Pause), Some(0x13));
        assert_eq!(code_to_vk(Code::Space), Some(0x20));
        assert_eq!(code_to_vk(Code::Enter), Some(0x0D));
        assert_eq!(code_to_vk(Code::Escape), Some(0x1B));
        assert_eq!(code_to_vk(Code::Tab), Some(0x09));
        assert_eq!(code_to_vk(Code::Backspace), Some(0x08));

        assert_eq!(key_name_to_vk("ArrowUp"), Some(0x26));
        assert_eq!(key_name_to_vk("Home"), Some(0x24));
        assert_eq!(key_name_to_vk("PrintScreen"), Some(0x2C));
        assert_eq!(key_name_to_vk("Pause"), Some(0x13));
    }

    #[test]
    fn test_modifiers() {
        let alt = 1 << 0;
        let ctrl = 1 << 1;
        let shift = 1 << 2;
        let win = 1 << 3;

        let flags = modifiers_to_win32(ctrl);
        assert_eq!(flags, MOD_CONTROL | MOD_NOREPEAT);

        let flags_all = modifiers_to_win32(alt | ctrl | shift | win);
        assert_eq!(flags_all, MOD_ALT | MOD_CONTROL | MOD_SHIFT | MOD_WIN | MOD_NOREPEAT);
    }
}
