// SPDX-License-Identifier: GPL-3.0-or-later

//! `bank_dump <file.bank> [--tree] [--wav <folder>]`: the chunk tree of a FMOD 1.08 bank and
//! its samples (a list, or WAV files), read without FMOD (`rustyac_audio::bank`).

use rustyac_audio::bank::{wav, Bank, Chunk};

fn tree(chunk: &Chunk, depth: usize) {
    println!("{}{} {}", "  ".repeat(depth), chunk.tag_str(), chunk.data.len());
    for child in chunk.children() {
        tree(&child, depth + 1);
    }
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let file = args.next().ok_or("bank_dump <file.bank> [--tree] [--wav <folder>]")?;
    let bank = Bank::open(std::path::Path::new(&file))?;
    let mut folder = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tree" => bank.chunks().iter().for_each(|c| tree(c, 0)),
            "--wav" => folder = Some(args.next().ok_or("--wav needs a folder")?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let samples = bank.samples()?;
    for (i, s) in samples.iter().enumerate() {
        println!("{i:3} {:40} {} Hz {} ch {} frames ({:.2} s)", s.name, s.rate, s.channels, s.frames, s.frames as f64 / s.rate as f64);
    }
    if let Some(folder) = folder {
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        for (i, s) in samples.iter().enumerate() {
            let name: String = s.name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
            std::fs::write(format!("{folder}/{i:03}_{name}.wav"), wav(s)).map_err(|e| e.to_string())?;
        }
        println!("{} WAV files in {folder}", samples.len());
    }
    Ok(())
}
