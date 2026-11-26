//! System diagnostics, report generation, and GitHub issue URLs.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Basic system information pre-filled into GitHub issue forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInfo {
    pub version: String,
    pub os: String,
    pub desktop: String,
    pub gpu: String,
    pub install: String,
}

/// Detects the current system information.
pub fn system_info() -> SystemInfo {
    SystemInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: detect_os(),
        desktop: detect_desktop(),
        gpu: detect_gpu(),
        install: detect_install(),
    }
}

#[cfg(not(windows))]
fn detect_os() -> String {
    // Inside a Flatpak /etc/os-release is the runtime's; the host's is under /run/host.
    let pretty_name = ["/run/host/os-release", "/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .find_map(|c| {
            c.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches(|c| c == '"' || c == '\'').trim().to_string())
        })
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Linux".to_string());
    match std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        Ok(kernel) if !kernel.trim().is_empty() => format!("{} ({})", pretty_name, kernel.trim()),
        _ => pretty_name,
    }
}

#[cfg(windows)]
fn detect_os() -> String {
    let reg_output = crate::utils::no_window(&mut Command::new("reg"))
        .args(["query", r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion"])
        .output();
    if let Ok(out) = reg_output {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut prod_name = String::new();
            let mut disp_ver = String::new();
            let mut build = String::new();
            for line in s.lines() {
                let trimmed = line.trim();
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 3 {
                    if parts[0].eq_ignore_ascii_case("ProductName") {
                        prod_name = parts[2..].join(" ");
                    } else if parts[0].eq_ignore_ascii_case("DisplayVersion") {
                        disp_ver = parts[2..].join(" ");
                    } else if parts[0].eq_ignore_ascii_case("CurrentBuild")
                        || parts[0].eq_ignore_ascii_case("CurrentBuildNumber")
                    {
                        build = parts[2..].join(" ");
                    }
                }
            }
            if !prod_name.is_empty() {
                let mut res = prod_name;
                if !disp_ver.is_empty() {
                    res.push(' ');
                    res.push_str(&disp_ver);
                }
                if !build.is_empty() {
                    res.push_str(&format!(" (build {})", build));
                }
                return res;
            }
        }
    }

    if let Ok(out) = crate::utils::no_window(&mut Command::new("cmd")).args(["/c", "ver"]).output() {
        let s = String::from_utf8_lossy(&out.stdout);
        for line in s.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    "Windows".to_string()
}

#[cfg(not(windows))]
fn detect_desktop() -> String {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    match (desktop.is_empty(), session.is_empty()) {
        (false, false) => format!("{} ({})", desktop, session),
        (false, true) => desktop,
        (true, false) => session,
        (true, true) => "unknown".to_string(),
    }
}

#[cfg(windows)]
fn detect_desktop() -> String {
    "-".to_string()
}

#[cfg(not(windows))]
fn detect_gpu() -> String {
    let gpus = crate::hardware::gpu::get_gpus();
    if gpus.is_empty() {
        "Unknown / None detected".to_string()
    } else {
        gpus.iter()
            .map(|g| g.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(windows)]
fn detect_gpu() -> String {
    if let Ok(out) = crate::utils::no_window(&mut Command::new("cmd"))
        .args(["/c", "wmic path win32_videocontroller get name"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let names: Vec<String> = s
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("name"))
                .collect();
            if !names.is_empty() {
                return names.join(", ");
            }
        }
    }

    if let Ok(out) = crate::utils::no_window(&mut Command::new("powershell"))
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }",
        ])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let names: Vec<String> = s
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if !names.is_empty() {
                return names.join(", ");
            }
        }
    }

    "Unknown".to_string()
}

fn detect_install() -> String {
    if std::env::var_os("APPIMAGE").is_some() {
        return "AppImage".to_string();
    }
    if std::env::var_os("FLATPAK_ID").is_some() {
        return "Flatpak".to_string();
    }
    if let Ok(exe) = std::env::current_exe() {
        let exe_str = exe.to_string_lossy();
        if exe_str.contains("Program Files") {
            return "Windows installer".to_string();
        }
        if exe_str.contains("/.local/bin") || exe_str.contains(r"\.local\bin") {
            return "~/.local".to_string();
        }
        if exe_str.starts_with("/usr/bin") || exe_str.contains("/usr/bin") {
            return "system package".to_string();
        }
    }
    "from source / other".to_string()
}

