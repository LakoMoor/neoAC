// SPDX-License-Identifier: GPL-3.0-or-later

//! Mods, as Content Manager installs them: a car, a track or a skin comes as a folder or a .zip
//! with the game's own layout (`content/cars/<id>/...`, or the `<id>` folder alone), and is copied
//! into `content/cars` or `content/tracks`. Nothing that is already there is deleted: files are
//! merged over it (a skin joins its car). Removing a mod moves it to `<own folder>/removed`.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Car,
    Track,
}

impl Kind {
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Car => "cars",
            Kind::Track => "tracks",
        }
    }
}

/// What a folder is, by what is inside: a car has `ui/ui_car.json` (or its `data.acd`), a track
/// has `ui/ui_track.json` (or one in a layout's `ui/<layout>`) or `models.ini`.
fn kind_of(dir: &Path) -> Option<Kind> {
    if dir.join("ui/ui_car.json").is_file() || dir.join("data.acd").is_file() {
        return Some(Kind::Car);
    }
    if dir.join("ui/ui_track.json").is_file() || dir.join("models.ini").is_file() {
        return Some(Kind::Track);
    }
    if let Ok(entries) = fs::read_dir(dir.join("ui")) {
        if entries.flatten().any(|e| e.path().join("ui_track.json").is_file()) {
            return Some(Kind::Track);
        }
    }
    None
}

/// The cars and tracks in `dir`, however deep (up to six folders): the first folder that is one
/// ends the search below it.
fn find(dir: &Path, depth: u32, out: &mut Vec<(Kind, PathBuf)>) {
    if let Some(kind) = kind_of(dir) {
        out.push((kind, dir.to_path_buf()));
        return;
    }
    if depth == 0 {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for d in dirs {
            find(&d, depth - 1, out);
        }
    }
}

