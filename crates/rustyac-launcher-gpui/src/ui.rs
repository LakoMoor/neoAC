// SPDX-License-Identifier: GPL-3.0-or-later

//! The launcher's window.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    div, img, px, rgb, AnyElement, App, Bounds, Context, FocusHandle, FontWeight, KeyDownEvent, ModifiersChangedEvent, ObjectFit, Render, SharedString, Window, WindowBounds, WindowOptions,
};

use crate::data::{read_cars, read_tracks, Car, Track};
use crate::ini::Ini;
use crate::state::{key_from_gpui, key_name, own_folder, Assists, Level, Settings, BINDINGS, STEERING};

const BG: u32 = 0x0d0e11;
const PANEL: u32 = 0x15171c;
const CARD: u32 = 0x1c1f26;
const CARD_HOVER: u32 = 0x252932;
const LINE: u32 = 0x2b2f39;
const ACCENT: u32 = 0xe5432d;
const ACCENT_DIM: u32 = 0x5a2219;
const TEXT: u32 = 0xeceef2;
const MUTED: u32 = 0x8d93a1;
const GOOD: u32 = 0x4cc38a;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Drive,
    Cars,
    Tracks,
    Assists,
    Controls,
    Graphics,
    Content,
    Csp,
    Settings,
}

impl Page {
    const ALL: [(Page, &'static str, &'static str); 9] = [
        (Page::Drive, "Drive", "▶"),
        (Page::Cars, "Cars", "◈"),
        (Page::Tracks, "Tracks", "◎"),
        (Page::Assists, "Assists", "✦"),
        (Page::Controls, "Controls", "⌨"),
        (Page::Graphics, "Graphics", "▣"),
        (Page::Content, "Content", "⬇"),
        (Page::Csp, "Shaders Patch", "✧"),
        (Page::Settings, "Settings", "⚙"),
    ];
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    ModPath,
    Server,
    Name,
    Team,
    Password,
}

pub struct Launcher {
    root: Option<PathBuf>,
    own: PathBuf,
    cars: Vec<Car>,
    tracks: Vec<Track>,
    settings: Settings,
    assists: Assists,
    video_ini: Ini,
    mod_path: String,
    mod_status: String,
    install_mod_later: Option<String>,
    csp_install_later: bool,
    csp_toggle_later: Option<(String, bool)>,
    remove_later: Option<(crate::mods::Kind, String)>,
    assists_ini: Ini,
    controls_ini: Ini,
    page: Page,
    /// The index in [`BINDINGS`] waiting for a key.
    capture: Option<usize>,
    editing: Option<Field>,
    focus: FocusHandle,
    game: Option<Child>,
    status: String,
}

fn find_root(given: Option<PathBuf>) -> Option<PathBuf> {
    let is_root = |p: &Path| p.join("content/cars").is_dir() && p.join("content/tracks").is_dir();
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    let home = std::env::var_os("HOME").map(PathBuf::from);
    given
        .into_iter()
        .chain(std::env::var_os("AC_ROOT").map(PathBuf::from))
        .chain(exe_dir.clone())
        .chain(std::env::current_dir().ok())
        .chain(exe_dir.and_then(|d| d.parent().map(Path::to_path_buf)))
        .chain(home.iter().map(|h| h.join(".steam/steam/steamapps/common/assettocorsa")))
        .chain(home.iter().map(|h| h.join(".local/share/Steam/steamapps/common/assettocorsa")))
        .chain(home.iter().map(|h| h.join("Library/Application Support/Steam/steamapps/common/assettocorsa")))
        .find(|p| is_root(p))
}

impl Launcher {
    fn new(root: Option<PathBuf>, cx: &mut Context<Self>) -> Launcher {
        let own = own_folder();
        let mut settings = Settings::load(&own.join("launcher.ini"));
        let (cars, tracks) = root.as_deref().map_or((Vec::new(), Vec::new()), |r| (read_cars(r), read_tracks(r)));
        if !cars.iter().any(|c| c.id == settings.car) {
            settings.car = cars.first().map(|c| c.id.clone()).unwrap_or_default();
            settings.skin.clear();
        }
        if !tracks.iter().any(|t| t.id == settings.track) {
            settings.track = tracks.first().map(|t| t.id.clone()).unwrap_or_default();
            settings.layout.clear();
        }
        // the player's own files: made from the game's the first time
        let seed = |name: &str| {
            let mine = own.join("cfg").join(name);
            if !mine.is_file() {
                if let Some(shipped) = root.as_ref().map(|r| r.join("cfg").join(name)).filter(|p| p.is_file()) {
                    std::fs::create_dir_all(own.join("cfg")).ok();
                    std::fs::copy(shipped, &mine).ok();
                }
            }
            Ini::load(&mine)
        };
        let controls_ini = seed("controls.ini");
        let assists_ini = seed("assists.ini");
        let assists = Assists::load(&assists_ini);
        let video_ini = Ini::load(&own.join("cfg/video.ini"));
        Launcher { root, own, cars, tracks, settings, assists, video_ini, assists_ini, controls_ini, page: Page::Drive, capture: None, editing: None, focus: cx.focus_handle(), game: None, status: String::new(), mod_path: String::new(), mod_status: String::new(), install_mod_later: None, csp_install_later: false, csp_toggle_later: None, remove_later: None }
    }

    fn car(&self) -> Option<&Car> {
        self.cars.iter().find(|c| c.id == self.settings.car)
    }

