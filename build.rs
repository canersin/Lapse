//! Compiles the gpu-screen-recorder plugin (src/focus/gsr_plugin.rs) that Lapse embeds.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/focus/gsr_plugin.rs");
    println!("cargo:rerun-if-changed=src/focus/shared.rs");

    let var = |name: &str| std::env::var(name).unwrap_or_default();
    if var("CARGO_CFG_TARGET_OS") != "linux" {
        return;
    }
    let mut cmd = Command::new(var("RUSTC"));
    cmd.args(["--edition=2021", "--crate-type=cdylib", "--crate-name=lapse_gsr_plugin", "-Copt-level=3", "-Cpanic=abort", "-Cstrip=symbols"])
        .arg("--target")
        .arg(var("TARGET"))
        .arg("-o")
        .arg(format!("{}/gsr_plugin.so", var("OUT_DIR")))
        .arg("src/focus/gsr_plugin.rs");
    if var("CARGO_CFG_TARGET_ENV") == "musl" {
        cmd.arg("-Ctarget-feature=-crt-static");
    }
    if let Ok(linker) = std::env::var("RUSTC_LINKER") {
        cmd.arg(format!("-Clinker={linker}"));
    }
    let status = cmd.status().expect("failed to run rustc");
    assert!(status.success(), "compiling the gpu-screen-recorder plugin failed");
}
