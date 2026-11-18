// SPDX-License-Identifier: GPL-3.0-or-later

//! The FMOD 1.08 functions acs.exe uses, answered by the native mixer instead of FMOD: the
//! same names and signatures as `fmod::raw`'s table, so the sound code above (engine, cars,
//! track) runs unchanged. Banks are read from their bytes, events played by
//! [`super::mixer`], the mix goes to the sound card with cpal (or nowhere, or into a WAV file,
//! as `raw::Output` says).
//!
//! Not done: the effects (the two DSP plug-ins, reverb, geometry occlusion), the buses'
//! levels, events that need FMOD's timeline (transitions). Those are accepted and ignored.

#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr};
use std::sync::{Arc, Mutex};

use super::mixer::{Playing, Wave};
use super::model::{Event, Model};
use crate::bank::Bank;
use crate::fmod::raw::{api, Handle, In, Out, OutHandle, Output, Quiet, Sized, Str};
use crate::fmod::types::*;

const STUDIO: usize = 0x10;
const LOW: usize = 0x20;
const MASTER: usize = 0x30;
const ERR_INVALID_HANDLE: i32 = 30;
const ERR_INVALID_PARAM: i32 = 31;
const ERR_FILE_NOTFOUND: i32 = 15;
const LOADING_LOADED: i32 = 3;

struct Loaded {
    model: Model,
    waves: Vec<Arc<Wave>>,
}

struct Inst {
    bank: usize,
    event: Event,
    playing: Playing,
    started: bool,
    paused: bool,
    attrs: Attributes3d,
}

struct State {
    next: usize,
    banks: Vec<Arc<Loaded>>,
    events: HashMap<[u8; 16], usize>,
    descs: HashMap<usize, [u8; 16]>,
    insts: HashMap<usize, Inst>,
    groups: HashMap<usize, usize>,
    listener: Attributes3d,
    master_volume: f32,
    rate: u32,
    block: u32,
    wav: Option<(std::path::PathBuf, Vec<i16>)>,
    device: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.lock().ok()?.as_mut().map(f)
}

fn id(h: Handle) -> usize {
    h.0 as usize
}

fn raw(id: usize) -> RawHandle {
    id as RawHandle
}

impl State {
    fn new() -> State {
        State { next: 0x100, banks: Vec::new(), events: HashMap::new(), descs: HashMap::new(), insts: HashMap::new(), groups: HashMap::new(), listener: Attributes3d::default(), master_volume: 1.0, rate: 48000, block: 512, wav: None, device: false }
    }

    fn handle(&mut self) -> usize {
        self.next += 16;
        self.next
    }

    fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let rate = self.rate;
        let listener = self.listener;
        for inst in self.insts.values_mut() {
            if !inst.started || inst.paused {
                continue;
            }
            let bank = self.banks[inst.bank].clone();
            apply_3d(inst, &listener, &bank.model);
            inst.playing.mix(&bank.model, &bank.waves, rate, out);
            if inst.playing.finished {
                inst.started = false;
            }
        }
        let g = self.master_volume;
        // headroom for the sum of the layers, and a soft knee instead of hard clipping
        out.iter_mut().for_each(|x| {
            let v = *x * g * 1.2;
            *x = if v.abs() < 0.6 { v } else { v.signum() * (0.6 + 0.4 * ((v.abs() - 0.6) / 0.4).tanh()) };
        });
    }
}

fn sub(a: Vector, b: Vector) -> Vector {
    Vector { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }
}
fn dot(a: Vector, b: Vector) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
fn cross(a: Vector, b: Vector) -> Vector {
    Vector { x: a.y * b.z - a.z * b.y, y: a.z * b.x - a.x * b.z, z: a.x * b.y - a.y * b.x }
}

