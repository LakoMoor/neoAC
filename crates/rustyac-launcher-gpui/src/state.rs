// SPDX-License-Identifier: GPL-3.0-or-later

//! What the launcher keeps: the drive's choices (`launcher.ini`, the same file the terminal
//! menu of `rustyac-play` uses), the assists (`cfg/assists.ini`) and the keys
//! (`cfg/controls.ini`), both as AC's own files so `rustyac-play` reads them as the game does.

use std::path::{Path, PathBuf};

use crate::ini::Ini;

/// rustyAC's own folder, where `rustyac-play` keeps its files too.
pub fn own_folder() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/rustyAC")
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")).join("rustyAC")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub car: String,
    pub skin: String,
    pub track: String,
    pub layout: String,
    pub spawn: String,
    /// a weather preset folder of content/weather, and the hour of day (8..19)
    pub weather: String,
    pub hour: u32,
    /// the whole track's grip: 1 dry, less for a wet road
    pub grip: f32,
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
        Settings { car: String::new(), skin: String::new(), track: String::new(), layout: String::new(), spawn: "pit".into(), weather: "3_clear".into(), hour: 13, grip: 1.0, size: (1280, 720), sound: true, volume: 1.0, server: String::new(), name: String::new(), team: String::new(), password: String::new() }
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
                "weather" => s.weather = value,
                "grip" => s.grip = value.parse().unwrap_or(1.0f32).clamp(0.4, 1.0),
                "hour" => s.hour = value.parse().unwrap_or(13).clamp(0, 23),
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

    /// Keeps `auto_shifter` of the terminal menu in step with the assists (read back from them).
    pub fn save(&self, file: &Path, auto_shifter: bool) {
        let text = format!(
            "car={}\nskin={}\ntrack={}\nlayout={}\nspawn={}\nweather={}\ngrip={}\nhour={}\nauto_shifter={}\nsize={}x{}\nsound={}\nvolume={}\nserver={}\nname={}\nteam={}\npassword={}\n",
            self.car, self.skin, self.track, self.layout, self.spawn, self.weather, self.grip, self.hour, auto_shifter as u8, self.size.0, self.size.1, self.sound as u8, self.volume, self.server, self.name, self.team, self.password
        );
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(file, text).ok();
    }
}

/// The driving aids of `assists.ini [ASSISTS]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Assists {
    pub abs: i32,
    pub traction_control: i32,
    pub stability: i32,
    pub auto_shifter: bool,
    pub auto_clutch: bool,
    pub auto_blip: bool,
    pub ideal_line: bool,
    pub damage: i32,
    pub fuel_rate: f32,
    pub tyre_wear: f32,
    pub slipstream: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Newbie,
    Amateur,
    Pro,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Newbie, Level::Amateur, Level::Pro];

    pub fn label(self) -> &'static str {
        match self {
            Level::Newbie => "Newbie",
            Level::Amateur => "Amateur",
            Level::Pro => "Pro",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Level::Newbie => "Every aid on: ABS, traction and stability control, automatic gearbox and clutch. No damage, no wear.",
            Level::Amateur => "Light aids: ABS and traction control on low, half the stability control, automatic gearbox, some wear and damage.",
            Level::Pro => "No aids: manual gearbox and clutch, full damage, tyre wear and fuel use. Just you and the car.",
        }
    }

    pub fn assists(self) -> Assists {
        match self {
            Level::Newbie => Assists { abs: 2, traction_control: 2, stability: 100, auto_shifter: true, auto_clutch: true, auto_blip: true, ideal_line: true, damage: 0, fuel_rate: 0.0, tyre_wear: 0.0, slipstream: true },
            Level::Amateur => Assists { abs: 1, traction_control: 1, stability: 50, auto_shifter: true, auto_clutch: true, auto_blip: true, ideal_line: true, damage: 50, fuel_rate: 1.0, tyre_wear: 1.0, slipstream: true },
            Level::Pro => Assists { abs: 0, traction_control: 0, stability: 0, auto_shifter: false, auto_clutch: false, auto_blip: false, ideal_line: false, damage: 100, fuel_rate: 1.0, tyre_wear: 1.0, slipstream: true },
        }
    }
}

