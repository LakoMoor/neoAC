// SPDX-License-Identifier: GPL-3.0-or-later

//! The mixer: an event's instruments, each a looping or one-shot sample read at a pitch and a
//! volume that the event's parameters set (see [`super::model`]).

use std::collections::HashMap;
use std::sync::Arc;

use super::model::{Event, Guid, Instrument, Model, Target};

/// A sample, as the bank holds it.
pub struct Wave {
    pub rate: u32,
    pub channels: usize,
    /// Interleaved 16-bit samples.
    pub frames: Vec<i16>,
}

impl Wave {
    pub fn from_pcm16(rate: u32, channels: u32, pcm: &[u8]) -> Wave {
        let frames = pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        Wave { rate, channels: channels as usize, frames }
    }

    fn len(&self) -> usize {
        self.frames.len() / self.channels.max(1)
    }

    /// The frame at a fractional position, linearly interpolated (wrapping when `looping`).
    fn at(&self, position: f64, looping: bool) -> Option<[f32; 2]> {
        let n = self.len();
        if n == 0 {
            return None;
        }
        let (mut i, frac) = (position.floor() as isize, (position - position.floor()) as f32);
        let mut j = i + 1;
        if looping {
            i = i.rem_euclid(n as isize);
            j = j.rem_euclid(n as isize);
        } else if i as usize >= n {
            return None;
        } else if j as usize >= n {
            j = i;
        }
        let read = |k: isize| -> [f32; 2] {
            let k = k as usize * self.channels;
            let f = |i: usize| self.frames[i] as f32 / 32768.0;
            if self.channels == 1 {
                [f(k), f(k)]
            } else {
                [f(k), f(k + 1)]
            }
        };
        let (a, b) = (read(i), read(j));
        Some([a[0] + (b[0] - a[0]) * frac, a[1] + (b[1] - a[1]) * frac])
    }
}

/// One event instance playing.
pub struct Playing {
    pub event: Event,
    pub params: HashMap<Guid, f32>,
    pub volume: f32,
    pub pitch: f32,
    /// Distance gain and pan, set by the caller from the 3D position.
    pub gain: f32,
    pub pan: f32,
    voices: Vec<Voice>,
    pub finished: bool,
}

struct Voice {
    instrument: usize,
    position: f64,
    gain: f32,
    done: bool,
}

pub fn db(x: f32) -> f32 {
    // (the banks' curves use -42 dB for "off": the bottom of Studio's fader)
    if x <= -41.5 {
        0.0
    } else {
        10f32.powf(x / 20.0)
    }
}

impl Playing {
    pub fn new(event: Event) -> Playing {
        let voices = event.instruments.iter().map(|&instrument| Voice { instrument, position: 0.0, gain: -1.0, done: false }).collect();
        Playing { event, params: HashMap::new(), volume: 1.0, pitch: 1.0, gain: 1.0, pan: 0.0, voices, finished: false }
    }

    pub fn set_param(&mut self, guid: Guid, value: f32) {
        self.params.insert(guid, value);
    }

    fn param(&self, g: &Guid) -> f32 {
        self.params.get(g).copied().unwrap_or(0.0)
    }

    /// The instrument's gain and pitch ratio now.
    pub fn settings(&self, i: &Instrument) -> (f32, f32) {
        for &(p, start, len) in &i.places {
            let v = self.param(&p);
            if v < start || v > start + len {
                return (0.0, 1.0);
            }
        }
        let mut gain = db(i.volume_db);
        let mut semitones = i.pitch_semitones;
        for c in &i.controls {
            let v = self.param(&c.param);
            let y = c.curve.eval(v);
            match c.target {
                Target::VolumeDb => gain *= db(y),
                Target::VolumeLinear => gain *= y.clamp(0.0, 1.0),
                Target::Pitch => semitones += y,
                Target::Other(_) => {}
            }
        }
        let mut ratio = 2f32.powf(semitones / 12.0);
        if let Some((p, reference)) = i.autopitch {
            if reference > 0.0 {
                ratio *= (self.param(&p) / reference).max(0.05);
            }
        }
        // a recording pitched far down is not what the sound design meant to be heard
        let fade = ((ratio - 0.35) / 0.15).clamp(0.0, 1.0);
        gain *= fade;
        if fade == 0.0 {
            return (0.0, ratio);
        }
        (gain, ratio)
    }

    /// Adds `frames` frames of the event into `out` (interleaved stereo) at `rate` Hz.
    pub fn mix(&mut self, model: &Model, waves: &[Arc<Wave>], rate: u32, out: &mut [f32]) {
        let frames = out.len() / 2;
        let mut alive = false;
        let (pan_l, pan_r) = {
            let p = self.pan.clamp(-1.0, 1.0);
            (((1.0 - p) * 0.5 * std::f32::consts::PI * 0.5).cos().max(0.0) * 1.0, ((1.0 + p) * 0.5 * std::f32::consts::PI * 0.5).sin().max(0.0) * 1.0)
        };
        let master = self.volume * self.gain;
        for k in 0..self.voices.len() {
            if self.voices[k].done {
                continue;
            }
            let inst = &model.instruments[self.voices[k].instrument];
            let Some(wave) = waves.get(inst.sample) else { continue };
            let (target_gain, ratio) = self.settings(inst);
            let step = wave.rate as f64 / rate as f64 * (ratio * self.pitch) as f64;
            let v = &mut self.voices[k];
            let from = if v.gain < 0.0 { target_gain } else { v.gain };
            if target_gain > 0.0 || from > 0.0 {
                alive = true;
            }
            for n in 0..frames {
                let g = (from + (target_gain - from) * (n + 1) as f32 / frames as f32) * master;
                match wave.at(v.position, inst.looping) {
                    Some(s) => {
                        out[2 * n] += s[0] * g * pan_l * std::f32::consts::SQRT_2;
                        out[2 * n + 1] += s[1] * g * pan_r * std::f32::consts::SQRT_2;
                        v.position += step;
                    }
                    None => {
                        v.done = true;
                        break;
                    }
                }
            }
            v.gain = target_gain;
            if !v.done {
                alive = true;
            }
        }
        if !alive && self.voices.iter().all(|v| v.done) {
            self.finished = true;
        }
    }
}