    fn track(&self) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == self.settings.track)
    }

    fn skin_preview(&self, car: &Car, skin: &str) -> Option<PathBuf> {
        car.skins.iter().find(|s| s.id == skin).or(car.skins.first()).and_then(|s| s.preview.clone())
    }

    fn save_settings(&self) {
        self.settings.save(&self.own.join("launcher.ini"), self.assists.auto_shifter);
    }

    fn save_assists(&mut self) {
        self.assists.store(&mut self.assists_ini);
        self.assists_ini.save(&self.own.join("cfg/assists.ini")).ok();
        self.save_settings();
    }

    fn video(&self, section: &str, key: &str, default: i32) -> i32 {
        self.video_ini.get(section, key).and_then(|v| v.split(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse().ok())).unwrap_or(default)
    }

    fn set_video(&mut self, section: &str, key: &str, value: i32) {
        self.video_ini.set(section, key, &value.to_string());
        std::fs::create_dir_all(self.own.join("cfg")).ok();
        self.video_ini.save(&self.own.join("cfg/video.ini")).ok();
    }

    fn page_graphics(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let v = |t: &Self, s: &str, k: &str, d: i32| t.video(s, k, d);
        let presets: Vec<AnyElement> = [("Low", GFX_LOW), ("Medium", GFX_MEDIUM), ("High", GFX_HIGH)]
            .into_iter()
            .map(|(name, values)| {
                let on = values.iter().all(|(s, k, val)| self.video(s, k, i32::MIN) == *val);
                self.chip(SharedString::from(format!("gfx-{name}")), name, on, cx, move |this| {
                    for (s, k, val) in values {
                        this.set_video(s, k, *val);
                    }
                })
            })
            .collect();
        let rows = vec![
            self.pick_row("g-aa", "Anti-aliasing", "Samples per pixel", v(self, "VIDEO", "AASAMPLES", 1), &[("Off", 1), ("2×", 2), ("4×", 4)], cx, |t, x| t.set_video("VIDEO", "AASAMPLES", x)),
            self.pick_row("g-af", "Anisotropic filtering", "Sharper textures at an angle", v(self, "VIDEO", "ANISOTROPIC", 8), &[("Off", 0), ("4×", 4), ("8×", 8), ("16×", 16)], cx, |t, x| t.set_video("VIDEO", "ANISOTROPIC", x)),
            self.pick_row("g-sh", "Shadows", "Shadow map size", v(self, "VIDEO", "SHADOW_MAP_SIZE", 2048), &[("512", 512), ("1024", 1024), ("2048", 2048), ("4096", 4096)], cx, |t, x| t.set_video("VIDEO", "SHADOW_MAP_SIZE", x)),
            self.pick_row("g-wd", "World detail", "How far objects are drawn", v(self, "ASSETTOCORSA", "WORLD_DETAIL", 5), &[("0", 0), ("1", 1), ("2", 2), ("3", 3), ("4", 4), ("5", 5)], cx, |t, x| t.set_video("ASSETTOCORSA", "WORLD_DETAIL", x)),
            self.pick_row("g-cube", "Reflections", "Car reflection map", v(self, "CUBEMAP", "SIZE", 512), &[("Off", 0), ("256", 256), ("512", 512), ("1024", 1024)], cx, |t, x| t.set_video("CUBEMAP", "SIZE", x)),
            self.pick_row("g-mir", "Mirrors", "Rear-view mirror resolution", v(self, "MIRROR", "SIZE", 512), &[("Off", 0), ("256", 256), ("512", 512), ("1024", 1024)], cx, |t, x| t.set_video("MIRROR", "SIZE", x)),
            self.pick_row("g-smoke", "Smoke", "Tyre and engine smoke", v(self, "EFFECTS", "SMOKE", 3), &[("Off", 0), ("Low", 1), ("Medium", 3), ("High", 5)], cx, |t, x| t.set_video("EFFECTS", "SMOKE", x)),
            self.pick_row("g-pp", "Post-processing", "Glare, depth of field, light shafts", v(self, "POST_PROCESS", "ENABLED", 1), &[("On", 1), ("Off", 0)], cx, |t, x| t.set_video("POST_PROCESS", "ENABLED", x)),
            self.pick_row("g-fxaa", "FXAA", "Cheap extra edge smoothing", v(self, "POST_PROCESS", "FXAA", 0), &[("Off", 0), ("On", 1)], cx, |t, x| t.set_video("POST_PROCESS", "FXAA", x)),
            self.pick_row("g-blur", "Motion blur", "", v(self, "EFFECTS", "MOTION_BLUR", 0), &[("Off", 0), ("On", 1)], cx, |t, x| t.set_video("EFFECTS", "MOTION_BLUR", x)),
        ];
        div()
            .flex()
            .flex_col()
            .child(div().flex().items_center().justify_between().child(div().text_sm().text_color(rgb(MUTED)).child("Quality preset, then change anything below. Window size is on the Settings page.")).child(div().flex().gap_2().children(presets)))
            .child(div().mt_3().children(rows))
            .into_any_element()
    }

    /// Installs `path` (a folder or a .zip) and reloads the lists.
    fn install_mod(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            self.mod_status = "No game folder: start the launcher from the folder with content and system.".into();
            return;
        };
        let path = path.trim().trim_matches('\'').trim_matches('"').replace("\\ ", " ");
        self.mod_status = match crate::mods::install(std::path::Path::new(&path), &root, &self.own.join("tmp")) {
            Ok(done) => {
                self.cars = read_cars(&root);
                self.tracks = read_tracks(&root);
                format!("Installed: {}", done.join(", "))
            }
            Err(why) => why,
        };
        cx.notify();
    }

    fn remove_mod(&mut self, kind: crate::mods::Kind, id: &str, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else { return };
        self.mod_status = match crate::mods::remove(kind, id, &root, &self.own.join("removed")) {
            Ok(()) => {
                self.cars = read_cars(&root);
                self.tracks = read_tracks(&root);
                format!("{id} removed (kept in {})", self.own.join("removed").display())
            }
            Err(why) => why,
        };
        cx.notify();
    }

    fn page_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use crate::mods::{is_mod, Kind};
        let field = self.text_field(Field::ModPath, "Mod: a .zip or a folder (or drop it here)", self.mod_path.clone(), false, cx);
        let install = self.chip("mod-install", "Install", true, cx, |t| {
            let p = t.mod_path.clone();
            t.install_mod_later = Some(p);
        });
        let mut rows: Vec<AnyElement> = Vec::new();
        let items: Vec<(Kind, String, String)> = self.cars.iter().filter(|c| is_mod(&c.id)).map(|c| (Kind::Car, c.id.clone(), c.name.clone())).chain(self.tracks.iter().filter(|t| is_mod(&t.id)).map(|t| (Kind::Track, t.id.clone(), t.name.clone()))).collect();
        for (i, (kind, id, name)) in items.into_iter().enumerate() {
            let remove_id = id.clone();
            let remove = self.chip(("mod-remove", i), "Remove", false, cx, move |t| t.remove_later = Some((kind, remove_id.clone())));
            rows.push(div().flex().items_center().justify_between().py_2().border_b_1().border_color(rgb(LINE)).child(div().child(div().font_weight(FontWeight::SEMIBOLD).child(SharedString::from(name))).child(div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(format!("{} · {id}", if kind == Kind::Car { "car" } else { "track" }))))).child(remove).into_any_element());
        }
        let empty = rows.is_empty();
        div()
            .id("content-drop")
            .flex()
            .flex_col()
            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                for p in paths.paths() {
                    let p = p.display().to_string();
                    this.install_mod(&p, cx);
                }
            }))
            .child(div().text_sm().text_color(rgb(MUTED)).child("Install cars, tracks and skins from a .zip or a folder laid out as the game's content (content/cars/<id>, content/tracks/<id>). Files are merged, nothing is deleted. A .7z or .rar must be unpacked first."))
            .child(div().mt_3().flex().items_center().gap_3().child(field).child(install))
            .child(div().mt_2().text_sm().text_color(rgb(ACCENT)).child(SharedString::from(self.mod_status.clone())))
            .child(div().mt_4().text_lg().font_weight(FontWeight::SEMIBOLD).child("Installed mods"))
            .child(div().mt_1().text_sm().text_color(rgb(MUTED)).child(if empty { "None yet: every car and track here is the game's own." } else { "Not part of the base game:" }))
            .child(div().mt_2().children(rows))
            .into_any_element()
    }

    fn page_csp(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(root) = self.root.clone() else { return div().child("No game folder.").into_any_element() };
        let note = "rustyAC draws with its own renderer, not acs.exe, so the patch's DLL cannot run here. Its configuration (the extension folder and your switches below, kept in your own cfg/extension) is read; an effect shows only where rustyAC has implemented it: so far Lighting FX's brightness, the track's detail distance, the render density and the upscaler (set below).";
        let head = div().text_sm().text_color(rgb(MUTED)).child(note);
        if !crate::csp::installed(&root) {
            let button = self.chip("csp-get", format!("Download Custom Shaders Patch {}", crate::csp::VERSION), true, cx, |t| t.csp_install_later = true);
            return div().flex().flex_col().child(head).child(div().mt_3().child(button)).child(div().mt_2().text_sm().text_color(rgb(ACCENT)).child(SharedString::from(self.mod_status.clone()))).into_any_element();
        }
        let modules = crate::csp::modules(&root, &self.own);
        let rows: Vec<AnyElement> = modules
            .into_iter()
            .enumerate()
            .map(|(i, m)| {
                let on = m.enabled.unwrap_or(true);
                let file = m.file.clone();
                let toggle = self.chip(("csp-m", i), if on { "On" } else { "Off" }, on, cx, move |t| t.csp_toggle_later = Some((file.clone(), !on)));
                div().flex().items_center().justify_between().py_2().border_b_1().border_color(rgb(LINE)).child(div().child(div().font_weight(FontWeight::SEMIBOLD).child(SharedString::from(m.name))).child(div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(m.about)))).child(toggle).into_any_element()
            })
            .collect();
        let brightness = Ini::load(&crate::csp::override_path(&self.own, "lighting_fx")).get("BASIC", "BRIGHTNESS_MULT").or_else(|| Ini::load(&root.join("extension/config/lighting_fx.ini")).get("BASIC", "BRIGHTNESS_MULT")).and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
        let own = self.own.clone();
        let chips: Vec<AnyElement> = [0.7f32, 0.85, 1.0, 1.25, 1.5, 2.0]
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                let own = own.clone();
                self.chip(("csp-b", i), format!("{v}×"), (brightness - v).abs() < 0.01, cx, move |_| {
                    let path = crate::csp::override_path(&own, "lighting_fx");
                    let mut ini = Ini::load(&path);
                    ini.set("BASIC", "BRIGHTNESS_MULT", &v.to_string());
                    ini.save(&path).ok();
                })
            })
            .collect();
        let bright = div().mt_3().flex().items_center().justify_between().child(div().child(div().font_weight(FontWeight::SEMIBOLD).child("Lighting FX brightness (applied)")).child(div().text_sm().text_color(rgb(MUTED)).child("Scales the exposure range of the picture, from the next start of the game"))).child(div().flex().gap_2().children(chips));
        let lod = Ini::load(&crate::csp::override_path(&self.own, "graphics_adjustments")).get("LODS", "TRACK_DISTANCE_MULT").or_else(|| Ini::load(&root.join("extension/config/graphics_adjustments.ini")).get("LODS", "TRACK_DISTANCE_MULT")).and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
        let own = self.own.clone();
        let lod_chips: Vec<AnyElement> = [0.5f32, 1.0, 1.5, 2.0, 3.2]
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                let own = own.clone();
                self.chip(("csp-l", i), format!("{v}×"), (lod - v).abs() < 0.01, cx, move |_| {
                    let path = crate::csp::override_path(&own, "graphics_adjustments");
                    let mut ini = Ini::load(&path);
                    ini.set("LODS", "TRACK_DISTANCE_MULT", &v.to_string());
                    ini.save(&path).ok();
                })
            })
            .collect();
        let lods = div().mt_3().flex().items_center().justify_between().child(div().child(div().font_weight(FontWeight::SEMIBOLD).child("Graphics adjustments: detail distance (applied)")).child(div().text_sm().text_color(rgb(MUTED)).child("More than 1× keeps detailed models for longer, less is lighter on the GPU; from the next start"))).child(div().flex().gap_2().children(lod_chips));
        let density = Ini::load(&crate::csp::override_path(&self.own, "graphics_adjustments")).get("RENDER_SCALE", "SCALE").or_else(|| Ini::load(&root.join("extension/config/graphics_adjustments.ini")).get("RENDER_SCALE", "SCALE")).and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
        let own = self.own.clone();
        let density_chips: Vec<AnyElement> = [0.5f32, 0.67, 0.75, 1.0]
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                let own = own.clone();
                self.chip(("csp-d", i), format!("{}%", (v * 100.0).round()), (density - v).abs() < 0.01, cx, move |_| {
                    let path = crate::csp::override_path(&own, "graphics_adjustments");
                    let mut ini = Ini::load(&path);
                    ini.set("RENDER_SCALE", "SCALE", &v.to_string());
                    ini.save(&path).ok();
                })
            })
            .collect();
        let scale = div().mt_3().flex().items_center().justify_between().child(div().child(div().font_weight(FontWeight::SEMIBOLD).child("Graphics adjustments: render density (applied)")).child(div().text_sm().text_color(rgb(MUTED)).child("The picture is drawn smaller and stretched to the window: a lot lighter on a big display; from the next start"))).child(div().flex().gap_2().children(density_chips));
        let fsr_file = |t: &Self, key: &str| Ini::load(&crate::csp::override_path(&t.own, "graphics_adjustments")).get("FSR", key).or_else(|| Ini::load(&root.join("extension/config/graphics_adjustments.ini")).get("FSR", key)).and_then(|v| v.parse::<f32>().ok());
        let active = fsr_file(self, "ACTIVE").unwrap_or(0.0) > 0.0;
        let sharp = fsr_file(self, "SHARPEN2").unwrap_or(0.5);
        let own = self.own.clone();
        let fsr_chips: Vec<AnyElement> = [("Off", 0.0f32), ("Soft", 0.3), ("Balanced", 0.55), ("Sharp", 0.85)]
            .into_iter()
            .enumerate()
            .map(|(i, (name, v))| {
                let own = own.clone();
                let on = if v == 0.0 { !active } else { active && (sharp - v).abs() < 0.01 };
                self.chip(("csp-f", i), name, on, cx, move |_| {
                    let path = crate::csp::override_path(&own, "graphics_adjustments");
                    let mut ini = Ini::load(&path);
                    ini.set("FSR", "ACTIVE", if v == 0.0 { "0" } else { "1" });
                    if v > 0.0 {
                        ini.set("FSR", "SHARPEN2", &v.to_string());
                    }
                    ini.save(&path).ok();
                })
            })
            .collect();
        let fsr = div().mt_3().flex().items_center().justify_between().child(div().child(div().font_weight(FontWeight::SEMIBOLD).child("Upscaler (FSR-style, applied)")).child(div().text_sm().text_color(rgb(MUTED)).child("Bicubic scaling with adaptive sharpening: use it with a render density below 100 %; from the next start"))).child(div().flex().gap_2().children(fsr_chips));
        div().flex().flex_col().child(head).child(bright).child(lods).child(scale).child(fsr).child(div().mt_3().children(rows)).into_any_element()
    }

    fn save_controls(&self) {
        self.controls_ini.save(&self.own.join("cfg/controls.ini")).ok();
    }

    fn play_binary() -> PathBuf {
        let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("rustyac-play")));
        beside.filter(|p| p.is_file()).unwrap_or_else(|| PathBuf::from("rustyac-play"))
    }

    fn drive(&mut self, cx: &mut Context<Self>) {
        if self.game.is_some() {
            return;
        }
        let Some(root) = self.root.clone() else { return };
        self.save_assists();
        self.save_controls();
        let s = &self.settings;
        let mut command = Command::new(Self::play_binary());
        command.current_dir(&root).env("AC_ROOT", &root).args(["--car", &s.car, "--track", &s.track, "--spawn", &s.spawn, "--weather", &s.weather, "--hour", &s.hour.to_string(), "--grip", &s.grip.to_string()]);
        if !s.skin.is_empty() {
            command.args(["--skin", &s.skin]);
        }
        if !s.layout.is_empty() {
            command.args(["--layout", &s.layout]);
        }
        command.args(["--size", &format!("{}x{}", s.size.0, s.size.1)]);
        if !s.sound {
            command.arg("--no-audio");
        } else {
            command.args(["--volume", &format!("{}", s.volume)]);
        }
        if !self.assists.auto_shifter {
            command.arg("--manual");
        }
        if !s.server.is_empty() {
            command.args(["--join", &s.server]);
            if !s.name.is_empty() {
                command.args(["--name", &s.name]);
            }
            if !s.team.is_empty() {
                command.args(["--team", &s.team]);
            }
            if !s.password.is_empty() {
                command.args(["--password", &s.password]);
            }
        }
        let log = std::fs::File::create(self.own.join("play.log")).ok();
        match log {
            Some(file) => {
                command.stdout(file.try_clone().map(Stdio::from).unwrap_or_else(|_| Stdio::null())).stderr(Stdio::from(file));
            }
            None => {
                command.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        match command.spawn() {
            Ok(child) => {
                self.game = Some(child);
                self.status = "The game is running".into();
                // look for its end twice a second
                cx.spawn(async move |this, cx| loop {
                    cx.background_executor().timer(Duration::from_millis(500)).await;
                    let alive = this
                        .update(cx, |this, cx| {
                            let done = this.game.as_mut().is_none_or(|g| g.try_wait().ok().flatten().is_some());
                            if done {
                                this.game = None;
                                this.status = format!("The game closed. Its log: {}", this.own.join("play.log").display());
                                cx.notify();
                            }
                            !done
                        })
                        .unwrap_or(false);
                    if !alive {
                        break;
                    }
                })
                .detach();
            }
            Err(e) => self.status = format!("rustyac-play did not start ({}): {e}", Self::play_binary().display()),
        }
        cx.notify();
    }

    // --- pieces -----------------------------------------------------------------------------

    fn chip(&self, id: impl Into<gpui::ElementId>, label: impl Into<SharedString>, on: bool, cx: &mut Context<Self>, f: impl Fn(&mut Launcher) + 'static) -> AnyElement {
        div()
            .id(id)
            .px_3()
            .py_1p5()
            .rounded_md()
            .border_1()
            .border_color(rgb(if on { ACCENT } else { LINE }))
            .bg(rgb(if on { ACCENT_DIM } else { CARD }))
            .text_sm()
            .text_color(rgb(if on { TEXT } else { MUTED }))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(if on { ACCENT_DIM } else { CARD_HOVER })))
            .child(label.into())
            .on_click(cx.listener(move |this, _, _, cx| {
                f(this);
                cx.notify();
            }))
            .into_any_element()
    }

    fn section_title(text: &str) -> Div {
        div().text_xs().text_color(rgb(MUTED)).font_weight(FontWeight::BOLD).mt_4().mb_2().child(SharedString::from(text.to_uppercase()))
    }

    fn picture(path: Option<PathBuf>, w: f32, h: f32) -> Div {
        let frame = div().flex_none().w(px(w)).h(px(h)).rounded_md().overflow_hidden().bg(rgb(0x0a0b0d)).flex().items_center().justify_center();
        match path {
            Some(p) => frame.child(img(p).w(px(w)).h(px(h)).object_fit(ObjectFit::Cover)),
            None => frame.child(div().text_color(rgb(LINE)).child("no preview")),
        }
    }

    fn spec(label: &str, value: &str) -> Div {
        div().flex().flex_col().min_w(px(90.)).child(div().text_xs().text_color(rgb(MUTED)).child(SharedString::from(label.to_string()))).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(SharedString::from(if value.is_empty() { "—".to_string() } else { value.to_string() })))
    }

    // --- pages ------------------------------------------------------------------------------

    fn car_details(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(car) = self.car().cloned() else { return div().into_any_element() };
        let preview = self.skin_preview(&car, &self.settings.skin);
        let skins: Vec<AnyElement> = car
            .skins
            .iter()
            .enumerate()
            .map(|(i, skin)| {
                let on = skin.id == self.settings.skin || (self.settings.skin.is_empty() && i == 0);
                let id = skin.id.clone();
                div()
                    .id(("skin", i))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_1()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(if on { ACCENT } else { LINE }))
                    .bg(rgb(CARD))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(CARD_HOVER)))
                    .child(Self::picture(skin.preview.clone(), 150., 84.))
                    .child(div().w(px(150.)).text_xs().text_color(rgb(if on { TEXT } else { MUTED })).overflow_hidden().child(SharedString::from(skin.id.trim_start_matches(|c: char| c.is_ascii_digit() || c == '_').replace('_', " "))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings.skin = id.clone();
                        this.save_settings();
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .items_start()
            .gap_5()
            .p_4()
            .rounded_lg()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(LINE))
            .child(Self::picture(preview, 560., 315.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_xs().text_color(rgb(ACCENT)).font_weight(FontWeight::BOLD).child(SharedString::from(format!("{} · {}", car.brand, car.class).to_uppercase())))
                    .child(div().text_2xl().font_weight(FontWeight::BOLD).child(SharedString::from(car.name.clone())))
                    .child(div().flex().flex_wrap().gap_4().child(Self::spec("Power", &car.bhp)).child(Self::spec("Torque", &car.torque)).child(Self::spec("Weight", &car.weight)).child(Self::spec("Top speed", &car.top_speed)).child(Self::spec("0–100", &car.acceleration)))
                    .child(div().text_sm().text_color(rgb(MUTED)).max_h(px(110.)).overflow_hidden().child(SharedString::from(car.description.clone())))
                    .child(div().text_xs().text_color(rgb(MUTED)).mt_2().child("SKINS"))
                    .child(div().id("skins").flex().gap_2().overflow_x_scroll().children(skins)),
            )
            .into_any_element()
    }

    fn page_cars(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let cards: Vec<AnyElement> = self
            .cars
            .clone()
            .into_iter()
            .enumerate()
            .map(|(i, car)| {
                let on = car.id == self.settings.car;
                let skin = if on { self.settings.skin.clone() } else { String::new() };
                let id = car.id.clone();
                div()
                    .id(("car", i))
                    .w(px(300.))
                    .rounded_lg()
                    .overflow_hidden()
                    .border_1()
                    .border_color(rgb(if on { ACCENT } else { LINE }))
                    .bg(rgb(CARD))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(CARD_HOVER)))
                    .child(Self::picture(self.skin_preview(&car, &skin), 298., 168.))
                    .child(
                        div()
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().font_weight(FontWeight::BOLD).child(SharedString::from(car.name.clone())))
                            .child(div().text_xs().text_color(rgb(MUTED)).child(SharedString::from(format!("{} · {} · {}", car.brand, car.bhp, car.weight)))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.settings.car != id {
                            this.settings.car = id.clone();
                            this.settings.skin.clear();
                        }
                        this.save_settings();
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        div().flex().flex_col().gap_4().child(self.car_details(cx)).child(Self::section_title("All cars")).child(div().flex().flex_wrap().gap_4().children(cards)).into_any_element()
    }

    fn page_tracks(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(track) = self.track().cloned() else { return div().child("no tracks").into_any_element() };
        let layout = track.layouts.iter().find(|l| l.id == self.settings.layout).or(track.layouts.first()).cloned().unwrap_or_default();
        let layout_chips: Vec<AnyElement> = if track.layouts.len() > 1 {
            track
                .layouts
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let id = l.id.clone();
                    let on = l.id == layout.id;
                    self.chip(("layout", i), l.name.clone(), on, cx, move |this| {
                        this.settings.layout = id.clone();
                        this.save_settings();
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        let details = div()
            .flex()
            .items_start()
            .gap_5()
            .p_4()
            .rounded_lg()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(LINE))
            .child(Self::picture(layout.preview.clone(), 560., 315.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_xs().text_color(rgb(ACCENT)).font_weight(FontWeight::BOLD).child(SharedString::from(track.country.to_uppercase())))
                    .child(div().text_2xl().font_weight(FontWeight::BOLD).child(SharedString::from(track.name.clone())))
                    .child(div().flex().gap_4().child(Self::spec("Layout", &layout.name)).child(Self::spec("Length", &layout.length)).child(Self::spec("Pit boxes", &layout.pitboxes)))
                    .child(div().text_sm().text_color(rgb(MUTED)).max_h(px(90.)).overflow_hidden().child(SharedString::from(track.description.clone())))
                    .when(!layout_chips.is_empty(), |d| d.child(div().text_xs().text_color(rgb(MUTED)).mt_2().child("LAYOUTS")).child(div().flex().flex_wrap().gap_2().children(layout_chips)))
                    .child(div().mt_2().child(Self::picture(layout.outline.clone(), 160., 120.))),
            );
        let cards: Vec<AnyElement> = self
            .tracks
            .clone()
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                let on = t.id == self.settings.track;
                let id = t.id.clone();
                let first = t.layouts.first().cloned().unwrap_or_default();
                div()
                    .id(("track", i))
                    .w(px(300.))
                    .rounded_lg()
                    .overflow_hidden()
                    .border_1()
                    .border_color(rgb(if on { ACCENT } else { LINE }))
                    .bg(rgb(CARD))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(CARD_HOVER)))
                    .child(Self::picture(first.preview, 298., 168.))
                    .child(div().p_3().flex().flex_col().gap_1().child(div().font_weight(FontWeight::BOLD).child(SharedString::from(t.name.clone()))).child(div().text_xs().text_color(rgb(MUTED)).child(SharedString::from(format!("{} · {} layout(s)", t.country, t.layouts.len())))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.settings.track != id {
                            this.settings.track = id.clone();
                            this.settings.layout.clear();
                        }
                        this.save_settings();
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        div().flex().flex_col().gap_4().child(details).child(Self::section_title("All tracks")).child(div().flex().flex_wrap().gap_4().children(cards)).into_any_element()
    }

    fn page_drive(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let car = self.car().cloned();
        let track = self.track().cloned();
        let layout = track.as_ref().and_then(|t| t.layouts.iter().find(|l| l.id == self.settings.layout).or(t.layouts.first()).cloned()).unwrap_or_default();
        let big = |title: &str, name: String, sub: String, path: Option<PathBuf>, go: Page, cx: &mut Context<Self>| {
            div()
                .id(SharedString::from(format!("big-{title}")))
                .flex_1()
                .min_w_0()
                .rounded_lg()
                .overflow_hidden()
                .border_1()
                .border_color(rgb(LINE))
                .bg(rgb(CARD))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(CARD_HOVER)))
                .child(Self::picture(path, 470., 264.))
                .child(div().p_4().flex().flex_col().gap_1().child(div().text_xs().text_color(rgb(ACCENT)).font_weight(FontWeight::BOLD).child(SharedString::from(title.to_uppercase()))).child(div().text_xl().font_weight(FontWeight::BOLD).child(SharedString::from(name))).child(div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(sub))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = go;
                    cx.notify();
                }))
        };
        let car_name = car.as_ref().map_or("no car".to_string(), |c| c.name.clone());
        let car_sub = car.as_ref().map_or(String::new(), |c| format!("{} · {} · {}", c.brand, c.bhp, c.weight));
        let car_pic = car.as_ref().and_then(|c| self.skin_preview(c, &self.settings.skin));
        let level = self.assists.level();
        let level_cards: Vec<AnyElement> = Level::ALL
            .into_iter()
            .map(|l| {
                let on = level == Some(l);
                div()
                    .id(SharedString::from(format!("level-{}", l.label())))
                    .flex_1()
                    .min_w_0()
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(if on { ACCENT } else { LINE }))
                    .bg(rgb(if on { ACCENT_DIM } else { CARD }))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(if on { ACCENT_DIM } else { CARD_HOVER })))
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child(l.label()))
                    .child(div().text_sm().text_color(rgb(MUTED)).mt_1().child(l.about()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.assists = l.assists();
                        this.save_assists();
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let spawns: Vec<AnyElement> = [("pit", "From the pits"), ("start", "On the grid"), ("hotlap", "Hot lap")]
            .into_iter()
            .map(|(id, label)| {
                let on = self.settings.spawn == id;
                self.chip(SharedString::from(format!("spawn-{id}")), label, on, cx, move |this| {
                    this.settings.spawn = id.to_string();
                    this.save_settings();
                })
            })
            .collect();
        let weathers: Vec<AnyElement> = [("3_clear", "Clear"), ("4_mid_clear", "Mild"), ("5_light_clouds", "Light clouds"), ("6_mid_clouds", "Clouds"), ("7_heavy_clouds", "Overcast"), ("2_light_fog", "Light fog"), ("1_heavy_fog", "Fog")]
            .into_iter()
            .map(|(id, label)| {
                let on = self.settings.weather == id;
                self.chip(SharedString::from(format!("wx-{id}")), label, on, cx, move |this| {
                    this.settings.weather = id.to_string();
                    this.save_settings();
                })
            })
            .collect();
        let hours: Vec<AnyElement> = [8u32, 10, 12, 14, 16, 18, 19]
            .into_iter()
            .map(|h| {
                let on = self.settings.hour == h;
                self.chip(SharedString::from(format!("hour-{h}")), format!("{h}:00"), on, cx, move |this| {
                    this.settings.hour = h;
                    this.save_settings();
                })
            })
            .collect();
        let grips: Vec<AnyElement> = [("Dry", 1.0f32), ("Damp", 0.85), ("Wet", 0.7), ("Soaked", 0.55)]
            .into_iter()
            .map(|(label, g)| {
                let on = (self.settings.grip - g).abs() < 0.01;
                self.chip(SharedString::from(format!("grip-{label}")), label, on, cx, move |this| {
                    this.settings.grip = g;
                    this.save_settings();
                })
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().flex().gap_4().child(big("Car", car_name, car_sub, car_pic, Page::Cars, cx)).child(big("Track", track.as_ref().map_or("no track".into(), |t| t.name.clone()), format!("{} · {}", layout.name, layout.length), layout.preview.clone(), Page::Tracks, cx)))
            .child(Self::section_title("Driver level"))
            .child(div().flex().gap_4().children(level_cards))
            .child(Self::section_title("Start"))
            .child(div().flex().gap_2().children(spawns))
            .child(Self::section_title("Weather"))
            .child(div().flex().flex_wrap().gap_2().children(weathers))
            .child(Self::section_title("Road surface (grip of the whole track)"))
            .child(div().flex().gap_2().children(grips))
            .child(Self::section_title("Time of day"))
            .child(div().flex().gap_2().children(hours))
            .into_any_element()
    }

    fn option_row(&self, label: &str, hint: &str, options: Vec<AnyElement>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .py_3()
            .border_b_1()
            .border_color(rgb(LINE))
            .child(div().flex().flex_col().child(div().font_weight(FontWeight::SEMIBOLD).child(SharedString::from(label.to_string()))).child(div().text_xs().text_color(rgb(MUTED)).child(SharedString::from(hint.to_string()))))
            .child(div().flex().gap_2().children(options))
            .into_any_element()
    }

    /// A row of chips, one per `(label, value)`, that sets a value.
    fn pick_row<T: PartialEq + Clone + 'static>(&mut self, id: &'static str, label: &str, hint: &str, current: T, options: &[(&str, T)], cx: &mut Context<Self>, set: impl Fn(&mut Launcher, T) + Clone + 'static) -> AnyElement {
        let chips = options
            .iter()
            .enumerate()
            .map(|(i, (text, value))| {
                let (value, set) = (value.clone(), set.clone());
                let on = current == value;
                self.chip((id, i), text.to_string(), on, cx, move |this| {
                    set(this, value.clone());
                    this.save_assists();
                })
            })
            .collect();
        self.option_row(label, hint, chips)
    }

    fn page_assists(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let a = self.assists.clone();
        let level = a.level();
        let level_chips: Vec<AnyElement> = Level::ALL
            .into_iter()
            .map(|l| {
                self.chip(SharedString::from(format!("lv-{}", l.label())), l.label(), level == Some(l), cx, move |this| {
                    this.assists = l.assists();
                    this.save_assists();
                })
            })
            .collect();
        let rows = vec![
            self.pick_row("abs", "ABS", "Stops the wheels locking under braking", a.abs, &[("Off", 0), ("Low", 1), ("Full", 2)], cx, |t, v| t.assists.abs = v),
            self.pick_row("tc", "Traction control", "Keeps the driven wheels from spinning", a.traction_control, &[("Off", 0), ("Low", 1), ("Full", 2)], cx, |t, v| t.assists.traction_control = v),
            self.pick_row("stab", "Stability control", "Catches the car when it starts to slide", a.stability, &[("Off", 0), ("25 %", 25), ("50 %", 50), ("75 %", 75), ("100 %", 100)], cx, |t, v| t.assists.stability = v),
            self.pick_row("gear", "Gearbox", "Who changes gear", a.auto_shifter, &[("Automatic", true), ("Manual", false)], cx, |t, v| t.assists.auto_shifter = v),
            self.pick_row("clutch", "Clutch", "Automatic clutch for starts and shifts", a.auto_clutch, &[("Automatic", true), ("Manual", false)], cx, |t, v| t.assists.auto_clutch = v),
            self.pick_row("blip", "Throttle blip", "Revs matched on downshifts", a.auto_blip, &[("On", true), ("Off", false)], cx, |t, v| t.assists.auto_blip = v),
            self.pick_row("damage", "Mechanical damage", "How much a crash costs the car", a.damage, &[("None", 0), ("25 %", 25), ("50 %", 50), ("100 %", 100)], cx, |t, v| t.assists.damage = v),
            self.pick_row("fuel", "Fuel use", "1× is real consumption", a.fuel_rate, &[("Off", 0.0), ("1×", 1.0), ("2×", 2.0), ("5×", 5.0)], cx, |t, v| t.assists.fuel_rate = v),
            self.pick_row("wear", "Tyre wear", "1× is real wear", a.tyre_wear, &[("Off", 0.0), ("1×", 1.0), ("2×", 2.0), ("5×", 5.0)], cx, |t, v| t.assists.tyre_wear = v),
            self.pick_row("slip", "Slipstream", "Drafting behind other cars", a.slipstream, &[("On", true), ("Off", false)], cx, |t, v| t.assists.slipstream = v),
        ];
        div()
            .flex()
            .flex_col()
            .child(div().flex().items_center().justify_between().child(div().text_sm().text_color(rgb(MUTED)).child("Pick a level, then change anything below.")).child(div().flex().gap_2().children(level_chips)))
            .child(div().mt_2().text_sm().text_color(rgb(MUTED)).child(level.map_or("Custom: your own mix of aids.", |l| l.about())))
            .child(div().mt_3().children(rows))
            .into_any_element()
    }

    fn stepper(&mut self, id: &'static str, value: String, cx: &mut Context<Self>, minus: impl Fn(&mut Launcher) + 'static, plus: impl Fn(&mut Launcher) + 'static) -> AnyElement {
        let button = |this: &mut Self, id: (&'static str, usize), text: &'static str, cx: &mut Context<Self>, f: Box<dyn Fn(&mut Launcher)>| {
            this.chip(id, text, false, cx, move |t| {
                f(t);
                t.controls_ini.save(&t.own.join("cfg/controls.ini")).ok();
                t.save_settings();
            })
        };
        let minus: Box<dyn Fn(&mut Launcher)> = Box::new(minus);
        let plus: Box<dyn Fn(&mut Launcher)> = Box::new(plus);
        div().flex().items_center().gap_2().child(button(self, (id, 0), "−", cx, minus)).child(div().w(px(64.)).flex().justify_center().font_weight(FontWeight::SEMIBOLD).child(SharedString::from(value))).child(button(self, (id, 1), "+", cx, plus)).into_any_element()
    }

    fn page_controls(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<AnyElement> = BINDINGS
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let vk = self.controls_ini.get_key(b.section, b.key);
                let waiting = self.capture == Some(i);
                let keycap = div()
                    .id(("bind", i))
                    .min_w(px(130.))
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(if waiting { ACCENT } else { LINE }))
                    .bg(rgb(if waiting { ACCENT_DIM } else { CARD }))
                    .flex()
                    .justify_center()
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(CARD_HOVER)))
                    .child(SharedString::from(if waiting { "press a key…".to_string() } else { key_name(vk) }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.capture = if this.capture == Some(i) { None } else { Some(i) };
                        this.editing = None;
                        window.focus(&this.focus);
                        cx.notify();
                    }))
                    .into_any_element();
                let clear = self.chip(("unbind", i), "Clear", false, cx, move |this| {
                    this.controls_ini.set_key(BINDINGS[i].section, BINDINGS[i].key, -1);
                    this.save_controls();
                });
                self.option_row(b.label, if vk == b.default { "default" } else { "changed" }, vec![keycap, clear])
            })
            .collect();
        let feel: Vec<AnyElement> = STEERING
            .iter()
            .map(|&(label, key, step, lo, hi)| {
                let now = self.controls_ini.get_f32("KEYBOARD", key, lo.max(1.0));
                let stepper = self.stepper(
                    key,
                    format!("{now:.2}"),
                    cx,
                    move |t| {
                        let v = (t.controls_ini.get_f32("KEYBOARD", key, now) - step).max(lo);
                        t.controls_ini.set("KEYBOARD", key, &format!("{v:.2}"));
                    },
                    move |t| {
                        let v = (t.controls_ini.get_f32("KEYBOARD", key, now) + step).min(hi);
                        t.controls_ini.set("KEYBOARD", key, &format!("{v:.2}"));
                    },
                );
                self.option_row(label, "keyboard steering", vec![stepper])
            })
            .collect();
        let reset = self.chip("reset-keys", "Back to the default keys", false, cx, |this| {
            for b in BINDINGS {
                this.controls_ini.set_key(b.section, b.key, b.default);
            }
            this.capture = None;
            this.save_controls();
        });
        div()
            .flex()
            .flex_col()
            .child(div().flex().items_center().justify_between().child(div().text_sm().text_color(rgb(MUTED)).child("Click a key, then press the new one. Esc cancels.")).child(reset))
            .child(Self::section_title("Keys"))
            .children(rows)
            .child(Self::section_title("Steering feel"))
            .children(feel)
            .into_any_element()
    }

    fn text_field(&mut self, field: Field, label: &str, value: String, secret: bool, cx: &mut Context<Self>) -> AnyElement {
        let on = self.editing == Some(field);
        let shown = if secret && !value.is_empty() { "•".repeat(value.chars().count()) } else { value.clone() };
        let id = match field {
            Field::ModPath => "f-mod",
            Field::Server => "f-server",
            Field::Name => "f-name",
            Field::Team => "f-team",
            Field::Password => "f-pass",
        };
        let input = div()
            .id(id)
            .w(px(280.))
            .px_3()
            .py_1p5()
            .rounded_md()
            .border_1()
            .border_color(rgb(if on { ACCENT } else { LINE }))
            .bg(rgb(CARD))
            .cursor_text()
            .child(SharedString::from(format!("{}{}", shown, if on { "▏" } else { "" })))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.editing = Some(field);
                this.capture = None;
                window.focus(&this.focus);
                cx.notify();
            }))
            .into_any_element();
        self.option_row(label, "", vec![input])
    }

    fn page_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.clone();
        let sizes: Vec<AnyElement> = [(1280, 720), (1600, 900), (1920, 1080), (2560, 1440), (2880, 1800)]
            .into_iter()
            .enumerate()
            .map(|(i, (w, h))| self.chip(("size", i), format!("{w}×{h}"), s.size == (w, h), cx, move |this| {
                this.settings.size = (w, h);
                this.save_settings();
            }))
            .collect();
        let sound: Vec<AnyElement> = [("On", true), ("Off", false)]
            .into_iter()
            .enumerate()
            .map(|(i, (t, v))| self.chip(("sound", i), t, s.sound == v, cx, move |this| {
                this.settings.sound = v;
                this.save_settings();
            }))
            .collect();
        let percent = (s.volume * 100.0).round() as i32;
        let volume = self.stepper(
            "volume",
            format!("{percent} %"),
            cx,
            |t| t.settings.volume = (((t.settings.volume * 100.0).round() - 10.0).max(0.0)) / 100.0,
            |t| t.settings.volume = (((t.settings.volume * 100.0).round() + 10.0).min(200.0)) / 100.0,
        );
        let clear = self.chip("server-clear", "Drive alone", s.server.is_empty(), cx, |this| {
            this.settings.server.clear();
            this.save_settings();
        });
        let server = self.text_field(Field::Server, "Server (host:port)", s.server.clone(), false, cx);
        let name = self.text_field(Field::Name, "Your name", s.name.clone(), false, cx);
        let team = self.text_field(Field::Team, "Team", s.team.clone(), false, cx);
        let password = self.text_field(Field::Password, "Server password", s.password.clone(), true, cx);
        div()
            .flex()
            .flex_col()
            .child(Self::section_title("Picture"))
            .child(self.option_row("Window size", "of the game", sizes))
            .child(Self::section_title("Sound"))
            .child(self.option_row("Sound", "AC's own engine and tyre sounds", sound))
            .child(self.option_row("Volume", "master", vec![volume]))
            .child(Self::section_title("Multiplayer"))
            .child(div().text_sm().text_color(rgb(MUTED)).child("Join an acServer on the local network or anywhere: type its address, e.g. 192.168.1.20:9600. With an address set, Drive joins that server."))
            .child(self.option_row("Mode", if s.server.is_empty() { "single player" } else { "online" }, vec![clear]))
            .child(server)
            .child(name)
            .child(team)
            .child(password)
            .into_any_element()
    }

    fn sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let items: Vec<AnyElement> = Page::ALL
            .into_iter()
            .map(|(page, label, icon)| {
                let on = self.page == page;
                div()
                    .id(label)
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_2p5()
                    .mx_2()
                    .rounded_md()
                    .bg(rgb(if on { CARD } else { PANEL }))
                    .text_color(rgb(if on { TEXT } else { MUTED }))
                    .font_weight(if on { FontWeight::BOLD } else { FontWeight::NORMAL })
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(CARD_HOVER)).text_color(rgb(TEXT)))
                    .child(div().w(px(18.)).text_color(rgb(if on { ACCENT } else { MUTED })).child(icon))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.page = page;
                        this.capture = None;
                        this.editing = None;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        div()
            .w(px(220.))
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(LINE))
            .child(div().px_5().pt_6().pb_6().flex().child(div().text_2xl().font_weight(FontWeight::BOLD).child("rusty")).child(div().text_2xl().font_weight(FontWeight::BOLD).text_color(rgb(ACCENT)).child("AC")))
            .children(items)
            .child(div().flex_1())
            .child(div().p_4().text_xs().text_color(rgb(MUTED)).child(SharedString::from(self.root.as_ref().map_or("no game folder".to_string(), |r| r.display().to_string()))))
            .into_any_element()
    }

    fn bottom_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let car = self.car().map_or("—".to_string(), |c| c.name.clone());
        let track = self.track().map_or("—".to_string(), |t| t.name.clone());
        let level = self.assists.level().map_or("Custom", |l| l.label());
        let running = self.game.is_some();
        let online = !self.settings.server.is_empty();
        let go = div()
            .id("drive")
            .px_10()
            .py_3()
            .rounded_md()
            .bg(rgb(if running { LINE } else { ACCENT }))
            .text_color(rgb(TEXT))
            .text_xl()
            .font_weight(FontWeight::BOLD)
            .cursor_pointer()
            .hover(|s| s.opacity(0.88))
            .child(if running { "RUNNING" } else if online { "JOIN" } else { "DRIVE" })
            .on_click(cx.listener(|this, _, _, cx| this.drive(cx)));
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_6()
            .px_6()
            .py_4()
            .bg(rgb(PANEL))
            .border_t_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child(SharedString::from(format!("{car}  ·  {track}"))))
                    .child(div().text_sm().text_color(rgb(if self.status.is_empty() { MUTED } else { GOOD })).child(SharedString::from(if self.status.is_empty() { format!("{level} · {} · {}×{}{}", self.settings.spawn, self.settings.size.0, self.settings.size.1, if online { format!(" · server {}", self.settings.server) } else { String::new() }) } else { self.status.clone() }))),
            )
            .child(go)
            .into_any_element()
    }
}

