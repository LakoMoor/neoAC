// SPDX-License-Identifier: GPL-3.0-or-later

//! `event_render <bank> [--list] [--event <n>] [--param name=value ...] [--seconds s] [--sweep name=from:to] --out <file.wav>`:
//! plays one event of a bank through the native mixer into a WAV file (no FMOD).

use std::collections::HashMap;
use std::sync::Arc;

use rustyac_audio::bank::Bank;
use rustyac_audio::native::mixer::{Playing, Wave};
use rustyac_audio::native::model::{guid_text, Model};

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let file = args.next().ok_or("event_render <bank> ...")?;
    let bank = Bank::open(std::path::Path::new(&file))?;
    let mut model = Model::read(&bank);
    let samples = bank.samples()?;
    model.name_layers(&samples.iter().map(|s| s.name.clone()).collect::<Vec<_>>());
    let mut events: Vec<_> = model.events.values().cloned().collect();
    events.sort_by_key(|e| e.guid);
    let (mut pick, mut seconds, mut out, mut list) = (0usize, 5.0f32, None, false);
    let mut params: Vec<(String, f32)> = Vec::new();
    let mut sweep: Option<(String, f32, f32)> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--list" => list = true,
            "--event" => pick = args.next().ok_or("--event n")?.parse().map_err(|e| format!("{e}"))?,
            "--seconds" => seconds = args.next().ok_or("--seconds s")?.parse().map_err(|e| format!("{e}"))?,
            "--param" => {
                let t = args.next().ok_or("--param name=value")?;
                let (n, v) = t.split_once('=').ok_or("name=value")?;
                params.push((n.to_string(), v.parse().map_err(|e| format!("{e}"))?));
            }
            "--sweep" => {
                let t = args.next().ok_or("--sweep name=from:to")?;
                let (n, r) = t.split_once('=').ok_or("name=from:to")?;
                let (a, b) = r.split_once(':').ok_or("from:to")?;
                sweep = Some((n.to_string(), a.parse().map_err(|e| format!("{e}"))?, b.parse().map_err(|e| format!("{e}"))?));
            }
            "--out" => out = Some(args.next().ok_or("--out file")?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if list {
        for (i, e) in events.iter().enumerate() {
            let names: Vec<String> = e.params.iter().filter_map(|g| model.params.get(g)).map(|p| p.name.clone()).collect();
            let insts: Vec<String> = e.instruments.iter().map(|&k| samples.get(model.instruments[k].sample).map_or("?".into(), |s| s.name.clone())).collect();
            println!("{i:2} {} dist {}..{} params {:?} instruments {}: {}", guid_text(&e.guid), e.min_distance, e.max_distance, names, insts.len(), insts.join(" "));
        }
        return Ok(());
    }
    let event = events.get(pick).ok_or("no such event")?.clone();
    let waves: Vec<Arc<Wave>> = samples.iter().map(|s| Arc::new(Wave::from_pcm16(s.rate, s.channels, &s.pcm))).collect();
    let mut playing = Playing::new(event.clone());
    let by_name: HashMap<String, _> = event.params.iter().filter_map(|g| model.params.get(g).map(|p| (p.name.clone(), *g))).collect();
    for (n, v) in &params {
        playing.set_param(*by_name.get(n).ok_or(format!("the event has no parameter {n}"))?, *v);
    }
    if std::env::var_os("EXPLAIN").is_some() {
        for &k in &event.instruments {
            let i = &model.instruments[k];
            let (g, r) = playing.settings(i);
            let peak = waves[i.sample].frames.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f32 / 32768.0;
            println!("   {}", i.raw.iter().map(|b| format!("{b:02x}")).collect::<String>());
            for c in &i.controls {
                println!("      control param {} target {:?} points {:?}", model.params.get(&c.param).map_or("?", |p| p.name.as_str()), c.target, c.curve.points);
            }
            println!("   autopitch_min {} loopcount {} flags {} chance {} delay {:?} seek {} polyphony {} trim {} seekpct {}", i.autopitch_min, i32::from_le_bytes(i.raw[24..28].try_into().unwrap()), i.raw[28], f32::from_le_bytes(i.raw[33..37].try_into().unwrap()), (f32::from_le_bytes(i.raw[37..41].try_into().unwrap()), f32::from_le_bytes(i.raw[41..45].try_into().unwrap())), f32::from_le_bytes(i.raw[73..77].try_into().unwrap()), i32::from_le_bytes(i.raw[77..81].try_into().unwrap()), u32::from_le_bytes(i.raw[111..115].try_into().unwrap()), f32::from_le_bytes(i.raw[115..119].try_into().unwrap()));
            println!("{:18} gain {:.3} ratio {:.3} vol_db {:6.2} pitch {:5.2} loop {} auto {:?} place {:?} wave_peak {:.2} controls {}", samples[i.sample].name, g, r, i.volume_db, i.pitch_semitones, i.looping, i.autopitch.map(|a| a.1), i.places.iter().map(|p| (model.params.get(&p.0).map_or("?", |q| q.name.as_str()), p.1, p.2)).collect::<Vec<_>>(), peak, i.controls.len());
        }
    }
    let rate = 48000u32;
    let block = 512usize;
    let total = (seconds * rate as f32) as usize / block * block;
    let mut pcm: Vec<i16> = Vec::with_capacity(total * 2);
    let mut buffer = vec![0f32; block * 2];
    for b in 0..total / block {
        if let Some((n, from, to)) = &sweep {
            let v = from + (to - from) * (b as f32 / (total / block) as f32);
            playing.set_param(*by_name.get(n).ok_or(format!("no parameter {n}"))?, v);
        }
        buffer.fill(0.0);
        playing.mix(&model, &waves, rate, &mut buffer);
        pcm.extend(buffer.iter().map(|x| (x.clamp(-1.0, 1.0) * 32767.0) as i16));
    }
    let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    let wav = rustyac_audio::bank::wav(&rustyac_audio::bank::Sample { name: String::new(), rate, channels: 2, frames: (bytes.len() / 4) as u32, pcm: bytes });
    let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    let out = out.ok_or("--out file.wav")?;
    std::fs::write(&out, wav).map_err(|e| e.to_string())?;
    println!("{} frames, peak {:.2}, in {out}", pcm.len() / 2, peak as f32 / 32768.0);
    Ok(())
}
