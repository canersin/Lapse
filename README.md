# Lapse

A replay recorder and clip editor for Linux games, written in Rust.

Lapse keeps the last minutes of your screen in a buffer. Press a key, and that
buffer is saved as a clip, sorted into a folder for the game you were playing.
You can then trim it, export it for Discord or as a vertical video, or share
a link to it, all from the same window.

Recording is done by [gpu-screen-recorder](https://git.dec05eba.com/gpu-screen-recorder/about/),
which encodes on the graphics card, so the cost in frame rate is very small.
Lapse works on Wayland and X11.

- [Features](#features)
- [Installation](#installation)
- [First start](#first-start)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Command line](#command-line)
- [Settings](#settings)
- [Clip editor](#clip-editor)
- [Sharing](#sharing)
- [Discord status](#discord-status)
- [Automatic clips](#automatic-clips)
- [Dual GPU recording](#dual-gpu-recording)
- [How it works](#how-it-works)
- [Files and folders](#files-and-folders)
- [Troubleshooting](#troubleshooting)
- [Uninstalling](#uninstalling)
- [Building from source](#building-from-source)
- [License](#license)

## Features

- **Replay buffer** kept in RAM or on disk, from a few seconds up to an hour.
- **Hardware encoding** with H.264, HEVC or AV1 (VAAPI on AMD and Intel, NVENC on NVIDIA), with optional HDR.
- **Global shortcuts** that work in games on Wayland (through the GlobalShortcuts portal) and on X11.
- **Gamepad shortcut**: save a replay by holding a button combination on your controller.
- **Game detection**: Steam games are recognised by their app id, and each game gets its own clip folder and cover art. Games Lapse does not know can be added by hand.
- **Games-only mode**: the buffer runs only while a game is open.
- **Automatic clips** for Counter-Strike 2 and Dota 2, through the games' official Game State Integration.
- **Separate audio tracks** for desktop audio and microphone, or record only the apps you choose.
- **Clip editor** with a filmstrip timeline, frame stepping, markers, highlight detection and fullscreen playback.
- **Exports**: lossless cut, exact cut, Discord (under 10 MB), vertical 9:16, GIF and MP3.
- **Sharing** through a public link (Cloudflare tunnel) or a Discord webhook.
- **Discord status**: "Counter-Strike 2 with Lapse" with the game's cover on your profile.
- **Library** with search, favourites, rename, merging several clips into one, and an optional storage limit.
- **Audio sync** correction for setups where sound and picture drift apart.
- **Dual GPU recording**: play on one graphics card and encode on the other.
- **Self-healing recorder**: if gpu-screen-recorder crashes (driver reset, monitor unplugged), Lapse starts it again.
- **Command line** for everything the shortcuts do, for scripts and window manager bindings.
- English and Turkish interface.

## Installation

Lapse needs these programs at run time. The packages below pull them in for
you; with the plain binary you install them yourself.

| Program | Needed for | Required |
|---|---|---|
| `gpu-screen-recorder` | Recording | Yes |
| `ffmpeg` (with `ffprobe`) | Trimming, exports, thumbnails | Yes |
| `xdg-desktop-portal` (with your desktop's backend) or `zenity` | File dialogs | Recommended |
| `cloudflared` | Public share links | No, Lapse downloads it the first time you share |

### Arch Linux and derivatives (AUR)

Two packages are available. Use an AUR helper such as `yay` or `paru`.

Compiled on your machine from the latest source:

```bash
yay -S lapse-git
```

Ready-made binary, no compiling:

```bash
yay -S lapse-bin
```

Updates come with your normal system update (`yay -Syu`).

### Debian, Ubuntu, Linux Mint, Pop!_OS (.deb)

Download `lapse_<version>-1_amd64.deb` from the
[releases page](https://github.com/canersin/Lapse/releases/latest), then:

```bash
sudo apt install ./lapse_<version>-1_amd64.deb
```

`apt` installs the dependencies listed in the package. gpu-screen-recorder is
not in the Debian and Ubuntu archives; install it from
[its own instructions](https://git.dec05eba.com/gpu-screen-recorder/about/)
or use the Flatpak or AppImage below.

### Fedora, openSUSE, Nobara (.rpm)

Download `lapse-<version>-1.x86_64.rpm` from the
[releases page](https://github.com/canersin/Lapse/releases/latest), then:

```bash
sudo dnf install ./lapse-<version>-1.x86_64.rpm
```

On openSUSE use `sudo zypper install ./lapse-<version>-1.x86_64.rpm`.

Fedora's own `ffmpeg-free` can't encode H.264 in software, so exports then always
use the GPU. For software exports too, switch to the full ffmpeg from
[RPM Fusion](https://rpmfusion.org/Configuration):
`sudo dnf swap ffmpeg-free ffmpeg --allowerasing`. Install gpu-screen-recorder
from [its own instructions](https://git.dec05eba.com/gpu-screen-recorder/about/)
if your distribution doesn't have it.

### Gentoo (compiled with make)

Download `lapse-<version>.ebuild` from the
[releases page](https://github.com/canersin/Lapse/releases/latest) and put it in
a local overlay:

```bash
sudo mkdir -p /var/db/repos/local/media-video/lapse
sudo cp lapse-<version>.ebuild /var/db/repos/local/media-video/lapse/
cd /var/db/repos/local/media-video/lapse
sudo ebuild lapse-<version>.ebuild manifest
sudo emerge --ask media-video/lapse
```

Portage downloads the source and the Rust crates and compiles Lapse.

If you have not set up a local overlay yet, see
[the Gentoo wiki](https://wiki.gentoo.org/wiki/Creating_an_ebuild_repository).

Or build it with the install script, from the source tarball
(`lapse-<version>-source.tar.gz` on the releases page) or a git clone:

```bash
tar xzf lapse-<version>-source.tar.gz
cd lapse-<version>
./install.sh
```

It checks for Rust, `pkg-config` and ALSA first and prints the `emerge` command
for anything missing, builds Lapse, and installs it into `~/.local` for your
user. For every user, build as yourself and then run `sudo ./install.sh --no-build`
(installs into `/usr/local`). gpu-screen-recorder is in the GURU overlay:

```bash
sudo eselect repository enable guru
sudo emaint sync -r guru
sudo emerge --ask media-video/gpu-screen-recorder
```

To build by hand with `make` instead (on Gentoo or any other system):

```bash
git clone https://github.com/canersin/Lapse.git
cd Lapse
make
sudo make install          # to /usr/local
```

Without `sudo`, `make install` installs to `~/.local` for your user only.
`make uninstall` removes it again (use the same `sudo` or not as for install).

### musl systems (Alpine, Void musl)

The ready-made packages need glibc, so on musl Lapse is built from source.
Install the build tools and the libraries the window loads.

Alpine:

```bash
sudo apk add cargo pkgconf alsa-lib-dev make libxkbcommon-x11 ffmpeg
```

Void musl:

```bash
sudo xbps-install cargo pkg-config alsa-lib-devel make libxkbcommon-x11 ffmpeg gpu-screen-recorder
```

Then build and install it for your user:

```bash
git clone https://github.com/canersin/Lapse.git
cd Lapse
./install.sh
```

Rust from your distribution or from [rustup](https://rustup.rs) both work.
Alpine doesn't package gpu-screen-recorder; build it from
[its source](https://git.dec05eba.com/gpu-screen-recorder/about/), or use the
Flatpak, which includes it.

### NixOS and Nix

Run it once without installing:

```bash
nix run github:canersin/Lapse
```

Install it into your profile:

```bash
nix profile install github:canersin/Lapse
```

On NixOS, add the flake to your system configuration:

```nix
{
  inputs.lapse.url = "github:canersin/Lapse";

  outputs = { nixpkgs, lapse, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      modules = [
        ({ pkgs, ... }: {
          environment.systemPackages = [ lapse.packages.${pkgs.system}.default ];
          programs.gpu-screen-recorder.enable = true;
        })
      ];
    };
  };
}
```

`programs.gpu-screen-recorder.enable` gives gpu-screen-recorder the capability
it needs to capture the screen without asking for a password each time.

### Flatpak

Download `Lapse-<version>-x86_64.flatpak` from the
[releases page](https://github.com/canersin/Lapse/releases/latest), then:

```bash
flatpak install --user ./Lapse-<version>-x86_64.flatpak
flatpak run com.canersin.Lapse
```

gpu-screen-recorder is included in the Flatpak, and ffmpeg comes with its runtime.

### AppImage

Download `Lapse-<version>-x86_64.AppImage` from the
[releases page](https://github.com/canersin/Lapse/releases/latest), then:

```bash
chmod +x Lapse-<version>-x86_64.AppImage
./Lapse-<version>-x86_64.AppImage
```

It starts the recording service and opens the window. It runs on any
distribution with glibc 2.35 or newer (Ubuntu 22.04, Debian 12 and later);
gpu-screen-recorder and ffmpeg must be installed on the system. On musl systems
(Alpine, Void musl) build from source (see above) or use the Flatpak.

### Plain binary (any distribution)

Download `Lapse-<version>-x86_64.tar.gz` from the
[releases page](https://github.com/canersin/Lapse/releases/latest), then:

```bash
tar xzf Lapse-<version>-x86_64.tar.gz
cd Lapse-<version>-x86_64
./install.sh
```

This installs Lapse for your user into `~/.local/bin`, with a menu entry and
icon. Run it with `sudo` to install for every user into `/usr/local/bin`.
Make sure `~/.local/bin` is in your `PATH`.

### Which one should I pick?

- Your distribution has a package above (AUR, .deb, .rpm, Gentoo, Nix): use it. Updates then come with the rest of your system.
- Alpine, Void musl or another musl system: build from source with `./install.sh`.
- You want it sandboxed: Flatpak.
- You want one file and nothing installed: AppImage.
- Anything else: the plain binary.

## First start

Start Lapse from your application menu, or run:

```bash
lapse start
```

This starts the background service (the daemon) and opens the window. On the
first start a short setup asks for the language, the quality and buffer length,
and shows your shortcuts. Closing the window does not stop recording; Lapse keeps running in the
tray. The tray menu has **Show Lapse**, **Save Replay Now**,
**Start / Stop Recording**, **Open Clips Folder** and **Quit Lapse**.

On Wayland, GNOME and KDE ask once whether Lapse may use global shortcuts.
Accept, or the keys will only work while the Lapse window has focus.

Lapse starts by itself when you log in. You can turn this off under
Settings > Autostart.

Drag the sidebar's right edge to make it wider or narrower. The button in its
top-left corner shrinks it to icons and back; dragging the edge almost shut does
the same. Lapse remembers the width.

## Keyboard shortcuts

### Recording (global, work inside games)

| Shortcut | Action |
|---|---|
| `F10` | Save the replay buffer as a clip |
| `F9` | Start or stop a manual recording |
| `Shift+F10` | Save only the last 30 seconds (length can be changed) |
| `Ctrl+F10` | Take a screenshot (PNG) of the recorded monitor |
| `Shift+F9` | Mark this moment; the mark shows on the timeline of the saved clip |

All of them can be changed under Settings > Hotkeys. Click **Change Key**,
then press the new combination. `Ctrl`, `Alt`, `Shift` and `Super` can be
combined with any key. The last three shortcuts can be turned off together
with **More shortcuts**.

A gamepad can save the replay too (default: hold **Select + R3**). Turn it on
under Settings > Hotkeys > Gamepad. Reading the controller needs your user to
be in the `input` group:

```bash
sudo usermod -aG input $USER
```

Log out and in again afterwards.

### Clip editor

| Key | Action |
|---|---|
| `Space` or `K` | Play / pause |
| `Left` / `Right` | Back / forward 5 seconds |
| `J` / `L` | Back / forward 10 seconds |
| `,` / `.` | Previous / next frame |
| `Up` / `Down` | Volume |
| `M` | Mute |
| `0` to `9` | Jump to 0% to 90% of the clip |
| `I` / `O` | Set the start / end of the selection |
| `F` or double-click | Fullscreen (`Esc` leaves it) |

## Command line

Every command talks to the running daemon. Start it first with `lapse start`.

| Command | What it does |
|---|---|
| `lapse start` | Start the daemon and open the window |
| `lapse start --no-gui` | Start only the daemon |
| `lapse stop` | Stop the daemon |
| `lapse gui` | Open the window (brings it to the front if it is already open) |
| `lapse clip` | Save the replay buffer (same as `F10`) |
| `lapse save-last [seconds]` | Save only the last seconds, e.g. `lapse save-last 30` |
| `lapse screenshot` | Save a PNG of the recorded monitor |
| `lapse mark` | Mark this moment for the next saved clip |
| `lapse status` | Show whether the daemon runs, what it records, and any share link |
| `lapse clips` | List saved clips |
| `lapse share [clip]` | Get a public link for a clip (the newest one if none is given) |
| `lapse reload` | Make the daemon re-read the configuration file |
| `lapse config` | Print the configuration file and its path |
| `lapse open-config` | Open the configuration file in `$EDITOR` |
| `lapse list-keys` | List key names that can be used for shortcuts |
| `lapse doctor` | Check GPU, audio, init system and tools, and print what is missing |
| `lapse gpus` | Show the graphics cards and the dual GPU setup |
| `lapse run -- <command>` | Start a game on the game GPU (see [Dual GPU recording](#dual-gpu-recording)) |
| `lapse uninstall` | Remove the autostart entry (keeps clips and settings) |
| `lapse help` | Show this list |

The commands make it easy to bind Lapse in a window manager. For example in
Hyprland:

```
bind = , F10, exec, lapse clip
bind = SHIFT, F10, exec, lapse save-last 30
```

or in Sway:

```
bindsym F10 exec lapse clip
```

## Settings

Settings are grouped in tabs. Changes take effect when you press
**Save and Apply**.

### Recording

- **Quick quality**: Light, Balanced, High, Ultra or Custom. Each sets the codec quality, bitrate and frame rate together, and shows how much memory the buffer will use.
- **Replay buffer storage**: RAM is fastest. Disk saves memory, which helps with long or 4K buffers.
- **HDR**: record 10-bit HEVC or AV1. Only useful when HDR is on for your monitor; HDR clips look pale on normal screens and on many websites.
- **Graphics card**: which GPU captures and encodes. **Auto** uses the card your monitor is connected to.
- **Video codec**: AV1 gives the best quality for the size, HEVC is widely supported, H.264 plays everywhere.
- **Quality and bitrate**: a fixed bitrate profile, or constant quality.
- **Maximum bitrate**: the encoder never goes above this. Calm scenes use less, so clips are usually smaller.
- **Buffer duration**: how many seconds the replay buffer holds (up to 3600).
- **Frame rate**: up to 500 FPS.
- **Monitor**: which screen to record. **Auto** follows the focused screen.
- **Advanced**: [Dual GPU recording](#dual-gpu-recording). Not needed on most systems.
- **When a clip is saved**:
  - **Play a sound**: a short shutter sound. **Choose file...** sets your own sound (ogg, wav, mp3 or flac, cut after 4 seconds); **Reset** goes back to the shutter.
  - **Show a notification**: shows the clip name with **Open** and **Trim** buttons.
  - **Clear the buffer after saving**: off means two saves close together share footage; on means each clip only has what happened since the last save.

### Games

- **What to record**: **Desktop** keeps the buffer running all the time. **Games only** starts it when a game opens and stops it on the desktop.
- **Your games**: add games Lapse does not recognise. **Detect by** is any word from the game's process name, command line or window title. Each game can have its own frame rate and bitrate (0 means use the Recording settings).
- **Auto clips**: see [Automatic clips](#automatic-clips).

### Audio

- **Desktop output**: the PipeWire or PulseAudio device whose sound is recorded.
- **Microphone**: the input device, or None.
- **Separate tracks**: desktop sound and microphone become two audio tracks, so you can mute your voice later.
- **Record only these apps**: pick running apps (for example the game and Discord) to record only their sound. Leave empty for all desktop sound.
- **Audio sync**: if sound arrives before or after the picture in your clips, shift it here (in milliseconds). **Measure from the last clip** compares loud sounds with sudden changes on screen and suggests a value; it works best with gunshots or explosions.

### Hotkeys

- The recording shortcuts listed under [Keyboard shortcuts](#keyboard-shortcuts), and the length used by **Save the last seconds**.
- **Clips folder**: where clips are saved. Default: `~/Videos/Lapse`.
- **Gamepad**: on/off and which button combination saves.
- **Shortcut system**: **Auto** uses the Wayland portal when available and X11 otherwise. Force one of them if shortcuts do not work.

### Services

- **Launch on login**: starts Lapse from your desktop's autostart (`~/.config/autostart/lapse.desktop`), inside the session, so it can reach the screen. Window managers that don't run autostart entries (Hyprland, sway) need `exec lapse --daemon` in their config.
- **Storage limit**: delete the oldest clips when the folder grows past a size. Off by default. Starred clips are never deleted.
- **Sharing and privacy**: whether Lapse may fall back to third-party SSH tunnels (localhost.run, serveo.net) when Cloudflare fails.
- **Discord**:
  - **Show on my profile**: while a game is open, your Discord status reads "Counter-Strike 2 with Lapse", with the game's cover and the Lapse logo. Off by default. See [Discord status](#discord-status).
  - **Webhook link**: used by **Send to Discord**. It is stored only in your configuration file.

### Appearance

- **Language**: English or Turkish.
- **Colour palette** for the window.

### Updates

- **Check on startup**: look for a new release when the window opens.
- **Check for updates**: check now.

## Clip editor

Click a clip in the library to open it. From top to bottom:

- **Name**: click the pencil to rename the clip.
- **Player**: click to play or pause, double-click or press `F` for fullscreen.
- **Timeline**: a strip of frames from the clip. Drag the two handles to select a part. Marks you set with `Shift+F9` appear as small chips, and in Counter-Strike 2 and Dota 2 (with auto clips on) every kill gets a red crosshair mark; click one to jump there. **Find highlights** looks for the loudest moments.
- **Selection**: exact start and end, quick buttons for the last 10, 15 or 30 seconds, and a loop switch to preview only the selection.
- **Export**:

| Preset | Result |
|---|---|
| Fast | Lossless stream copy. Instant, but the cut can land up to about 2 seconds off (on the nearest keyframe). |
| Precise | Re-encoded, cut on the exact frame. |
| Discord | Re-encoded to fit Discord's 10 MB limit. |
| Vertical | 9:16 crop for Shorts, Reels and TikTok, with a slider to choose which part of the picture stays. |
| GIF | Animated GIF. |
| MP3 | Audio only. |

## Sharing

**Share** on a clip offers two ways:

- **Public link (Cloudflare proxy)**: Lapse serves the clip from your computer and opens a Cloudflare tunnel to it. Anyone with the link can watch it while Lapse runs; nothing is uploaded to a server. This uses Cloudflare's `cloudflared`. If it isn't installed, Lapse downloads the official release from GitHub the first time you share (about 40 MB), checks it against the published SHA-256, and keeps it in `~/.local/share/lapse/bin`. A copy from your package manager is used instead when there is one.
- **Send to Discord**: uploads the clip to the channel of your webhook. Clips over 10 MB are first shrunk with the Discord preset. If a webhook is set, public links are also posted there.

To make a webhook in Discord: Channel settings > Integrations > Webhooks >
New Webhook > Copy Webhook URL. Paste it under Settings > Services > Discord.

## Discord status

With Settings > Services > Discord > **Show on my profile** on, Lapse tells
the Discord app on your computer what you are playing:

- The status line and the member list read "Counter-Strike 2 with Lapse".
- The profile card shows the game's cover (for Steam games) with the Lapse logo in its corner, how long you have been playing, and how many clips you saved in this game.
- A **Get Lapse** button that opens the Lapse GitHub page can be turned off under the same setting.
- Nothing is shown while no game is open.

This goes through Discord's local connection on your computer; no account,
token or internet access is involved on Lapse's side. The Discord app
(native, Flatpak, Snap or Vesktop) must be running.

## Automatic clips

Lapse can save clips by itself in **Counter-Strike 2** and **Dota 2**. It
uses Game State Integration, the official interface Valve provides for
tournament overlays and tools: the game sends its state to Lapse. Nothing is
injected into the game, and it is allowed by Valve.

1. Open Settings > Games > Auto clips and turn the game on.
2. Click **Connect**. Lapse writes a small configuration file into the game folder.
3. Restart the game if it was running.

Dota 2 also needs the launch option `-gamestateintegration`
(Steam > Dota 2 > Properties > Launch options).

When the card shows **Receiving live data**, clips save themselves:

- **Counter-Strike 2**: after a round where you reach the chosen number of kills (2, 3, 4 or Ace). The clip runs until the next round begins, so the end of the round is in it, and never ends sooner than 8 seconds after your last kill. Kills after the round is decided (exit frags) are marked but do not start a new clip.
- **Dota 2**: after a multi-kill (Double, Triple, Ultra or Rampage). The clip runs until the fight is over.

**Before the first kill** sets, for each game, how much of the build-up is kept
(5 to 30 seconds). Clips are named after the map or hero and the kills, for
example `dust2_3K` or `axe_Triple_Kill`.

## Dual GPU recording

gpu-screen-recorder always captures and encodes on the card your monitor is
plugged into. To play on one card and record on another:

1. Plug your monitor into the card that should record (the encoder GPU).
2. Under Settings > Recording > Advanced, turn on **Dual GPU recording** and pick the game GPU and the encoder GPU.
3. Start games on the game GPU. In Steam, set the game's launch options to:

   ```
   lapse run -- %command%
   ```

   This sets `DRI_PRIME` and `MESA_VK_DEVICE_SELECT` for the game. Frames reach the encoder GPU over PCIe (PRIME).

Run `lapse gpus` to check the setup.

## How it works

Lapse is one program that runs in two roles.

**The daemon** (`lapse --daemon`, started by `lapse start`) runs in the
background:

- It starts gpu-screen-recorder in replay mode, which keeps the last N seconds of encoded video in RAM or on disk. Nothing is written as a file until you save.
- It listens for shortcuts: the Wayland GlobalShortcuts portal, X11 key grabs, and gamepads through `/dev/input`.
- When you save, it sends gpu-screen-recorder a signal, which writes the buffer to a file. Lapse then trims it to the exact length you asked for, applies the audio sync shift, renames it, moves it into the game's folder, makes a thumbnail, plays the sound and shows the notification.
- It watches running processes and Steam to know which game is open, and restarts gpu-screen-recorder if it stops unexpectedly.
- It serves Game State Integration data for automatic clips on a local port.
- The tray icon lives here. It is a StatusNotifierItem with a symbolic icon, so the panel draws it in its own colours like the other tray icons.

**The window** (`lapse gui`) is a separate process that talks to the daemon
over a Unix socket (`$XDG_RUNTIME_DIR/lapse.sock`). Closing it never stops
recording. Only one window is open at a time; starting it again brings the
open one to the front.

Trimming and exports run `ffmpeg`. A Fast export copies the video stream
without re-encoding, which is why it is instant and lossless.

## Files and folders

| Path | Contents |
|---|---|
| `~/Videos/Lapse/` | Clips, in one folder per game (can be changed) |
| `~/Videos/Lapse/.thumbnails/` | Thumbnails and markers |
| `~/.config/lapse/config.toml` | Settings |
| `~/.local/state/lapse/recorder.log` | gpu-screen-recorder output, useful when recording fails |
| `$XDG_RUNTIME_DIR/lapse.sock` | Socket between the window, the CLI and the daemon |

The configuration file can be edited by hand. Run `lapse reload` afterwards.

## Troubleshooting

Start with:

```bash
lapse doctor
```

It checks the graphics card, audio devices, init system and required tools,
and tells you what is missing.

- **Nothing is recorded**: look at `~/.local/state/lapse/recorder.log`. Check that `gpu-screen-recorder` is installed and can run on your card.
- **Shortcuts do nothing in games on Wayland**: you may have declined the portal prompt. Allow Lapse in your desktop's shortcut settings, or bind `lapse clip` in your compositor (see [Command line](#command-line)).
- **Shortcuts do nothing on Hyprland, Sway or other wlroots compositors**: these have no GlobalShortcuts portal for most setups. Bind the CLI commands in the compositor's configuration.
- **Sound is early or late in clips**: Settings > Audio > Audio sync.
- **The gamepad shortcut does nothing**: your user must be in the `input` group (see [Keyboard shortcuts](#keyboard-shortcuts)).
- **Auto clips do nothing**: the card must say **Receiving live data**. Restart the game after **Connect**; Dota 2 also needs `-gamestateintegration`.

## Uninstalling

First remove the autostart entry:

```bash
lapse uninstall
```

Then remove the program the same way you installed it:

| Installed with | Remove with |
|---|---|
| AUR | `yay -R lapse-git` or `yay -R lapse-bin` |
| .deb | `sudo apt remove lapse` |
| .rpm | `sudo dnf remove lapse` |
| Gentoo | `sudo emerge --deselect media-video/lapse && sudo emerge --depclean` |
| Nix | `nix profile remove lapse`, or remove it from your NixOS configuration |
| Flatpak | `flatpak uninstall com.canersin.Lapse` |
| AppImage | Delete the file |
| `install.sh` | `./uninstall.sh` from the same folder (`sudo` for a system install; `--purge` also deletes settings) |
| `make install` | `make uninstall` (with `sudo` if you installed with it) |

Moving from `install.sh` to a package? Remove the old copy first. Otherwise the
menu entry and the `lapse` command keep starting the old one from `~/.local`.

Your clips and settings are kept. Delete `~/Videos/Lapse` and
`~/.config/lapse` to remove them too.

## Building from source

You need Rust (stable, through [rustup](https://rustup.rs) or your
distribution), `pkg-config`, and the development files for ALSA:

| Distribution | Command |
|---|---|
| Arch | `sudo pacman -S --needed rust pkgconf alsa-lib` |
| Debian / Ubuntu | `sudo apt install cargo pkg-config libasound2-dev` |
| Fedora | `sudo dnf install cargo pkgconf alsa-lib-devel` |
| Gentoo | `sudo emerge --ask dev-lang/rust media-libs/alsa-lib` |
| Alpine | `sudo apk add cargo pkgconf alsa-lib-dev make libxkbcommon-x11` |
| Void | `sudo xbps-install cargo pkg-config alsa-lib-devel make libxkbcommon-x11` |

It builds on glibc and on musl. Then:

```bash
git clone https://github.com/canersin/Lapse.git
cd Lapse
cargo build --release
./target/release/Lapse start
```

`make` builds the same way, and `make install` installs it (see
[Gentoo](#gentoo-compiled-with-make)). `cargo test` runs the tests.

To build every release package into `dist/`:

```bash
make packages
```

That runs `packaging/build-release.sh`, which needs `podman`. The binary is built
in an Ubuntu 22.04 container so that it also runs on older distributions; the
.deb, .rpm (in a Fedora container), AppImage, tar.gz, source tarball, AUR
recipes (`dist/aur/`), Gentoo ebuild and Flatpak follow from it. Single ones:
`make deb`, `make rpm`, `make appimage`, `make tar`, `make aur`, `make gentoo`,
`make flatpak` (the Flatpak needs `flatpak install --user flathub org.flatpak.Builder`).

| File | What it is |
|---|---|
| `packaging/arch/lapse-bin/PKGBUILD`, `packaging/arch/lapse-git/PKGBUILD` | AUR recipes |
| `packaging/debian/control` | .deb metadata |
| `packaging/rpm/lapse.spec` | .rpm recipe |
| `packaging/gentoo/ebuild.py` | writes the ebuild, with the crates from `Cargo.lock` |
| `packaging/appimage/AppRun` | AppImage start script |
| `com.canersin.Lapse.yml`, `flatpak/` | Flatpak manifest; `flatpak/cargo-sources.py` lists the crates |
| `flake.nix`, `nix/package.nix` | Nix flake |

## Reporting problems

If you run into an issue or want to confirm that Lapse works on your setup, you can report it directly from the app or CLI:

- **In the GUI**: Open the **About** tab and find the **Report a problem** card. Clicking **Report a bug** generates `lapse-report.txt`, opens the folder containing it, and opens a prefilled GitHub issue in your browser. Clicking **It works on my PC** opens a compatibility report with your system details prefilled.
- **In the CLI**: Run `lapse report`. It prints your system details, generates `lapse-report.txt`, and outputs direct links to create a bug report or compatibility report on GitHub.

The report file contains your Lapse version, OS and kernel / Windows build, desktop session, detected GPU, install method, recorder backend, active capture settings, and the last 150 lines of recent logs (`daemon.log`, `recorder.log`, `gui.log`). For privacy, your home directory is replaced with `~`, your user name is replaced with `<user>`, and Discord webhook URLs are redacted.

"It works on my PC" compatibility reports are always welcome because we cannot test every GPU and OS combination, and these reports help build the compatibility list.

## License

Lapse is free software under the [GNU General Public License v3.0](LICENSE).

Source code, releases and bug reports: <https://github.com/canersin/Lapse>
