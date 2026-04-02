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

### AUR (Arch Linux)
You can install Lapse from the AUR using an AUR helper like `yay` or `paru`:

**Build from source:**
```bash
yay -S lapse-git
```

**Pre-compiled binary:**
```bash
yay -S lapse-bin
```

### Debian / Ubuntu (.deb)
Download the latest `.deb` package from the [releases](https://github.com/canersin/lapse/releases) page and install it:
```bash
sudo apt install ./lapse_0.1.0_amd64.deb
```

### AppImage (Universal)
Download the `.AppImage` from the [releases](https://github.com/canersin/lapse/releases) page, make it executable, and run it:
```bash
chmod +x lapse-x86_64.AppImage
./lapse-x86_64.AppImage
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
This will safely kill background services and remove all associated binaries and shortcuts.