fn copy_merge(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if src.is_dir() {
            copy_merge(&src, &dst)?;
        } else {
            fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn unzip(archive: &Path, to: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a zip archive: {e}"))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        // (a name that climbs out of the folder is skipped)
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = to.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut f = fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Installs what `source` (a folder or a .zip) holds into the game folder `root`. The names of
/// what was installed, "car ks_foo" style, or why nothing was.
pub fn install(source: &Path, root: &Path, scratch: &Path) -> Result<Vec<String>, String> {
    let source = source.to_path_buf();
    if !source.exists() {
        return Err(format!("{}: no such file or folder", source.display()));
    }
    let (search, temp) = if source.is_dir() {
        (source.clone(), None)
    } else if source.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
        let temp = scratch.join("unpack");
        fs::remove_dir_all(&temp).ok();
        fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
        unzip(&source, &temp)?;
        (temp.clone(), Some(temp))
    } else {
        return Err("only .zip archives and folders can be installed: unpack a .7z or .rar first".into());
    };
    let mut found = Vec::new();
    find(&search, 6, &mut found);
    let mut done = Vec::new();
    for (kind, dir) in &found {
        let Some(id) = dir.file_name().and_then(|n| n.to_str()) else { continue };
        let id = if dir == &search { source.file_stem().and_then(|n| n.to_str()).unwrap_or(id).to_string() } else { id.to_string() };
        copy_merge(dir, &root.join("content").join(kind.folder()).join(&id)).map_err(|e| format!("{id}: {e}"))?;
        done.push(format!("{} {id}", if *kind == Kind::Car { "car" } else { "track" }));
    }
    if let Some(temp) = temp {
        fs::remove_dir_all(temp).ok();
    }
    if done.is_empty() {
        Err("no car or track in there (a car has ui/ui_car.json, a track ui/ui_track.json)".into())
    } else {
        Ok(done)
    }
}

/// Takes a car or track out of the game: moved to `removed` beside the launcher's own files, so it
/// can be put back by hand.
pub fn remove(kind: Kind, id: &str, root: &Path, removed: &Path) -> Result<(), String> {
    if id.is_empty() || id.contains('/') || id.contains("..") {
        return Err("bad name".into());
    }
    let from = root.join("content").join(kind.folder()).join(id);
    fs::create_dir_all(removed).map_err(|e| e.to_string())?;
    let mut to = removed.join(format!("{}_{id}", kind.folder()));
    let mut n = 1;
    while to.exists() {
        to = removed.join(format!("{}_{id}_{n}", kind.folder()));
        n += 1;
    }
    fs::rename(&from, &to).map_err(|e| format!("{id}: {e}"))
}

/// Cars and tracks that are not Kunos' own (their ids start with `ks_`, and the base game's
/// tracks and cars have no mod's marker): what the Content page lists to take out again.
pub fn is_mod(id: &str) -> bool {
    !id.starts_with("ks_") && !matches!(id, "imola" | "magione" | "drift" | "monza" | "mugello" | "silverstone" | "spa" | "nurburgring" | "vallelunga" | "trento-bondone" | "brands_hatch" | "barcelona" | "red_bull_ring" | "highlands" | "ruapuna" | "awd_test" | "zandvoort" | "kunos_skidpad")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rustyac-mods-{name}-{}", std::process::id()));
        fs::remove_dir_all(&d).ok();
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_car_in_a_nested_folder_is_installed_and_a_skin_joins_it() {
        let t = scratch("install");
        let root = t.join("game");
        fs::create_dir_all(root.join("content/cars")).unwrap();
        let src = t.join("pack/content/cars/mod_car/ui");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("ui_car.json"), "{}").unwrap();
        let done = install(&t.join("pack"), &root, &t.join("tmp")).unwrap();
        assert_eq!(done, vec!["car mod_car".to_string()]);
        assert!(root.join("content/cars/mod_car/ui/ui_car.json").is_file());
        // a second pack with only a skin of that car merges
        let skin = t.join("skin/mod_car/ui");
        fs::create_dir_all(&skin).unwrap();
        fs::write(skin.join("ui_car.json"), "{\"x\":1}").unwrap();
        fs::create_dir_all(t.join("skin/mod_car/skins/red")).unwrap();
        fs::write(t.join("skin/mod_car/skins/red/livery.png"), "p").unwrap();
        install(&t.join("skin"), &root, &t.join("tmp")).unwrap();
        assert!(root.join("content/cars/mod_car/skins/red/livery.png").is_file());
        remove(Kind::Car, "mod_car", &root, &t.join("removed")).unwrap();
        assert!(!root.join("content/cars/mod_car").exists());
        assert!(t.join("removed/cars_mod_car").is_dir());
        fs::remove_dir_all(t).ok();
    }

    #[test]
    fn a_zip_is_unpacked_and_installed() {
        use std::io::Write;
        let t = scratch("zip");
        let root = t.join("game");
        fs::create_dir_all(&root).unwrap();
        let file = fs::File::create(t.join("pack.zip")).unwrap();
        let mut z = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("content/tracks/mod_ring/ui/ui_track.json", opts).unwrap();
        z.write_all(b"{}").unwrap();
        z.start_file("../escape.txt", opts).unwrap();
        z.write_all(b"x").unwrap();
        z.finish().unwrap();
        let done = install(&t.join("pack.zip"), &root, &t.join("tmp")).unwrap();
        assert_eq!(done, vec!["track mod_ring".to_string()]);
        assert!(root.join("content/tracks/mod_ring/ui/ui_track.json").is_file());
        assert!(!t.join("escape.txt").exists());
        fs::remove_dir_all(t).ok();
    }

    #[test]
    fn nothing_to_install_is_said() {
        let t = scratch("none");
        fs::create_dir_all(t.join("empty/a")).unwrap();
        assert!(install(&t.join("empty"), &t, &t.join("tmp")).unwrap_err().contains("no car or track"));
        assert!(install(&t.join("x.7z"), &t, &t.join("tmp")).is_err());
        assert!(remove(Kind::Car, "../x", &t, &t.join("r")).is_err());
        fs::remove_dir_all(t).ok();
    }
}
