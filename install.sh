#!/bin/bash

set -e

echo "======================================"
echo "    Lapse - Universal Installation    "
echo "======================================"

# 1. Install system dependencies based on package manager
echo "[1/5] Checking system dependencies..."
if command -v pacman &> /dev/null; then
    echo "Arch Linux detected. Installing dependencies via pacman..."
    sudo pacman -S --needed pkgconf alsa-lib gtk3 libayatana-appindicator gcc make
elif command -v apt-get &> /dev/null; then
    echo "Debian/Ubuntu detected. Installing dependencies via apt..."
    sudo apt-get update
    sudo apt-get install -y pkg-config libasound2-dev libgtk-3-dev libayatana-appindicator3-dev build-essential
elif command -v dnf &> /dev/null; then
    echo "Fedora detected. Installing dependencies via dnf..."
    sudo dnf install -y pkgconf-pkg-config alsa-lib-devel gtk3-devel libayatana-appindicator-devel gcc
elif command -v zypper &> /dev/null; then
    echo "openSUSE detected. Installing dependencies via zypper..."
    sudo zypper install -y pkg-config alsa-devel gtk3-devel libayatana-appindicator3-1 gcc
else
    echo "Unsupported package manager. Please install pkg-config, alsa-lib, gtk3, and libayatana-appindicator dependencies manually."
fi

# 2. Check for Rust/Cargo
echo "[2/5] Checking for Rust and Cargo..."
if ! command -v cargo &> /dev/null; then
    echo "Rust/Cargo not found. Installing Rust toolchain..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
else
    echo "Rust is already installed."
fi

# 3. Build the project
echo "[3/5] Building Lapse (Release Mode)..."
cargo build --release

# 4. Install binaries
echo "[4/5] Installing Lapse to ~/.local/bin..."
BIN_DIR="$HOME/.local/bin"
mkdir -p "$BIN_DIR"
cp target/release/lapse "$BIN_DIR/"

if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo "Note: $BIN_DIR is not in your PATH. You may need to add it to your shell configuration (e.g. ~/.bashrc or ~/.zshrc) later."
fi

# 5. Create Desktop Entry and Autostart
echo "[5/5] Creating Desktop Entries & Autostart..."
APPS_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"

mkdir -p "$APPS_DIR"
mkdir -p "$AUTOSTART_DIR"

# Application Entry (GUI)
cat <<EOF > "$APPS_DIR/lapse.desktop"
[Desktop Entry]
Name=Lapse
Comment=Lapse Game Clipper
Exec=$BIN_DIR/lapse --gui
Icon=media-record
Terminal=false
Type=Application
Categories=Utility;AudioVideo;
EOF

# KDE / GNOME Autostart (Daemon)
cat <<EOF > "$AUTOSTART_DIR/lapse-daemon.desktop"
[Desktop Entry]
Type=Application
Exec=$BIN_DIR/lapse --daemon
Hidden=false
NoDisplay=false
X-GNOME-Autostart-enabled=true
Name=Lapse Daemon
Comment=Lapse background recording daemon
EOF

echo "======================================"
echo "Installation completed successfully!"
echo "Lapse will start automatically in the background on your next login."
echo ""
echo "To start the background service right now, run:"
echo "  lapse --daemon &"
echo ""
echo "To open the user interface, click Lapse in your Application menu, or run:"
echo "  lapse --gui"
echo ""
echo "To uninstall Lapse, run:"
echo "  ./uninstall.sh"
echo "======================================"
echo "⚠️ IMPORTANT: Lapse requires 'gpu-screen-recorder' to work!"
echo "If you haven't installed it yet:"
echo "  Arch:   sudo pacman -S gpu-screen-recorder"
echo "  Others: flatpak install flathub com.dec05eba.gpu_screen_recorder"
echo "======================================"
