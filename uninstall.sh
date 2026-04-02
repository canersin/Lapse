#!/bin/bash

echo "======================================"
echo "    Lapse - Uninstall Script          "
echo "======================================"

echo "Stopping Lapse background processes..."
killall lapse 2>/dev/null || true

echo "Removing binaries..."
BIN_DIR="$HOME/.local/bin"
rm -f "$BIN_DIR/lapse"

echo "Removing Desktop Entries & Autostart..."
APPS_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"
rm -f "$APPS_DIR/lapse.desktop"
rm -f "$AUTOSTART_DIR/lapse-daemon.desktop"

# Alternatively, if you want to also clean user config, ask or just do it:
# echo "Removing configuration files..."
# rm -rf "$HOME/.config/lapse"

echo "======================================"
echo "Lapse has been successfully uninstalled!"
echo "Note: Replays saved in your target folder were NOT deleted."
echo "======================================"
