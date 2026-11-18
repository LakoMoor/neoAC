// SPDX-License-Identifier: GPL-3.0-or-later

//! An ini file edited in place: the lines the launcher does not touch (comments, keys of other
//! programs) stay as they were, so the player's own `controls.ini` is not rewritten from nothing.

use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct Ini {
    lines: Vec<String>,
}

fn section_of(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[').and_then(|t| t.split(']').next())
}

/// `KEY=value ; comment` → (key, value without the comment and the blanks).
fn pair(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    if t.starts_with(';') || t.starts_with('[') {
        return None;
    }
    let (k, v) = t.split_once('=')?;
    Some((k.trim(), v.split(';').next().unwrap_or("").trim()))
}

impl Ini {
    pub fn parse(text: &str) -> Ini {
        Ini { lines: text.replace("\r\n", "\n").lines().map(str::to_string).collect() }
    }

    pub fn load(path: &Path) -> Ini {
        Ini::parse(&std::fs::read_to_string(path).unwrap_or_default())
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.lines.join("\n") + "\n")
    }

    pub fn get(&self, section: &str, key: &str) -> Option<String> {
        let mut inside = false;
        for line in &self.lines {
            if let Some(s) = section_of(line) {
                inside = s.eq_ignore_ascii_case(section);
            } else if inside {
                if let Some((k, v)) = pair(line) {
                    if k.eq_ignore_ascii_case(key) {
                        return Some(v.to_string());
                    }
                }
            }
        }
        None
    }

    pub fn get_f32(&self, section: &str, key: &str, default: f32) -> f32 {
        self.get(section, key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    pub fn get_i32(&self, section: &str, key: &str, default: i32) -> i32 {
        self.get(section, key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    /// A key code: `0x4C`, or decimal, `-1` when none.
    pub fn get_key(&self, section: &str, key: &str) -> i32 {
        let Some(v) = self.get(section, key) else { return -1 };
        match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
            Some(hex) => i32::from_str_radix(hex, 16).unwrap_or(-1),
            None => v.parse().unwrap_or(-1),
        }
    }

    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let mut inside = false;
        let mut last_in_section = None;
        for (i, line) in self.lines.iter().enumerate() {
            if let Some(s) = section_of(line) {
                inside = s.eq_ignore_ascii_case(section);
                if inside {
                    last_in_section = Some(i);
                }
            } else if inside {
                if let Some((k, _)) = pair(line) {
                    if k.eq_ignore_ascii_case(key) {
                        let at = i;
                        self.lines[at] = format!("{k}={value}");
                        return;
                    }
                }
                if !line.trim().is_empty() {
                    last_in_section = Some(i);
                }
            }
        }
        match last_in_section {
            Some(at) => self.lines.insert(at + 1, format!("{key}={value}")),
            None => {
                if self.lines.last().is_some_and(|l| !l.trim().is_empty()) {
                    self.lines.push(String::new());
                }
                self.lines.push(format!("[{section}]"));
                self.lines.push(format!("{key}={value}"));
            }
        }
    }

    pub fn set_key(&mut self, section: &str, key: &str, vk: i32) {
        self.set(section, key, &if vk < 0 { "-1".to_string() } else { format!("0x{vk:02X}") });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_keep_the_rest() {
        let mut ini = Ini::parse("[KEYBOARD]\nGAS=0x26 ; Up\nBRAKE=0x28\n\n[GEARUP]\nJOY=-1\nKEY=0x20 ; SPACE\n");
        assert_eq!(ini.get("KEYBOARD", "GAS").as_deref(), Some("0x26"));
        assert_eq!(ini.get_key("GEARUP", "KEY"), 0x20);
        ini.set_key("KEYBOARD", "GAS", 0x57);
        ini.set("GEARUP", "KEY", "0xA2");
        ini.set("NEW", "A", "1");
        ini.set("KEYBOARD", "SPEED", "2");
        assert_eq!(ini.get_key("KEYBOARD", "GAS"), 0x57);
        assert_eq!(ini.get_key("KEYBOARD", "BRAKE"), 0x28);
        assert_eq!(ini.get_key("GEARUP", "KEY"), 0xA2);
        assert_eq!(ini.get("NEW", "A").as_deref(), Some("1"));
        assert_eq!(ini.get("KEYBOARD", "SPEED").as_deref(), Some("2"));
        assert_eq!(ini.get_key("KEYBOARD", "MISSING"), -1);
        let text = ini.lines.join("\n");
        assert!(text.contains("JOY=-1"));
    }
}