impl Assists {
    pub fn level(&self) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.assists() == *self)
    }

    pub fn load(ini: &Ini) -> Assists {
        let flag = |k: &str, d: i32| ini.get_i32("ASSISTS", k, d) != 0;
        Assists {
            abs: ini.get_i32("ASSISTS", "ABS", 2).clamp(0, 2),
            traction_control: ini.get_i32("ASSISTS", "TRACTION_CONTROL", 2).clamp(0, 2),
            stability: ini.get_i32("ASSISTS", "STABILITY_CONTROL", 100).clamp(0, 100),
            auto_shifter: flag("AUTO_SHIFTER", 1),
            auto_clutch: flag("AUTO_CLUTCH", 1),
            auto_blip: flag("AUTO_BLIP", 1),
            ideal_line: flag("IDEAL_LINE", 1),
            damage: ini.get_i32("ASSISTS", "DAMAGE", 0).clamp(0, 100),
            fuel_rate: ini.get_f32("ASSISTS", "FUEL_RATE", 0.0).clamp(0.0, 5.0),
            tyre_wear: ini.get_f32("ASSISTS", "TYRE_WEAR", 0.0).clamp(0.0, 5.0),
            slipstream: flag("SLIPSTREAM", 1),
        }
    }

    pub fn store(&self, ini: &mut Ini) {
        let b = |v: bool| (v as i32).to_string();
        ini.set("ASSISTS", "ABS", &self.abs.to_string());
        ini.set("ASSISTS", "TRACTION_CONTROL", &self.traction_control.to_string());
        ini.set("ASSISTS", "STABILITY_CONTROL", &self.stability.to_string());
        ini.set("ASSISTS", "AUTO_SHIFTER", &b(self.auto_shifter));
        ini.set("ASSISTS", "AUTO_CLUTCH", &b(self.auto_clutch));
        ini.set("ASSISTS", "AUTO_BLIP", &b(self.auto_blip));
        ini.set("ASSISTS", "IDEAL_LINE", &b(self.ideal_line));
        ini.set("ASSISTS", "DAMAGE", &self.damage.to_string());
        ini.set("ASSISTS", "VISUALDAMAGE", &b(self.damage > 0));
        ini.set("ASSISTS", "FUEL_RATE", &format!("{}", self.fuel_rate));
        ini.set("ASSISTS", "TYRE_WEAR", &format!("{}", self.tyre_wear));
        ini.set("ASSISTS", "SLIPSTREAM", &b(self.slipstream));
    }
}

/// One rebindable key: its label, and where `controls.ini` keeps it.
pub struct Binding {
    pub label: &'static str,
    pub section: &'static str,
    pub key: &'static str,
    pub default: i32,
}

pub const BINDINGS: &[Binding] = &[
    Binding { label: "Throttle", section: "KEYBOARD", key: "GAS", default: 0x26 },
    Binding { label: "Brake", section: "KEYBOARD", key: "BRAKE", default: 0x28 },
    Binding { label: "Steer left", section: "KEYBOARD", key: "LEFT", default: 0x25 },
    Binding { label: "Steer right", section: "KEYBOARD", key: "RIGHT", default: 0x27 },
    Binding { label: "Shift up", section: "GEARUP", key: "KEY", default: 0x20 },
    Binding { label: "Shift down", section: "GEARDN", key: "KEY", default: 0xA2 },
    Binding { label: "Handbrake", section: "HANDBRAKE", key: "KEY", default: -1 },
    Binding { label: "Start engine", section: "STARTER", key: "KEY", default: -1 },
    Binding { label: "Headlights", section: "ACTION_HEADLIGHTS", key: "KEY", default: 0x4C },
    Binding { label: "Horn", section: "ACTION_HORN", key: "KEY", default: 0x48 },
    Binding { label: "Look left", section: "GLANCELEFT", key: "KEY", default: 0x51 },
    Binding { label: "Look right", section: "GLANCERIGHT", key: "KEY", default: 0x45 },
    Binding { label: "Look back", section: "GLANCEBACK", key: "KEY", default: 0x57 },
    Binding { label: "DRS", section: "DRS", key: "KEY", default: -1 },
    Binding { label: "KERS", section: "KERS", key: "KEY", default: -1 },
];

