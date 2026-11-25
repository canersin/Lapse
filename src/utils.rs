use std::process::{Command, Output};
#[cfg(windows)]
use libc as _;
#[cfg(target_os = "linux")]
use std::process::Stdio;

pub fn is_inside_container() -> bool {
    std::path::Path::new("/run/.containerenv").exists()
}

/// The desktop's notification service, through D-Bus.
#[cfg(target_os = "linux")]
fn notifications() -> Option<zbus::blocking::Proxy<'static>> {
    let conn = zbus::blocking::Connection::session().ok()?;
    zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .ok()
}

#[cfg(target_os = "linux")]
pub fn has_notification_server() -> bool {
    static AVAILABLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    let current = AVAILABLE.load(std::sync::atomic::Ordering::Relaxed);
    if current == 1 {
        return true;
    }
    if current == 2 {
        return false;
    }
    let has = zbus::blocking::Connection::session()
        .ok()
        .and_then(|conn| zbus::blocking::fdo::DBusProxy::new(&conn).ok())
        .and_then(|dbus| dbus.name_has_owner("org.freedesktop.Notifications".try_into().ok()?).ok())
        .unwrap_or(false);
    AVAILABLE.store(if has { 1 } else { 2 }, std::sync::atomic::Ordering::Relaxed);
    has
}

#[cfg(not(target_os = "linux"))]
pub fn has_notification_server() -> bool {
    false
}

/// Shows a notification and returns its id.
#[cfg(target_os = "linux")]
fn notify(proxy: &zbus::blocking::Proxy<'_>, summary: &str, body: &str, icon: &str, actions: &[&str], timeout_ms: i32) -> Option<u32> {
    let hints: std::collections::HashMap<&str, zbus::zvariant::Value> = std::collections::HashMap::new();
    proxy
        .call("Notify", &("Lapse", 0u32, icon, summary, body, actions, hints, timeout_ms))
        .ok()
}

