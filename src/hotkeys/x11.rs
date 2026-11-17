//! X11 key grabs via the `global-hotkey` crate. On Wayland these only fire while an
//! XWayland window (most Proton games) is focused; the portal backend covers the rest.

use super::HotkeyEvent;
use crate::config::Config;
use global_hotkey::hotkey::{HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;

/// Xlib's default error handler exits the whole process. Grabbing a key that another
/// app already grabbed (BadAccess, e.g. F10 taken by another recorder) must not kill the
/// daemon, so X errors are only logged. The handler is process-wide, which also covers
/// the connection `global-hotkey` opens.
fn install_x_error_handler() {
    use std::ffi::{c_int, c_uchar, c_ulong, c_void};

    #[repr(C)]
    struct XErrorEvent {
        kind: c_int,
        display: *mut c_void,
        resourceid: c_ulong,
        serial: c_ulong,
        error_code: c_uchar,
        request_code: c_uchar,
        minor_code: c_uchar,
    }
    type Handler = unsafe extern "C" fn(*mut c_void, *mut XErrorEvent) -> c_int;

    unsafe extern "C" fn log_error(_display: *mut c_void, event: *mut XErrorEvent) -> c_int {
        if let Some(e) = event.as_ref() {
            // 10 = BadAccess, 33 = X_GrabKey: the shortcut is already taken.
            if e.error_code == 10 && e.request_code == 33 {
                eprintln!("Lapse: a shortcut key is already grabbed by another app (X11); the Wayland portal can still deliver it");
            } else {
                eprintln!("Lapse: X11 error {} (request {})", e.error_code, e.request_code);
            }
        }
        0
    }

    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        let lib = libc::dlopen(c"libX11.so.6".as_ptr(), libc::RTLD_LAZY | libc::RTLD_GLOBAL);
        if lib.is_null() {
            return;
        }
        let sym = libc::dlsym(lib, c"XSetErrorHandler".as_ptr());
        if sym.is_null() {
            return;
        }
        let set_handler: unsafe extern "C" fn(Option<Handler>) -> Option<Handler> = std::mem::transmute(sym);
        set_handler(Some(log_error));
    });
}

/// Whether an X server answers. global_hotkey crashes (not errors) without one, e.g. on
/// a Wayland session without XWayland.
fn x_display_available() -> bool {
    if std::env::var_os("DISPLAY").map(|d| d.is_empty()).unwrap_or(true) {
        return false;
    }
    unsafe {
        let lib = libc::dlopen(c"libX11.so.6".as_ptr(), libc::RTLD_LAZY | libc::RTLD_GLOBAL);
        if lib.is_null() {
            return false;
        }
        let open = libc::dlsym(lib, c"XOpenDisplay".as_ptr());
        let close = libc::dlsym(lib, c"XCloseDisplay".as_ptr());
        if open.is_null() || close.is_null() {
            return false;
        }
        let open: unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_void = std::mem::transmute(open);
        let close: unsafe extern "C" fn(*mut std::ffi::c_void) -> std::ffi::c_int = std::mem::transmute(close);
        let display = open(std::ptr::null());
        if display.is_null() {
            return false;
        }
        close(display);
        true
    }
}

pub struct X11Hotkeys {
    manager: Arc<GlobalHotKeyManager>,
    hotkey_ids: Arc<Mutex<Vec<(u32, HotkeyEvent)>>>,
    registered_keys: Mutex<Vec<HotKey>>,
}

impl X11Hotkeys {
    /// Returns None when no X server is reachable.
    pub fn new(tx_hotkey: mpsc::Sender<HotkeyEvent>) -> Option<Self> {
        if !x_display_available() {
            eprintln!("Lapse: no X server; X11 shortcuts are off (the Wayland portal or gamepad still work)");
            return None;
        }
        install_x_error_handler();
        let manager = Arc::new(GlobalHotKeyManager::new().ok()?);
        let hotkey_ids = Arc::new(Mutex::new(Vec::<(u32, HotkeyEvent)>::new()));
        let hotkey_ids_clone = Arc::clone(&hotkey_ids);
        let registered_keys = Mutex::new(Vec::new());

        std::thread::spawn(move || {
            let receiver = GlobalHotKeyEvent::receiver();
            while let Ok(event) = receiver.recv() {
                if event.state == HotKeyState::Released {
                    continue;
                }

                let ids = hotkey_ids_clone.lock().unwrap_or_else(|e| e.into_inner());
                if let Some((_, ev)) = ids.iter().find(|(id, _)| *id == event.id) {
                    let _ = tx_hotkey.send(*ev);
                }
            }
        });

        Some(Self {
            manager,
            hotkey_ids,
            registered_keys,
        })
    }

    pub fn unregister(&self) {
        if let Ok(mut current) = self.registered_keys.lock() {
            if !current.is_empty() {
                let _ = self.manager.unregister_all(&current);
                current.clear();
            }
        }
    }

    pub fn register_from_config(&self, cfg: &Config) {
        let keys: Vec<(HotKey, HotkeyEvent)> = cfg
            .hotkey_bindings()
            .into_iter()
            .map(|(event, code, mods)| (HotKey::new(Self::get_mod(mods), code), event))
            .collect();

        if let Ok(mut current) = self.registered_keys.lock() {
            if !current.is_empty() {
                let _ = self.manager.unregister_all(&current);
                current.clear();
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
            for (hk, _) in &keys {
                if !current.contains(hk) {
                    let _ = self.manager.register(*hk);
                    current.push(*hk);
                }
            }
        }

        if let Ok(mut ids) = self.hotkey_ids.lock() {
            *ids = keys.iter().map(|(hk, ev)| (hk.id(), *ev)).collect();
        }
    }

    pub fn get_mod(bits: u32) -> Option<Modifiers> {
        let mut m = Modifiers::empty();
        if bits & (1 << 0) != 0 { m |= Modifiers::ALT; }
        if bits & (1 << 1) != 0 { m |= Modifiers::CONTROL; }
        if bits & (1 << 2) != 0 { m |= Modifiers::SHIFT; }
        if bits & (1 << 3) != 0 { m |= Modifiers::SUPER; }
        if m.is_empty() { None } else { Some(m) }
    }
}