/// The steering feel of `[KEYBOARD]`: label, key, step, from, to.
pub const STEERING: &[(&str, &str, f32, f32, f32)] = &[
    ("Steering speed", "STEERING_SPEED", 0.25, 0.25, 4.0),
    ("Opposite-direction speed", "STEERING_OPPOSITE_DIRECTION_SPEED", 0.25, 0.25, 5.0),
    ("Return-to-centre speed", "STEER_RESET_SPEED", 0.2, 0.2, 4.0),
    ("Steering gain", "STEER_GAIN", 0.02, 0.02, 0.5),
];

/// A Windows virtual-key code as the key's name.
pub fn key_name(vk: i32) -> String {
    match vk {
        -1 => "—".into(),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0d => "Enter".into(),
        0x20 => "Space".into(),
        0x21 => "Page up".into(),
        0x22 => "Page down".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x2d => "Insert".into(),
        0x2e => "Delete".into(),
        0xa0 | 0x10 => "Left Shift".into(),
        0xa1 => "Right Shift".into(),
        0xa2 | 0x11 => "Left Ctrl".into(),
        0xa3 => "Right Ctrl".into(),
        0xa4 | 0x12 => "Left Alt".into(),
        0xa5 => "Right Alt".into(),
        0x30..=0x39 | 0x41..=0x5a => char::from(vk as u8).to_string(),
        0x70..=0x7b => format!("F{}", vk - 0x6f),
        0xba => ";".into(),
        0xbb => "=".into(),
        0xbc => ",".into(),
        0xbd => "-".into(),
        0xbe => ".".into(),
        0xbf => "/".into(),
        0xc0 => "`".into(),
        0xdb => "[".into(),
        0xdc => "\\".into(),
        0xdd => "]".into(),
        0xde => "'".into(),
        other => format!("0x{other:02X}"),
    }
}

/// What GPUI calls a key (`"w"`, `"up"`, `"f5"`) as a virtual-key code.
pub fn key_from_gpui(key: &str) -> Option<i32> {
    Some(match key {
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "space" => 0x20,
        "enter" => 0x0d,
        "tab" => 0x09,
        "backspace" => 0x08,
        "delete" => 0x2e,
        "insert" => 0x2d,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        ";" => 0xba,
        "=" => 0xbb,
        "," => 0xbc,
        "-" => 0xbd,
        "." => 0xbe,
        "/" => 0xbf,
        "`" => 0xc0,
        "[" => 0xdb,
        "\\" => 0xdc,
        "]" => 0xdd,
        "'" => 0xde,
        k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphabetic() => k.as_bytes()[0].to_ascii_uppercase() as i32,
        k if k.len() == 1 && k.as_bytes()[0].is_ascii_digit() => k.as_bytes()[0] as i32,
        k if k.starts_with('f') && k[1..].parse::<i32>().is_ok_and(|n| (1..=12).contains(&n)) => 0x6f + k[1..].parse::<i32>().unwrap(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_both_ways() {
        assert_eq!(key_from_gpui("w"), Some(0x57));
        assert_eq!(key_from_gpui("f5"), Some(0x74));
        assert_eq!(key_from_gpui("up"), Some(0x26));
        assert_eq!(key_from_gpui("escape"), None);
        assert_eq!(key_name(0x57), "W");
        assert_eq!(key_name(0x74), "F5");
        assert_eq!(key_name(-1), "—");
    }

    #[test]
    fn the_levels_round_trip_through_assists_ini() {
        for level in Level::ALL {
            let mut ini = Ini::default();
            level.assists().store(&mut ini);
            assert_eq!(Assists::load(&ini).level(), Some(level));
        }
    }
}
