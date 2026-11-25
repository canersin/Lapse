#[cfg(not(target_os = "linux"))]
pub fn get_target_exe() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "Lapse.exe".into())
}

#[cfg(target_os = "windows")]
pub fn sync_autostart(enable: bool) {
    crate::gui::platform::sync_autostart_windows(enable);
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn sync_autostart(_enable: bool) {}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub fn get_target_exe() -> String {
        // Inside Flatpak the binary path only exists in the sandbox; start it through flatpak.
        if let Some(id) = std::env::var_os("FLATPAK_ID") {
            return format!("/usr/bin/flatpak run --command=lapse {}", id.to_string_lossy());
        }
        // An AppImage runs from a temporary mount; start the AppImage file itself.
        if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|p| Path::new(p).is_file()) {
            return appimage.to_string_lossy().to_string();
        }
        // Nix store paths go away after an update; the profile puts `lapse` in PATH.
        if std::env::current_exe().map(|p| p.starts_with("/nix/store")).unwrap_or(false) {
            return "lapse".to_string();
        }
        if Path::new("/usr/local/bin/Lapse").exists() {
            "/usr/local/bin/Lapse".to_string()
        } else if Path::new("/usr/bin/Lapse").exists() {
            "/usr/bin/Lapse".to_string()
        } else {
            std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "Lapse".into())
        }
    }

    pub fn sync_autostart(enable: bool) {
        let exe = get_target_exe();
        let home = std::env::var("HOME").unwrap_or_default();
        let home_path = PathBuf::from(&home);

        sync_xdg_desktop(&home_path, &exe, enable);
        // A Flatpak can't manage the host's services.
        if std::env::var_os("FLATPAK_ID").is_some() {
            return;
        }
        // User services start before the desktop session hands out its display, so the
        // recorder can't reach the screen from them; every desktop runs the autostart
        // entry inside the session instead. Remove the services older versions created.
        remove_systemd_user(&home_path);
        remove_runit_user(&home_path);
        remove_dinit_user(&home_path);
    }

    fn sync_xdg_desktop(home_path: &Path, exe: &str, enable: bool) {
        let autostart_dir = home_path.join(".config/autostart");
        let desktop_file = autostart_dir.join("lapse.desktop");
        if enable {
            let _ = std::fs::create_dir_all(&autostart_dir);
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName=Lapse Replay Service\nComment=Lapse background clipping service\nExec={} --daemon\nIcon=lapse\nTerminal=false\nHidden=false\nX-GNOME-Autostart-enabled=true\n",
                exe
            );
            let _ = std::fs::write(desktop_file, entry);
        } else if desktop_file.exists() {
            let _ = std::fs::remove_file(desktop_file);
        }
    }

    fn run_init_cmd(bin: &str, args: &[&str]) {
        if let Ok(st) = Command::new(bin).args(args).status() {
            if st.success() {
                return;
            }
        }
        if crate::utils::is_inside_container() {
            let _ = Command::new("distrobox-host-exec").arg(bin).args(args).status();
        }
    }

    fn remove_systemd_user(home_path: &Path) {
        let unit_file = home_path.join(".config/systemd/user/lapse.service");
        if unit_file.exists() {
            run_init_cmd("systemctl", &["--user", "disable", "lapse.service"]);
            let _ = std::fs::remove_file(unit_file);
            run_init_cmd("systemctl", &["--user", "daemon-reload"]);
        }
    }

    fn remove_runit_user(home_path: &Path) {
        for base in [home_path.join(".service"), home_path.join(".config/service"), home_path.join("service")] {
            let svc_dir = base.join("lapse");
            if svc_dir.exists() {
                let _ = std::fs::remove_dir_all(svc_dir);
            }
        }
    }

    fn remove_dinit_user(home_path: &Path) {
        let service_file = home_path.join(".config/dinit.d/lapse");
        if service_file.exists() {
            run_init_cmd("dinitctl", &["--user", "disable", "lapse"]);
            let _ = std::fs::remove_file(service_file);
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::*;