/// The event's distance gain, its pan, and the built-in parameters `Distance` and `Event Cone
/// Angle`, from where the event and the listener are (FMOD's left-handed frame).
fn apply_3d(inst: &mut Inst, listener: &Attributes3d, model: &Model) {
    let event = &inst.event;
    if event.max_distance <= 0.0 {
        inst.playing.gain = 1.0;
        inst.playing.pan = 0.0;
        return;
    }
    let to = sub(inst.attrs.position, listener.position);
    let d = dot(to, to).sqrt();
    let (min, max) = (event.min_distance.max(0.0), event.max_distance);
    inst.playing.gain = if d <= min { 1.0 } else { (1.0 - (d - min) / (max - min).max(0.001)).clamp(0.0, 1.0).powi(2) };
    let right = cross(listener.up, listener.forward);
    inst.playing.pan = if d > 1e-4 { dot(to, right) / d } else { 0.0 };
    if let Some(g) = model.param_named(event, "Distance") {
        inst.playing.set_param(g, d);
    }
    if let Some(g) = model.param_named(event, "Event Cone Angle") {
        let back = Vector { x: -to.x, y: -to.y, z: -to.z };
        let angle = if d > 1e-4 { (dot(back, inst.attrs.forward) / d).clamp(-1.0, 1.0).acos().to_degrees() } else { 0.0 };
        inst.playing.set_param(g, angle);
    }
}

// --- the sound card ----------------------------------------------------------------------------

fn start_device() {
    let _ = std::thread::Builder::new().name("rustyac-sound".into()).spawn(|| {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            eprintln!("audio: no output device");
            return;
        };
        let Ok(config) = device.default_output_config() else { return };
        let channels = config.channels() as usize;
        let rate = config.sample_rate().0;
        with(|s| s.rate = rate);
        let mut scratch: Vec<f32> = Vec::new();
        let stream = device.build_output_stream(
            &config.config(),
            move |data: &mut [f32], _| {
                let frames = data.len() / channels;
                scratch.resize(frames * 2, 0.0);
                if with(|s| s.render(&mut scratch)).is_none() {
                    data.fill(0.0);
                    return;
                }
                for (n, frame) in data.chunks_mut(channels).enumerate() {
                    frame.fill(0.0);
                    frame[0] = scratch[2 * n];
                    if channels > 1 {
                        frame[1] = scratch[2 * n + 1];
                    }
                }
            },
            |e| eprintln!("audio: {e}"),
            None,
        );
        match stream {
            Ok(stream) => {
                if stream.play().is_ok() {
                    // the stream plays as long as this thread keeps it
                    loop {
                        std::thread::park();
                    }
                }
            }
            Err(e) => eprintln!("audio: the output device could not be opened: {e}"),
        }
    });
}

/// FMOD's no-sound output in real time: the events go on, nothing is heard.
fn start_silent() {
    let _ = std::thread::Builder::new().name("rustyac-sound".into()).spawn(|| {
        let mut buffer = vec![0f32; 960 * 2];
        loop {
            std::thread::sleep(std::time::Duration::from_millis(20));
            if with(|s| s.render(&mut buffer)).is_none() {
                return;
            }
        }
    });
}

// --- System ------------------------------------------------------------------------------------

macro_rules! ok {
    () => {
        FMOD_OK
    };
}

unsafe fn put<T>(p: Quiet<T>, v: T) {
    if !p.0.is_null() {
        p.0.write_unaligned(v);
    }
}

