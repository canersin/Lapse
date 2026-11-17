//! Global shortcuts through the xdg-desktop-portal GlobalShortcuts interface.
//!
//! Unlike X11 key grabs, this works on Wayland no matter which window is focused.
//! GNOME and KDE show a one-time dialog asking the user to confirm the shortcuts.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use global_hotkey::hotkey::Code;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use super::HotkeyEvent;

const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const DESKTOP_PATH: &str = "/org/freedesktop/portal/desktop";
const APP_ID: &str = "lapse";

pub struct PortalShortcut {
    pub id: &'static str,
    pub description: &'static str,
    pub trigger: String,
    pub event: HotkeyEvent,
}

/// Bumped on every rebind so threads of an older session stop forwarding events.
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn is_available() -> bool {
    let Ok(conn) = Connection::session() else { return false };
    Proxy::new(&conn, DESTINATION, DESKTOP_PATH, "org.freedesktop.portal.GlobalShortcuts")
        .and_then(|p| p.get_property::<u32>("version"))
        .is_ok()
}

/// Starts a portal session in the background and forwards activations to `tx`.
/// Calling it again replaces the previous session.
pub fn bind(shortcuts: Vec<PortalShortcut>, tx: Sender<HotkeyEvent>, on_error: Arc<dyn Fn(String) + Send + Sync>) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::Builder::new()
        .name("lapse-portal-shortcuts".into())
        .spawn(move || {
            if let Err(e) = run(generation, shortcuts, tx) {
                on_error(e.to_string());
            }
        })
        .ok();
}

fn run(generation: u64, shortcuts: Vec<PortalShortcut>, tx: Sender<HotkeyEvent>) -> zbus::Result<()> {
    let conn = Connection::session()?;

    register_app(&conn);

    let portal = Proxy::new(&conn, DESTINATION, DESKTOP_PATH, "org.freedesktop.portal.GlobalShortcuts")?;

    let session = create_session(&conn, &portal, generation)?;
    let session_path = ObjectPath::try_from(session.as_str())?;

    // BindShortcuts (GNOME/KDE show a confirmation dialog the first time).
    let list: Vec<(String, HashMap<&str, Value>)> = shortcuts
        .iter()
        .map(|s| {
            let mut props = HashMap::new();
            props.insert("description", Value::from(s.description));
            props.insert("preferred_trigger", Value::from(s.trigger.clone()));
            (s.id.to_string(), props)
        })
        .collect();
    let bind_token = format!("lapse_bind_{}_{}", std::process::id(), generation);
    request(&conn, &bind_token, |opts| {
        portal
            .call_method("BindShortcuts", &(&session_path, &list, "", opts))
            .map(|_| ())
    })?;

    // Forward activations until a newer session replaces this one.
    let activations = portal.receive_signal("Activated")?;
    for msg in activations {
        if GENERATION.load(Ordering::SeqCst) != generation {
            break;
        }
        let Ok((sess, id, _ts, _opts)) =
            msg.body().deserialize::<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>)>()
        else {
            continue;
        };
        if sess.as_str() != session {
            continue;
        }
        if let Some(s) = shortcuts.iter().find(|s| s.id == id) {
            let _ = tx.send(s.event);
        }
    }

    if let Ok(sess) = Proxy::new(&conn, DESTINATION, session_path, "org.freedesktop.portal.Session") {
        let _ = sess.call_method("Close", &());
    }
    Ok(())
}

/// Unsandboxed apps must tell the portal who they are (xdg-desktop-portal >= 1.19),
/// before making any other portal call on the connection. The id matches lapse.desktop.
fn register_app(conn: &Connection) {
    if let Ok(registry) = Proxy::new(conn, DESTINATION, DESKTOP_PATH, "org.freedesktop.host.portal.Registry") {
        let _ = registry.call_method("Register", &(APP_ID, HashMap::<&str, Value>::new()));
    }
}

