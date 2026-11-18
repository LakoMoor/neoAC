// SPDX-License-Identifier: GPL-3.0-or-later

//! What an FMOD 1.08 bank says about its events, read from the bytes (no FMOD). The format is
//! not documented; what is read here was worked out from AC's banks, see
//! `docs/port/sound_native.md`.
//!
//! A bank holds, as `LIST`s under `PROJ`: parameters (`PRMS`), events (`EVTS`), the sound
//! instruments an event plays (`WAIS`: `WAIB` the instrument's and its waveform's GUID, `INST`
//! its settings), waveforms (`WAVS`: GUID to the sample's number in the FSB5), the place of an
//! instrument along a parameter (`PMLS`: `PMLB`), what a parameter does to an instrument
//! (`CTRS`: `CTRL`, with its curve in `CRVS`) and timelines (`TLNS`) for the events that
//! have a single instrument and no parameter.

use std::collections::HashMap;

use crate::bank::{chunks, Bank, Chunk};

pub type Guid = [u8; 16];

fn f32_at(b: &[u8], at: usize) -> f32 {
    b.get(at..at + 4).map_or(0.0, |w| f32::from_le_bytes(w.try_into().unwrap()))
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2).map_or(0, |w| u16::from_le_bytes(w.try_into().unwrap()))
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map_or(0, |w| u32::from_le_bytes(w.try_into().unwrap()))
}

fn guid_at(b: &[u8], at: usize) -> Option<Guid> {
    b.get(at..at + 16).map(|w| w.try_into().unwrap())
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub min: f32,
    pub max: f32,
}

/// A curve of the automation: points (x, y, the shape that leaves the point).
#[derive(Clone, Debug, Default)]
pub struct Curve {
    pub points: Vec<[f32; 3]>,
}

/// How much a segment's shape value bends it: at 0.2547 (the value the banks' crossfades have)
/// the middle of a fade is at the equal-power level.
const BEND: f32 = 0.8128;

impl Curve {
    pub fn eval(&self, x: f32) -> f32 {
        let (Some(first), Some(last)) = (self.points.first(), self.points.last()) else { return 0.0 };
        if x <= first[0] {
            return first[1];
        }
        if x >= last[0] {
            return last[1];
        }
        for pair in self.points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if x <= b[0] {
                let t = if b[0] > a[0] { (x - a[0]) / (b[0] - a[0]) } else { 1.0 };
                let u = (t - BEND * a[2] * (std::f32::consts::PI * t).sin()).clamp(0.0, 1.0);
                return a[1] + (b[1] - a[1]) * u;
            }
        }
        last[1]
    }
}

/// What a parameter does to an instrument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// Volume, the curve in decibels.
    VolumeDb,
    /// Volume, the curve a factor (0..1): the crossfades.
    VolumeLinear,
    /// Pitch, the curve in semitones.
    Pitch,
    /// Anything else (the cone angle's filter, ...): not played yet.
    Other(u32),
}

#[derive(Clone, Debug)]
pub struct Control {
    pub param: Guid,
    pub target: Target,
    pub curve: Curve,
}

#[derive(Clone, Debug)]
pub struct Instrument {
    pub guid: Guid,
    /// The sample's number in the bank's FSB5.
    pub sample: usize,
    pub volume_db: f32,
    pub pitch_semitones: f32,
    pub looping: bool,
    /// The parameter whose value sets the pitch against `reference` (rpm against the rpm the
    /// sample was recorded at).
    pub autopitch: Option<(Guid, f32)>,
    /// `AutoPitchAtMinimum` (FMOD Studio's "autopitch at minimum", INST byte 119): the parameter
    /// value below which the autopitch stops lowering the pitch.
    pub autopitch_min: f32,
    /// Where on a parameter the instrument sounds: (parameter, start, length); it sounds only
    /// where it is placed on every parameter that places it.
    pub places: Vec<(Guid, f32, f32)>,
    pub controls: Vec<Control>,
    /// The INST chunk's bytes (for study).
    pub raw: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub guid: Guid,
    pub params: Vec<Guid>,
    /// The parameter named `throttle`, if the event has one.
    pub throttle: Option<Guid>,
    pub min_distance: f32,
    pub max_distance: f32,
    pub instruments: Vec<usize>,
}

/// A bank's events and their instruments.
#[derive(Default)]
pub struct Model {
    pub params: HashMap<Guid, Param>,
    pub events: HashMap<Guid, Event>,
    pub instruments: Vec<Instrument>,
}

