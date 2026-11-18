// SPDX-License-Identifier: GPL-3.0-or-later

//! Every car's `.ini` data, read as the game reads it: all the files of each `data.acd`
//! parse, `lights.ini` is well formed (and names meshes the car's model has), and the files
//! the physics and the renderer start from hold their keys.
//!
//! Needs an Assetto Corsa install (`AC_ROOT`, or `ac/` of the checkout); without cars it
//! prints NOT TESTED and passes. A car folder with no `data.acd` (a loose `data` folder, or
//! nothing but `ui`) is skipped.

use std::path::{Path, PathBuf};

use rustyac_content::acd::Acd;
use rustyac_physics::data::ini::IniReader;

struct Car {
    name: String,
    folder: PathBuf,
    data: Acd,
}

fn cars() -> Vec<Car> {
    let root = std::env::var_os("AC_ROOT").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ac"));
    let Ok(read) = std::fs::read_dir(root.join("content/cars")) else {
        println!("NOT TESTED: no content/cars in {}", root.display());
        return Vec::new();
    };
    let mut cars = Vec::new();
    for entry in read.flatten() {
        let folder = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !folder.join("data.acd").is_file() {
            continue;
        }
        match Acd::open_as(&folder.join("data.acd"), &name) {
            Ok(data) => cars.push(Car { name, folder, data }),
            Err(e) => panic!("{name}: data.acd does not open: {e}"),
        }
    }
    cars.sort_by(|a, b| a.name.cmp(&b.name));
    if cars.is_empty() {
        println!("NOT TESTED: no packed car data");
    }
    cars
}

fn ini(car: &Car, file: &str) -> Option<IniReader> {
    let bytes = car.data.get(file)?;
    Some(IniReader::from_text(Path::new(file), &String::from_utf8_lossy(bytes)))
}

