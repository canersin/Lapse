# Packs the release binary (Lapse-<version>-x86_64.tar.gz) for Fedora, openSUSE
# and other RPM distributions. packaging/build-release.sh builds it in Fedora.
%global debug_package %{nil}

Name:           lapse
Version:        @VERSION@
Release:        1
Summary:        Save the last moments of your game and trim clips
License:        GPL-3.0-only
URL:            https://github.com/canersin/Lapse
Source0:        Lapse-%{version}-x86_64.tar.gz
ExclusiveArch:  x86_64

# Linked libraries are found automatically; these are loaded at start-up.
Requires:       libEGL.so.1()(64bit)
Requires:       libGL.so.1()(64bit)
Requires:       libwayland-client.so.0()(64bit)
Requires:       libwayland-egl.so.1()(64bit)
Requires:       libxkbcommon.so.0()(64bit)
Requires:       libxkbcommon-x11.so.0()(64bit)
Requires:       libX11.so.6()(64bit)
Requires:       libXcursor.so.1()(64bit)
Requires:       libXrandr.so.2()(64bit)
Requires:       libXi.so.6()(64bit)
Requires:       libdbus-1.so.3()(64bit)
# Programs, by path: the package names differ between Fedora and openSUSE.
Requires:       /usr/bin/ffmpeg
Requires:       /usr/bin/ffprobe
Recommends:     gpu-screen-recorder
Recommends:     mpv

%description
Lapse keeps the last minutes of your screen in memory with GPU Screen Recorder.
Press a key and the moment is saved as a clip, in a folder named after the
game. It has a clip library, a lossless trimmer, automatic clips for
Counter-Strike 2 and Dota 2, per-game settings and Discord Rich Presence.

%prep
%setup -q -n Lapse-%{version}-x86_64

%build

%install
install -Dm755 lapse %{buildroot}%{_bindir}/lapse
install -Dm644 lapse.desktop %{buildroot}%{_datadir}/applications/lapse.desktop
install -Dm644 icon.png %{buildroot}%{_datadir}/icons/hicolor/512x512/apps/lapse.png
install -Dm644 lapse.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/lapse.svg
install -Dm644 lapse-symbolic.svg %{buildroot}%{_datadir}/icons/hicolor/symbolic/apps/lapse-symbolic.svg

%files
%license LICENSE
%doc README.md
%{_bindir}/lapse
%{_datadir}/applications/lapse.desktop
%{_datadir}/icons/hicolor/512x512/apps/lapse.png
%{_datadir}/icons/hicolor/scalable/apps/lapse.svg
%{_datadir}/icons/hicolor/symbolic/apps/lapse-symbolic.svg

%changelog
* Tue Nov 17 2026 Ersin Can Karaca <canersinkaraca@gmail.com> - @VERSION@-1
- Release @VERSION@
