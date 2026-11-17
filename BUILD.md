# Building Lapse

Lapse builds on both glibc and musl systems.

## Building
To build for your current Linux distribution:
```bash
cargo build --release
```
The binary will be located at `target/release/Lapse`. `./install.sh` and `make` build and
install it in one step.

## musl (Alpine, Void musl)
Lapse must be linked dynamically on musl: the window loads the graphics libraries (EGL,
Wayland, X11) at run time, which a static binary can't do. The Rust that Alpine and Void
ship already links dynamically. rustup's `x86_64-unknown-linux-musl` target links
statically by default, so turn that off:

```bash
RUSTFLAGS="-C target-feature=-crt-static" cargo build --release
```

`./install.sh` and `make` do this by themselves on musl.
