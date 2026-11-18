// SPDX-License-Identifier: GPL-3.0-or-later

//! Custom Shaders Patch's data, as Content Manager handles it: the `extension` folder beside the
//! game (its modules are `extension/config/<module>.ini`, each with an `[ℹ]` block of name and
//! description), and the player's changes to a module, which go to their own
//! `<own folder>/cfg/extension/<module>.ini` over it. The patch's DLL (`dwrite.dll`) is not
//! installed: it only runs inside the Windows `acs.exe`.

use crate::ini::Ini;
use std::path::{Path, PathBuf};

pub const VERSION: &str = "0.2.11";

#[derive(Clone, Debug)]
pub struct Module {
    /// the file's name without `.ini`
    pub file: String,
    pub name: String,
    pub about: String,
    /// `[BASIC] ENABLED` of the patch's own file and the player's override: None when the module has no switch
    pub enabled: Option<bool>,
}

/// The modules with a switch, in the order of their names.
pub fn modules(root: &Path, own: &Path) -> Vec<Module> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(root.join("extension/config")) else { return out };
    for entry in dir.flatten() {
        let path = entry.path();
        let Some(file) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".ini")) else { continue };
        let ini = Ini::load(&path);
        let Some(base) = ini.get("BASIC", "ENABLED").or_else(|| ini.get("BASIC", "ACTIVE")) else { continue };
        let Some(name) = ini.get("ℹ", "FULLNAME") else { continue };
        let key = if ini.get("BASIC", "ENABLED").is_some() { "ENABLED" } else { "ACTIVE" };
        let mine = Ini::load(&override_path(own, file)).get("BASIC", key);
        let on = |v: &str| v.trim() != "0" && !v.trim().eq_ignore_ascii_case("false");
        out.push(Module { file: file.to_string(), name, about: ini.get("ℹ", "SHORT_DESCRIPTION").unwrap_or_default(), enabled: Some(on(mine.as_deref().unwrap_or(&base))) });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

pub fn override_path(own: &Path, file: &str) -> PathBuf {
    own.join("cfg/extension").join(format!("{file}.ini"))
}

/// Turns a module on or off in the player's own file.
pub fn set_enabled(root: &Path, own: &Path, file: &str, on: bool) -> std::io::Result<()> {
    let base = Ini::load(&root.join("extension/config").join(format!("{file}.ini")));
    let key = if base.get("BASIC", "ENABLED").is_some() { "ENABLED" } else { "ACTIVE" };
    let path = override_path(own, file);
    let mut ini = Ini::load(&path);
    ini.set("BASIC", key, if on { "1" } else { "0" });
    ini.save(&path)
}

pub fn installed(root: &Path) -> bool {
    root.join("extension/config/general.ini").is_file()
}

/// Downloads the patch (a 120 MB .zip) and unpacks its `extension` folder beside the game.
pub fn install(root: &Path, scratch: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    let zip_path = scratch.join("csp.zip");
    let status = std::process::Command::new("curl").args(["-fsSL", "-m", "900", "-o"]).arg(&zip_path).arg(format!("https://acstuff.club/patch/?get={VERSION}")).status().map_err(|e| format!("curl: {e}"))?;
    if !status.success() {
        return Err("the download failed (is there a network?)".into());
    }
    let file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a zip: {e}"))?;
    let mut count = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else { continue };
        if !rel.starts_with("extension") || entry.is_dir() {
            continue;
        }
        let out = root.join(&rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
        count += 1;
    }
    std::fs::remove_file(zip_path).ok();
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_is_listed_and_the_players_file_wins() {
        let t = std::env::temp_dir().join(format!("rustyac-csp-{}", std::process::id()));
        std::fs::remove_dir_all(&t).ok();
        let (root, own) = (t.join("g"), t.join("own"));
        std::fs::create_dir_all(root.join("extension/config")).unwrap();
        std::fs::write(root.join("extension/config/lighting_fx.ini"), "[ℹ]\nFULLNAME=Lighting FX\nSHORT_DESCRIPTION=Dynamic lights\n\n[BASIC]\nENABLED=1 ; Active\n").unwrap();
        std::fs::write(root.join("extension/config/plain.ini"), "[BASIC]\nX=1\n").unwrap();
        let m = modules(&root, &own);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].name.as_str(), m[0].enabled), ("Lighting FX", Some(true)));
        set_enabled(&root, &own, "lighting_fx", false).unwrap();
        assert_eq!(modules(&root, &own)[0].enabled, Some(false));
        std::fs::remove_dir_all(t).ok();
    }
}
