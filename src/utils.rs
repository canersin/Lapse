use std::process::{Command, Output, Stdio};

pub fn is_inside_container() -> bool {
    std::path::Path::new("/run/.containerenv").exists()
}

pub fn has_notification_server() -> bool {
    static AVAILABLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    let current = AVAILABLE.load(std::sync::atomic::Ordering::Relaxed);
    if current == 1 {
        return true;
    }
    if current == 2 {
        return false;
    }
    let output = Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.DBus",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus.GetNameOwner",
            "string:org.freedesktop.Notifications",
        ])
        .output();
    let has = if let Ok(out) = output {
        out.status.success()
    } else {
        false
    };
    AVAILABLE.store(if has { 1 } else { 2 }, std::sync::atomic::Ordering::Relaxed);
    has
}

pub fn send_notification(summary: &str, body: &str, icon: &str) {
    if !has_notification_server() {
        return;
    }
    let s = summary.to_string();
    let b = body.to_string();
    let ic = icon.to_string();
    std::thread::spawn(move || {
        let _ = Command::new("notify-send")
            .args(["-t", "1500", "-i", &ic, &s, &b])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    });
}

/// File the GUI watches: a clip path written here opens in the trimmer.
pub fn open_request_path() -> std::path::PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("lapse-open-request")
}

/// Asks the GUI to open a clip in the trimmer, starting the GUI if it isn't running.
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
pub fn send_clip_notification(summary: &str, body: &str, clip: std::path::PathBuf) {
    if !has_notification_server() {
        return;
    }
    let (s, b) = (summary.to_string(), body.to_string());
    std::thread::spawn(move || {
        let child = Command::new("notify-send")
            .args(["-t", "6000", "-i", "media-record", "-A", "open=Open", "-A", "trim=Trim", &s, &b])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return };
        // Some desktops keep notifications with buttons around; don't wait forever.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(200)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
        }
        let mut action = String::new();
        if let Some(mut out) = child.stdout.take() {
            use std::io::Read;
            let _ = out.read_to_string(&mut action);
        }
        match action.trim() {
            "open" => {
                let _ = open::that(&clip);
            }
            "trim" => request_trim(&clip),
            _ => {}
        }
    });
}

pub fn run_system_cmd(bin: &str, args: &[&str]) -> Option<Output> {
    if let Ok(out) = Command::new(bin).args(args).output() {
        if out.status.success() {
            return Some(out);
        }
    }
    if is_inside_container() {
        if let Ok(out) = Command::new("distrobox-host-exec").arg(bin).args(args).output() {
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

#[allow(dead_code)]
pub fn open_in_player(path: &std::path::Path) {
    let p_str = path.to_string_lossy().to_string();
    if is_inside_container() {
        if Command::new("distrobox-host-exec")
            .args(["xdg-open", &p_str])
            .spawn()
            .is_ok()
        {
            return;
        }
    }
    if Command::new("xdg-open").arg(path).spawn().is_err() {
        if Command::new("mpv").arg(path).spawn().is_err() {
            if Command::new("vlc").arg(path).spawn().is_err() {
                let _ = open::that(path);
            }
        }
    }
}

/// Free bytes on the filesystem holding `path` (walks up to the nearest existing parent).
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

/// Starts `lapse --daemon` in its own session, so closing the terminal or the window
/// that launched it (SIGHUP) never stops the recorder.
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
