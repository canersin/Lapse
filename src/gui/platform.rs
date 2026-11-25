//! Platform-specific GUI helpers and Windows/Linux feature toggles.

/// Returns true when running on Windows.
#[inline]
#[allow(dead_code)]
pub fn is_windows() -> bool {
    cfg!(windows)
}

/// Returns true when running on Linux.
#[inline]
#[allow(dead_code)]
pub fn is_linux() -> bool {
    !cfg!(windows)
}

/// Sync autostart on Windows via HKCU Run registry key.
#[cfg(windows)]
pub fn sync_autostart_windows(enable: bool) {
    if enable {
        if let Ok(exe) = std::env::current_exe() {
            let exe_str = format!("\"{}\"", exe.to_string_lossy());
            let mut cmd = std::process::Command::new("reg");
            cmd.args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "Lapse",
                "/t",
                "REG_SZ",
                "/d",
                &exe_str,
                "/f",
            ]);
            crate::utils::no_window(&mut cmd);
            let _ = cmd.output();
        }
    } else {
        let mut cmd = std::process::Command::new("reg");
        cmd.args([
            "delete",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "Lapse",
            "/f",
        ]);
        crate::utils::no_window(&mut cmd);
        let _ = cmd.output();
    }
}

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub fn sync_autostart_windows(_enable: bool) {}