pub unsafe extern "C" fn system_set_driver(_: Handle, _: i32) -> i32 {
    ok!()
}
pub unsafe extern "C" fn system_get_driver_info(_: Handle, _: i32, name: Quiet<c_char>, name_len: i32, guid: Quiet<Guid>, rate: Quiet<i32>, mode: Quiet<i32>, channels: Quiet<i32>) -> i32 {
    if !name.0.is_null() && name_len > 0 {
        let text = b"rustyAC native\0";
        let n = text.len().min(name_len as usize);
        std::ptr::copy_nonoverlapping(text.as_ptr(), name.0.cast::<u8>(), n);
        *name.0.add(n - 1) = 0;
    }
    put(guid, Guid::default());
    put(rate, 48000);
    put(mode, SPEAKERMODE_STEREO);
    put(channels, 2);
    ok!()
}
pub unsafe extern "C" fn system_set_software_channels(_: Handle, _: i32) -> i32 {
    ok!()
}
pub unsafe extern "C" fn system_get_output(_: Handle, output: Quiet<i32>) -> i32 {
    put(output, OUTPUTTYPE_NOSOUND);
    ok!()
}
pub unsafe extern "C" fn c_system_get_driver(_: Handle, driver: Quiet<i32>) -> i32 {
    put(driver, 0);
    ok!()
}
pub unsafe extern "C" fn system_set_software_format(_: Handle, _: i32, _: i32, _: i32) -> i32 {
    ok!()
}
pub unsafe extern "C" fn system_get_software_format(_: Handle, rate: Quiet<i32>, mode: Quiet<i32>, raw_speakers: Quiet<i32>) -> i32 {
    put(rate, with(|s| s.rate as i32).unwrap_or(48000));
    put(mode, SPEAKERMODE_STEREO);
    put(raw_speakers, 0);
    ok!()
}
pub unsafe extern "C" fn system_get_driver(_: Handle, driver: Quiet<i32>) -> i32 {
    put(driver, 0);
    ok!()
}
pub unsafe extern "C" fn system_get_version(_: Handle, version: Out<u32>) -> i32 {
    if !version.0.is_null() {
        *version.0 = FMOD_VERSION;
    }
    ok!()
}
pub unsafe extern "C" fn system_get_channels_playing(_: Handle, channels: Quiet<i32>, real: Quiet<i32>) -> i32 {
    let n = with(|s| s.insts.values().filter(|i| i.started).count() as i32).unwrap_or(0);
    put(channels, n);
    put(real, n);
    ok!()
}
pub unsafe extern "C" fn system_get_dsp_buffer_size(_: Handle, length: Quiet<u32>, count: Quiet<i32>) -> i32 {
    put(length, with(|s| s.block).unwrap_or(512));
    put(count, 4);
    ok!()
}
pub unsafe extern "C" fn system_set_callback(_: Handle, _: Quiet<c_void>, _: u32) -> i32 {
    ok!()
}
pub unsafe extern "C" fn system_get_3d_settings(_: Handle, doppler: Out<f32>, distance: Out<f32>, rolloff: Out<f32>) -> i32 {
    for (p, v) in [(doppler, 1.0), (distance, 1.0), (rolloff, 1.0)] {
        if !p.0.is_null() {
            *p.0 = v;
        }
    }
    ok!()
}
pub unsafe extern "C" fn system_get_num_drivers(_: Handle, count: Quiet<i32>) -> i32 {
    put(count, 1);
    ok!()
}
pub unsafe extern "C" fn system_set_3d_settings(_: Handle, _: f32, _: f32, _: f32) -> i32 {
    ok!()
}
unsafe fn give(out: OutHandle, kind: usize) -> i32 {
    let h = with(|s| match kind {
        0 => s.handle(),
        k => k,
    });
    match h {
        Some(h) => {
            if !out.0.is_null() {
                *out.0 = raw(h);
            }
            ok!()
        }
        None => ERR_INVALID_HANDLE,
    }
}
pub unsafe extern "C" fn system_create_reverb_3d(_: Handle, reverb: OutHandle) -> i32 {
    give(reverb, 0)
}
pub unsafe extern "C" fn system_get_master_channel_group(_: Handle, group: OutHandle) -> i32 {
    give(group, MASTER)
}
pub unsafe extern "C" fn system_create_geometry(_: Handle, _: i32, _: i32, geometry: OutHandle) -> i32 {
    give(geometry, 0)
}
pub unsafe extern "C" fn system_get_geometry_occlusion(_: Handle, _: In<Vector>, _: In<Vector>, direct: Out<f32>, reverb: Out<f32>) -> i32 {
    for p in [direct, reverb] {
        if !p.0.is_null() {
            *p.0 = 0.0;
        }
    }
    ok!()
}
pub unsafe extern "C" fn system_set_geometry_settings(_: Handle, _: f32) -> i32 {
    ok!()
}
pub unsafe extern "C" fn system_set_advanced_settings(_: Handle, _: Sized) -> i32 {
    ok!()
}

