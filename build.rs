//! Compiles the gpu-screen-recorder plugin (src/focus/gsr_plugin.rs) that Lapse embeds, and
//! passes LAPSE_ISSUES_URL (from the environment or a local, git-ignored `.env`) to the code.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/focus/gsr_plugin.rs");
    println!("cargo:rerun-if-changed=src/focus/shared.rs");

    issues_url();

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

/// Where "Report a bug" sends people: the environment wins over `.env`; without either,
/// src/report.rs falls back to the project's GitHub issues.
fn issues_url() {
    const KEY: &str = "LAPSE_ISSUES_URL";
    println!("cargo:rerun-if-changed=.env");
    println!("cargo:rerun-if-env-changed={KEY}");
    let from_file = || {
        std::fs::read_to_string(".env").ok()?.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == KEY).then(|| v.trim().trim_matches('"').to_string())
        })
    };
    if let Some(url) = std::env::var(KEY).ok().or_else(from_file).filter(|u| !u.is_empty()) {
        println!("cargo:rustc-env={KEY}={}", url.trim_end_matches('/'));
    }
}