/// Builds the full diagnostic report string.
pub fn build_report() -> String {
    let info = system_info();
    let mut report = String::new();

    report.push_str("=== Lapse System Report ===\n");
    report.push_str(&format!("Version: {}\n", info.version));
    report.push_str(&format!("OS: {}\n", info.os));
    report.push_str(&format!("Desktop: {}\n", info.desktop));
    report.push_str(&format!("GPU: {}\n", info.gpu));
    report.push_str(&format!("Install: {}\n", info.install));

    #[cfg(windows)]
    let recorder_backend = "Media Foundation".to_string();
    #[cfg(not(windows))]
    let recorder_backend = {
        Command::new("gpu-screen-recorder")
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "gpu-screen-recorder (not found or error)".to_string())
    };
    report.push_str(&format!("Recorder backend: {}\n", recorder_backend));

    report.push_str("\n=== Config Settings ===\n");
    match crate::config::Config::load() {
        Ok(cfg) => {
            report.push_str(&format!("Capture Mode: {}\n", cfg.capture_mode));
            report.push_str(&format!("FPS: {}\n", cfg.fps));
            report.push_str(&format!("Encoder: {}\n", cfg.video_codec));
            report.push_str(&format!("Replay Seconds: {}\n", cfg.replay_seconds));
            report.push_str(&format!("Resolution: {}\n", cfg.resolution));
            report.push_str(&format!("Monitor: {}\n", cfg.monitor));
        }
        Err(e) => {
            report.push_str(&format!("Config: failed to load ({})\n", e));
        }
    }

    let mut logs: Vec<(&str, PathBuf)> = Vec::new();
    logs.push(("daemon.log", crate::daemon::log_path()));
    #[cfg(not(windows))]
    logs.push(("recorder.log", crate::recorder::log_path()));
    logs.push(("gui.log", crate::daemon::log_path().with_file_name("gui.log")));

    for (name, path) in logs {
        if path.is_file() {
            if let Ok(bytes) = std::fs::read(&path) {
                let content = String::from_utf8_lossy(&bytes);
                let lines: Vec<&str> = content.lines().collect();
                let start = lines.len().saturating_sub(150);
                let tail = lines[start..].join("\n");
                report.push_str(&format!("\n=== {} (last 150 lines) ===\n{}\n", name, tail));
            }
        }
    }

    redact(&report)
}

/// Writes build_report() to lapse-report.txt next to daemon.log.
pub fn write_report() -> std::io::Result<PathBuf> {
    let report = build_report();
    let report_path = crate::daemon::log_path().with_file_name("lapse-report.txt");
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&report_path, report)?;
    Ok(report_path)
}

/// Generates a diagnostic report, opens the GitHub bug report form in a browser,
/// and reveals the folder containing the report.
pub fn open_bug_report() -> std::io::Result<PathBuf> {
    let report_path = write_report()?;
    let info = system_info();
    let url = issue_url(IssueKind::Bug, &info);
    let _ = open::that(&url);
    if let Some(parent) = report_path.parent() {
        let _ = open::that(parent);
    } else {
        let _ = open::that(&report_path);
    }
    Ok(report_path)
}

/// GitHub issues page reports go to; a build can point it elsewhere with LAPSE_ISSUES_URL (see build.rs).
const ISSUES_URL: &str = match option_env!("LAPSE_ISSUES_URL") {
    Some(url) => url,
    None => "https://github.com/canersin/Lapse/issues",
};

/// The kind of GitHub issue to file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    Bug,
    WorksForMe,
}

/// Generates a GitHub issue creation URL with prefilled system details.
pub fn issue_url(kind: IssueKind, info: &SystemInfo) -> String {
    let template = match kind {
        IssueKind::Bug => "bug_report.yml",
        IssueKind::WorksForMe => "works_on_my_system.yml",
    };
    let mut url = format!(
        "{}/new?template={}&version={}&os={}&desktop={}&gpu={}&install={}",
        ISSUES_URL,
        template,
        percent_encode(&info.version),
        percent_encode(&info.os),
        percent_encode(&info.desktop),
        percent_encode(&info.gpu),
        percent_encode(&info.install),
    );
    if kind == IssueKind::Bug {
        url.push_str("&title=[Bug]%20");
    }
    url
}

/// Percent-encodes a string, preserving RFC 3986 unreserved characters (ALPHA, DIGIT, '-', '_', '.', '~').
pub fn percent_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match *byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char);
            }
            b => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

/// Redacts sensitive information from report text using environment variables.
pub fn redact(text: &str) -> String {
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .or_else(|| {
            directories::BaseDirs::new()
                .map(|b| b.home_dir().to_string_lossy().to_string())
        });
    let username = std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok());

    redact_custom(text, home.as_deref(), username.as_deref())
}

/// Redacts sensitive information with explicitly provided home directory and username.
pub fn redact_custom(text: &str, home: Option<&str>, username: Option<&str>) -> String {
    let mut out = redact_webhooks(text);

    if let Some(h) = home {
        let h = h.trim();
        if !h.is_empty() && h != "/" && h != "\\" {
            out = out.replace(h, "~");
            let h_alt = h.replace('\\', "/");
            if h_alt != h {
                out = out.replace(&h_alt, "~");
            }
        }
    }

    if let Some(u) = username {
        let u = u.trim();
        // A very short name would also hit ordinary words.
        if u.len() >= 3 {
            out = out.replace(u, "<user>");
        }
    }

    out
}

/// The path with the home folder shown as `~`, for paths shown in the window, so
/// screenshots do not show the user name.
pub fn show_path(path: &Path) -> String {
    match directories::BaseDirs::new() {
        Some(dirs) => show_path_in(path, dirs.home_dir()),
        None => path.display().to_string(),
    }
}

fn show_path_in(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rel) if home.parent().is_some() => Path::new("~").join(rel).display().to_string(),
        _ => path.display().to_string(),
    }
}

