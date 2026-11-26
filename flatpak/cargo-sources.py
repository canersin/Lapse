#!/usr/bin/env python3
"""Lists every crate in Cargo.lock as a Flatpak source, so cargo builds offline.

Run it again after Cargo.lock changes:

    python3 flatpak/cargo-sources.py

It writes flatpak/cargo-sources.json, which io.github.canersin.Lapse.yml includes.
Only crates.io packages are supported (Lapse has no git dependencies).
"""

import json
import pathlib
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
VENDOR = "cargo/vendor"


def main() -> int:
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    sources = []
    for pkg in lock.get("package", []):
        source = pkg.get("source")
        if source is None:
            continue  # Lapse itself
        if not source.startswith("registry+"):
            print(f"unsupported source for {pkg['name']}: {source}", file=sys.stderr)
            return 1
        name, version, checksum = pkg["name"], pkg["version"], pkg["checksum"]
        dest = f"{VENDOR}/{name}-{version}"
        sources.append({
            "type": "archive",
            "archive-type": "tar-gzip",
            "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
            "sha256": checksum,
            "dest": dest,
        })
        sources.append({
            "type": "inline",
            "contents": json.dumps({"package": checksum, "files": {}}),
            "dest": dest,
            "dest-filename": ".cargo-checksum.json",
        })
    sources.append({
        "type": "inline",
        "contents": (
            "[source.vendored-sources]\n"
            f'directory = "{VENDOR}"\n\n'
            "[source.crates-io]\n"
            'replace-with = "vendored-sources"\n'
        ),
        "dest": "cargo",
        "dest-filename": "config.toml",
    })
    out = ROOT / "flatpak" / "cargo-sources.json"
    out.write_text(json.dumps(sources, indent=2) + "\n")
    print(f"{len(sources) // 2} crates -> {out.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
