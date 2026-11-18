// SPDX-License-Identifier: GPL-3.0-or-later

//! The window, the device and the frame loop: `rustyac-web`'s `Game::frame` without the page.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use rustyac_game::input_file::event;
use rustyac_game::render::ac::{AcRenderer, CrewCamera};
use rustyac_render::wgpu;
use rustyac_web::full::{self, GuiLoop};
use rustyac_web::session::Session;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::keys::vk;

struct Options {
    car: Option<String>,
    track: Option<String>,
    layout: String,
    spawn: String,
    race: Option<PathBuf>,
    size: (u32, u32),
    join: Option<(String, u16)>,
    no_audio: bool,
    volume: Option<f32>,
    audio_wav: Option<PathBuf>,
    quit_after: Option<f64>,
    start_on_foot: bool,
    debug_panel: bool,
    weather: Option<String>,
    grip: Option<f32>,
    hour: Option<f32>,
    /// `--press KeyF@5`: a key pressed by the game itself after that many seconds, for checks without a keyboard
    press: Vec<(f64, KeyCode)>,
    shot: Option<PathBuf>,
    skin: String,
    manual: bool,
    name: Option<String>,
    team: String,
    password: String,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options { car: None, track: None, layout: String::new(), spawn: "pit".to_string(), race: None, size: (1280, 720), join: None, name: None, team: String::new(), password: String::new(), no_audio: false, volume: None, audio_wav: None, quit_after: None, start_on_foot: false, debug_panel: false, weather: None, grip: None, hour: None, press: Vec::new(), shot: None, skin: String::new(), manual: false };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--car" => options.car = Some(value()?),
            "--track" => options.track = Some(value()?),
            "--layout" => options.layout = value()?,
            "--spawn" => options.spawn = value()?,
            "--race" => options.race = Some(value()?.into()),
            "--join" => options.join = Some(crate::online::parse_address(&value()?)?),
            "--name" => options.name = Some(value()?),
            "--team" => options.team = value()?,
            "--password" => options.password = value()?,
            "--quit-after" => options.quit_after = Some(value()?.parse().map_err(|e| format!("--quit-after: {e}"))?),
            "--skin" => options.skin = value()?,
            "--manual" => options.manual = true,
            "--start-on-foot" => options.start_on_foot = true,
            "--debug-panel" => options.debug_panel = true,
            "--weather" => options.weather = Some(value()?),
            "--grip" => options.grip = Some(value()?.parse().map_err(|e| format!("--grip: {e}"))?),
            "--hour" => options.hour = Some(value()?.parse().map_err(|e| format!("--hour: {e}"))?),
            "--press" => {
                let v = value()?;
                let (key, at) = v.split_once('@').ok_or("--press <Key>@<seconds>")?;
                let code = match key {
                    "KeyF" => KeyCode::KeyF,
                    "KeyW" => KeyCode::KeyW,
                    "F10" => KeyCode::F10,
                    "KeyL" => KeyCode::KeyL,
                    "F1" => KeyCode::F1,
                    "KeyH" => KeyCode::KeyH,
                    "Backspace" => KeyCode::Backspace,
                    other => return Err(format!("--press: only KeyF, KeyW, KeyL and F10 are known, not {other}")),
                };
                options.press.push((at.parse().map_err(|e| format!("--press: {e}"))?, code));
            }
            "--shot" => options.shot = Some(PathBuf::from(value()?)),
            "--no-audio" => options.no_audio = true,
            "--volume" => options.volume = Some(value()?.parse().map_err(|e| format!("--volume: {e}"))?),
            "--audio-wav" => options.audio_wav = Some(PathBuf::from(value()?)),
            "--size" => {
                let text = value()?;
                let (w, h) = text.split_once('x').ok_or("--size <width>x<height>")?;
                options.size = (w.parse().map_err(|e| format!("--size: {e}"))?, h.parse().map_err(|e| format!("--size: {e}"))?);
            }
            "--help" | "-h" => {
                println!("rustyac-play [--car <id>] [--track <id>] [--layout <l>] [--spawn pit|hotlap|start] [--race <race.ini>] [--size 1280x720] [--start-on-foot] [--weather <content/weather folder>] [--grip <0.4-1: the whole track's grip, a wet road below 1>] [--hour <0-23>] [--debug-panel: F10's panel open from the start] [--shot <file.png>: the last picture when --quit-after ends the run] [--skin <s>] [--manual] [--no-audio] [--volume 0..1] [--audio-wav <file>] [--quit-after <seconds>]\n  on a server (Kunos' acServer, the local network too): --join <host>:<port> [--car <the server's model>] [--name <driver>] [--team <t>] [--password <p>]\nthe game's folder: AC_ROOT, or Steam's usual place");
                std::process::exit(0);
            }
            other => return Err(format!("unknown option {other} (--help)")),
        }
    }
    Ok(options)
}

