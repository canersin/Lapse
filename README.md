# Lapse

A modern, high-performance, lightweight game clipper for Linux, written in Rust.

## Features
- **Modern Interface**: Premium, dark-mode Medal.tv-like UI built natively with `egui`.
- **Near Zero Performance Impact**: Hardware encoding powered by `gpu-screen-recorder`.
- **Hardware Selection**: Dynamically select which GPU handles your encoding, your microphone, and your speaker output right from the GUI.
- **Global Hotkeys**: Save replays or start manual recordings instantly with a single keypress.
- **Highly Portable**: Supports X11 and Wayland effortlessly.
- **Lightweight**: Minimal disk and memory footprint with a system tray background daemon.

## Requirements
- Linux (Wayland or X11)
- `gpu-screen-recorder` installed on your system.

## Usage
1. Launch `lapse --gui`.
2. Use the GUI to configure your replay buffer, GPU encoding, hardware devices, and hotkeys.
3. Press the hotkey (default: `F10`) to save the last X seconds of gameplay.

### Arch Linux (AUR)
You can install Lapse from the AUR using an AUR helper like `yay` or `paru`.

**Build from source:**
```bash
yay -S lapse-git
```

### Debian / Ubuntu (.deb)
To build a .deb package yourself, ensure `cargo-deb` is installed and run:
```bash
cargo deb
```

### Manual Installation
If you prefer to install manually via the provided script:
```bash
git clone https://github.com/canersin/lapse.git
cd lapse
./install.sh
```

This will automatically:
1. Install system dependencies via your package manager.
2. Install the Rust toolchain (if missing).
3. Compile the application optimally.
4. Add lapse to your background Autostart applications.
5. Create an Applications Menu shortcut.

## Uninstallation
If you wish to remove Lapse from your system, simply run:
```bash
./uninstall.sh
```
This will safely kill background services and remove all associated binaries, shortcuts, and configurations, without touching your saved video clips.

