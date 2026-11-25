#!/bin/sh
# Builds the Windows NSIS installer for Lapse into dist/
# Run from repository root: ./packaging/windows/build.sh

set -e

cd "$(dirname "$0")/../.."
ROOT="$PWD"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [ -z "$VERSION" ]; then
    echo "error: could not read version from Cargo.toml" >&2
    exit 1
fi

DIST="$ROOT/dist"
STAGE="$ROOT/target/windows-stage"
CACHE="$ROOT/packaging/windows/cache"
FFMPEG_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-win64-lgpl-8.1.zip"
FFMPEG_ZIP="$CACHE/ffmpeg-n8.1-latest-win64-lgpl-8.1.zip"
DEBIAN_IMAGE="docker.io/library/debian:trixie"

mkdir -p "$DIST" "$STAGE" "$CACHE"

echo "==> Cross-building lapse.exe for Windows..."
if [ -x "./wincheck.sh" ]; then
    ./wincheck.sh build
else
    podman run --rm -v "$ROOT":/w:z -v lapse-wincargo:/usr/local/cargo/registry -v lapse-wintarget:/t -w /w lapse-wincheck \
        sh -c "CARGO_TARGET_DIR=/t cargo build --release --target x86_64-pc-windows-gnu --message-format short 2>&1"
fi

if [ -f "$ROOT/target/x86_64-pc-windows-gnu/release/Lapse.exe" ]; then
    cp "$ROOT/target/x86_64-pc-windows-gnu/release/Lapse.exe" "$STAGE/lapse.exe"
elif [ -f "$ROOT/target/x86_64-pc-windows-gnu/release/lapse.exe" ]; then
    cp "$ROOT/target/x86_64-pc-windows-gnu/release/lapse.exe" "$STAGE/lapse.exe"
else
    echo "==> Copying lapse.exe from lapse-wintarget volume..."
    podman run --rm -v lapse-wintarget:/t:ro -v "$STAGE":/stage:z lapse-wincheck \
        sh -c 'cp /t/x86_64-pc-windows-gnu/release/[Ll]apse.exe /stage/lapse.exe'
fi

if [ ! -f "$STAGE/lapse.exe" ]; then
    echo "error: lapse.exe not found in staging area ($STAGE/lapse.exe)" >&2
    exit 1
fi

echo "==> Fetching FFmpeg Windows release..."
if [ ! -f "$FFMPEG_ZIP" ]; then
    echo "Downloading FFmpeg from $FFMPEG_URL..."
    if command -v curl >/dev/null 2>&1; then
        curl -L -f -o "$FFMPEG_ZIP" "$FFMPEG_URL"
    elif command -v wget >/dev/null 2>&1; then
        wget -O "$FFMPEG_ZIP" "$FFMPEG_URL"
    else
        podman run --rm -v "$CACHE":/cache:z "$DEBIAN_IMAGE" \
            sh -c "apt-get update -qq && apt-get install -y --no-install-recommends curl ca-certificates && curl -L -f -o /cache/ffmpeg-n8.1-latest-win64-lgpl-8.1.zip '$FFMPEG_URL'"
    fi
fi

echo "==> Extracting FFmpeg binaries and license..."
if command -v unzip >/dev/null 2>&1; then
    unzip -j -o "$FFMPEG_ZIP" "*/bin/ffmpeg.exe" "*/bin/ffprobe.exe" -d "$STAGE" || \
    unzip -j -o "$FFMPEG_ZIP" "*/ffmpeg.exe" "*/ffprobe.exe" -d "$STAGE"
    unzip -p "$FFMPEG_ZIP" "*/LICENSE.txt" > "$STAGE/FFMPEG-LICENSE.txt"
else
    podman run --rm -v "$CACHE":/cache:ro -v "$STAGE":/stage:z "$DEBIAN_IMAGE" \
        sh -c 'apt-get update -qq && apt-get install -y --no-install-recommends unzip && \
               (unzip -j -o /cache/ffmpeg-n8.1-latest-win64-lgpl-8.1.zip "*/bin/ffmpeg.exe" "*/bin/ffprobe.exe" -d /stage || \
                unzip -j -o /cache/ffmpeg-n8.1-latest-win64-lgpl-8.1.zip "*/ffmpeg.exe" "*/ffprobe.exe" -d /stage) && \
               unzip -p /cache/ffmpeg-n8.1-latest-win64-lgpl-8.1.zip "*/LICENSE.txt" > /stage/FFMPEG-LICENSE.txt'
fi

cp "$ROOT/LICENSE" "$STAGE/LICENSE"

echo "==> Generating assets and running makensis in container ($DEBIAN_IMAGE)..."
podman run --rm \
    -v "$ROOT":/src:z \
    -w /src \
    -e VERSION="$VERSION" \
    "$DEBIAN_IMAGE" \
    sh -c '
        set -e
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -qq
        apt-get install -y --no-install-recommends nsis imagemagick librsvg2-bin

        echo "==> Converting assets/lapse.svg to lapse.ico..."
        TMP_DIR="$(mktemp -d)"
        rsvg-convert -w 16 -h 16 assets/lapse.svg -o "$TMP_DIR/icon-16.png"
        rsvg-convert -w 32 -h 32 assets/lapse.svg -o "$TMP_DIR/icon-32.png"
        rsvg-convert -w 48 -h 48 assets/lapse.svg -o "$TMP_DIR/icon-48.png"
        rsvg-convert -w 256 -h 256 assets/lapse.svg -o "$TMP_DIR/icon-256.png"

        CONVERT_CMD="convert"
        command -v magick >/dev/null 2>&1 && CONVERT_CMD="magick"

        $CONVERT_CMD "$TMP_DIR/icon-16.png" \
                     "$TMP_DIR/icon-32.png" \
                     "$TMP_DIR/icon-48.png" \
                     "$TMP_DIR/icon-256.png" \
                     target/windows-stage/lapse.ico
        rm -rf "$TMP_DIR"

        echo "==> Compiling NSIS installer..."
        makensis -DVERSION="$VERSION" \
                 -DSTAGE="/src/target/windows-stage" \
                 -DOUTDIR="/src/dist" \
                 packaging/windows/lapse.nsi
    '

SETUP_EXE="$DIST/Lapse-$VERSION-setup.exe"
if [ ! -f "$SETUP_EXE" ]; then
    echo "error: installer not found at $SETUP_EXE" >&2
    exit 1
fi

SHA256="$(sha256sum "$SETUP_EXE" | awk '{print $1}')"

echo ""
echo "============================================================"
echo "Windows setup build complete!"
echo "Path:    $SETUP_EXE"
echo "SHA-256: $SHA256"
echo "============================================================"