/// The first folder of `content/<kind>` (sorted): the default car and track.
fn first_of(root: &Path, kind: &str) -> Result<String, String> {
    let mut names: Vec<String> = rustyac_content::vfs::read_dir(&root.join("content").join(kind))
        .map_err(|e| format!("content/{kind}: {e}"))?
        .into_iter()
        .filter(|(_, dir)| *dir)
        .map(|(name, _)| name)
        .collect();
    names.sort();
    names.into_iter().next().ok_or(format!("content/{kind} is empty"))
}

/// Where AC's apps keep their own files: not the game's folder, not Documents.
fn own_folder() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/rustyAC")
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")).join("rustyAC")
    }
}

struct Game {
    session: Session,
    full: Box<AcRenderer>,
    gui: GuiLoop,
    paused: bool,
    last: Option<Instant>,
    /// AC's sound (the native mixer, `rustyac_audio::native`), when it started.
    sound: Option<rustyac_game::audio::GameAudio>,
    /// A change of the mouse grab asked for by the walk (the window applies it).
    grab: Option<bool>,
    last_right: Option<Instant>,
    /// The render density (1 = the window's own size).
    scale: f32,
    /// The key that brakes (controls.ini [KEYBOARD] BRAKE), held for the car left standing.
    brake_key: i32,
}

struct Screen {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    game: Game,
    debug: crate::debug::DebugUi,
    controls: crate::debug::Controls,
    last_draw: Option<Instant>,
}

struct App {
    options: Options,
    root: PathBuf,
    screen: Option<Screen>,
    modifiers: ModifiersState,
    error: Option<String>,
}

pub fn run() -> Result<(), String> {
    let options = parse_options()?;
    // the game's folder: AC_ROOT, else the folder this program sits in or is run from (so a copy
    // of `rustyac-play` in the game's folder just runs), else Steam's usual place
    if std::env::var_os("AC_ROOT").is_none() {
        let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
        let here = std::env::current_dir().ok();
        if let Some(folder) = beside.into_iter().chain(here).find(|f| rustyac_content::install::is_ac_root(f)) {
            rustyac_content::install::set_ac_root(Some(folder));
        }
    }
    let root = rustyac_content::install::ac_root().ok_or_else(rustyac_content::install::not_found_hint)?;
    // no car, track, race or server on the command line, and a terminal to ask: the launcher
    let mut options = options;
    if options.car.is_none() && options.track.is_none() && options.race.is_none() && options.join.is_none() && std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        match crate::menu::launcher(&root, &own_folder())? {
            crate::menu::Choice::Quit => return Ok(()),
            crate::menu::Choice::Play(s) => {
                options.car = Some(s.car);
                options.skin = s.skin;
                options.track = Some(s.track);
                options.layout = s.layout;
                options.spawn = s.spawn;
                options.manual = !s.auto_shifter;
                options.size = s.size;
                options.no_audio = !s.sound;
                options.volume = Some(s.volume);
                if !s.server.is_empty() {
                    options.join = Some(crate::online::parse_address(&s.server)?);
                    options.name = Some(if s.name.is_empty() { crate::online::default_name() } else { s.name });
                    options.team = s.team;
                    options.password = s.password;
                }
            }
        }
    }
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let mut app = App { options, root, screen: None, modifiers: ModifiersState::empty(), error: None };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    app.error.map_or(Ok(()), Err)
}