fn redact_webhooks(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;
    let prefixes = [
        "https://discord.com/api/webhooks/",
        "https://discordapp.com/api/webhooks/",
    ];

    while !remaining.is_empty() {
        let mut next_match: Option<(usize, usize)> = None;
        for prefix in &prefixes {
            if let Some(idx) = remaining.find(prefix) {
                match next_match {
                    Some((best_idx, _)) if idx < best_idx => {
                        next_match = Some((idx, prefix.len()));
                    }
                    None => {
                        next_match = Some((idx, prefix.len()));
                    }
                    _ => {}
                }
            }
        }

        if let Some((start_idx, prefix_len)) = next_match {
            result.push_str(&remaining[..start_idx]);
            result.push_str("<webhook>");
            let after_prefix = &remaining[start_idx + prefix_len..];
            let end_idx = after_prefix
                .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | ',' | '`'))
                .unwrap_or(after_prefix.len());
            remaining = &after_prefix[end_idx..];
        } else {
            result.push_str(remaining);
            break;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redaction_home_dir() {
        let raw = "Config loaded from /home/gamer/.config/lapse/config.json";
        let redacted = redact_custom(raw, Some("/home/gamer"), Some("gamer"));
        assert_eq!(redacted, "Config loaded from ~/.config/lapse/config.json");
    }

    #[test]
    fn test_redaction_user_name() {
        let raw = "Logged in as gamer on host machine";
        let redacted = redact_custom(raw, Some("/home/gamer"), Some("gamer"));
        assert_eq!(redacted, "Logged in as <user> on host machine");
    }

    #[test]
    fn test_redaction_webhook() {
        let raw = "Posting to https://discord.com/api/webhooks/123456789/abcdef-ghijk/xyz with payload";
        let redacted = redact_custom(raw, None, None);
        assert_eq!(redacted, "Posting to <webhook> with payload");

        let raw_app = "URL: https://discordapp.com/api/webhooks/987654321/secret_token";
        let redacted_app = redact_custom(raw_app, None, None);
        assert_eq!(redacted_app, "URL: <webhook>");
    }

    #[test]
    fn test_redaction_combined() {
        let raw = "User gamer at /home/gamer sent https://discord.com/api/webhooks/111/aaa done";
        let redacted = redact_custom(raw, Some("/home/gamer"), Some("gamer"));
        assert_eq!(redacted, "User <user> at ~ sent <webhook> done");
    }

    #[test]
    fn test_show_path() {
        let home = Path::new("/home/gamer");
        assert_eq!(show_path_in(Path::new("/home/gamer/.local/state/lapse/daemon.log"), home), "~/.local/state/lapse/daemon.log");
        assert_eq!(show_path_in(Path::new("/var/log/x.log"), home), "/var/log/x.log");
        assert_eq!(show_path_in(Path::new("/etc/x"), Path::new("/")), "/etc/x");
    }

    #[test]
    fn test_percent_encoding_spaces_ampersand_hash_non_ascii() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("foo & bar"), "foo%20%26%20bar");
        assert_eq!(percent_encode("test#1"), "test%231");
        assert_eq!(percent_encode("türkçe"), "t%C3%BCrk%C3%A7e");
        assert_eq!(percent_encode("AZaz09-._~"), "AZaz09-._~");
    }

    #[test]
    fn test_issue_url_bug() {
        let info = SystemInfo {
            version: "2.3.0".to_string(),
            os: "Ubuntu 22.04 LTS (6.6.1)".to_string(),
            desktop: "GNOME (wayland)".to_string(),
            gpu: "AMD Radeon RX 6800".to_string(),
            install: "system package".to_string(),
        };
        let url = issue_url(IssueKind::Bug, &info);
        assert!(url.starts_with(&format!("{}/new?template=bug_report.yml", ISSUES_URL)));
        assert!(url.contains("title=[Bug]%20"));
        assert!(url.contains("version=2.3.0"));
        assert!(url.contains("os=Ubuntu%2022.04%20LTS%20%286.6.1%29"));
        assert!(url.contains("desktop=GNOME%20%28wayland%29"));
        assert!(url.contains("gpu=AMD%20Radeon%20RX%206800"));
        assert!(url.contains("install=system%20package"));
    }

    #[test]
    fn test_issue_url_works_for_me() {
        let info = SystemInfo {
            version: "2.3.0".to_string(),
            os: "Windows 11 (build 22631)".to_string(),
            desktop: "-".to_string(),
            gpu: "NVIDIA GeForce RTX 4080".to_string(),
            install: "Windows installer".to_string(),
        };
        let url = issue_url(IssueKind::WorksForMe, &info);
        assert!(url.starts_with(&format!("{}/new?template=works_on_my_system.yml", ISSUES_URL)));
        assert!(!url.contains("title=[Bug]%20"));
        assert!(url.contains("version=2.3.0"));
        assert!(url.contains("desktop=-"));
        assert!(url.contains("gpu=NVIDIA%20GeForce%20RTX%204080"));
        assert!(url.contains("install=Windows%20installer"));
    }
}
