# Windows Packaging for Lapse

This directory contains the NSIS setup script and build automation for packaging Lapse on Windows.

## Overview

The build process produces a standalone Windows installer (`Lapse-<version>-setup.exe`) containing:
- `lapse.exe`: The Lapse application binary.
- `ffmpeg.exe` & `ffprobe.exe`: Pinned Windows build of FFmpeg (v7.1).
- `FFMPEG-LICENSE.txt`: The FFmpeg LGPL license text.
- `lapse.ico`: Multi-resolution application icon (16, 32, 48, 256 px).

The installer supports English and Turkish languages with an initial language picker, provides a silent install mode (`/S`) for package managers such as winget, creates Start Menu and optional Desktop shortcuts, registers the uninstaller under `HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse`, and configures autostart via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

## How to Build

### Prerequisites

- Linux host system with [Podman](https://podman.io/) installed.
- Internet access during the first build to pull container images and download the FFmpeg build.

### Running the Build

From the repository root on Linux:

```sh
./packaging/windows/build.sh
```

The script performs the following steps:
1. Reads the current version from `Cargo.toml`.
2. Cross-compiles `lapse.exe` targeting `x86_64-pc-windows-gnu` using the MingW container toolchain (`wincheck.sh`).
3. Downloads the pinned FFmpeg Windows build into `packaging/windows/cache/` (cached to avoid redownloading).
4. Extracts `ffmpeg.exe`, `ffprobe.exe`, and `LICENSE.txt` (saved as `FFMPEG-LICENSE.txt`) into `target/windows-stage/`.
5. Converts `assets/lapse.svg` into multi-resolution `lapse.ico` (16, 32, 48, 256) inside a Debian Trixie container (`docker.io/library/debian:trixie`) using `rsvg-convert` and ImageMagick.
6. Invokes `makensis` (NSIS 3) inside the container with the staged files and parameters.
7. Generates `dist/Lapse-<version>-setup.exe` and outputs its path and SHA-256 hash.

## Build Artifacts and Caches

- `packaging/windows/cache/`: Stores downloaded external archives (such as the FFmpeg zip) so they are preserved across builds.
- `dist/`: Contains the generated Windows installer executable.
- `target/windows-stage/`: Staging directory where binaries, licenses, and icons are assembled before packaging.

These directories contain generated or downloaded build artifacts and are not tracked in version control.

## FFmpeg LGPL License Notice

Lapse bundles prebuilt binaries of FFmpeg (`ffmpeg.exe`, `ffprobe.exe`) from the BtbN release:
- Release archive: `ffmpeg-n8.1-latest-win64-lgpl-8.1.zip`
- Source repository: <https://github.com/BtbN/FFmpeg-Builds>

In accordance with the **GNU Lesser General Public License (LGPL)**:
- These binaries are built under LGPL terms and do not incorporate GPL-only code.
- The FFmpeg license is included as `FFMPEG-LICENSE.txt` and installed into the application directory alongside the binaries.
- FFmpeg runs as an independent external process called by Lapse; it is not statically linked into the Lapse binary.
- Source code for FFmpeg can be obtained from <https://ffmpeg.org/> or the upstream BtbN release page.