impl App {
    fn open(&self, event_loop: &ActiveEventLoop) -> Result<Screen, String> {
        let (width, height) = self.options.size;
        let window = Arc::new(event_loop.create_window(Window::default_attributes().with_title("rustyAC").with_inner_size(PhysicalSize::new(width, height))).map_err(|e| e.to_string())?);
        let size = window.inner_size();
        let (width, height) = (size.width.max(16), size.height.max(16));
        // Custom Shaders Patch's render density: the picture is drawn smaller and stretched to the window
        let (render_width, render_height) = scaled(self.scale(), width, height);

        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = if cfg!(target_os = "macos") { wgpu::Backends::METAL } else { wgpu::Backends::VULKAN };
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance.create_surface(window.clone()).map_err(|e| format!("the window cannot be drawn to: {e}"))?;
        let adapter = pollster_block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), ..Default::default() }))
            .map_err(|e| format!("no graphics adapter: {e}"))?;
        let features = AcRenderer::wanted_features(adapter.features());
        let (device, queue) = pollster_block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("rustyAC"), required_features: features, required_limits: adapter.limits(), ..Default::default() }))
            .map_err(|e| format!("the graphics device could not be opened: {e}"))?;
        device.on_uncaptured_error(Arc::new(|e: wgpu::Error| eprintln!("wgpu error: {e}")));
        device.set_device_lost_callback(|reason, message| eprintln!("wgpu device lost ({reason:?}): {message}"));
        AcRenderer::give_device(device.clone(), queue.clone(), adapter.features());

        // the colours are written as they are (no sRGB step), as the browser build does
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).or(caps.formats.first().copied()).ok_or("the window offers no pixel format")?;
        let mut config = surface.get_default_config(&adapter, width, height).ok_or("the window cannot be drawn to by this device")?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);
        println!("picture: AC's own renderer on {} ({:?})", adapter.get_info().name, adapter.get_info().backend);

        let mut game = self.make_game(render_width, render_height)?;
        game.scale = self.scale();
        if self.options.start_on_foot {
            game.toggle_walk();
            if let Some(on) = game.grab.take() {
                grab(&window, on);
            }
        }
        let mut debug = crate::debug::DebugUi::new(&window, &device, config.format);
        debug.open = self.options.debug_panel;
        let controls = crate::debug::Controls { brightness: game.full.brightness, lod_distance: game.full.lod_distance, paused: false, reset: false, to_track: false };
        Ok(Screen { window, surface, device, queue, config, game, debug, controls, last_draw: None })
    }

    /// Graphics adjustments' `[RENDER_SCALE] SCALE` of Custom Shaders Patch.
    fn scale(&self) -> f32 {
        csp_value(&self.root, "graphics_adjustments", "RENDER_SCALE", "SCALE").unwrap_or(1.0).clamp(0.25, 1.0)
    }

    fn make_game(&self, width: u32, height: u32) -> Result<Game, String> {
        if std::env::var_os("RUSTYAC_HDR_LIGHTS").is_none() {
            std::env::set_var("RUSTYAC_HDR_LIGHTS", "1");
        }
        let root = &self.root;
        if let Some(grip) = self.options.grip {
            // (read by the session's setup)
            std::env::set_var("RUSTYAC_GRIP", grip.to_string());
        }
        let mut session = match (&self.options.join, &self.options.race) {
            (Some((host, port)), _) => {
                let target = crate::online::Target { host: host.clone(), port: *port, name: self.options.name.clone().unwrap_or_else(crate::online::default_name), team: self.options.team.clone(), password: self.options.password.clone(), car: self.options.car.clone().unwrap_or_default() };
                let online = crate::online::join(&target, &own_folder(), root)?;
                Session::new_online(online, true, &self.options.spawn)?
            }
            (None, race) => match race {
            Some(file) => {
                let text = |path: &Path| std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()));
                let assists = Some(file.with_file_name("assists.ini")).filter(|p| p.is_file()).map(|p| text(&p)).transpose()?;
                let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
                Session::from_race(&text(file)?, assists.as_deref(), seed, true)?
            }
            None => {
                let car = self.options.car.clone().map_or_else(|| first_of(root, "cars"), Ok)?;
                let track = self.options.track.clone().map_or_else(|| first_of(root, "tracks"), Ok)?;
                println!("drive: {car} on {track}{}", if self.options.layout.is_empty() { String::new() } else { format!(" ({})", self.options.layout) });
                {
                    let assists = std::fs::read_to_string(own_folder().join("cfg/assists.ini")).ok();
                    Session::with_assists(&car, &track, &self.options.layout, !self.options.manual, &self.options.spawn, None, Some(&self.options.skin), assists.as_deref())?
                }
            }
            },
        };
        let own = own_folder();
        let cfg = own.join("cfg");
        std::fs::create_dir_all(&cfg).map_err(|e| format!("{}: {e}", cfg.display()))?;
        // the keys the launcher set (its Controls page writes cfg/controls.ini)
        let controls = cfg.join("controls.ini");
        if controls.is_file() {
            match rustyac_game::input::ini::ControlsIni::load(&controls) {
                Ok(ini) => {
                    session.set_controls(&ini, "the launcher's controls.ini");
                    println!("controls: {}", controls.display());
                }
                Err(why) => eprintln!("controls.ini: {why}"),
            }
        }
        if let Some(weather) = &self.options.weather {
            if root.join("content/weather").join(weather).is_dir() {
                session.look.weather = weather.clone();
            }
        }
        if let Some(hour) = self.options.hour {
            // AC's sun angle: 13:00 is 0, 16 degrees an hour (08:00 is -80)
            session.look.sun_angle = (hour - 13.0) * 16.0;
        }
        let shake_mul = session.remote_shapes.last().unwrap_or(&session.shape).shake_mul;
        session.camera.load_settings(None, shake_mul);
        session.shape.read_documents_fov();
        session.remote_shapes.iter_mut().for_each(rustyac_game::render::scene::CarShape::read_documents_fov);

        let mut notes = Vec::new();
        let setup = full::gui_setup(root, &cfg, &session);
        // the launcher's Graphics page writes this file
        let video = Some(own_folder().join("cfg/video.ini")).filter(|p| p.is_file());
        let mut renderer = full::renderer(root, width, height, &session, false, false, video.as_deref(), Some(setup), &mut notes)?;
        for note in &notes {
            println!("{note}");
        }
        renderer.lod_distance = csp_value(root, "graphics_adjustments", "LODS", "TRACK_DISTANCE_MULT").unwrap_or(1.0).clamp(0.2, 3.2);
        // the patch's FSR switch: the picture drawn at the render density is scaled up and sharpened
        if csp_value(root, "graphics_adjustments", "FSR", "ACTIVE").is_some() {
            renderer.upscale_sharpen = csp_value(root, "graphics_adjustments", "FSR", "SHARPEN2").unwrap_or(0.5).clamp(0.01, 1.0);
        }
        // a wet road is rained on
        if let Some(grip) = self.options.grip.filter(|g| *g < 0.99) {
            renderer.rain = ((1.0 - grip) / 0.45).clamp(0.2, 1.0);
        }
        // the headlights matter when it is dark, foggy or raining
        let angle = session.look.sun_angle.abs();
        let gloomy = session.look.weather.contains("fog") || session.look.weather.starts_with('7') || self.options.grip.is_some_and(|g| g < 0.99);
        renderer.lamp_strength = (((angle - 25.0) / 55.0).clamp(0.0, 1.0)).max(if gloomy { 0.45 } else { 0.0 }).max(0.15);
        renderer.brightness = csp_brightness(root);
        if renderer.brightness != 1.0 {
            println!("Custom Shaders Patch: Lighting FX brightness x{}", renderer.brightness);
        }

        let mut gui = GuiLoop::default();
        let setup = &session.sim.setup;
        gui.track_cameras = rustyac_game::render::track_camera::of_track(&root.join("content/tracks").join(&setup.track), &setup.layout).unwrap_or_default();
        gui.random_camera = Some(rustyac_game::render::track_camera::RandomCamera::of_install(&mut session.camera.cockpit.rand));
        session.camera.track_sets = gui.track_cameras.sets.len();
        let (f5, cinematic) = rustyac_game::render::onboard_free::load(None, Some(&session.sim.car_info().data_path));
        (gui.onboard_free, gui.cinematic) = (Some(f5), cinematic);
        if session.sim.setup.esc_start {
            if renderer.gui.as_ref().is_some_and(|gui| gui.has_esc_menu()) {
                use rustyac_game::render::scene::StartCamera;
                let setup = &session.sim.setup;
                gui.start_camera = Some(StartCamera::new(StartCamera::crew_side(&root.join("content/tracks").join(&setup.track), &setup.layout)));
            } else {
                session.request(event::PITS_DRIVE);
            }
        }
        if let Some(f5) = gui.onboard_free.as_mut() {
            f5.esc_mode = gui.start_camera.is_some();
        }
        let feed = Arc::new(std::sync::Mutex::new(rustyac_game::audio::Feed::default()));
        let options = rustyac_game::audio::AudioOptions { cfg_dir: Some(cfg.clone()), off: self.options.no_audio, volume: self.options.volume, wav: self.options.audio_wav.clone(), ..Default::default() };
        let sound = rustyac_game::audio::GameAudio::start_for(&options, Arc::clone(&feed), &session.sim.car_info(), &session.sim.opponent_infos());
        println!("{}", if sound.is_some() { "sound: AC's banks, played by rustyAC's own mixer" } else { "sound: none (see 'audio:' above)" });
        session.sound = sound.is_some().then_some(feed);
        let brake_key = rustyac_game::input::ini::ControlsIni::load(&cfg.join("controls.ini")).map_or(0x28, |ini| ini.get_hex("KEYBOARD", "BRAKE")).max(0);
        let brake_key = if brake_key == 0 { 0x28 } else { brake_key };
        Ok(Game { session, full: renderer, gui, paused: false, last: None, sound, grab: None, last_right: None, scale: 1.0, brake_key })
    }
}