// --- Reverb3D, Geometry, ChannelControl --------------------------------------------------------

pub unsafe extern "C" fn reverb_set_active(_: Handle, _: bool) -> i32 {
    ok!()
}
pub unsafe extern "C" fn reverb_set_properties(_: Handle, _: In<ReverbProperties>) -> i32 {
    ok!()
}
pub unsafe extern "C" fn reverb_get_3d_attributes(_: Handle, position: Out<Vector>, min: Out<f32>, max: Out<f32>) -> i32 {
    if !position.0.is_null() {
        *position.0 = Vector::default();
    }
    for p in [min, max] {
        if !p.0.is_null() {
            *p.0 = 0.0;
        }
    }
    ok!()
}
pub unsafe extern "C" fn reverb_set_3d_attributes(_: Handle, _: In<Vector>, _: f32, _: f32) -> i32 {
    ok!()
}
pub unsafe extern "C" fn reverb_release(_: Handle) -> i32 {
    ok!()
}
pub unsafe extern "C" fn geometry_set_active(_: Handle, _: bool) -> i32 {
    ok!()
}
pub unsafe extern "C" fn geometry_set_polygon_attributes(_: Handle, _: i32, _: f32, _: f32, _: bool) -> i32 {
    ok!()
}
pub unsafe extern "C" fn geometry_get_num_polygons(_: Handle, count: Out<i32>) -> i32 {
    if !count.0.is_null() {
        *count.0 = 0;
    }
    ok!()
}
pub unsafe extern "C" fn geometry_release(_: Handle) -> i32 {
    ok!()
}
pub unsafe extern "C" fn geometry_add_polygon(_: Handle, _: f32, _: f32, _: bool, _: i32, _: *const Vector, index: *mut i32) -> i32 {
    if !index.is_null() {
        *index = 0;
    }
    ok!()
}
pub unsafe extern "C" fn channel_set_reverb_properties(_: Handle, _: i32, _: f32) -> i32 {
    ok!()
}
/// A channel group is the master, or an event's.
pub unsafe extern "C" fn channel_set_volume(this: Handle, volume: f32) -> i32 {
    let h = id(this);
    if trace() {
        eprintln!("audio: channel group {h:#x} volume {volume:.2}");
    }
    with(|s| {
        if h == MASTER {
            s.master_volume = volume;
        } else if let Some(&inst) = s.groups.get(&h) {
            if let Some(i) = s.insts.get_mut(&inst) {
                i.playing.volume = volume;
            }
        }
    });
    ok!()
}

// --- Studio ------------------------------------------------------------------------------------