use gpui::Div;

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(path) = self.install_mod_later.take() {
            self.install_mod(&path, cx);
        }
        if std::mem::take(&mut self.csp_install_later) {
            if let Some(root) = self.root.clone() {
                self.mod_status = "Downloading Custom Shaders Patch (120 MB)…".into();
                self.mod_status = match crate::csp::install(&root, &self.own.join("tmp")) {
                    Ok(n) => format!("Custom Shaders Patch {} installed ({n} files)", crate::csp::VERSION),
                    Err(why) => why,
                };
            }
        }
        if let Some((file, on)) = self.csp_toggle_later.take() {
            if let Some(root) = self.root.clone() {
                crate::csp::set_enabled(&root, &self.own, &file, on).ok();
            }
        }
        if let Some((kind, id)) = self.remove_later.take() {
            self.remove_mod(kind, &id, cx);
        }
        if !self.focus.is_focused(window) && self.capture.is_none() && self.editing.is_none() {
            // (the keys of a rebind and of a text field come to this view)
            window.focus(&self.focus);
        }
        let title = match self.page {
            Page::Drive => "Ready to drive",
            Page::Cars => "Choose your car",
            Page::Tracks => "Choose the track",
            Page::Assists => "Driving aids",
            Page::Graphics => "Graphics",
            Page::Content => "Content and mods",
            Page::Csp => "Custom Shaders Patch",
            Page::Controls => "Controls",
            Page::Settings => "Settings",
        };
        let body = if self.root.is_none() {
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_xl().font_weight(FontWeight::BOLD).child("The game's folder was not found"))
                .child(div().text_color(rgb(MUTED)).child("rustyAC reads the cars, tracks and sounds from your own Assetto Corsa. Put this launcher in that folder (the one with content/ and system/), or set AC_ROOT to it, or start it with --ac-root <folder>."))
                .into_any_element()
        } else {
            match self.page {
                Page::Drive => self.page_drive(cx),
                Page::Cars => self.page_cars(cx),
                Page::Tracks => self.page_tracks(cx),
                Page::Assists => self.page_assists(cx),
                Page::Graphics => self.page_graphics(cx),
                Page::Content => self.page_content(cx),
                Page::Csp => self.page_csp(cx),
                Page::Controls => self.page_controls(cx),
                Page::Settings => self.page_settings(cx),
            }
        };
        let sidebar = self.sidebar(cx);
        let bottom = self.bottom_bar(cx);
        div()
            .size_full()
            .flex()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| this.key(event, cx)))
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _window, cx| {
                if let Some(i) = this.capture {
                    let m = event.modifiers;
                    let vk = if m.control { Some(0xA2) } else if m.shift { Some(0xA0) } else if m.alt { Some(0xA4) } else { None };
                    if let Some(vk) = vk {
                        this.controls_ini.set_key(BINDINGS[i].section, BINDINGS[i].key, vk);
                        this.save_controls();
                        this.capture = None;
                        cx.notify();
                    }
                }
            }))
            .child(sidebar)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(div().px_8().pt_6().pb_2().text_3xl().font_weight(FontWeight::BOLD).child(title))
                    .child(div().id("content").flex_1().min_h_0().overflow_y_scroll().px_8().pb_6().child(body))
                    .child(bottom),
            )
    }
}