fn pollster_block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn raw() -> RawWaker {
        RawWaker::new(std::ptr::null(), &RawWakerVTable::new(|_| raw(), |_| {}, |_| {}, |_| {}))
    }
    // wgpu's requests on a native backend are ready at once
    let waker = unsafe { Waker::from_raw(raw()) };
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::yield_now();
    }
}

impl Game {
    /// One frame: `Game::frame` of the browser build, without the sound.
    fn frame(&mut self) -> Result<(), String> {
        let now = Instant::now();
        let seconds = self.last.map_or(0.0, |last| now.duration_since(last).as_secs_f64());
        self.last = Some(now);
        let paused = self.gui.paused || self.paused;
        if self.gui.walk.is_some() {
            // the empty car stands: the brake is held for it
            let mut input = self.session.input.lock().unwrap();
            input.keys.clear();
            input.keys.insert(self.brake_key);
        }
        if !paused {
            self.session.advance(seconds)?;
            self.session.drawn_frame(seconds);
        }
        let view = self.session.view();
        let hud = full::hud_info(&self.session, paused);
        // the pit crew's wheel guns: is their sound still playing
        let guns = self.full.crew_guns_wanted();
        if let Some(sound) = &mut self.sound {
            for (wheel, screw) in guns.iter().enumerate().filter_map(|(wheel, wanted)| wanted.map(|screw| (wheel, screw))) {
                self.full.crew.playing[wheel][screw as usize] = sound.crew_is_playing(wheel, screw);
            }
        }
        full::draw(&mut self.full, &mut self.session, &view, seconds as f32, &hud, &mut self.gui);
        let calls = std::mem::take(&mut self.full.crew.sound);
        // the crew's camera: F5 turned on the car, held still, and back at the stop's end
        for asked in std::mem::take(&mut self.full.crew.camera) {
            match asked {
                CrewCamera::SetMode(mode) => self.gui.set_mode(&mut self.session, mode),
                CrewCamera::ForceThetaPhi(theta, phi, radius) => self.gui.onboard_free.iter_mut().for_each(|f5| f5.force_theta_phi(theta, phi, radius)),
                CrewCamera::Lock(locked) => self.gui.onboard_free.iter_mut().for_each(|f5| f5.locked = locked),
            }
        }
        self.full.crew.camera_mode = if self.gui.free_camera.is_some() { 6 } else { self.session.camera.manager_mode() };
        if let Some(sound) = &mut self.sound {
            use rustyac_game::render::ac::CrewSound;
            for call in calls {
                match call {
                    CrewSound::Start { wheel, screw, matrix } => sound.crew_start(wheel, screw, &matrix.m),
                    CrewSound::Stop { wheel, screw } => sound.crew_stop(wheel, screw),
                }
            }
            if let Some(gui) = self.full.gui.as_mut() {
                gui.apps.sound = sound.app_sound();
                for action in std::mem::take(&mut gui.apps.sound_actions) {
                    sound.app_action(&action);
                }
            }
            sound.set_picture_backfires(&self.full.backfires_triggered());
        }
        // the Triple screen app's screens, into the camera
        for action in self.full.gui.as_mut().map(|gui| std::mem::take(&mut gui.apps.vr_actions)).unwrap_or_default() {
            self.full.open_vr_action(&action);
        }
        self.gui.exchange(&mut self.full, &mut self.session);
        // Game::renderAudio: after the cameras and the picture, before it is shown
        if let (Some(sound), Some(camera)) = (&mut self.sound, self.gui.last_frame) {
            sound.frame(&self.session.camera, &camera.matrix, seconds, paused);
        }
        Ok(())
    }

