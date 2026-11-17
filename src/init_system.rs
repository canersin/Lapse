use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitType {
    Auto,
    Systemd,
    Runit,
    Dinit,
    OpenRC,
    SysVinit,
    XdgDesktop,
}

impl InitType {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "systemd" => Self::Systemd,
            "runit" => Self::Runit,
            "dinit" => Self::Dinit,
            "openrc" => Self::OpenRC,
            "sysvinit" => Self::SysVinit,
            "xdg" | "desktop" => Self::XdgDesktop,
            _ => Self::Auto,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Systemd => "systemd",
            Self::Runit => "runit",
            Self::Dinit => "dinit",
            Self::OpenRC => "openrc",
            Self::SysVinit => "sysvinit",
            Self::XdgDesktop => "xdg",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Auto => "Auto (Detected)",
            Self::Systemd => "Systemd User Service",
            Self::Runit => "Runit User Service",
            Self::Dinit => "Dinit Service",
            Self::OpenRC => "OpenRC Service",
            Self::SysVinit => "SysVinit Script",
            Self::XdgDesktop => "XDG Desktop Autostart",
        }
    }
}

pub fn detect_host_init() -> InitType {
    let host_pid1 = crate::utils::run_system_cmd("ps", &["-p", "1", "-o", "comm="])
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_lowercase())
        .unwrap_or_default();

    if host_pid1.contains("systemd") || Path::new("/run/systemd/system").exists() {
        return InitType::Systemd;
    }
    if host_pid1.contains("runit") || Path::new("/run/runit").exists() || Path::new("/etc/runit").exists() {
        return InitType::Runit;
    }
    if host_pid1.contains("dinit") || Path::new("/etc/dinit").exists() {
        return InitType::Dinit;
    }
    if host_pid1.contains("openrc") || Path::new("/run/openrc").exists() {
        return InitType::OpenRC;
    }
    if host_pid1.contains("init") && Path::new("/etc/init.d").exists() {
        return InitType::SysVinit;
    }

    InitType::XdgDesktop
}

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

pub fn sync_autostart(enable: bool, preferred: &str) {
    let resolved = if preferred == "auto" {
        detect_host_init()
    } else {
        InitType::from_str(preferred)
    };

    let exe = get_target_exe();
    let home = std::env::var("HOME").unwrap_or_default();
    let home_path = PathBuf::from(&home);

    sync_xdg_desktop(&home_path, &exe, enable);
    // A Flatpak can't manage the host's services; the desktop autostart entry is enough.
    if std::env::var_os("FLATPAK_ID").is_some() {
        return;
    }

    match resolved {
        InitType::Systemd => {
            sync_systemd_user(&home_path, &exe, enable);
        }
        InitType::Runit => {
            sync_runit_user(&home_path, &exe, enable);
        }
        InitType::Dinit => {
            sync_dinit_user(&home_path, &exe, enable);
        }
        InitType::OpenRC | InitType::SysVinit | InitType::XdgDesktop | InitType::Auto => {}
    }
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

fn sync_systemd_user(home_path: &Path, exe: &str, enable: bool) {
    let unit_dir = home_path.join(".config/systemd/user");
    let unit_file = unit_dir.join("lapse.service");

    if enable {
        let _ = std::fs::create_dir_all(&unit_dir);
        let service_content = format!(
            "[Unit]\nDescription=Lapse Gameplay Replay Background Daemon\nAfter=graphical-session.target\n\n[Service]\nType=simple\nExecStart={} --daemon\nRestart=on-failure\nRestartSec=3\n\n[Install]\nWantedBy=default.target\n",
            exe
        );
        let _ = std::fs::write(&unit_file, service_content);

        run_init_cmd("systemctl", &["--user", "daemon-reload"]);
        run_init_cmd("systemctl", &["--user", "enable", "lapse.service"]);
    } else {
        run_init_cmd("systemctl", &["--user", "disable", "lapse.service"]);
        if unit_file.exists() {
            let _ = std::fs::remove_file(unit_file);
        }
        run_init_cmd("systemctl", &["--user", "daemon-reload"]);
    }
}

fn sync_runit_user(home_path: &Path, exe: &str, enable: bool) {
    for base in [home_path.join(".service"), home_path.join(".config/service"), home_path.join("service")] {
        let svc_dir = base.join("lapse");
        let run_file = svc_dir.join("run");

        if enable {
            let _ = std::fs::create_dir_all(&svc_dir);
            let script = format!("#!/bin/sh\nexec {} --daemon\n", exe);
            if std::fs::write(&run_file, script).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&run_file, std::fs::Permissions::from_mode(0o755));
                }
            }
        } else if svc_dir.exists() {
            let _ = std::fs::remove_dir_all(svc_dir);
        }
    }
}

fn sync_dinit_user(home_path: &Path, exe: &str, enable: bool) {
    let dinit_dir = home_path.join(".config/dinit.d");
    let service_file = dinit_dir.join("lapse");

    if enable {
        let _ = std::fs::create_dir_all(&dinit_dir);
        let content = format!("type = process\ncommand = {} --daemon\nrestart = true\n", exe);
        let _ = std::fs::write(&service_file, content);
        run_init_cmd("dinitctl", &["--user", "enable", "lapse"]);
    } else {
        run_init_cmd("dinitctl", &["--user", "disable", "lapse"]);
        if service_file.exists() {
            let _ = std::fs::remove_file(service_file);
        }
    }
}