fn create_session(conn: &Connection, portal: &Proxy, generation: u64) -> zbus::Result<String> {
    let token = format!("lapse_{}_{}", std::process::id(), generation);
    let results = request(conn, &token, |mut opts| {
        let session_token = format!("lapse_session_{}_{}", std::process::id(), generation);
        opts.insert("session_handle_token", Value::from(session_token));
        portal.call_method("CreateSession", &(opts,)).map(|_| ())
    })?;
    results
        .get("session_handle")
        .and_then(|v| {
            String::try_from(v.try_clone().ok()?)
                .ok()
                .or_else(|| OwnedObjectPath::try_from(v.try_clone().ok()?).ok().map(|p| p.to_string()))
        })
        .ok_or_else(|| zbus::Error::Failure("portal returned no session handle".into()))
}

/// Performs a portal request: subscribes to the Request's `Response` signal, makes the
/// call and waits for the answer. Returns the response's results dictionary.
fn request(
    conn: &Connection,
    token: &str,
    call: impl FnOnce(HashMap<&str, Value>) -> zbus::Result<()>,
) -> zbus::Result<HashMap<String, OwnedValue>> {
    let sender = conn
        .unique_name()
        .map(|n| n.trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default();
    let path = format!("{}/request/{}/{}", DESKTOP_PATH, sender, token);
    let req = Proxy::new(conn, DESTINATION, path, "org.freedesktop.portal.Request")?;
    let mut responses = req.receive_signal("Response")?;

    let mut opts = HashMap::new();
    opts.insert("handle_token", Value::from(token.to_string()));
    call(opts)?;

    let msg = responses
        .next()
        .ok_or_else(|| zbus::Error::Failure("portal closed the request".into()))?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize()?;
    match code {
        0 => Ok(results),
        1 => Err(zbus::Error::Failure("shortcuts were not approved".into())),
        _ => Err(zbus::Error::Failure("portal request failed".into())),
    }
}

/// Converts a hotkey to the XDG shortcuts format, e.g. `CTRL+SHIFT+F10`.
pub fn trigger_string(code: Code, mods: u32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if mods & (1 << 1) != 0 {
        parts.push("CTRL".into());
    }
    if mods & (1 << 0) != 0 {
        parts.push("ALT".into());
    }
    if mods & (1 << 2) != 0 {
        parts.push("SHIFT".into());
    }
    if mods & (1 << 3) != 0 {
        parts.push("LOGO".into());
    }
    parts.push(key_name(code));
    parts.join("+")
}

/// xkb keysym name for a key code.
fn key_name(code: Code) -> String {
    let raw = format!("{:?}", code);
    if let Some(letter) = raw.strip_prefix("Key") {
        return letter.to_lowercase();
    }
    if let Some(digit) = raw.strip_prefix("Digit") {
        return digit.to_string();
    }
    match code {
        Code::Space => "space",
        Code::Enter => "Return",
        Code::Escape => "Escape",
        Code::Tab => "Tab",
        Code::Backspace => "BackSpace",
        Code::Insert => "Insert",
        Code::Delete => "Delete",
        Code::Home => "Home",
        Code::End => "End",
        Code::PageUp => "Page_Up",
        Code::PageDown => "Page_Down",
        Code::PrintScreen => "Print",
        Code::ScrollLock => "Scroll_Lock",
        Code::Pause => "Pause",
        Code::Minus => "minus",
        Code::Equal => "equal",
        Code::Backquote => "grave",
        Code::ArrowUp => "Up",
        Code::ArrowDown => "Down",
        Code::ArrowLeft => "Left",
        Code::ArrowRight => "Right",
        _ => return raw.replace("Numpad", "KP_"),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to the real desktop portal; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn creates_portal_session() {
        let conn = Connection::session().unwrap();
        register_app(&conn);
        let portal = Proxy::new(&conn, DESTINATION, DESKTOP_PATH, "org.freedesktop.portal.GlobalShortcuts").unwrap();
        let session = create_session(&conn, &portal, 999).unwrap();
        assert!(session.starts_with("/org/freedesktop/portal/desktop/session/"), "{}", session);
    }

    #[test]
    fn triggers_use_xdg_names() {
        assert_eq!(trigger_string(Code::F10, 0), "F10");
        assert_eq!(trigger_string(Code::KeyT, 1), "ALT+t");
        assert_eq!(trigger_string(Code::Digit5, (1 << 1) | (1 << 2)), "CTRL+SHIFT+5");
        assert_eq!(trigger_string(Code::PrintScreen, 1 << 3), "LOGO+Print");
    }
}