    fn world(&self) -> Vec<rustyac_game::view::CarView> {
        std::iter::once(self.session.view()).chain(self.session.remote_views()).collect()
    }

    fn focus(&mut self, command: rustyac_game::focus::FocusCommand) {
        let world = self.world();
        self.gui.focus.get_or_insert_with(|| rustyac_game::focus::Focus::new(0)).command(command, &world, &[]);
    }

    /// A key down, as `app.js`'s keydown handler gives it: AC's GUI first, then the page's own
    /// command keys, then the driver's devices. False: leave the drive.
    fn key_down(&mut self, code: KeyCode, text: Option<&str>, modifiers: ModifiersState, repeat: bool) -> bool {
        use rustyac_game::focus::FocusCommand;
        let Some(vk_code) = vk(code) else { return true };
        let (ctrl, shift) = (modifiers.control_state_any(), modifiers.shift_key());
        // (before the HUD's own keys: F and the walking keys are the walk's)
        let alt = modifiers.alt_key() || modifiers.super_key() || ctrl;
        // GTA's way out of the car and back (first person only): F
        if !alt && code == KeyCode::KeyF && !repeat {
            self.toggle_walk();
            return true;
        }
        if let Some(walker) = self.gui.walk.as_mut() {
            use rustyac_web::walk::keys;
            match code {
                KeyCode::KeyW => walker.keys |= keys::FORWARD,
                KeyCode::KeyS => walker.keys |= keys::BACK,
                KeyCode::KeyA => walker.keys |= keys::LEFT,
                KeyCode::KeyD => walker.keys |= keys::RIGHT,
                KeyCode::ShiftLeft | KeyCode::ShiftRight => walker.keys |= keys::RUN,
                _ => {}
            }
            if matches!(code, KeyCode::KeyW | KeyCode::KeyA | KeyCode::KeyS | KeyCode::KeyD | KeyCode::ShiftLeft | KeyCode::ShiftRight | KeyCode::Space) {
                // (the car is not driven from outside)
                return true;
            }
        }
        let has_gui = self.full.gui.is_some();
        if has_gui && !modifiers.alt_key() && !modifiers.super_key() {
            if !repeat && self.gui.key(&mut self.full, &mut self.session, vk_code as u32, ctrl, shift) {
                return !std::mem::take(&mut self.gui.exit);
            }
            if let Some(gui) = self.full.gui.as_mut() {
                match (code, text) {
                    (KeyCode::Backspace, _) => gui.key_char(8),
                    (KeyCode::Enter, _) => gui.key_char(13),
                    (_, Some(text)) if !ctrl => text.encode_utf16().for_each(|unit| gui.key_char(unit as u32)),
                    _ => {}
                }
            }
        }
        if modifiers.super_key() || modifiers.alt_key() || (ctrl && !matches!(code, KeyCode::ControlLeft | KeyCode::ControlRight)) {
            return true;
        }
        if !repeat {
            let camera_keys = self.camera_keys();
            let layout_uses = false;
            let page_key = !camera_keys && !layout_uses;
            if page_key {
                match code {
                    KeyCode::Escape => {
                        if !has_gui {
                            return false;
                        }
                    }
                    KeyCode::KeyP | KeyCode::Pause => {
                        if self.session.sim.online.is_none() {
                            self.paused = !self.paused;
                        }
                        return true;
                    }
                    KeyCode::F1 | KeyCode::KeyC => rustyac_game::render::onboard_free::f1(&mut self.session.camera, &mut self.gui.start_camera, &mut self.gui.free_camera),
                    KeyCode::F6 => rustyac_game::render::onboard_free::f6(&mut self.session.camera, &mut self.gui.start_camera, &mut self.gui.free_camera, self.session.shape.car_cameras.len()),
                    KeyCode::F11 => {
                        self.full.toggle_virtual_mirror(false);
                    }
                    KeyCode::F4 => self.focus(FocusCommand::Player),
                    KeyCode::F3 => self.focus(FocusCommand::TrackCamera),
                    KeyCode::F2 => self.focus(FocusCommand::RandomCamera),
                    KeyCode::F5 if self.session.camera.shown != Some(5) => self.focus(FocusCommand::OnboardFreeCamera),
                    KeyCode::KeyR => self.session.request(if shift { event::TO_TRACK } else { event::RESET }),
                    KeyCode::KeyN => self.session.request(event::REBUILD),
                    KeyCode::KeyG => self.session.request(event::AUTO_SHIFTER),
                    KeyCode::F7 => {
                        if full::free_camera_allowed(&self.root_of_install()) {
                            self.gui.set_mode(&mut self.session, 6);
                        } else {
                            self.session.request(event::NEXT_SESSION);
                        }
                    }
                    KeyCode::F8 => self.session.request(event::RESTART_SESSION),
                    KeyCode::Numpad4 => self.session.request(event::PIT_LEFT),
                    KeyCode::Numpad8 => self.session.request(event::PIT_UP),
                    KeyCode::Numpad6 => self.session.request(event::PIT_RIGHT),
                    KeyCode::Numpad2 => self.session.request(event::PIT_DOWN),
                    KeyCode::KeyT => self.session.request(if shift { event::TC_DN } else { event::TC_UP }),
                    KeyCode::KeyY => self.session.request(if shift { event::ABS_DN } else { event::ABS_UP }),
                    KeyCode::BracketRight => self.session.request_bias(1),
                    KeyCode::BracketLeft => self.session.request_bias(-1),
                    _ => {}
                }
            }
        }
        self.session.input.lock().unwrap().keys.insert(vk_code);
        true
    }

