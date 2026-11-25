#!/bin/sh
# Type-checks Lapse for Windows inside a container that has the mingw toolchain.
podman run --rm -v "$(cd "$(dirname "$0")" && pwd)":/w:z -v lapse-wincargo:/usr/local/cargo/registry -v lapse-wintarget:/t -w /w lapse-wincheck \
  sh -c "CARGO_TARGET_DIR=/t cargo ${1:-check} --release --target x86_64-pc-windows-gnu --message-format short 2>&1"
