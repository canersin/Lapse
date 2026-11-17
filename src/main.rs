mod autoclip;
mod av_sync;
mod cleanup;
mod cli;
mod clip_index;
mod client;
mod config;
mod daemon;
mod discord;
mod discord_rpc;
mod export;
mod favorites;
mod markers;
mod merge;
mod gui;
mod hardware;
mod highlights;
mod hotkeys;
pub mod init_system;
mod ipc;
mod recorder;
mod rename;
pub mod share;
mod sound;
mod tray;
pub mod update;
mod utils;

fn main() -> anyhow::Result<()> {
    if let Ok(cfg) = config::Config::load() {
        init_system::sync_autostart(cfg.autostart);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();

    if cli::handle_cli(&args)? {
        return Ok(());
    }

    if args.iter().any(|a| a == "--gui") {
        return client::run_gui_client();
    }

    daemon::run_daemon()
}
