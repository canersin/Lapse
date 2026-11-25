#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(windows))]
mod audio_apps;
mod autoclip;
pub mod app_icons;
mod av_sync;
mod cleanup;
mod cli;
mod clip_index;
mod client;
mod config;
mod daemon;
mod denoise;
mod discord;
mod discord_rpc;
mod export;
mod favorites;
#[cfg(target_os = "linux")]
mod focus;
mod markers;
mod merge;
mod notes;
mod gui;
mod hardware;
mod highlights;
mod hotkeys;
mod http;
pub mod init_system;
mod ipc;
mod recorder;
mod recorder_windows;
mod rename;
pub mod share;
mod sound;
pub mod streamable;
pub mod tracks;
mod tray;
pub mod update;
mod utils;
mod voice;

fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, GENERIC_READ, GENERIC_WRITE};
        use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
        use windows_sys::Win32::System::Console::{
            AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        };

        let is_valid = |h: HANDLE| !h.is_null() && h != INVALID_HANDLE_VALUE;

        let h_out = GetStdHandle(STD_OUTPUT_HANDLE);
        let h_err = GetStdHandle(STD_ERROR_HANDLE);
        let out_valid = is_valid(h_out);
        let err_valid = is_valid(h_err);

        if (!out_valid || !err_valid) && AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            let conout: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
            if !out_valid {
                let con_h = CreateFileW(
                    conout.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                );
                if is_valid(con_h) {
                    SetStdHandle(STD_OUTPUT_HANDLE, con_h);
                }
            }
            if !err_valid {
                let con_h = CreateFileW(
                    conout.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                );
                if is_valid(con_h) {
                    SetStdHandle(STD_ERROR_HANDLE, con_h);
                }
            }
        }
    }

    if let Ok(cfg) = config::Config::load() {
        init_system::sync_autostart(cfg.autostart);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();

    #[cfg(windows)]
    if args.is_empty() {
        if !ipc::daemon_alive() {
            if let Ok(exe) = std::env::current_exe() {
                let mut cmd = std::process::Command::new(exe);
                cmd.arg("--daemon")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                use std::os::windows::process::CommandExt;
                const DETACHED_PROCESS: u32 = 0x00000008;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
                let _ = cmd.spawn();

                let start = std::time::Instant::now();
                while !ipc::daemon_alive() {
                    if start.elapsed() >= std::time::Duration::from_secs(5) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
        return client::run_gui_client();
    }

    if cli::handle_cli(&args)? {
        return Ok(());
    }

    if args.iter().any(|a| a == "--gui") {
        return client::run_gui_client();
    }

    daemon::run_daemon()
}