impl Launcher {
    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if let Some(i) = self.capture {
            if key == "escape" {
                self.capture = None;
            } else if let Some(vk) = key_from_gpui(key) {
                self.controls_ini.set_key(BINDINGS[i].section, BINDINGS[i].key, vk);
                self.save_controls();
                self.capture = None;
            }
            cx.notify();
            return;
        }
        if let Some(field) = self.editing {
            let target = match field {
                Field::ModPath => &mut self.mod_path,
                Field::Server => &mut self.settings.server,
                Field::Name => &mut self.settings.name,
                Field::Team => &mut self.settings.team,
                Field::Password => &mut self.settings.password,
            };
            match key {
                "escape" | "enter" | "tab" => self.editing = None,
                "backspace" => {
                    target.pop();
                }
                _ => {
                    if let Some(text) = event.keystroke.key_char.as_deref().filter(|t| !event.keystroke.modifiers.platform && !event.keystroke.modifiers.control && !t.chars().any(char::is_control)) {
                        target.push_str(text);
                    }
                }
            }
            self.save_settings();
            cx.notify();
        }
    }
}

pub fn run(ac_root: Option<PathBuf>, page: Option<String>) {
    gpui::Application::new().run(move |cx: &mut App| {
        let page = page.clone();
        let bounds = Bounds::centered(None, gpui::size(px(1280.), px(820.)), cx);
        let root = find_root(ac_root.clone());
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), titlebar: Some(gpui::TitlebarOptions { title: Some("rustyAC".into()), ..Default::default() }), ..Default::default() },
            |window, cx| {
                let view = cx.new(|cx| {
                    let mut l = Launcher::new(root, cx);
                    l.page = match page.as_deref() {
                        Some("cars") => Page::Cars,
                        Some("tracks") => Page::Tracks,
                        Some("assists") => Page::Assists,
                        Some("graphics") => Page::Graphics,
                        Some("content") => Page::Content,
                        Some("csp") => Page::Csp,
                        Some("controls") => Page::Controls,
                        Some("settings") => Page::Settings,
                        _ => Page::Drive,
                    };
                    l
                });
                window.focus(&view.read(cx).focus);
                view
            },
        )
        .unwrap();
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}