pub unsafe extern "C" fn studio_create(system: OutHandle, _header: u32) -> i32 {
    *STATE.lock().unwrap() = Some(State::new());
    if !system.0.is_null() {
        *system.0 = raw(STUDIO);
    }
    ok!()
}
pub unsafe extern "C" fn studio_get_low_level_system(_: Handle, system: OutHandle) -> i32 {
    if !system.0.is_null() {
        *system.0 = raw(LOW);
    }
    ok!()
}
pub unsafe extern "C" fn studio_set_advanced_settings(_: Handle, _: Sized) -> i32 {
    ok!()
}
pub unsafe extern "C" fn studio_initialize(_: Handle, _: i32, _: u32, _: u32, _: *mut c_void) -> i32 {
    match api().map(|a| a.output().clone()) {
        Some(Output::Device) => {
            with(|s| s.device = true);
            start_device();
        }
        Some(Output::NoSound) => start_silent(),
        Some(Output::WavNrt { file, rate, block }) => {
            with(|s| {
                s.rate = rate as u32;
                s.block = block;
                s.wav = Some((file, Vec::new()));
            });
        }
        Some(Output::NoSoundNrt { rate, block }) => {
            with(|s| {
                s.rate = rate as u32;
                s.block = block;
            });
        }
        None => {}
    }
    ok!()
}
pub unsafe extern "C" fn studio_register_plugin(_: Handle, _: *const c_void) -> i32 {
    ok!()
}
/// Real time: the mixer thread runs by itself. Non-real-time: one block per update.
pub unsafe extern "C" fn studio_update(_: Handle) -> i32 {
    with(|s| {
        let nrt = s.wav.is_some() || !s.device && matches!(api().map(|a| a.output().clone()), Some(Output::NoSoundNrt { .. }));
        if nrt {
            let mut block = vec![0f32; s.block as usize * 2];
            s.render(&mut block);
            if let Some((_, pcm)) = s.wav.as_mut() {
                pcm.extend(block.iter().map(|x| (x * 32767.0) as i16));
            }
        }
    });
    ok!()
}
pub unsafe extern "C" fn studio_flush_commands(_: Handle) -> i32 {
    ok!()
}
pub unsafe extern "C" fn studio_release(_: Handle) -> i32 {
    if let Some(Some(state)) = STATE.lock().ok().map(|mut s| s.take()) {
        if let Some((file, pcm)) = state.wav {
            let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
            let wav = crate::bank::wav(&crate::bank::Sample { name: String::new(), rate: state.rate, channels: 2, frames: (bytes.len() / 4) as u32, pcm: bytes });
            let _ = std::fs::write(file, wav);
        }
    }
    ok!()
}
pub unsafe extern "C" fn studio_get_cpu_usage(_: Handle, _: Quiet<c_void>) -> i32 {
    ok!()
}
pub unsafe extern "C" fn studio_get_buffer_usage(_: Handle, _: Quiet<c_void>) -> i32 {
    ok!()
}
pub unsafe extern "C" fn studio_get_listener_attributes(_: Handle, _: i32, attributes: Out<Attributes3d>) -> i32 {
    if !attributes.0.is_null() {
        *attributes.0 = with(|s| s.listener).unwrap_or_default();
    }
    ok!()
}
pub unsafe extern "C" fn studio_set_listener_attributes(_: Handle, listener: i32, attributes: In<Attributes3d>) -> i32 {
    if listener == 0 && !attributes.0.is_null() {
        let a = attributes.0.read_unaligned();
        with(|s| s.listener = a);
    }
    ok!()
}
pub unsafe extern "C" fn studio_get_event_by_id(_: Handle, guid: In<Guid>, description: OutHandle) -> i32 {
    if guid.0.is_null() {
        return ERR_INVALID_PARAM;
    }
    let bytes: [u8; 16] = std::ptr::read_unaligned(guid.0.cast::<[u8; 16]>());
    let found = with(|s| {
        s.events.get(&bytes)?;
        let h = s.handle();
        s.descs.insert(h, bytes);
        Some(h)
    })
    .flatten();
    match found {
        Some(h) => {
            if !description.0.is_null() {
                *description.0 = raw(h);
            }
            ok!()
        }
        None => 74,
    }
}
pub unsafe extern "C" fn studio_load_bank_file(_: Handle, file: Str, _flags: u32, bank: OutHandle) -> i32 {
    if file.0.is_null() {
        return ERR_INVALID_PARAM;
    }
    let path = CStr::from_ptr(file.0).to_string_lossy().into_owned();
    let loaded = Bank::open(std::path::Path::new(&path)).and_then(|b| {
        let mut model = Model::read(&b);
        let samples = b.samples().unwrap_or_default();
        model.name_layers(&samples.iter().map(|s| s.name.clone()).collect::<Vec<_>>());
        let waves = samples.iter().map(|s| Arc::new(Wave::from_pcm16(s.rate, s.channels, &s.pcm))).collect();
        Ok(Loaded { model, waves })
    });
    let loaded = match loaded {
        Ok(l) => Arc::new(l),
        Err(_) => return ERR_FILE_NOTFOUND,
    };
    let h = with(|s| {
        let index = s.banks.len();
        for guid in loaded.model.events.keys() {
            s.events.insert(*guid, index);
        }
        s.banks.push(loaded);
        s.handle()
    });
    match h {
        Some(h) => {
            if !bank.0.is_null() {
                *bank.0 = raw(h);
            }
            ok!()
        }
        None => ERR_INVALID_HANDLE,
    }
}
pub unsafe extern "C" fn bank_unload(_: Handle) -> i32 {
    ok!()
}

