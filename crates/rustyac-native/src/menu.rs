// SPDX-License-Identifier: GPL-3.0-or-later

//! The launcher: a terminal menu for what AC's launcher sets before a drive (car and skin,
//! track and layout, where to start, the gearbox), the picture's size, the sound, and a server
//! to join (the local network too). Shown when `rustyac-play` starts without `--car`, `--track`,
//! `--race` or `--join` (a double-click on `rustyac.command`). Everything is kept in
//! `launcher.ini` of rustyAC's own folder, so the next start offers the same.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// What the launcher keeps.
#[derive(Clone, Debug)]
pub struct Settings {
    pub car: String,
    pub skin: String,
    pub track: String,
    pub layout: String,
    pub spawn: String,
    pub auto_shifter: bool,
    pub size: (u32, u32),
    pub sound: bool,
    pub volume: f32,
    pub server: String,
    pub name: String,
    pub team: String,
    pub password: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { car: String::new(), skin: String::new(), track: String::new(), layout: String::new(), spawn: "pit".into(), auto_shifter: true, size: (1280, 720), sound: true, volume: 1.0, server: String::new(), name: String::new(), team: String::new(), password: String::new() }
    }
}

impl Settings {
    pub fn load(file: &Path) -> Settings {
        let mut s = Settings::default();
        for line in std::fs::read_to_string(file).unwrap_or_default().lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim().to_string();
            match key.trim() {
                "car" => s.car = value,
                "skin" => s.skin = value,
                "track" => s.track = value,
                "layout" => s.layout = value,
                "spawn" => s.spawn = value,
                "auto_shifter" => s.auto_shifter = value != "0",
                "size" => {
                    if let Some((w, h)) = value.split_once('x') {
                        if let (Ok(w), Ok(h)) = (w.parse(), h.parse()) {
                            s.size = (w, h);
                        }
                    }
                }
                "sound" => s.sound = value != "0",
                "volume" => s.volume = value.parse().unwrap_or(1.0),
                "server" => s.server = value,
                "name" => s.name = value,
                "team" => s.team = value,
                "password" => s.password = value,
                _ => {}
            }
        }
        s
    }

    pub fn save(&self, file: &Path) {
        let text = format!(
            "car={}\nskin={}\ntrack={}\nlayout={}\nspawn={}\nauto_shifter={}\nsize={}x{}\nsound={}\nvolume={}\nserver={}\nname={}\nteam={}\npassword={}\n",
            self.car, self.skin, self.track, self.layout, self.spawn, self.auto_shifter as u8, self.size.0, self.size.1, self.sound as u8, self.volume, self.server, self.name, self.team, self.password
        );
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(file, text).ok();
    }
}

fn folders(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = rustyac_content::vfs::read_dir(path).unwrap_or_default().into_iter().filter(|(_, dir)| *dir).map(|(name, _)| name).collect();
    names.sort();
    names
}

/// The layouts of a track: the folders with their own `data` or `ui` (none: the track has one).
pub fn layouts(root: &Path, track: &str) -> Vec<String> {
    let base = root.join("content/tracks").join(track);
    folders(&base).into_iter().filter(|n| !matches!(n.as_str(), "ai" | "data" | "skins" | "ui" | "sfx" | "extension") && (base.join(n).join("data").is_dir() || base.join(n).join("ui").is_dir())).collect()
}

pub fn skins(root: &Path, car: &str) -> Vec<String> {
    folders(&root.join("content/cars").join(car).join("skins"))
}

fn line(prompt: &str) -> Result<String, String> {
    print!("{prompt}");
    std::io::stdout().flush().ok();
    let mut text = String::new();
    if std::io::stdin().lock().read_line(&mut text).map_err(|e| e.to_string())? == 0 {
        return Err("no answer".to_string());
    }
    Ok(text.trim().to_string())
}

/// A choice from a list: a number, a part of a name, Enter to keep `current`.
fn pick(label: &str, items: &[String], current: &str, allow_none: bool) -> Result<String, String> {
    let mut shown: Vec<&String> = items.iter().collect();
    loop {
        println!("\n{label}:");
        if allow_none {
            println!("    0  (none / the first)");
        }
        for (i, name) in shown.iter().enumerate() {
            println!("  {:3}  {}{}", i + 1, name, if name.as_str() == current { "   <- now" } else { "" });
        }
        let answer = line("number, part of a name to filter, Enter to keep: ")?;
        if answer.is_empty() {
            return Ok(current.to_string());
        }
        if let Ok(n) = answer.parse::<usize>() {
            if n == 0 && allow_none {
                return Ok(String::new());
            }
            if let Some(name) = shown.get(n.wrapping_sub(1)) {
                return Ok((*name).clone());
            }
        } else {
            let needle = answer.to_lowercase();
            let found: Vec<&String> = items.iter().filter(|i| i.to_lowercase().contains(&needle)).collect();
            match found.len() {
                0 => println!("nothing matches {answer:?}"),
                1 => return Ok(found[0].clone()),
                _ => {
                    shown = found;
                    continue;
                }
            }
        }
        shown = items.iter().collect();
    }
}

