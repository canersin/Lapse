#!/bin/sh
# Builds the release packages into dist/:
#
#   packaging/build-release.sh              everything
#   packaging/build-release.sh deb rpm      only some (binary tar deb rpm appimage
#                                           source aur gentoo flatpak)
#
# Needs podman (the binary is built in Ubuntu 22.04, so it runs on older systems
# too) and, for the Flatpak, org.flatpak.Builder from Flathub.

set -e
cd "$(dirname "$0")/.."
ROOT="$PWD"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
DIST="$ROOT/dist"
STAGE="$ROOT/target/release-stage"
BIN="$ROOT/target/ubuntu2204/release/Lapse"
TARBALL="$DIST/Lapse-$VERSION-x86_64.tar.gz"
UBUNTU=localhost/lapse-build:ubuntu22.04
FEDORA=localhost/lapse-build:fedora
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/lapse-build"

say() { printf '\033[1;35m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

image() { # name containerfile
    podman image exists "$1" || podman build -t "$1" -f "packaging/build/$2" packaging/build
}

in_ubuntu() {
    image "$UBUNTU" Containerfile.ubuntu
    mkdir -p "$HOME/.cargo/registry"
    podman run --rm --userns=keep-id -v "$ROOT":/src:Z -v "$HOME/.cargo/registry":/opt/cargo/registry:Z \
        -w /src -e CARGO_TARGET_DIR=/src/target/ubuntu2204 "$UBUNTU" "$@"
}

subst() { # file -> stdout, with @VERSION@ and friends filled in
    sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHA256@/${SHA256:-}/g" -e "s/@SIZE@/${SIZE:-}/g" "$1"
}

build_binary() {
    say "Binary (Ubuntu 22.04, glibc 2.35)"
    in_ubuntu cargo build --release --locked
}

need_binary() {
    [ -f "$BIN" ] || build_binary
}

build_tar() {
    need_binary
    say "Binary tarball"
    d="$STAGE/Lapse-$VERSION-x86_64"
    rm -rf "$d" && mkdir -p "$d"
    install -m755 "$BIN" "$d/lapse"
    install -m755 install.sh uninstall.sh "$d/"
    install -m644 assets/icon.png assets/lapse.svg assets/lapse-symbolic.svg assets/lapse.desktop LICENSE README.md "$d/"
    mkdir -p "$DIST"
    tar -C "$STAGE" --owner=0 --group=0 -czf "$TARBALL" "Lapse-$VERSION-x86_64"
}

build_deb() {
    need_binary
    say "Debian package"
    d="$STAGE/deb"
    rm -rf "$d"
    install -Dm755 "$BIN" "$d/usr/bin/lapse"
    install -Dm644 assets/lapse.desktop "$d/usr/share/applications/lapse.desktop"
    install -Dm644 assets/icon.png "$d/usr/share/icons/hicolor/512x512/apps/lapse.png"
    install -Dm644 assets/lapse.svg "$d/usr/share/icons/hicolor/scalable/apps/lapse.svg"
    install -Dm644 assets/lapse-symbolic.svg "$d/usr/share/icons/hicolor/symbolic/apps/lapse-symbolic.svg"
    install -Dm644 LICENSE "$d/usr/share/doc/lapse/copyright"
    SIZE="$(du -sk "$d/usr" | cut -f1)"
    mkdir -p "$d/DEBIAN"
    subst packaging/debian/control > "$d/DEBIAN/control"
    mkdir -p "$DIST"
    in_ubuntu dpkg-deb --root-owner-group --build target/release-stage/deb "dist/lapse_${VERSION}-1_amd64.deb"
}

build_rpm() {
    [ -f "$TARBALL" ] || build_tar
    say "RPM package (Fedora)"
    image "$FEDORA" Containerfile.fedora
    d="$STAGE/rpm"
    rm -rf "$d" && mkdir -p "$d/SOURCES" "$d/SPECS"
    cp "$TARBALL" "$d/SOURCES/"
    subst packaging/rpm/lapse.spec > "$d/SPECS/lapse.spec"
    podman run --rm --userns=keep-id -v "$ROOT":/src:Z -w /src "$FEDORA" \
        rpmbuild -bb --define "_topdir /src/target/release-stage/rpm" target/release-stage/rpm/SPECS/lapse.spec
    cp "$d/RPMS/x86_64/lapse-$VERSION-1.x86_64.rpm" "$DIST/"
}

appimage_runtime() {
    rt="$CACHE/runtime-x86_64"
    if [ ! -s "$rt" ]; then
        mkdir -p "$CACHE"
        curl -fsSL -o "$rt.part" https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64
        mv "$rt.part" "$rt"
    fi
    echo "$rt"
}

build_appimage() {
    need_binary
    say "AppImage"
    d="$STAGE/AppDir"
    rm -rf "$d"
    install -Dm755 "$BIN" "$d/usr/bin/lapse"
    install -Dm755 packaging/appimage/AppRun "$d/AppRun"
    install -Dm644 assets/lapse.desktop "$d/lapse.desktop"
    install -Dm644 assets/lapse.desktop "$d/usr/share/applications/lapse.desktop"
    install -Dm644 assets/icon.png "$d/lapse.png"
    install -Dm644 assets/icon.png "$d/usr/share/icons/hicolor/512x512/apps/lapse.png"
    ln -sf lapse.png "$d/.DirIcon"
    rt="$(appimage_runtime)"
    out="$DIST/Lapse-$VERSION-x86_64.AppImage"
    rm -f "$STAGE/lapse.squashfs"
    in_ubuntu mksquashfs target/release-stage/AppDir target/release-stage/lapse.squashfs \
        -root-owned -noappend -comp zstd -quiet
    cat "$rt" "$STAGE/lapse.squashfs" > "$out"
    chmod 755 "$out"
}

build_source() {
    say "Source tarball"
    mkdir -p "$DIST"
    # The files that are (or would be) in git, without build output.
    git ls-files -co --exclude-standard | sort -u | while IFS= read -r f; do
        [ -e "$f" ] && printf '%s\n' "$f"
    done > "$STAGE.files"
    tar -czf "$DIST/lapse-$VERSION-source.tar.gz" --owner=0 --group=0 \
        --transform "s,^,lapse-$VERSION/," -T "$STAGE.files"
    rm -f "$STAGE.files"
}

build_aur() {
    [ -f "$TARBALL" ] || build_tar
    say "AUR recipes"
    SHA256="$(sha256sum "$TARBALL" | cut -d' ' -f1)"
    for p in lapse-bin lapse-git; do
        mkdir -p "$DIST/aur/$p"
        subst "packaging/arch/$p/PKGBUILD" > "$DIST/aur/$p/PKGBUILD"
        (cd "$DIST/aur/$p" && makepkg --printsrcinfo > .SRCINFO)
    done
}

build_gentoo() {
    say "Gentoo ebuild"
    out="$DIST/gentoo/media-video/lapse"
    mkdir -p "$out"
    python3 -I packaging/gentoo/ebuild.py "$VERSION" > "$out/lapse-$VERSION.ebuild"
    cp packaging/gentoo/metadata.xml "$out/"
    cp "$out/lapse-$VERSION.ebuild" "$DIST/"
}

build_flatpak() {
    say "Flatpak"
    flatpak info org.flatpak.Builder >/dev/null 2>&1 || die "install it first: flatpak install --user flathub org.flatpak.Builder"
    python3 -I flatpak/cargo-sources.py
    mkdir -p "$DIST"
    # The builder can't install from Flathub inside its own sandbox; do it here.
    branch="$(sed -n "s/^runtime-version: '\(.*\)'/\1/p" com.canersin.Lapse.yml)"
    flatpak install --user --noninteractive --or-update flathub \
        "org.freedesktop.Platform//$branch" "org.freedesktop.Sdk//$branch" \
        "org.freedesktop.Sdk.Extension.rust-stable//$branch"
    # The builder's sandbox has its own user installation; point it at the real one.
    userdir="${XDG_DATA_HOME:-$HOME/.local/share}/flatpak"
    flatpak run --filesystem="$userdir" --env=FLATPAK_USER_DIR="$userdir" \
        --command=flatpak-builder org.flatpak.Builder --user \
        --force-clean --disable-rofiles-fuse --repo=build-dir/repo build-dir/app com.canersin.Lapse.yml
    flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
        build-dir/repo "$DIST/Lapse-$VERSION-x86_64.flatpak" com.canersin.Lapse
}

checksums() {
    (cd "$DIST" && sha256sum Lapse-"$VERSION"-* lapse_"$VERSION"-* lapse-"$VERSION"-* lapse-"$VERSION".ebuild 2>/dev/null > SHA256SUMS) || true
}

mkdir -p "$STAGE"
steps="$*"
[ -n "$steps" ] || steps="binary tar deb rpm appimage source aur gentoo flatpak"
for step in $steps; do
    case "$step" in
        binary) build_binary ;;
        tar) build_tar ;;
        deb) build_deb ;;
        rpm) build_rpm ;;
        appimage) build_appimage ;;
        source) build_source ;;
        aur) build_aur ;;
        gentoo) build_gentoo ;;
        flatpak) build_flatpak ;;
        *) die "unknown step: $step" ;;
    esac
done
checksums
say "Done. Packages are in dist/"
