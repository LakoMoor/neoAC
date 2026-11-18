// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyac-launch: the launcher for macOS and Linux, drawn with GPUI. `--ac-root <folder>` names
//! the game's folder (else AC_ROOT, the folder it sits in, the current folder, Steam's).

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod csp;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod data;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod ini;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod mods;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod state;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod ui;

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn main() {
    let mut args = std::env::args().skip(1);
    let mut root = None;
    let mut page = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--ac-root" => root = args.next().map(std::path::PathBuf::from),
            "--page" => page = args.next(),
            "--help" | "-h" => {
                println!("rustyac-launch [--ac-root <folder>] [--page drive|cars|tracks|assists|controls|settings]: choose the car, track, skin, assists and keys, then Drive starts rustyac-play");
                return;
            }
            other => {
                eprintln!("unknown option {other} (--help)");
                std::process::exit(2);
            }
        }
    }
    ui::run(root, page);
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn main() {
    println!("rustyac-launch is for macOS and Linux; on Windows use rustyac-launcher");
}