fn section<'a>(top: &'a [Chunk<'a>], form: &[u8; 4]) -> Vec<Chunk<'a>> {
    top.iter().find(|c| c.form.as_ref() == Some(form)).map(|c| c.children()).unwrap_or_default()
}

impl Model {
    /// The model of one bank.
    pub fn read(bank: &Bank) -> Model {
        let all = bank.chunks();
        let Some(proj) = all.iter().find(|c| c.form.as_ref() == Some(b"PROJ")) else { return Model::default() };
        let top = proj.children();
        let mut model = Model::default();

        for list in section(&top, b"PRMS") {
            if list.form.as_ref() != Some(b"PARM") {
                continue;
            }
            for c in list.children() {
                if &c.tag == b"PRMB" {
                    let b = c.data;
                    let n = u16_at(b, 21) as usize;
                    if let (Some(guid), Some(name)) = (guid_at(b, 0), b.get(23..23 + n)) {
                        model.params.insert(guid, Param { name: String::from_utf8_lossy(name).into_owned(), min: f32_at(b, 23 + n), max: f32_at(b, 27 + n) });
                    }
                }
            }
        }

        let mut waves: HashMap<Guid, usize> = HashMap::new();
        for c in section(&top, b"WAVS") {
            if &c.tag == b"WAV " {
                if let Some(guid) = guid_at(c.data, 0) {
                    waves.insert(guid, u16_at(c.data, 22) as usize);
                }
            }
        }

        let mut curves: HashMap<Guid, Curve> = HashMap::new();
        for c in section(&top, b"CRVS") {
            if &c.tag == b"CURV" {
                if let Some(guid) = guid_at(c.data, 0) {
                    let points = (36..c.data.len().saturating_sub(15)).step_by(16).map(|at| [f32_at(c.data, at), f32_at(c.data, at + 4), f32_at(c.data, at + 8)]).collect();
                    curves.insert(guid, Curve { points });
                }
            }
        }

        // The group buses: an instrument is routed to one (INST byte 83), a bus to its parent
        // (GBSB byte 18: the routable's target), up to the event's master bus. Curves on the
        // buses (volume against `throttle`, `decay`, `speed`, ...) apply to what goes through.
        let mut parent: HashMap<Guid, Guid> = HashMap::new();
        for list in section(&top, b"GBSS") {
            for c in list.children() {
                if let (Some(bus), Some(to)) = (guid_at(c.data, 0), guid_at(c.data, 18)) {
                    parent.insert(bus, to);
                }
            }
        }
        let mut by_guid: HashMap<Guid, usize> = HashMap::new();
        let mut routed: Vec<Option<Guid>> = Vec::new();
        for list in section(&top, b"WAIS") {
            if list.form.as_ref() != Some(b"WAIT") {
                continue;
            }
            let kids = list.children();
            let find = |tag: &[u8; 4]| kids.iter().find(|c| &c.tag == tag && c.form.is_none()).map(|c| c.data);
            let (Some(waib), Some(inst)) = (find(b"WAIB"), find(b"INST")) else { continue };
            let (Some(guid), Some(wave)) = (guid_at(waib, 0), guid_at(waib, 16)) else { continue };
            let Some(&sample) = waves.get(&wave) else { continue };
            let auto_param = guid_at(inst, 53).filter(|g| g.iter().any(|&b| b != 0));
            by_guid.insert(guid, model.instruments.len());
            routed.push(guid_at(inst, 83));
            model.instruments.push(Instrument {
                guid,
                sample,
                volume_db: f32_at(inst, 16),
                pitch_semitones: f32_at(inst, 20),
                looping: u32_at(inst, 24) == 0xffff_ffff,
                autopitch: auto_param.map(|g| (g, f32_at(inst, 69))),
                autopitch_min: f32_at(inst, 119),
                places: Vec::new(),
                controls: Vec::new(),
                raw: inst.to_vec(),
            });
        }

        let mut bus_controls: HashMap<Guid, Vec<Control>> = HashMap::new();
        for c in section(&top, b"CTRS") {
            if &c.tag != b"CTRL" {
                continue;
            }
            let (Some(curve_guid), Some(inst), Some(param)) = (guid_at(c.data, 0), guid_at(c.data, 16), guid_at(c.data, 32)) else { continue };
            let Some(curve) = curves.get(&curve_guid) else { continue };
            let target = match u32_at(c.data, 64) {
                0 => Target::VolumeDb,
                4 => Target::VolumeLinear,
                1 => Target::Pitch,
                other => Target::Other(other),
            };
            let control = Control { param, target, curve: curve.clone() };
            match by_guid.get(&inst) {
                Some(&at) => model.instruments[at].controls.push(control),
                None => bus_controls.entry(inst).or_default().push(control),
            }
        }

        // what the buses do, handed to the instruments that go through them
        for (k, bus) in routed.iter().enumerate() {
            let mut bus = *bus;
            for _ in 0..6 {
                let Some(b) = bus else { break };
                if let Some(list) = bus_controls.get(&b) {
                    model.instruments[k].controls.extend(list.iter().cloned());
                }
                bus = parent.get(&b).copied();
            }
        }

        let mut event_instruments: HashMap<Guid, Vec<usize>> = HashMap::new();
        for list in section(&top, b"PMLS") {
            if list.form.as_ref() != Some(b"PMLO") {
                continue;
            }
            for c in list.children() {
                if &c.tag != b"PMLB" {
                    continue;
                }
                let (Some(param), Some(event)) = (guid_at(c.data, 0), guid_at(c.data, 32)) else { continue };
                let mut at = 52;
                while let Some(inst) = guid_at(c.data, at) {
                    if let Some(&i) = by_guid.get(&inst) {
                        model.instruments[i].places.push((param, f32_at(c.data, at + 16), f32_at(c.data, at + 20)));
                        event_instruments.entry(event).or_default().push(i);
                    }
                    at += 24;
                }
            }
        }

        for list in section(&top, b"TLNS") {
            for c in list.children() {
                if &c.tag == b"TLNB" && c.data.len() >= 54 {
                    let (Some(event), Some(inst)) = (guid_at(c.data, 16), guid_at(c.data, 38)) else { continue };
                    if let Some(&i) = by_guid.get(&inst) {
                        let list = event_instruments.entry(event).or_default();
                        if !list.contains(&i) {
                            list.push(i);
                        }
                    }
                }
            }
        }

        for list in section(&top, b"EVTS") {
            if list.form.as_ref() != Some(b"EVNT") {
                continue;
            }
            for c in list.children() {
                if &c.tag != b"EVTB" {
                    continue;
                }
                let Some(guid) = guid_at(c.data, 0) else { continue };
                let params: Vec<Guid> = model.params.keys().copied().filter(|p| c.data.windows(16).any(|w| w == p)).collect();
                let throttle = params.iter().copied().find(|g| model.params[g].name == "throttle");
                let n = c.data.len();
                model.events.insert(
                    guid,
                    Event { guid, params, throttle, min_distance: f32_at(c.data, n.saturating_sub(16)), max_distance: f32_at(c.data, n.saturating_sub(12)), instruments: event_instruments.remove(&guid).unwrap_or_default() },
                );
            }
        }
        let _ = chunks;
        model
    }

    /// The rpm a loop was recorded at comes with its name, and the one reference in the banks
    /// that disagrees with it is corrected (see below).
    pub fn name_layers(&mut self, names: &[String]) {
        for i in &mut self.instruments {
            let n = names.get(i.sample).map_or("", |n| n.as_str()).to_ascii_lowercase();
            // The name says the rpm a loop was recorded at (measured on the GT-R's samples: the
            // firing frequency of `5813a` is 290 Hz = 5800 rpm). One reference in the banks
            // disagrees by far (`ext_idle1036`: 1648, recorded at 1040, which plays the idle a
            // fifth too low): the recording is trusted.
            if let (Some((param, reference)), Some(recorded)) = (i.autopitch, rpm_in_name(&n)) {
                if (reference - recorded).abs() > 0.15 * recorded {
                    i.autopitch = Some((param, recorded));
                }
            }
        }
    }

    pub fn param_named(&self, event: &Event, name: &str) -> Option<Guid> {
        event.params.iter().copied().find(|g| self.params.get(g).is_some_and(|p| p.name == name))
    }
}

/// The first run of three to five digits in a sample's name, as the rpm it was recorded at.
fn rpm_in_name(name: &str) -> Option<f32> {
    let bytes = name.as_bytes();
    let start = bytes.iter().position(|b| b.is_ascii_digit())?;
    let len = bytes[start..].iter().take_while(|b| b.is_ascii_digit()).count();
    (3..=5).contains(&len).then(|| name[start..start + len].parse().ok()).flatten()
}

pub fn guid_text(g: &Guid) -> String {
    // (FMOD's GUID: Data1, Data2, Data3 little-endian, Data4 as bytes)
    let d1 = u32::from_le_bytes(g[0..4].try_into().unwrap());
    let d2 = u16::from_le_bytes(g[4..6].try_into().unwrap());
    let d3 = u16::from_le_bytes(g[6..8].try_into().unwrap());
    format!("{{{d1:08x}-{d2:04x}-{d3:04x}-{:02x}{:02x}-{}}}", g[8], g[9], g[10..16].iter().map(|b| format!("{b:02x}")).collect::<String>())
}