// --- EventDescription --------------------------------------------------------------------------

pub unsafe extern "C" fn description_load_sample_data(_: Handle) -> i32 {
    ok!()
}
pub unsafe extern "C" fn description_unload_sample_data(_: Handle) -> i32 {
    ok!()
}
pub unsafe extern "C" fn description_create_instance(this: Handle, instance: OutHandle) -> i32 {
    let made = with(|s| {
        let guid = *s.descs.get(&id(this))?;
        let bank = *s.events.get(&guid)?;
        let event = s.banks[bank].model.events.get(&guid)?.clone();
        let h = s.handle();
        s.insts.insert(h, Inst { bank, playing: Playing::new(event.clone()), event, started: false, paused: false, attrs: Attributes3d::default() });
        Some(h)
    })
    .flatten();
    match made {
        Some(h) => {
            if !instance.0.is_null() {
                *instance.0 = raw(h);
            }
            ok!()
        }
        None => ERR_INVALID_HANDLE,
    }
}
pub unsafe extern "C" fn description_get_maximum_distance(this: Handle, distance: Out<f32>) -> i32 {
    let d = with(|s| {
        let guid = s.descs.get(&id(this))?;
        let bank = *s.events.get(guid)?;
        Some(s.banks[bank].model.events.get(guid)?.max_distance)
    })
    .flatten();
    match d {
        Some(d) => {
            if !distance.0.is_null() {
                *distance.0 = d;
            }
            ok!()
        }
        None => ERR_INVALID_HANDLE,
    }
}

// --- EventInstance -----------------------------------------------------------------------------

fn inst<R>(this: Handle, f: impl FnOnce(&mut Inst, &Model) -> R) -> i32 {
    with(|s| match s.insts.get_mut(&id(this)) {
        Some(i) => {
            let bank = s.banks[i.bank].clone();
            f(i, &bank.model);
            ok!()
        }
        None => ERR_INVALID_HANDLE,
    })
    .unwrap_or(ERR_INVALID_HANDLE)
}

pub unsafe extern "C" fn event_set_parameter_value(this: Handle, name: Str, value: f32) -> i32 {
    if value.is_nan() {
        return ERR_INVALID_PARAM;
    }
    let name = if name.0.is_null() { String::new() } else { CStr::from_ptr(name.0).to_string_lossy().into_owned() };
    inst(this, |i, model| {
        if trace() && matches!(name.as_str(), "decay" | "rpms" | "throttle") && (i.playing.params.get(&model.param_named(&i.event, &name).unwrap_or_default()).copied().unwrap_or(-1.0) - value).abs() > 0.5 {
            eprintln!("audio: {} {name} = {value:.1}", super::model::guid_text(&i.event.guid));
        }
        if let Some(g) = model.param_named(&i.event, &name) {
            i.playing.set_param(g, value);
        }
    })
}
pub unsafe extern "C" fn event_release(this: Handle) -> i32 {
    with(|s| {
        s.insts.remove(&id(this));
        s.groups.retain(|_, v| *v != id(this));
    });
    ok!()
}
pub unsafe extern "C" fn event_get_playback_state(this: Handle, state: Out<i32>) -> i32 {
    let r = with(|s| s.insts.get(&id(this)).map(|i| if i.started { PLAYBACK_PLAYING } else { PLAYBACK_STOPPED })).flatten();
    match r {
        Some(v) => {
            if !state.0.is_null() {
                *state.0 = v;
            }
            ok!()
        }
        None => ERR_INVALID_HANDLE,
    }
}
pub unsafe extern "C" fn event_set_timeline_position(_: Handle, _: i32) -> i32 {
    ok!()
}
pub unsafe extern "C" fn event_stop(this: Handle, _mode: i32) -> i32 {
    inst(this, |i, _| i.started = false)
}
fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RUSTYAC_AUDIO_TRACE").is_some())
}

