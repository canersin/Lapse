#!/usr/bin/env python3
"""Prints the Gentoo ebuild for a version, with the crate list from Cargo.lock.

    python3 packaging/gentoo/ebuild.py 2.3.0 > lapse-2.3.0.ebuild

packaging/build-release.sh gentoo writes it to dist/gentoo/media-video/lapse/.
"""

import pathlib
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent.parent

TEMPLATE = """\
# Copyright 2026 Ersin Can Karaca
# Distributed under the terms of the GNU General Public License v3

EAPI=8

CRATES="
{crates}
"

RUST_MIN_VER="1.85.0"

inherit cargo desktop optfeature xdg

DESCRIPTION="Save the last moments of your game and trim clips (replay recorder)"
HOMEPAGE="https://github.com/canersin/Lapse"
SRC_URI="
\thttps://github.com/canersin/Lapse/archive/refs/tags/v${{PV}}.tar.gz -> ${{P}}.tar.gz
\t${{CARGO_CRATE_URIS}}
"
S="${{WORKDIR}}/Lapse-${{PV}}"

LICENSE="GPL-3"
# Licenses of the bundled crates.
LICENSE+=" 0BSD Apache-2.0 Apache-2.0-with-LLVM-exceptions BSD Boost-1.0 CC0-1.0 ISC MIT MPL-2.0 OFL-1.1 Unicode-3.0 UoI-NCSA ZLIB"
SLOT="0"
KEYWORDS="~amd64"

DEPEND="
\tmedia-libs/alsa-lib
\tsys-apps/dbus
"
# The graphics and window libraries are loaded at start-up, not linked.
RDEPEND="
\t${{DEPEND}}
\tmedia-libs/libglvnd
\tdev-libs/wayland
\tx11-libs/libxkbcommon[X]
\tx11-libs/libX11
\tx11-libs/libXcursor
\tx11-libs/libXrandr
\tx11-libs/libXi
\tmedia-video/ffmpeg
"
BDEPEND="virtual/pkgconfig"

QA_FLAGS_IGNORED="usr/bin/lapse"

src_install() {{
\tnewbin "$(cargo_target_dir)/Lapse" lapse
\tdomenu assets/lapse.desktop
\tnewicon -s 512 assets/icon.png lapse.png
\tdoicon -s scalable assets/lapse.svg
\tinsinto /usr/share/icons/hicolor/symbolic/apps
\tdoins assets/lapse-symbolic.svg
\teinstalldocs
}}

pkg_postinst() {{
\txdg_pkg_postinst
\toptfeature "recording (required, from the GURU overlay)" media-video/gpu-screen-recorder
\toptfeature "file dialogs" sys-apps/xdg-desktop-portal gnome-extra/zenity
}}
"""


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    crates = sorted(
        f"\t{p['name']}@{p['version']}"
        for p in lock["package"]
        if p.get("source", "").startswith("registry+")
    )
    print(TEMPLATE.format(crates="\n".join(crates)), end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())
