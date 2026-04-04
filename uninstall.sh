#!/bin/bash

echo "======================================"
echo "    Lapse - Uninstall Script          "
echo "======================================"

# Function to check if a file exists before removing it
remove_if_exists() {
    if [ -f "$1" ] || [ -d "$1" ]; then
        echo "Removing $1..."
        rm -rf "$1"
    fi
}

echo "Stopping Lapse background processes..."
killall lapse 2>/dev/null || true

echo "Removing binaries..."
BIN_DIR="$HOME/.local/bin"
remove_if_exists "$BIN_DIR/lapse"

echo "Removing Desktop Entries & Autostart..."
APPS_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"
remove_if_exists "$APPS_DIR/lapse.desktop"
remove_if_exists "$AUTOSTART_DIR/lapse-daemon.desktop"

# Optionally remove configuration files
if [[ "$*" == *"--all"* ]]; then
    echo "Removing configuration files..."
    remove_if_exists "$HOME/.config/lapse"
else
    echo ""
    echo "Note: Configuration files at ~/.config/lapse were NOT deleted."
    echo "To remove them, run: ./uninstall.sh --all"
fi

echo "======================================"
echo "Lapse has been successfully uninstalled!"
echo "Note: Replays saved in your target folder were NOT deleted."
echo "======================================"
