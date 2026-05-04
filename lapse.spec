Name:           lapse
Version:        0.1.1
Release:        1%{?dist}
Summary:        Modern and lightweight game clipper for Linux
License:        GPLv3
URL:            https://github.com/canersin/lapse
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo, rust, gtk3-devel, libwayland-client-devel
Requires:       gtk3, gpu-screen-recorder, libappindicator-gtk3

%description
Lapse is a native screen recording application in Rust for Wayland/X11, 
designed to be lightweight and easy to use.

%prep
# Bu kısım normalde kaynak kodunu açar, biz yerelde olduğumuz için atlayabiliriz
# veya basitçe dosyaları kopyalayabiliriz.

%build
cargo build --release

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}/usr/bin
mkdir -p %{buildroot}/usr/share/applications
mkdir -p %{buildroot}/usr/share/pixmaps
mkdir -p %{buildroot}/usr/share/sounds/lapse

install -m 755 target/release/Lapse %{buildroot}/usr/bin/lapse
install -m 644 assets/lapse.desktop %{buildroot}/usr/share/applications/lapse.desktop
install -m 644 assets/icon.png %{buildroot}/usr/share/pixmaps/lapse.png
install -m 644 assets/shutter.ogg %{buildroot}/usr/share/sounds/lapse/shutter.ogg

%files
/usr/bin/lapse
/usr/share/applications/lapse.desktop
/usr/share/pixmaps/lapse.png
/usr/share/sounds/lapse/shutter.ogg

%changelog
* Mon May 04 2026 Ersin Can Karaca <canersinkaraca@gmail.com> - 0.1.1-1
- Modernized hotkey system and updated to v0.1.1