    fn root_of_install(&self) -> PathBuf {
        rustyac_content::install::ac_root().unwrap_or_default()
    }

    /// F5's keyboard way or F7's cameras show: their keys are the game's.
    fn camera_keys(&self) -> bool {
        self.session.camera.shown == Some(5) || self.session.camera.shown == Some(6) || self.gui.free_camera.is_some()
    }

    /// F: out of the car (the view becomes the driver's own, the mouse looks) or back in, when the
    /// door is near.
    fn toggle_walk(&mut self) {
        let view = self.session.view();
        match self.gui.walk.take() {
            None => {
                self.gui.walk = Some(rustyac_web::walk::Walker::leave_car(&view.body));
                self.grab = Some(true);
                println!("on foot: WASD walks, Shift runs, the mouse looks, F near the car sits down");
            }
            Some(walker) => {
                if walker.near_car(&view.body) {
                    self.session.input.lock().unwrap().keys.remove(&self.brake_key);
                    self.grab = Some(false);
                } else {
                    // too far from the car: stay out
                    self.gui.walk = Some(walker);
                    println!("the car is too far: walk to it first");
                }
            }
        }
    }

    fn key_up(&mut self, code: KeyCode) {
        if let Some(walker) = self.gui.walk.as_mut() {
            use rustyac_web::walk::keys;
            match code {
                KeyCode::KeyW => walker.keys &= !keys::FORWARD,
                KeyCode::KeyS => walker.keys &= !keys::BACK,
                KeyCode::KeyA => walker.keys &= !keys::LEFT,
                KeyCode::KeyD => walker.keys &= !keys::RIGHT,
                KeyCode::ShiftLeft | KeyCode::ShiftRight => walker.keys &= !keys::RUN,
                _ => {}
            }
        }
        let Some(vk_code) = vk(code) else { return };
        self.session.input.lock().unwrap().keys.remove(&vk_code);
        if vk_code == 0x09 {
            self.gui.tab_held = false;
        }
    }

    fn release_keys(&mut self) {
        self.session.input.lock().unwrap().keys.clear();
        self.gui.tab_held = false;
    }

