#!/bin/sh
# Removes Lapse installed with install.sh.
#
#   ./uninstall.sh                 remove it from your user (~/.local)
#   sudo ./uninstall.sh            remove a system install (/usr/local)
#   ./uninstall.sh --prefix DIR    remove it from somewhere else
#   ./uninstall.sh --purge         also delete settings and Lapse's data
#
# Your clips are never deleted.

set -e

PREFIX=""
PURGE=0

usage() {
    sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) PREFIX="$2"; shift 2 ;;
        --prefix=*) PREFIX="${1#--prefix=}"; shift ;;
        --purge) PURGE=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

say() { printf '\033[1;35m==>\033[0m %s\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

if [ -z "$PREFIX" ]; then
    if [ "$(id -u)" -eq 0 ]; then PREFIX=/usr/local; else PREFIX="$HOME/.local"; fi
fi
BIN="$PREFIX/bin/lapse"
SHARE_DIR="$PREFIX/share"
HICOLOR="$SHARE_DIR/icons/hicolor"
MANIFEST="$SHARE_DIR/lapse/installed-files"

# Autostart entries and service files live in the user's home; Lapse knows which
# init system it set up, so let it remove them before the binary is gone.
if [ "$(id -u)" -ne 0 ]; then
    say "Removing autostart"
    if [ -x "$BIN" ]; then
        "$BIN" uninstall >/dev/null 2>&1 || true
    fi
    # Whatever the init system, the desktop autostart entry may exist too.
    rm -f "$HOME/.config/autostart/lapse.desktop" "$HOME/.config/autostart/lapse-daemon.desktop"
else
    echo "Note: run ./uninstall.sh as each user too (without sudo) to remove their autostart."
fi

say "Stopping Lapse"
pkill -x lapse 2>/dev/null || true

say "Removing files from $PREFIX"
if [ -f "$MANIFEST" ]; then
    while IFS= read -r f; do
        [ -n "$f" ] && rm -f "$f"
    done < "$MANIFEST"
    rm -f "$MANIFEST"
else
    # Installed by an older install.sh: remove the usual places.
    rm -f "$PREFIX/bin/lapse" "$PREFIX/bin/Lapse" \
        "$SHARE_DIR/applications/lapse.desktop" \
        "$HICOLOR/512x512/apps/lapse.png" \
        "$HICOLOR/scalable/apps/lapse.svg" \
        "$HICOLOR/symbolic/apps/lapse-symbolic.svg" \
        "$SHARE_DIR/licenses/lapse/LICENSE"
    rm -rf "$SHARE_DIR/sounds/lapse"
fi
rmdir "$SHARE_DIR/lapse" "$SHARE_DIR/licenses/lapse" 2>/dev/null || true

have gtk-update-icon-cache && gtk-update-icon-cache -f -t "$HICOLOR" >/dev/null 2>&1 || true
have update-desktop-database && update-desktop-database "$SHARE_DIR/applications" >/dev/null 2>&1 || true

if [ "$PURGE" -eq 1 ] && [ "$(id -u)" -ne 0 ]; then
    say "Deleting settings and data"
    rm -rf "${XDG_CONFIG_HOME:-$HOME/.config}/lapse" \
        "${XDG_STATE_HOME:-$HOME/.local/state}/lapse" \
        "${XDG_DATA_HOME:-$HOME/.local/share}/lapse"
fi

say "Lapse is removed."
if [ "$PURGE" -eq 0 ]; then
    echo "Your settings are kept in ~/.config/lapse (./uninstall.sh --purge deletes them)."
fi
echo "Your clips were not touched."
