//! Other programs' process info (name, command line, Steam id, GPU use) for game
//! detection. Natively it is read from /proc. A Flatpak has its own process list
//! and can't see the games, so there it is read from the host through
//! `flatpak-spawn --host`: one shell command collects everything, cached briefly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub fn in_flatpak() -> bool {
    static FLATPAK: OnceLock<bool> = OnceLock::new();
    *FLATPAK.get_or_init(|| Path::new("/.flatpak-info").exists())
}

/// A command that runs on the host, also from inside the Flatpak (xprop, hyprctl).
pub fn host_command(program: &str) -> Command {
    if in_flatpak() {
        let mut cmd = Command::new("flatpak-spawn");
        cmd.args(["--host", program]);
        cmd
    } else {
        Command::new(program)
    }
}

/// All process ids (the user's own ones in the Flatpak).
pub fn pids() -> Vec<u32> {
    if in_flatpak() {
        return host().keys().copied().collect();
    }
    std::fs::read_dir("/proc")
        .map(|d| d.flatten().filter_map(|e| e.file_name().to_str()?.parse().ok()).collect())
        .unwrap_or_default()
}

pub fn comm(pid: u32) -> String {
    if in_flatpak() {
        return host().get(&pid).map(|p| p.comm.clone()).unwrap_or_default();
    }
    std::fs::read_to_string(format!("/proc/{}/comm", pid)).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// Command line with spaces between the arguments.
pub fn cmdline(pid: u32) -> String {
    if in_flatpak() {
        return host().get(&pid).map(|p| p.cmdline.clone()).unwrap_or_default();
    }
    std::fs::read(format!("/proc/{}/cmdline", pid)).map(|b| String::from_utf8_lossy(&b).replace('\0', " ")).unwrap_or_default()
}

/// The Steam app id from the environment (SteamAppId or STEAM_COMPAT_APP_ID).
pub fn steam_app_id(pid: u32) -> Option<u32> {
    if in_flatpak() {
        return host().get(&pid).and_then(|p| p.steam_app);
    }
    let data = std::fs::read(format!("/proc/{}/environ", pid)).ok()?;
    data.split(|&b| b == 0).find_map(|chunk| steam_var(std::str::from_utf8(chunk).ok()?))
}

fn steam_var(line: &str) -> Option<u32> {
    line.strip_prefix("SteamAppId=")
        .or_else(|| line.strip_prefix("STEAM_COMPAT_APP_ID="))
        .and_then(|v| v.trim().parse().ok())
        .filter(|id| *id != 0)
}

pub fn exe(pid: u32) -> Option<PathBuf> {
    if in_flatpak() {
        return host().get(&pid).and_then(|p| p.exe.clone());
    }
    std::fs::read_link(format!("/proc/{}/exe", pid)).ok()
}

pub fn ppid(pid: u32) -> Option<u32> {
    if in_flatpak() {
        return host().get(&pid).map(|p| p.ppid);
    }
    let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

/// Has the GPU open (draws something).
pub fn uses_gpu(pid: u32) -> bool {
    if in_flatpak() {
        return host().get(&pid).map(|p| p.gpu).unwrap_or(false);
    }
    std::fs::read_dir(format!("/proc/{}/fd", pid))
        .map(|fds| fds.flatten().any(|fd| std::fs::read_link(fd.path()).map(|t| t.starts_with("/dev/dri/")).unwrap_or(false)))
        .unwrap_or(false)
}

#[derive(Clone, Debug, Default, PartialEq)]
struct HostProc {
    comm: String,
    cmdline: String,
    ppid: u32,
    steam_app: Option<u32>,
    exe: Option<PathBuf>,
    gpu: bool,
}

/// Runs on the host: each section lists one thing for every process of the user.
/// A handful of programs in total, so it stays fast with hundreds of processes.
const HOST_SCRIPT: &str = r#"
echo @@ps; ps -ww -u "$(id -u)" -o pid=,ppid=,args=
echo @@comm; grep -H . /proc/[0-9]*/comm 2>/dev/null
echo @@env; grep -aHz -e '^SteamAppId=' -e '^STEAM_COMPAT_APP_ID=' /proc/[0-9]*/environ 2>/dev/null | tr '\0' '\n'
echo @@exe; ls -l /proc/[0-9]*/exe 2>/dev/null
echo @@fd; ls -l /proc/[0-9]*/fd/ 2>/dev/null | grep -e '^/proc/' -e ' -> /dev/dri/'
"#;

const FRESH: Duration = Duration::from_millis(1500);

fn host() -> Arc<HashMap<u32, HostProc>> {
    static CACHE: OnceLock<Mutex<Option<(Instant, Arc<HashMap<u32, HostProc>>)>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, list)) = cache.as_ref() {
        if at.elapsed() < FRESH {
            return list.clone();
        }
    }
    let out = Command::new("flatpak-spawn").args(["--host", "sh", "-c", HOST_SCRIPT]).output();
    let list = Arc::new(out.map(|o| parse_host(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default());
    *cache = Some((Instant::now(), list.clone()));
    list
}

/// "/proc/123/comm:..." -> (123, "...")
fn proc_path(line: &str) -> Option<(u32, &str)> {
    let rest = line.strip_prefix("/proc/")?;
    let (pid, rest) = rest.split_once('/')?;
    Some((pid.parse().ok()?, rest))
}

fn parse_host(text: &str) -> HashMap<u32, HostProc> {
    let mut procs: HashMap<u32, HostProc> = HashMap::new();
    let mut section = "";
    let mut fd_pid: Option<u32> = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("@@") {
            section = name;
            continue;
        }
        match section {
            "ps" => {
                // "  900    42 /games/peak/PEAK.exe -windowed": columns padded with spaces.
                let Some((pid, rest)) = line.trim_start().split_once(char::is_whitespace) else { continue };
                let (ppid, cmdline) = rest.trim_start().split_once(char::is_whitespace).unwrap_or((rest.trim(), ""));
                let Ok(pid) = pid.parse() else { continue };
                procs.insert(pid, HostProc { ppid: ppid.parse().unwrap_or(0), cmdline: cmdline.trim_start().to_string(), ..Default::default() });
            }
            // Only processes `ps` listed (the user's own) are kept.
            "comm" => {
                if let Some((pid, rest)) = proc_path(line) {
                    if let (Some(p), Some(name)) = (procs.get_mut(&pid), rest.strip_prefix("comm:")) {
                        p.comm = name.to_string();
                    }
                }
            }
            "env" => {
                if let Some((pid, rest)) = proc_path(line) {
                    if let (Some(p), Some(var)) = (procs.get_mut(&pid), rest.strip_prefix("environ:")) {
                        p.steam_app = p.steam_app.or_else(|| steam_var(var));
                    }
                }
            }
            "exe" => {
                // lrwxrwxrwx 1 user user 0 Nov 17 15:19 /proc/123/exe -> /usr/bin/foo
                let Some((left, target)) = line.split_once(" -> ") else { continue };
                let Some(path) = left.rsplit(' ').next() else { continue };
                if let Some((pid, "exe")) = proc_path(path) {
                    if let Some(p) = procs.get_mut(&pid) {
                        p.exe = Some(PathBuf::from(target));
                    }
                }
            }
            "fd" => {
                // "/proc/123/fd/:" starts a process; its "-> /dev/dri/..." lines follow.
                if let Some((pid, "fd/:" | "fd:")) = proc_path(line) {
                    fd_pid = Some(pid);
                } else if line.contains(" -> /dev/dri/") {
                    if let Some(p) = fd_pid.and_then(|pid| procs.get_mut(&pid)) {
                        p.gpu = true;
                    }
                }
            }
            _ => {}
        }
    }
    procs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_listing_is_read() {
        let text = "\
@@ps
   42     1 /usr/bin/java -cp minecraft.jar net.minecraft.client.main.Main
  900    42 /games/peak/PEAK.exe -windowed
 1000     1 bash
@@comm
/proc/42/comm:java
/proc/900/comm:PEAK.exe
/proc/1000/comm:bash
/proc/1/comm:init
@@env
/proc/900/environ:SteamAppId=3527290
@@exe
lrwxrwxrwx 1 u u 0 Nov 17 15:19 /proc/42/exe -> /usr/lib/jvm/bin/java
@@fd
/proc/42/fd/:
total 0
lrwx------ 1 u u 64 Nov 17 15:19 7 -> /dev/dri/renderD128
/proc/1000/fd/:
";
        let p = parse_host(text);
        assert_eq!(p.len(), 3, "only the user's processes from ps");
        assert_eq!(p[&42].comm, "java");
        assert!(p[&42].cmdline.contains("net.minecraft"));
        assert!(p[&42].gpu && !p[&1000].gpu);
        assert_eq!(p[&42].exe.as_deref(), Some(Path::new("/usr/lib/jvm/bin/java")));
        assert_eq!((p[&900].ppid, p[&900].steam_app), (42, Some(3527290)));
        assert_eq!(p[&900].cmdline, "/games/peak/PEAK.exe -windowed");
    }
}

/// The host script on this machine must agree with reading /proc directly:
/// `cargo test host_script_matches_proc -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn host_script_matches_proc() {
    let t = Instant::now();
    let out = Command::new("sh").args(["-c", HOST_SCRIPT]).output().unwrap();
    let host = parse_host(&String::from_utf8_lossy(&out.stdout));
    println!("{} processes in {:?}", host.len(), t.elapsed());
    let (mut gpu, mut steam, mut bad) = (0, 0, 0);
    for (pid, p) in &host {
        let native = HostProc {
            comm: comm(*pid),
            // ps shows line breaks inside arguments as "?".
            cmdline: cmdline(*pid).trim_end().chars().map(|c| if c.is_control() { '?' } else { c }).collect(),
            ppid: ppid(*pid).unwrap_or(0),
            steam_app: steam_app_id(*pid),
            exe: exe(*pid),
            gpu: uses_gpu(*pid),
        };
        if native.comm.is_empty() {
            continue; // exited meanwhile
        }
        let mut host_p = p.clone();
        host_p.cmdline = host_p.cmdline.trim_end().to_string();
        gpu += p.gpu as u32;
        steam += p.steam_app.is_some() as u32;
        // ps shows "[name]" for processes without a command line.
        if native.cmdline.is_empty() {
            host_p.cmdline.clear();
        }
        if host_p != native {
            bad += 1;
            let mut fields = Vec::new();
            if host_p.comm != native.comm { fields.push(format!("comm {:?}/{:?}", host_p.comm, native.comm)); }
            if host_p.cmdline != native.cmdline { fields.push(format!("cmdline {:?}/{:?}", &host_p.cmdline[..host_p.cmdline.len().min(60)], &native.cmdline[..native.cmdline.len().min(60)])); }
            if host_p.ppid != native.ppid { fields.push("ppid".into()); }
            if host_p.steam_app != native.steam_app { fields.push("steam".into()); }
            if host_p.exe != native.exe { fields.push(format!("exe {:?}/{:?}", host_p.exe, native.exe)); }
            if host_p.gpu != native.gpu { fields.push("gpu".into()); }
            println!("DIFF {} {}: {}", pid, native.comm, fields.join(", "));
        }
    }
    println!("gpu {} steam {} differing {}", gpu, steam, bad);
    assert_eq!(bad, 0);
}