    /// `Game::gui_mouse` of the browser build: `action` 0 move, 1 down, 2 up, 3 the wheel.
    fn mouse(&mut self, x: i32, y: i32, action: u32, button: u32, delta: f32) {
        let right = match (action, button) {
            (1, 1) => true,
            (2, 1) => false,
            _ => self.gui.mouse.2,
        };
        self.gui.mouse = (x, y, right);
        if (action, button) == (1, 1) {
            let now = Instant::now();
            if self.last_right.is_some_and(|t| now.duration_since(t).as_secs_f32() < 0.4) {
                self.gui.orbit_reset = true;
                self.last_right = None;
            } else {
                self.last_right = Some(now);
            }
        }
        match (action, button) {
            (1 | 2, 0) => self.gui.buttons[0] = action == 1,
            (1 | 2, 2) => self.gui.buttons[1] = action == 1,
            (3, _) => {
                self.gui.onboard_free.iter_mut().for_each(|f5| f5.wheel((delta * 120.0) as i32));
                // the chase camera's distance: wheel up comes closer
                let z = if self.gui.zoom <= 0.0 { 1.0 } else { self.gui.zoom };
                self.gui.zoom = (z * (1.0 - delta * 0.08)).clamp(0.35, 3.5);
            }
            _ => {}
        }
        if let Some(gui) = self.full.gui.as_mut() {
            match action {
                0 => gui.mouse_move(x, y),
                1 => gui.mouse_down(x, y, button as i32),
                2 => gui.mouse_up(x, y, button as i32),
                _ => gui.mouse_wheel(x, y, delta),
            }
        }
    }
}

trait ControlAny {
    fn control_state_any(&self) -> bool;
}

impl ControlAny for ModifiersState {
    /// Ctrl, and on macOS Command too is not it: the games's keys use Control.
    fn control_state_any(&self) -> bool {
        self.control_key()
    }
}

/// Custom Shaders Patch's Lighting FX brightness (`extension/config/lighting_fx.ini`, over which
/// the launcher's own `cfg/extension/lighting_fx.ini` wins): the factor, 1 when the module is off.
fn csp_brightness(root: &std::path::Path) -> f32 {
    csp_value(root, "lighting_fx", "BASIC", "BRIGHTNESS_MULT").unwrap_or(1.0).clamp(0.2, 4.0)
}

/// A number of a Custom Shaders Patch module, the launcher's own file over the patch's; None when
/// the module is off (`[BASIC] ENABLED=0`) or has no such key.
fn csp_value(root: &std::path::Path, module: &str, section: &str, key: &str) -> Option<f32> {
    fn value(text: &str, section: &str, key: &str) -> Option<String> {
        let mut inside = false;
        let mut found = None;
        for line in text.lines() {
            let t = line.trim();
            if let Some(s) = t.strip_prefix('[') {
                inside = s.split(']').next().is_some_and(|s| s.eq_ignore_ascii_case(section));
            } else if inside {
                if let Some((k, v)) = t.split_once('=') {
                    if k.trim().eq_ignore_ascii_case(key) {
                        found = Some(v.split(';').next().unwrap_or("").trim().to_string());
                    }
                }
            }
        }
        found
    }
    let base = std::fs::read_to_string(root.join(format!("extension/config/{module}.ini"))).unwrap_or_default();
    let mine = std::fs::read_to_string(own_folder().join(format!("cfg/extension/{module}.ini"))).unwrap_or_default();
    let get = |section: &str, key: &str| value(&mine, section, key).or_else(|| value(&base, section, key));
    if get("BASIC", "ENABLED").is_some_and(|v| v == "0") {
        return None;
    }
    get(section, key).and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0)
}

/// The size drawn for a window of `width` x `height` at density `scale`.
fn scaled(scale: f32, width: u32, height: u32) -> (u32, u32) {
    (((width as f32 * scale) as u32).max(16), ((height as f32 * scale) as u32).max(16))
}

/// The mouse held by the window (it looks round, the cursor is hidden), or let go.
fn grab(window: &winit::window::Window, on: bool) {
    use winit::window::CursorGrabMode;
    if on {
        if window.set_cursor_grab(CursorGrabMode::Locked).is_err() {
            window.set_cursor_grab(CursorGrabMode::Confined).ok();
        }
    } else {
        window.set_cursor_grab(CursorGrabMode::None).ok();
    }
    window.set_cursor_visible(!on);
}