fn ask_text(label: &str, current: &str) -> Result<String, String> {
    let answer = line(&format!("{label} [{current}] (a space clears): "))?;
    Ok(if answer.is_empty() { current.to_string() } else if answer.trim().is_empty() { String::new() } else { answer })
}

/// What the launcher decided.
pub enum Choice {
    Play(Settings),
    Quit,
}

/// The launcher's loop; returns when the player starts a drive or leaves.
pub fn launcher(root: &Path, own: &Path) -> Result<Choice, String> {
    let file: PathBuf = own.join("launcher.ini");
    let mut s = Settings::load(&file);
    let cars = folders(&root.join("content/cars"));
    let tracks = folders(&root.join("content/tracks"));
    if cars.is_empty() || tracks.is_empty() {
        return Err("the game's content/cars or content/tracks is empty".to_string());
    }
    if !cars.contains(&s.car) {
        s.car = cars[0].clone();
        s.skin.clear();
    }
    if !tracks.contains(&s.track) {
        s.track = tracks[0].clone();
        s.layout.clear();
    }
    loop {
        let skin = if s.skin.is_empty() { "(first)".to_string() } else { s.skin.clone() };
        let layout = if s.layout.is_empty() { "(default)".to_string() } else { s.layout.clone() };
        println!("\n=== rustyAC ===");
        println!("  1  Car        {}", s.car);
        println!("  2  Skin       {skin}");
        println!("  3  Track      {}", s.track);
        println!("  4  Layout     {layout}");
        println!("  5  Start at   {}   (pit / hotlap / start)", s.spawn);
        println!("  6  Gearbox    {}", if s.auto_shifter { "automatic" } else { "manual" });
        println!("  7  Window     {}x{}", s.size.0, s.size.1);
        println!("  8  Sound      {}, volume {:.0}%", if s.sound { "on" } else { "off" }, s.volume * 100.0);
        println!("  9  Server     {}", if s.server.is_empty() { "(none: drive alone)".to_string() } else { format!("{} as {:?}", s.server, s.name) });
        println!("  Enter  Drive      q  Quit");
        let answer = line("> ")?;
        match answer.as_str() {
            "" => {
                s.save(&file);
                return Ok(Choice::Play(s));
            }
            "q" | "Q" => {
                s.save(&file);
                return Ok(Choice::Quit);
            }
            "1" => {
                let car = pick("Car", &cars, &s.car, false)?;
                if car != s.car {
                    s.skin.clear();
                }
                s.car = car;
            }
            "2" => s.skin = pick("Skin", &skins(root, &s.car), &s.skin, true)?,
            "3" => {
                let track = pick("Track", &tracks, &s.track, false)?;
                if track != s.track {
                    s.layout.clear();
                }
                s.track = track;
            }
            "4" => {
                let l = layouts(root, &s.track);
                if l.is_empty() {
                    println!("this track has one layout");
                } else {
                    s.layout = pick("Layout", &l, &s.layout, true)?;
                }
            }
            "5" => {
                let spawns = ["pit".to_string(), "hotlap".to_string(), "start".to_string()];
                s.spawn = pick("Start at", &spawns, &s.spawn, false)?;
            }
            "6" => s.auto_shifter = !s.auto_shifter,
            "7" => {
                let text = ask_text("Window size WIDTHxHEIGHT", &format!("{}x{}", s.size.0, s.size.1))?;
                match text.split_once('x').and_then(|(w, h)| Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))) {
                    Some((w, h)) if (320..=7680).contains(&w) && (240..=4320).contains(&h) => s.size = (w, h),
                    _ => println!("write it as 1920x1080"),
                }
            }
            "8" => {
                let on = ask_text("Sound on (1) or off (0)", if s.sound { "1" } else { "0" })?;
                s.sound = on != "0";
                let v = ask_text("Volume in percent", &format!("{:.0}", s.volume * 100.0))?;
                match v.parse::<f32>() {
                    Ok(p) if (0.0..=200.0).contains(&p) => s.volume = p / 100.0,
                    _ => println!("a number from 0 to 200"),
                }
            }
            "9" => {
                s.server = ask_text("Server HOST:PORT (acServer, e.g. 192.168.1.20:9600)", &s.server)?;
                if !s.server.is_empty() {
                    if crate::online::parse_address(&s.server).is_err() {
                        println!("write it as host:port");
                        s.server.clear();
                    } else {
                        s.name = ask_text("Your name", &if s.name.is_empty() { crate::online::default_name() } else { s.name.clone() })?;
                        s.team = ask_text("Team", &s.team)?;
                        s.password = ask_text("Server password", &s.password)?;
                        println!("(the car on a server is the server's: the one chosen here is asked for)");
                    }
                }
            }
            other => println!("{other:?}: choose 1-9, Enter or q"),
        }
        s.save(&file);
    }
}