#[cfg(target_os = "linux")]
pub fn send_notification(summary: &str, body: &str, icon: &str) {
    if cfg!(test) || !has_notification_server() {
        return;
    }
    let (s, b, ic) = (summary.to_string(), body.to_string(), icon.to_string());
    std::thread::spawn(move || {
        if let Some(proxy) = notifications() {
            notify(&proxy, &s, &b, &ic, &[], 1500);
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn send_notification(summary: &str, body: &str, _icon: &str) {
    eprintln!("[notification] {summary}: {body}");
}

/// File the GUI watches: a clip path written here opens in the trimmer.
pub fn open_request_path() -> std::path::PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("lapse-open-request")
}

/// Asks the GUI to open a clip in the trimmer, starting the GUI if it isn't running.
#[cfg(target_os = "linux")]
pub fn request_trim(clip: &std::path::Path) {
    let req = open_request_path();
    if std::fs::write(&req, clip.to_string_lossy().as_bytes()).is_err() {
        return;
    }
    // A running GUI picks it up within a second; otherwise start one, which reads it.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    if req.exists() {
        if let Ok(exe) = std::env::current_exe() {
            let _ = Command::new(exe).arg("--gui").stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        }
    }
}

/// "Clip saved" notification with Open and Trim buttons (when the desktop supports them).
#[cfg(target_os = "linux")]
pub fn send_clip_notification(summary: &str, body: &str, clip: std::path::PathBuf) {
    if !has_notification_server() {
        return;
    }
    let (s, b) = (summary.to_string(), body.to_string());
    std::thread::spawn(move || {
        let Some(proxy) = notifications() else { return };
        // Listen before showing it, so a quick click isn't missed.
        let Ok(signals) = proxy.receive_all_signals() else { return };
        let Some(id) = notify(
            &proxy,
            &s,
            &b,
            "media-record",
            &["keep15", "Keep last 15 s", "keep30", "Keep last 30 s", "open", "Open", "trim", "Trim"],
            6000,
        ) else {
            return;
        };

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for msg in signals {
                let header = msg.header();
                let result = match header.member().map(|m| m.as_str()) {
                    Some("ActionInvoked") => msg.body().deserialize::<(u32, String)>().ok().map(|(nid, a)| (nid, Some(a))),
                    Some("NotificationClosed") => msg.body().deserialize::<(u32, u32)>().ok().map(|(nid, _)| (nid, None)),
                    _ => None,
                };
                if let Some((nid, action)) = result {
                    if nid == id {
                        let _ = tx.send(action);
                        return;
                    }
                }
            }
        });
        // Some desktops keep notifications with buttons around; don't wait forever.
        match rx.recv_timeout(std::time::Duration::from_secs(120)) {
            Ok(Some(action)) if action == "keep15" => {
                crate::recorder::trim_to_last(&clip, 15);
            }
            Ok(Some(action)) if action == "keep30" => {
                crate::recorder::trim_to_last(&clip, 30);
            }
            Ok(Some(action)) if action == "open" => {
                let _ = open::that(&clip);
            }
            Ok(Some(action)) if action == "trim" => request_trim(&clip),
            Ok(_) => {}
            Err(_) => {
                // Closing it ends the listener above.
                let _ = proxy.call::<_, _, ()>("CloseNotification", &(id,));
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn send_clip_notification(summary: &str, body: &str, _clip: std::path::PathBuf) {
    eprintln!("[notification] {summary}: {body}");
}

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
fn bundled_or_path_cmd(bin: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let candidate = exe_dir.map(|d| d.join(format!("{}.exe", bin)));
    let mut cmd = match candidate {
        Some(ref p) if p.is_file() => Command::new(p),
        _ => Command::new(bin),
    };
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Helper to set `CREATE_NO_WINDOW` on Windows commands.
pub fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Creates a `Command` for ffmpeg.
pub fn ffmpeg_cmd() -> Command {
    #[cfg(windows)]
    {
        bundled_or_path_cmd("ffmpeg")
    }
    #[cfg(not(windows))]
    {
        Command::new("ffmpeg")
    }
}

/// Creates a `Command` for ffprobe.
pub fn ffprobe_cmd() -> Command {
    #[cfg(windows)]
    {
        bundled_or_path_cmd("ffprobe")
    }
    #[cfg(not(windows))]
    {
        Command::new("ffprobe")
    }
}

/// Where `program` would run from: itself when it is a path, else the first match in PATH.
pub fn find_in_path(program: &str) -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    let runnable = |p: &std::path::Path| {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    };
    #[cfg(not(unix))]
    let runnable = |p: &std::path::Path| p.is_file();

    if program.contains('/') || program.contains('\\') {
        let p = std::path::PathBuf::from(program);
        return runnable(&p).then_some(p);
    }
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidates = [dir.join(program), dir.join(format!("{}.exe", program))];
            if let Some(p) = candidates.into_iter().find(|p| runnable(p)) {
                return Some(p);
            }
        }
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .flat_map(|dir| {
            let base = dir.join(program);
            #[cfg(windows)]
            {
                let with_exe = dir.join(format!("{}.exe", program));
                vec![base, with_exe]
            }
            #[cfg(not(windows))]
            {
                vec![base]
            }
        })
        .find(|p| runnable(p))
}

pub fn run_system_cmd(bin: &str, args: &[&str]) -> Option<Output> {
    let mut cmd = Command::new(bin);
    cmd.args(args);
    no_window(&mut cmd);
    if let Ok(out) = cmd.output() {
        if out.status.success() {
            return Some(out);
        }
    }
    if is_inside_container() {
        let mut cmd = Command::new("distrobox-host-exec");
        cmd.arg(bin).args(args);
        no_window(&mut cmd);
        if let Ok(out) = cmd.output() {
            if out.status.success() {
                return Some(out);
            }
        }
    }
    None
}

pub fn load_icon() -> (Vec<u8>, u32, u32) {
    let mut image = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .unwrap()
        .into_rgba8();
    let (width, height) = image.dimensions();

    let size = width.min(height);
    let x = (width - size) / 2;
    let y = (height - size) / 2;

    let cropped = image::imageops::crop(&mut image, x, y, size, size).to_image();
    (cropped.into_raw(), size, size)
}

/// Free bytes on the filesystem holding `path` (walks up to the nearest existing parent).
#[cfg(unix)]
pub fn free_space_bytes(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let mut p = path;
    while !p.exists() {
        p = p.parent()?;
    }
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some(st.f_bavail as u64 * st.f_frsize as u64)
}

#[cfg(windows)]
pub fn free_space_bytes(path: &std::path::Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let mut p = path;
    while !p.exists() {
        p = p.parent()?;
    }
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free_bytes: u64 = 0;
    extern "system" {
        fn GetDiskFreeSpaceExW(
            lpDirectoryName: *const u16,
            lpFreeBytesAvailableToCaller: *mut u64,
            lpTotalNumberOfBytes: *mut u64,
            lpTotalNumberOfFreeBytes: *mut u64,
        ) -> i32;
    }
    let res = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_bytes,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (res != 0).then_some(free_bytes)
}

/// Starts `lapse --daemon` in its own session, so closing the terminal or the window
/// that launched it (SIGHUP) never stops the recorder.
#[cfg(unix)]
pub fn spawn_daemon() -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.arg("--daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn()
}

#[cfg(windows)]
pub fn spawn_daemon() -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.arg("--daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    const DETACHED_PROCESS: u32 = 0x00000008;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    cmd.spawn()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::collections::HashMap;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::Mutex;

    struct FakeNotifications {
        shown: Mutex<Sender<(String, Vec<String>)>>,
    }

    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl FakeNotifications {
        #[allow(clippy::too_many_arguments)]
        fn notify(
            &self,
            _app: String,
            _replaces: u32,
            _icon: String,
            summary: String,
            _body: String,
            actions: Vec<String>,
            _hints: HashMap<String, zbus::zvariant::OwnedValue>,
            _timeout: i32,
        ) -> u32 {
            let _ = self.shown.lock().unwrap().send((summary, actions));
            7
        }

        fn close_notification(&self, _id: u32) {}
    }

    /// Clicking "Trim" on a clip notification opens the clip in the trimmer. Runs on its
    /// own session bus with a fake notification server:
    /// `dbus-run-session -- cargo test clip_notification -- --ignored`.
    #[test]
    #[ignore]
    fn clip_notification_trim_button() {
        let runtime = std::env::temp_dir().join(format!("lapse-notify-test-{}", std::process::id()));
        std::fs::create_dir_all(&runtime).unwrap();
        std::env::set_var("XDG_RUNTIME_DIR", &runtime);

        let (tx, rx) = channel();
        let server = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name("org.freedesktop.Notifications")
            .unwrap()
            .serve_at("/org/freedesktop/Notifications", FakeNotifications { shown: Mutex::new(tx) })
            .unwrap()
            .build()
            .unwrap();

        assert!(super::has_notification_server());
        let clip = std::path::PathBuf::from("/clips/round.mp4");
        super::send_clip_notification("Lapse - Replay Saved", "Saved", clip.clone());

        let (summary, actions) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(summary, "Lapse - Replay Saved");
        assert_eq!(actions, ["keep15", "Keep last 15 s", "keep30", "Keep last 30 s", "open", "Open", "trim", "Trim"]);

        server
            .emit_signal(None::<()>, "/org/freedesktop/Notifications", "org.freedesktop.Notifications", "ActionInvoked", &(7u32, "trim"))
            .unwrap();
        // The request file appears; remove it before Lapse would start a window for it.
        let request = super::open_request_path();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !request.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let asked = std::fs::read_to_string(&request).unwrap();
        let _ = std::fs::remove_file(&request);
        let _ = std::fs::remove_dir_all(&runtime);
        assert_eq!(asked, "/clips/round.mp4");
    }
}