impl ApplicationHandler for App {
    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let (Some(screen), DeviceEvent::MouseMotion { delta }) = (self.screen.as_mut(), event) {
            if let (false, Some(walker)) = (screen.debug.open, screen.game.gui.walk.as_mut()) {
                walker.look[0] += delta.0 as f32;
                walker.look[1] += delta.1 as f32;
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.screen.is_some() {
            return;
        }
        match self.open(event_loop) {
            Ok(screen) => {
                screen.window.request_redraw();
                self.screen = Some(screen);
            }
            Err(why) => {
                self.error = Some(why);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(screen) = self.screen.as_mut() else { return };
        if !matches!(event, WindowEvent::RedrawRequested | WindowEvent::Resized(_) | WindowEvent::CloseRequested) && screen.debug.event(&screen.window, &event) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::Focused(false) => screen.game.release_keys(),
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else { return };
                if code == KeyCode::F10 && event.state == ElementState::Pressed && !event.repeat {
                    screen.debug.open = !screen.debug.open;
                    // the mouse belongs to the panel while it is open, to the walk (if any) after
                    grab(&screen.window, !screen.debug.open && screen.game.gui.walk.is_some());
                    return;
                }
                match event.state {
                    ElementState::Pressed => {
                        if !screen.game.key_down(code, event.text.as_deref(), self.modifiers, event.repeat) {
                            event_loop.exit();
                        }
                    }
                    ElementState::Released => screen.game.key_up(code),
                }
                if let Some(on) = screen.game.grab.take() {
                    grab(&screen.window, on);
                }
            }
            WindowEvent::CursorMoved { position, .. } => screen.game.mouse((position.x * screen.game.scale as f64) as i32, (position.y * screen.game.scale as f64) as i32, 0, 0, 0.0),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    _ => return,
                };
                let (x, y, _) = screen.game.gui.mouse;
                screen.game.mouse(x, y, if state == ElementState::Pressed { 1 } else { 2 }, button, 0.0);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                let (x, y, _) = screen.game.gui.mouse;
                screen.game.mouse(x, y, 3, 0, notches);
            }
            WindowEvent::Resized(size) => {
                let (width, height) = (size.width.max(16), size.height.max(16));
                screen.config.width = width;
                screen.config.height = height;
                screen.surface.configure(&screen.device, &screen.config);
                let (render_width, render_height) = scaled(screen.game.scale, width, height);
                if let Err(why) = screen.game.full.resize(render_width, render_height) {
                    eprintln!("the new size failed: {why}");
                }
            }
            WindowEvent::RedrawRequested => {
                static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
                let started = *START.get_or_init(Instant::now);
                if self.options.quit_after.is_some_and(|s| started.elapsed().as_secs_f64() >= s) {
                    if let Some(path) = &self.options.shot {
                        match screen.game.full.read_pixels() {
                            Ok(pixels) => {
                                let (w, h) = screen.game.full.size();
                                let mean = pixels.iter().step_by(4).map(|&v| v as u64).sum::<u64>() as f64 / (pixels.len() / 4).max(1) as f64;
                                println!("shot: {w}x{h}, mean red {mean:.1}, {}", path.display());
                                std::fs::write(path, rustyac_render::gui_paint::encode_png(w, h, &pixels)).ok();
                            }
                            Err(why) => eprintln!("shot: {why}"),
                        }
                    }
                    if std::env::var_os("RUSTYAC_STATS").is_some() {
                        println!("stats: {} triangles, {} draw calls", screen.game.full.triangles, screen.game.full.draw_calls);
                    }
                    event_loop.exit();
                    return;
                }
                if !self.options.press.is_empty() {
                    let elapsed = started.elapsed().as_secs_f64();
                    let due: Vec<KeyCode> = self.options.press.iter().filter(|(at, _)| *at <= elapsed).map(|(_, c)| *c).collect();
                    self.options.press.retain(|(at, _)| *at > elapsed);
                    for code in due {
                        println!("press {code:?}");
                        if code == KeyCode::F10 {
                            screen.debug.open = !screen.debug.open;
                        } else {
                            screen.game.key_down(code, None, self.modifiers, false);
                            if let Some(on) = screen.game.grab.take() {
                                grab(&screen.window, on);
                            }
                            if code == KeyCode::KeyW {
                                // (held: the walk's own key bit stays set)
                            }
                        }
                    }
                }
                if let Err(why) = screen.game.frame() {
                    self.error = Some(why);
                    event_loop.exit();
                    return;
                }
                let frame = match screen.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                    wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                        screen.surface.configure(&screen.device, &screen.config);
                        screen.window.request_redraw();
                        return;
                    }
                    wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                        screen.window.request_redraw();
                        return;
                    }
                    other => {
                        self.error = Some(format!("the window gave no picture to draw into ({other:?})"));
                        event_loop.exit();
                        return;
                    }
                };
                let target = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                screen.game.full.present_to(&target, screen.config.format);
                let now = Instant::now();
                let dt = screen.last_draw.map_or(0.0, |t| now.duration_since(t).as_secs_f32());
                screen.last_draw = Some(now);
                {
                    let open = screen.debug.open;
                    let view = open.then(|| screen.game.session.view());
                    let walker = screen.game.gui.walk.as_ref().map(|w| (w.pos, w.yaw));
                    let camera = screen.game.gui.last_frame;
                    let (triangles, draw_calls) = (screen.game.full.triangles, screen.game.full.draw_calls);
                    let render_size = screen.game.full.size();
                    let mut controls = screen.controls.clone();
                    let idle = rustyac_game::view::CarView::default();
                    let data = crate::debug::Data { view: view.as_ref().unwrap_or(&idle), camera, triangles, draw_calls, render_size, walker };
                    screen.debug.frame(&screen.window, &screen.device, &screen.queue, &target, (screen.config.width, screen.config.height), dt, &data, &mut controls);
                    if open {
                        screen.game.full.brightness = controls.brightness;
                        screen.game.full.lod_distance = controls.lod_distance;
                        screen.game.paused = controls.paused;
                        if std::mem::take(&mut controls.reset) {
                            screen.game.session.request(rustyac_game::input_file::event::RESET);
                        }
                        if std::mem::take(&mut controls.to_track) {
                            screen.game.session.request(rustyac_game::input_file::event::TO_TRACK);
                        }
                    }
                    screen.controls = controls;
                }
                screen.window.pre_present_notify();
                screen.queue.present(frame);
                screen.window.request_redraw();
            }
            _ => {}
        }
    }
}
