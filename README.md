# Lapse

A high-performance, lightweight game clipper for Linux, written in Rust.

## Features
- **Near Zero Performance Impact**: Powered by `gpu-screen-recorder`.
- **Native GUI**: Built with `egui`, no HTML/JS/Python required.
- **Global Hotkeys**: Save replays instantly with a single keypress.
- **Highly Portable**: Static binaries available (musl support).
- **Lightweight**: Minimal disk and memory footprint.

## Requirements
- Linux (Wayland or X11)
- `gpu-screen-recorder` installed on your system.

## Usage
1. Launch `lapse`.
2. Use the GUI to configure your replay buffer and hotkeys.
3. Press the hotkey (default: `F10`) to save the last X seconds of gameplay.

## Installation
The easiest way to install Lapse on any Linux distribution is via the universal installation script:

```bash
git clone https://github.com/yourusername/lapse.git
cd lapse
./install.sh
```

This will automatically:
1. Install system dependencies via your package manager.
2. Install the Rust toolchain (if missing).
3. Compile the application optimally.
4. Add lapse to your background Autostart applications.
5. Create an Applications Menu shortcut.