const GFX_LOW: &[(&str, &str, i32)] = &[("VIDEO", "AASAMPLES", 1), ("VIDEO", "ANISOTROPIC", 0), ("VIDEO", "SHADOW_MAP_SIZE", 512), ("ASSETTOCORSA", "WORLD_DETAIL", 1), ("CUBEMAP", "SIZE", 0), ("MIRROR", "SIZE", 256), ("EFFECTS", "SMOKE", 1), ("POST_PROCESS", "ENABLED", 0), ("POST_PROCESS", "FXAA", 0), ("EFFECTS", "MOTION_BLUR", 0)];
const GFX_MEDIUM: &[(&str, &str, i32)] = &[("VIDEO", "AASAMPLES", 2), ("VIDEO", "ANISOTROPIC", 8), ("VIDEO", "SHADOW_MAP_SIZE", 2048), ("ASSETTOCORSA", "WORLD_DETAIL", 5), ("CUBEMAP", "SIZE", 512), ("MIRROR", "SIZE", 512), ("EFFECTS", "SMOKE", 3), ("POST_PROCESS", "ENABLED", 1), ("POST_PROCESS", "FXAA", 0), ("EFFECTS", "MOTION_BLUR", 0)];
const GFX_HIGH: &[(&str, &str, i32)] = &[("VIDEO", "AASAMPLES", 4), ("VIDEO", "ANISOTROPIC", 16), ("VIDEO", "SHADOW_MAP_SIZE", 2048), ("ASSETTOCORSA", "WORLD_DETAIL", 5), ("CUBEMAP", "SIZE", 1024), ("MIRROR", "SIZE", 1024), ("EFFECTS", "SMOKE", 5), ("POST_PROCESS", "ENABLED", 1), ("POST_PROCESS", "FXAA", 1), ("EFFECTS", "MOTION_BLUR", 0)];
