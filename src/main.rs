mod client;
mod config;
mod daemon;
mod gui;
mod hardware;
mod hotkeys;
mod ipc;
mod recorder;
mod tray;
mod utils;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--gui") {
        return client::run_gui_client();
    }

    daemon::run_daemon()
}
