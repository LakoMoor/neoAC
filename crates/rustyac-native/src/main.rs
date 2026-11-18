// SPDX-License-Identifier: GPL-3.0-or-later

//! `rustyac-play`: rustyAC on macOS and Linux. A window (winit) and a wgpu surface (Metal,
//! Vulkan) around the loop the browser build runs (`rustyac-web`'s `Session`, AC's renderer
//! `render::ac` on kgl's wgpu backend, AC's GUI): the keys, the mouse, the pause, the cameras.
//! Not here yet: sound (AC's FMOD is a Windows library), the pad, AC's launcher, Python apps.
//!
//! `rustyac-play [--car <id>] [--track <id>] [--layout <l>] [--spawn pit|hotlap|start]
//! [--race <race.ini>] [--size 1280x720]`, with the game's folder in `AC_ROOT` (or Steam's usual
//! place). Without `--car` / `--track`, the first of each in the game's folder.

#[cfg(all(unix, not(target_family = "wasm")))]
mod app;
#[cfg(all(unix, not(target_family = "wasm")))]
mod debug;
#[cfg(all(unix, not(target_family = "wasm")))]
mod keys;
#[cfg(all(unix, not(target_family = "wasm")))]
mod menu;
#[cfg(all(unix, not(target_family = "wasm")))]
mod online;

#[cfg(all(unix, not(target_family = "wasm")))]
fn main() {
    if let Err(message) = app::run() {
        eprintln!("rustyac-play: {message}");
        std::process::exit(1);
    }
}

#[cfg(not(all(unix, not(target_family = "wasm"))))]
fn main() {
    eprintln!("rustyac-play is the macOS and Linux build; on Windows run rustyac.exe");
    std::process::exit(2);
}
