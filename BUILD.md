# Building Lapse

Lapse can be built for both `glibc` (dynamic) and `musl` (static) targets.

## Building for glibc (Default)
To build for your current Linux distribution:
```bash
cargo build --release
```
The binary will be located at `target/release/Lapse`.

## Building for musl (Static Linking)
To create a fully static binary that works on any Linux distribution (including Alpine):

1.  **Install musl-tools**:
    - Ubuntu/Debian: `sudo apt install musl-tools`
    - Arch: `sudo pacman -S musl`

2.  **Add the musl target to Rust**:
    ```bash
    rustup target add x86_64-unknown-linux-musl
    ```

3.  **Build**:
    ```bash
    cargo build --release --target x86_64-unknown-linux-musl
    ```
The binary will be located at `target/x86_64-unknown-linux-musl/release/Lapse`.