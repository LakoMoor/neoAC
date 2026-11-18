// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's cars and tracks as the launcher shows them: names, specs and previews, read from
//! each folder's `ui` files (AC's JSON has raw line breaks inside strings, which a JSON parser
//! refuses, so the few fields are read by hand).

use std::path::{Path, PathBuf};

/// The string after `"key"`; `None` when the key is missing or its value is not a string.
pub fn json_str(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut from = 0;
    while let Some(at) = text[from..].find(&needle) {
        let rest = text[from + at + needle.len()..].trim_start();
        from += at + needle.len();
        let Some(rest) = rest.strip_prefix(':') else { continue };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else { return None };
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(out.trim().to_string()),
                '\\' => match chars.next() {
                    Some('n') => out.push(' '),
                    Some('t') => out.push(' '),
                    Some(other) => out.push(other),
                    None => break,
                },
                '\n' | '\r' => out.push(' '),
                other => out.push(other),
            }
        }
        return Some(out.trim().to_string());
    }
    None
}

#[derive(Clone, Debug, Default)]
pub struct Skin {
    pub id: String,
    pub preview: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct Car {
    pub id: String,
    pub name: String,
    pub brand: String,
    pub class: String,
    pub bhp: String,
    pub torque: String,
    pub weight: String,
    pub top_speed: String,
    pub acceleration: String,
    pub description: String,
    #[allow(dead_code)]
    pub badge: Option<PathBuf>,
    pub skins: Vec<Skin>,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// The folder's name, empty for the track's own (default) layout.
    pub id: String,
    pub name: String,
    pub length: String,
    pub pitboxes: String,
    pub preview: Option<PathBuf>,
    pub outline: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub country: String,
    pub description: String,
    pub layouts: Vec<Layout>,
}

fn dirs(path: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(path).map(|r| r.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
    v.sort();
    v
}

fn file(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

fn pretty(id: &str) -> String {
    id.trim_start_matches("ks_").replace('_', " ")
}

pub fn read_cars(root: &Path) -> Vec<Car> {
    let base = root.join("content/cars");
    dirs(&base)
        .into_iter()
        .map(|id| {
            let dir = base.join(&id);
            let ui = std::fs::read(dir.join("ui/ui_car.json")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
            let skins = dirs(&dir.join("skins")).into_iter().map(|s| Skin { preview: file(dir.join("skins").join(&s).join("preview.jpg")), id: s }).collect();
            Car {
                name: json_str(&ui, "name").filter(|n| !n.is_empty()).unwrap_or_else(|| pretty(&id)),
                brand: json_str(&ui, "brand").unwrap_or_default(),
                class: json_str(&ui, "class").unwrap_or_default(),
                bhp: json_str(&ui, "bhp").unwrap_or_default(),
                torque: json_str(&ui, "torque").unwrap_or_default(),
                weight: json_str(&ui, "weight").unwrap_or_default(),
                top_speed: json_str(&ui, "topspeed").unwrap_or_default(),
                acceleration: json_str(&ui, "acceleration").unwrap_or_default(),
                description: json_str(&ui, "description").unwrap_or_default().replace("</br>", " ").replace("<br>", " ").replace("  ", " "),
                badge: file(dir.join("ui/badge.png")),
                skins,
                id,
            }
        })
        .collect()
}

pub fn read_tracks(root: &Path) -> Vec<Track> {
    let base = root.join("content/tracks");
    dirs(&base)
        .into_iter()
        .map(|id| {
            let dir = base.join(&id);
            let read = |p: PathBuf| std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
            let layout_of = |ui: &Path, folder: &str| {
                let json = read(ui.join("ui_track.json"));
                Layout {
                    id: folder.to_string(),
                    name: json_str(&json, "name").unwrap_or_else(|| pretty(if folder.is_empty() { &id } else { folder })),
                    length: json_str(&json, "length").unwrap_or_default(),
                    pitboxes: json_str(&json, "pitboxes").unwrap_or_default(),
                    preview: file(ui.join("preview.png")),
                    outline: file(ui.join("outline.png")),
                }
            };
            let mut layouts = Vec::new();
            let own = dir.join("ui");
            if own.join("ui_track.json").is_file() {
                layouts.push(layout_of(&own, ""));
            }
            for sub in dirs(&dir) {
                let ui = dir.join("ui").join(&sub);
                if ui.join("ui_track.json").is_file() {
                    layouts.push(layout_of(&ui, &sub));
                }
            }
            if layouts.is_empty() {
                layouts.push(Layout { name: pretty(&id), ..Layout::default() });
            }
            let json = read(dir.join("ui/ui_track.json"));
            Track {
                name: json_str(&json, "name").unwrap_or_else(|| pretty(&id)),
                country: json_str(&json, "country").unwrap_or_default(),
                description: json_str(&json, "description").unwrap_or_default().replace("</br>", " "),
                layouts,
                id,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_strings_with_raw_line_breaks_and_escapes() {
        let text = "{\n\t\"name\": \"Nissan GT-R\",\n \"description\": \"one\ntwo \\\"quoted\\\"\",\n \"specs\": { \"bhp\": \"592bhp\" , \"range\" : 116 }}";
        assert_eq!(json_str(text, "name").as_deref(), Some("Nissan GT-R"));
        assert_eq!(json_str(text, "description").as_deref(), Some("one two \"quoted\""));
        assert_eq!(json_str(text, "bhp").as_deref(), Some("592bhp"));
        assert_eq!(json_str(text, "range"), None);
        assert_eq!(json_str(text, "missing"), None);
    }
}
