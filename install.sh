#!/bin/sh
# Lapse installer.
#
# From the source tree it builds Lapse with cargo and installs it; from a release
# tarball (which holds a ready `lapse` binary) it only installs.
#
#   ./install.sh                 build and install for your user (~/.local)
#   sudo ./install.sh --no-build install a build made earlier, for every user (/usr/local)
#   ./install.sh --prefix DIR    install somewhere else
#
# Nothing is installed with your package manager; missing tools are listed with the
# command that installs them.

set -e

PREFIX=""
BUILD=1

usage() {
    sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) PREFIX="$2"; shift 2 ;;
        --prefix=*) PREFIX="${1#--prefix=}"; shift ;;
        --no-build) BUILD=0; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

cd "$(dirname "$0")"

say() { printf '\033[1;35m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# Distribution id from os-release (gentoo, arch, debian, fedora, ...).
DISTRO="$( (. /etc/os-release 2>/dev/null; echo "${ID:-} ${ID_LIKE:-}") )"

# The command that installs packages on this system, for the hints below.
pkg_hint() {
    case "$DISTRO" in
        *gentoo*) echo "sudo emerge --ask $1" ;;
        *arch*|*artix*|*manjaro*|*endeavouros*) echo "sudo pacman -S --needed $2" ;;
        *fedora*|*rhel*|*nobara*) echo "sudo dnf install $3" ;;
        *suse*) echo "sudo zypper install $3" ;;
        *debian*|*ubuntu*) echo "sudo apt install $4" ;;
        *void*) echo "sudo xbps-install $5" ;;
        *) echo "install: $2" ;;
    esac
}

# --- Build -------------------------------------------------------------------

BIN_SOURCE=""
if [ -f ./lapse ] && [ ! -f ./Cargo.toml ]; then
    # Release tarball: the binary is already here.
    BIN_SOURCE="./lapse"
elif [ "$BUILD" -eq 0 ]; then
    BIN_SOURCE="target/release/Lapse"
    [ -f "$BIN_SOURCE" ] || die "no build found at $BIN_SOURCE. Run ./install.sh without --no-build first (as your user)."
else
    missing=""
    have cargo || missing="$missing rust"
    have pkg-config || have pkgconf || missing="$missing pkg-config"
    if have pkg-config && ! pkg-config --exists alsa 2>/dev/null; then
        missing="$missing alsa"
    fi
    if [ -n "$missing" ]; then
        echo "Building Lapse needs:$missing" >&2
        echo "Install them with:" >&2
        echo "  $(pkg_hint "dev-lang/rust dev-util/pkgconf media-libs/alsa-lib" \
            "rust pkgconf alsa-lib" "cargo pkgconf alsa-lib-devel" \
            "cargo pkg-config libasound2-dev" "rust pkg-config alsa-lib-devel")" >&2
        echo "(or Rust from https://rustup.rs)" >&2
        exit 1
    fi
    if [ "$(id -u)" -eq 0 ]; then
        warn "building as root. Better: ./install.sh as your user, then sudo ./install.sh --no-build for a system install."
    fi
    say "Building Lapse (release). The first build takes a few minutes..."
    # Build for the host explicitly, so a cargo config that sets another target
    # (e.g. musl) doesn't send the binary elsewhere.
    HOST="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')"
    LOCKED=""
    [ -f Cargo.lock ] && LOCKED="--locked"
    if [ -n "$HOST" ]; then
        cargo build --release $LOCKED --target "$HOST"
        mkdir -p target/release
        cp -f "target/$HOST/release/Lapse" target/release/Lapse
    else
        cargo build --release $LOCKED
    fi
    BIN_SOURCE="target/release/Lapse"
fi
[ -f "$BIN_SOURCE" ] || die "compiled binary not found at $BIN_SOURCE"

# --- Where to ----------------------------------------------------------------

if [ -z "$PREFIX" ]; then
    if [ "$(id -u)" -eq 0 ]; then PREFIX=/usr/local; else PREFIX="$HOME/.local"; fi