fn kn5_text(car: &Car) -> Vec<u8> {
    // the car's own models (not its level-of-detail copies or colliders): the node names are in the file as plain text
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&car.folder).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".kn5") && !name.contains("collider") {
            out.extend(std::fs::read(entry.path()).unwrap_or_default());
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn every_ini_of_every_car_parses() {
    let mut bad = Vec::new();
    let mut files = 0;
    for car in cars() {
        for name in car.data.names().filter(|n| n.to_ascii_lowercase().ends_with(".ini")) {
            files += 1;
            let text = String::from_utf8_lossy(car.data.get(name).unwrap()).to_string();
            let reader = IniReader::from_text(Path::new(name), &text);
            let has_content = text.lines().any(|l| {
                let l = l.trim();
                !l.is_empty() && !l.starts_with(';') && !l.starts_with("//")
            });
            if has_content && reader.sections().next().is_none() {
                bad.push(format!("{}/{name}: content but no section", car.name));
            }
        }
    }
    println!("{files} ini files");
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn lights_ini_is_well_formed_and_names_meshes_of_the_model() {
    let mut bad = Vec::new();
    for car in cars() {
        let Some(lights) = ini(&car, "lights.ini") else {
            continue;
        };
        let model = kn5_text(&car);
        if lights.has_section("HEADER") {
            match lights.get_int("HEADER", "VERSION") {
                Ok(v) if (1..=3).contains(&v) => {}
                other => bad.push(format!("{}: HEADER VERSION {other:?}", car.name)),
            }
        }
        let mut brake = 0;
        let mut light = 0;
        for section in lights.sections() {
            let upper = section.to_ascii_uppercase();
            let is_header = upper == "HEADER";
            let numbered = ["BRAKE_", "LIGHT_"].iter().any(|p| upper.strip_prefix(p).is_some_and(|n| n.parse::<u32>().is_ok()));
            if !is_header && !numbered {
                // other sections (the pit and kers lamps, flashers) are read by the game under their own names
                if !(upper.starts_with("PIT") || upper.starts_with("KERS") || upper.starts_with("FLASH") || upper.starts_with("LIGHT_ANIMATION_")) {
                    bad.push(format!("{}: unknown section [{section}]", car.name));
                }
                continue;
            }
            if is_header {
                continue;
            }
            let name = lights.get_string(section, "NAME");
            if name.is_empty() {
                bad.push(format!("{}: [{section}] has no NAME", car.name));
            } else if !contains(&model, &name) {
                // the game skips an entry whose mesh is not there: a note, unless it is the lamp that matters (below)
                println!("note: {}: [{section}] NAME={name} is in no model of the car (the game skips it)", car.name);
            }
            match lights.get_float3(section, "COLOR") {
                Ok(c) if c.iter().all(|v| v.is_finite() && *v >= 0.0) => {}
                other => bad.push(format!("{}: [{section}] COLOR {other:?}", car.name)),
            }
            if lights.has_key(section, "OFF_COLOR") {
                match lights.get_float3(section, "OFF_COLOR") {
                    Ok(c) if c.iter().all(|v| v.is_finite() && *v >= 0.0) => {}
                    other => bad.push(format!("{}: [{section}] OFF_COLOR {other:?}", car.name)),
                }
            }
            if upper.starts_with("BRAKE_") {
                brake += 1;
            } else {
                light += 1;
            }
        }
        // the brightest lamp of each kind is the one the lighting is made from: its mesh must be there
        for prefix in ["BRAKE_", "LIGHT_"] {
            let mut best: Option<(f32, String)> = None;
            for n in 0.. {
                let section = format!("{prefix}{n}");
                if !lights.has_key(&section, "NAME") {
                    break;
                }
                let peak = lights.get_float3(&section, "COLOR").map_or(0.0, |c| c[0].max(c[1]).max(c[2]));
                if best.as_ref().is_none_or(|b| peak > b.0) {
                    best = Some((peak, lights.get_string(&section, "NAME")));
                }
            }
            if let Some((_, name)) = best {
                if !contains(&model, &name) {
                    bad.push(format!("{}: the brightest {prefix}* lamp, NAME={name}, is in no model of the car", car.name));
                }
            }
        }
        // the game reads BRAKE_0, BRAKE_1 ... and LIGHT_0 ... up to the first that is missing: a gap hides the rest
        for (prefix, count) in [("BRAKE_", brake), ("LIGHT_", light)] {
            for n in 0..count {
                if !lights.has_key(&format!("{prefix}{n}"), "NAME") {
                    bad.push(format!("{}: {prefix}{n} is missing among {count} {prefix}* sections: the game stops there", car.name));
                    break;
                }
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn every_car_with_lights_has_a_headlight_and_a_brake_lamp_bright_enough_to_find() {
    let mut bad = Vec::new();
    for car in cars() {
        let Some(lights) = ini(&car, "lights.ini") else { continue };
        let peak = |prefix: &str| (0..).map(|n| format!("{prefix}{n}")).take_while(|s| lights.has_key(s, "NAME")).filter_map(|s| lights.get_float3(&s, "COLOR").ok()).map(|c| c[0].max(c[1]).max(c[2])).fold(0.0f32, f32::max);
        if peak("BRAKE_") <= 0.0 {
            bad.push(format!("{}: no BRAKE_ lamp with a colour", car.name));
        }
        if peak("LIGHT_") <= 0.0 {
            bad.push(format!("{}: no LIGHT_ lamp with a colour", car.name));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn the_files_the_simulation_starts_from_hold_their_keys() {
    // (file, section, key, what it must be)
    type Check = fn(&IniReader, &str, &str) -> bool;
    let checks: [(&str, &str, &str, Check); 9] = [
        ("car.ini", "BASIC", "TOTALMASS", |i, s, k| i.get_float(s, k).is_ok_and(|v| v > 100.0 && v < 20000.0)),
        ("car.ini", "BASIC", "INERTIA", |i, s, k| i.get_float3(s, k).is_ok()),
        ("engine.ini", "ENGINE_DATA", "LIMITER", |i, s, k| i.get_float(s, k).is_ok_and(|v| v > 1000.0)),
        ("engine.ini", "ENGINE_DATA", "MINIMUM", |i, s, k| i.get_float(s, k).is_ok_and(|v| v > 0.0)),
        ("drivetrain.ini", "TRACTION", "TYPE", |i, s, k| !i.get_string(s, k).is_empty()),
        ("drivetrain.ini", "GEARS", "COUNT", |i, s, k| i.get_int(s, k).is_ok_and(|v| (1..=12).contains(&v))),
        ("brakes.ini", "DATA", "MAX_TORQUE", |i, s, k| i.get_float(s, k).is_ok_and(|v| v > 0.0)),
        ("tyres.ini", "FRONT", "RADIUS", |i, s, k| i.get_float(s, k).is_ok_and(|v| v > 0.1 && v < 1.0)),
        ("suspensions.ini", "BASIC", "WHEELBASE", |i, s, k| i.get_float(s, k).is_ok_and(|v| v > 1.0 && v < 6.0)),
    ];
    let mut bad = Vec::new();
    for car in cars() {
        for (file, section, key, ok) in checks {
            match ini(&car, file) {
                None => bad.push(format!("{}: {file} is missing", car.name)),
                Some(reader) => {
                    if !ok(&reader, section, key) {
                        bad.push(format!("{}: {file} [{section}] {key} = {:?}", car.name, reader.get_string(section, key)));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// The numbers at the start of a text such as "592bhp", "1750kg".
fn leading_number(text: &str) -> Option<f64> {
    let t = text.trim();
    let end = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
    t[..end].parse().ok()
}

/// A `.lut` file's pairs (`rpm|value`).
fn lut(car: &Car, name: &str) -> Vec<(f64, f64)> {
    let Some(bytes) = car.data.get(name) else { return Vec::new() };
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|l| {
            let l = l.split(';').next()?.trim();
            let (a, b) = l.split_once('|')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        })
        .collect()
}

#[test]
fn the_cars_data_agrees_with_the_cars_own_spec_sheet() {
    // ui/ui_car.json is the content's own description: power, weight. A natural-aspirated car's power curve
    // must peak where it says; a turbo car's curve is the engine without boost, so it only has to stay below.
    let mut table = Vec::new();
    let mut bad = Vec::new();
    for car in cars() {
        let Ok(json) = std::fs::read_to_string(car.folder.join("ui/ui_car.json")) else { continue };
        let value: serde_json::Value = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(_) => match serde_json::from_str(&json.replace('\t', " ").replace('\r', "").replace('\n', " ")) {
                Ok(v) => v,
                Err(_) => continue,
            },
        };
        let spec = |key: &str| value["specs"][key].as_str().and_then(leading_number);
        let Some(engine) = ini(&car, "engine.ini") else { continue };
        let curve_file = engine.get_string("HEADER", "POWER_CURVE");
        let curve = lut(&car, &curve_file);
        if curve.is_empty() {
            bad.push(format!("{}: engine.ini POWER_CURVE={curve_file} is empty or missing", car.name));
            continue;
        }
        // the curve is the engine without boost; each turbo multiplies the torque by 1 + its wastegate level (its max boost when it has no wastegate)
        let boost: f64 = (0..4)
            .map(|n| format!("TURBO_{n}"))
            .filter(|sec| engine.has_section(sec))
            .map(|sec| {
                let gate = engine.get_float(&sec, "WASTEGATE").unwrap_or(0.0);
                f64::from(if gate > 0.0 { gate } else { engine.get_float(&sec, "MAX_BOOST").unwrap_or(0.0) })
            })
            .sum();
        let peak_ps = curve.iter().map(|(rpm, nm)| nm * (1.0 + boost) * rpm * std::f64::consts::TAU / 60.0 / 735.5).fold(0.0f64, f64::max);
        let turbo = boost > 0.0;
        let mass = ini(&car, "car.ini").and_then(|i| i.get_float("BASIC", "TOTALMASS").ok()).map(f64::from);
        table.push(format!("{:36} curve peak {:6.0} ps{} spec {:?} bhp | car.ini mass {:?} spec {:?} kg", car.name, peak_ps, if turbo { " (+turbo)" } else { "" }, spec("bhp"), mass.map(|m| m.round()), spec("weight")));
        if let Some(bhp) = spec("bhp") {
            let ratio = peak_ps / bhp;
            // (the sheets are rounded and quote crank power; the game's curves with the boost land within a quarter of them)
            if !(0.75..=1.25).contains(&ratio) {
                bad.push(format!("{}: the power curve with its boost peaks at {peak_ps:.0} ps, the sheet says {bhp} bhp", car.name));
            }
        }
        if let (Some(m), Some(w)) = (mass, spec("weight")) {
            if !(0.85..=1.2).contains(&(m / w)) {
                bad.push(format!("{}: car.ini TOTALMASS {m} against the sheet's {w} kg", car.name));
            }
        }
    }
    for line in &table {
        println!("{line}");
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