pub unsafe extern "C" fn event_start(this: Handle) -> i32 {
    inst(this, |i, _| {
        if trace() {
            eprintln!("audio: start {}", super::model::guid_text(&i.event.guid));
        }
        // (a new start begins the sounds again)
        let params = std::mem::take(&mut i.playing.params);
        let (volume, pitch) = (i.playing.volume, i.playing.pitch);
        i.playing = Playing::new(i.event.clone());
        i.playing.params = params;
        i.playing.volume = volume;
        i.playing.pitch = pitch;
        i.started = true;
    })
}
pub unsafe extern "C" fn event_set_paused(this: Handle, paused: bool) -> i32 {
    inst(this, |i, _| i.paused = paused)
}
pub unsafe extern "C" fn event_get_paused(this: Handle, paused: Out<bool>) -> i32 {
    match with(|s| s.insts.get(&id(this)).map(|i| i.paused)).flatten() {
        Some(p) => {
            if !paused.0.is_null() {
                *paused.0 = p;
            }
            ok!()
        }
        None => ERR_INVALID_HANDLE,
    }
}
pub unsafe extern "C" fn event_set_3d_attributes(this: Handle, attributes: In<Attributes3d>) -> i32 {
    if attributes.0.is_null() {
        return ERR_INVALID_PARAM;
    }
    let a = attributes.0.read_unaligned();
    inst(this, |i, _| i.attrs = a)
}
pub unsafe extern "C" fn event_get_3d_attributes(this: Handle, attributes: Out<Attributes3d>) -> i32 {
    match with(|s| s.insts.get(&id(this)).map(|i| i.attrs)).flatten() {
        Some(a) => {
            if !attributes.0.is_null() {
                *attributes.0 = a;
            }
            ok!()
        }
        None => ERR_INVALID_HANDLE,
    }
}
pub unsafe extern "C" fn event_set_pitch(this: Handle, pitch: f32) -> i32 {
    inst(this, |i, _| i.playing.pitch = pitch)
}
pub unsafe extern "C" fn event_set_volume(this: Handle, volume: f32) -> i32 {
    inst(this, |i, _| {
        if trace() && (i.playing.volume - volume).abs() > 0.2 {
            eprintln!("audio: volume {} -> {volume:.2}", super::model::guid_text(&i.event.guid));
        }
        i.playing.volume = volume
    })
}
pub unsafe extern "C" fn event_get_channel_group(this: Handle, group: *mut RawHandle) -> i32 {
    let g = with(|s| {
        if !s.insts.contains_key(&id(this)) {
            return None;
        }
        let h = s.handle();
        s.groups.insert(h, id(this));
        Some(h)
    })
    .flatten();
    match g {
        Some(h) => {
            if !group.is_null() {
                *group = raw(h);
            }
            ok!()
        }
        None => ERR_INVALID_HANDLE,
    }
}

// --- what the layer itself asks ----------------------------------------------------------------

pub unsafe extern "C" fn x_set_output(_: RawHandle, _: i32) -> i32 {
    ok!()
}
pub unsafe extern "C" fn x_set_dsp_buffer_size(_: RawHandle, length: u32, _: i32) -> i32 {
    with(|s| s.block = length.max(64));
    ok!()
}
pub unsafe extern "C" fn x_flush_sample_loading(_: RawHandle) -> i32 {
    ok!()
}
pub unsafe extern "C" fn x_get_sample_loading_state(_: RawHandle, state: *mut i32) -> i32 {
    if !state.is_null() {
        *state = LOADING_LOADED;
    }
    ok!()
}