fi
BIN_DIR="$PREFIX/bin"
SHARE_DIR="$PREFIX/share"
HICOLOR="$SHARE_DIR/icons/hicolor"
MANIFEST="$SHARE_DIR/lapse/installed-files"

# First file found among the source tree and the release tarball layout.
asset() {
    for f in "assets/$1" "$1"; do
        [ -f "$f" ] && { echo "$f"; return 0; }
    done
    return 1
}

INSTALLED=""
put() { # mode source destination
    mkdir -p "$(dirname "$3")"
    # Copy next to it and rename, so a running Lapse doesn't block the update.
    cp -f "$2" "$3.new"
    chmod "$1" "$3.new"
    mv -f "$3.new" "$3"
    INSTALLED="$INSTALLED
$3"
}

say "Installing to $PREFIX"
put 755 "$BIN_SOURCE" "$BIN_DIR/lapse"
ln -sf lapse "$BIN_DIR/Lapse"
INSTALLED="$INSTALLED
$BIN_DIR/Lapse"

icon="$(asset icon.png || asset lapse.png || true)"
[ -n "$icon" ] && put 644 "$icon" "$HICOLOR/512x512/apps/lapse.png"
svg="$(asset lapse.svg || true)"
[ -n "$svg" ] && put 644 "$svg" "$HICOLOR/scalable/apps/lapse.svg"
sym="$(asset lapse-symbolic.svg || true)"
[ -n "$sym" ] && put 644 "$sym" "$HICOLOR/symbolic/apps/lapse-symbolic.svg"
lic="$(asset LICENSE || true)"
[ -n "$lic" ] && put 644 "$lic" "$SHARE_DIR/licenses/lapse/LICENSE"

desktop="$(mktemp)"
cat > "$desktop" <<EOF
[Desktop Entry]
Name=Lapse
GenericName=Replay Recorder
Comment=Save the last moments of your game and trim clips
Exec=$BIN_DIR/lapse --gui
Icon=lapse
Terminal=false
Type=Application
Categories=AudioVideo;Video;Recorder;
Keywords=replay;record;screen;clip;gaming;gpu;wayland;
StartupWMClass=lapse
EOF
put 644 "$desktop" "$SHARE_DIR/applications/lapse.desktop"
rm -f "$desktop"

# What we installed, so uninstall.sh removes exactly that.
mkdir -p "$(dirname "$MANIFEST")"
printf '%s\n' "$INSTALLED" | sed '/^$/d' > "$MANIFEST"

have gtk-update-icon-cache && gtk-update-icon-cache -f -t "$HICOLOR" >/dev/null 2>&1 || true
have update-desktop-database && update-desktop-database "$SHARE_DIR/applications" >/dev/null 2>&1 || true

# --- Checks ------------------------------------------------------------------

say "Lapse is installed: $BIN_DIR/lapse"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$BIN_DIR is not in your PATH. Add it, e.g. in ~/.profile: export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac

if ! have gpu-screen-recorder; then
    case "$DISTRO" in
        *gentoo*) hint="it is in the GURU overlay: sudo eselect repository enable guru && sudo emaint sync -r guru && sudo emerge --ask media-video/gpu-screen-recorder" ;;
        *) hint="$(pkg_hint media-video/gpu-screen-recorder gpu-screen-recorder gpu-screen-recorder gpu-screen-recorder gpu-screen-recorder)" ;;
    esac
    warn "gpu-screen-recorder is missing; Lapse records with it. $hint"
fi
if ! have ffmpeg || ! have ffprobe; then
    warn "ffmpeg is missing; trimming and exports need it. $(pkg_hint media-video/ffmpeg ffmpeg ffmpeg ffmpeg ffmpeg)"
fi

if pgrep -x lapse >/dev/null 2>&1; then
    echo "Lapse is running the old version. Restart it with:  pkill -x lapse; lapse start"
else
    echo "Start it from your app menu, or run:  lapse start"
fi
