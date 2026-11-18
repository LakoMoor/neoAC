// SPDX-License-Identifier: GPL-3.0-or-later
include!("../../packaging/version_res.rs");

fn main() {
    // `ac_wgpu`: AC's renderer runs on kgl's wgpu backend (the browser, macOS and Linux, or the
    // `wgpu` feature on Windows for the native proofs of Full mode)
    println!("cargo:rustc-check-cfg=cfg(ac_wgpu)");
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let (os, family) = (var("CARGO_CFG_TARGET_OS"), var("CARGO_CFG_TARGET_FAMILY"));
    if std::env::var_os("CARGO_FEATURE_WGPU").is_some() || os == "macos" || os == "linux" || (family == "wasm" && os == "unknown") {
        println!("cargo:rustc-cfg=ac_wgpu");
    }
    version_res("rustyac", "rustyac.exe", "rustyAC");
}
