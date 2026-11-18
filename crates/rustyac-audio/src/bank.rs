// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's FMOD 1.08 sound banks read without FMOD: the RIFF `FEV ` container (event data in
//! `LIST` chunks) and its `SND ` chunk, an FSB5 file holding the samples. The banks AC ships
//! hold uncompressed PCM16, so a sample is its bytes, no decoder.
//!
//! This is the first layer of a mixer that does not need FMOD (macOS, Linux). What is not here
//! yet: the events themselves (`EVNT`, `TMLN`, the sound modules `WAIT`, the parameter
//! automation `PMLO` and its curves `CRVS`), see `docs/port/sound_native.md`.

use std::path::Path;

/// One chunk of the bank's RIFF tree.
#[derive(Clone, Debug)]
pub struct Chunk<'a> {
    pub tag: [u8; 4],
    /// For a `LIST`: its form (`PROJ`, `EVNT`, ...); the data then starts after it.
    pub form: Option<[u8; 4]>,
    pub data: &'a [u8],
}

impl<'a> Chunk<'a> {
    /// The chunks inside a `LIST`.
    pub fn children(&self) -> Vec<Chunk<'a>> {
        if self.form.is_none() {
            return Vec::new();
        }
        chunks(self.data)
    }

    pub fn tag_str(&self) -> String {
        String::from_utf8_lossy(&self.form.unwrap_or(self.tag)).into_owned()
    }
}

/// The chunks of a RIFF body, one level.
pub fn chunks(mut body: &[u8]) -> Vec<Chunk<'_>> {
    let mut out = Vec::new();
    while body.len() >= 8 {
        let tag: [u8; 4] = body[0..4].try_into().unwrap();
        let size = u32::from_le_bytes(body[4..8].try_into().unwrap()) as usize;
        let end = (8 + size).min(body.len());
        let data = &body[8..end];
        if &tag == b"LIST" && data.len() >= 4 {
            out.push(Chunk { tag, form: Some(data[0..4].try_into().unwrap()), data: &data[4..] });
        } else {
            out.push(Chunk { tag, form: None, data });
        }
        body = &body[(end + (size & 1)).min(body.len())..];
    }
    out
}

/// One sample of the FSB5 inside a bank.
#[derive(Clone, Debug)]
pub struct Sample {
    pub name: String,
    pub rate: u32,
    pub channels: u32,
    /// Frames (samples per channel).
    pub frames: u32,
    /// Interleaved 16-bit little-endian PCM.
    pub pcm: Vec<u8>,
}

/// A bank: the event chunks and the samples.
pub struct Bank {
    bytes: Vec<u8>,
    snd: (usize, usize),
}

const RATES: [u32; 10] = [4000, 8000, 11000, 11025, 16000, 22050, 24000, 32000, 44100, 48000];

impl Bank {
    pub fn open(path: &Path) -> Result<Bank, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Bank::parse(bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(bytes: Vec<u8>) -> Result<Bank, String> {
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"FEV " {
            return Err("not a FMOD bank (RIFF FEV)".to_string());
        }
        let mut snd = (0, 0);
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            if &bytes[at..at + 4] == b"SND " {
                snd = (at + 8, size.min(bytes.len() - at - 8));
            }
            at += 8 + size + (size & 1);
        }
        Ok(Bank { bytes, snd })
    }

    /// The top-level chunks (`FMT `, the `PROJ` list, `SND `).
    pub fn chunks(&self) -> Vec<Chunk<'_>> {
        chunks(&self.bytes[12..])
    }

    /// Every sample of the bank's FSB5. Only PCM16 is read (what AC's banks hold).
    pub fn samples(&self) -> Result<Vec<Sample>, String> {
        // (the FSB5 starts at the next 32 bytes of the file, after zeros)
        let snd = &self.bytes[self.snd.0..self.snd.0 + self.snd.1];
        let start = snd.windows(4).take(64).position(|w| w == b"FSB5").ok_or("the bank has no FSB5 sample data")?;
        let fsb = &snd[start..];
        if fsb.len() < 60 {
            return Err("the bank's FSB5 is cut short".to_string());
        }
        let u32_at = |at: usize| u32::from_le_bytes(fsb[at..at + 4].try_into().unwrap());
        let (version, count, header_size, names_size, data_size, mode) = (u32_at(4), u32_at(8) as usize, u32_at(12) as usize, u32_at(16) as usize, u32_at(20) as usize, u32_at(24));
        if mode != 2 {
            return Err(format!("sample format {mode} (only PCM16, 2, is read)"));
        }
        let table = if version == 0 { 64 } else { 60 };
        let (names_at, data_at) = (table + header_size, table + header_size + names_size);
        let mut at = table;
        let mut heads = Vec::with_capacity(count);
        for _ in 0..count {
            let v = u64::from_le_bytes(fsb[at..at + 8].try_into().unwrap());
            at += 8;
            let rate = RATES.get(((v >> 1) & 0xf) as usize).copied().ok_or("an unknown sample rate")?;
            heads.push((rate, ((v >> 5) & 1) as u32 + 1, ((v >> 6) & 0x0fff_ffff) as usize * 16, (v >> 34) as u32));
            // chunks that follow a header (loop points, ...): skipped
            if v & 1 != 0 {
                loop {
                    let c = u32::from_le_bytes(fsb[at..at + 4].try_into().unwrap());
                    at += 4 + ((c >> 1) & 0xff_ffff) as usize;
                    if c & 1 == 0 {
                        break;
                    }
                }
            }
        }
        let mut out = Vec::with_capacity(count);
        for (i, &(rate, channels, offset, frames)) in heads.iter().enumerate() {
            let end = heads.get(i + 1).map_or(data_size, |next| next.2);
            let pcm = fsb.get(data_at + offset..data_at + end.min(offset + frames as usize * channels as usize * 2)).ok_or("a sample lies outside the bank")?.to_vec();
            let name = if names_size > 0 {
                let name_at = names_at + u32_at(names_at + i * 4) as usize;
                let end = fsb[name_at..].iter().position(|&b| b == 0).map_or(fsb.len(), |n| name_at + n);
                String::from_utf8_lossy(&fsb[name_at..end]).into_owned()
            } else {
                format!("sample{i}")
            };
            out.push(Sample { name, rate, channels, frames, pcm });
        }
        Ok(out)
    }
}

/// A 16-bit PCM WAV file.
pub fn wav(sample: &Sample) -> Vec<u8> {
    let n = sample.pcm.len() as u32;
    let mut out = Vec::with_capacity(44 + sample.pcm.len());
    out.extend(b"RIFF");
    out.extend((36 + n).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((sample.channels as u16).to_le_bytes());
    out.extend(sample.rate.to_le_bytes());
    out.extend((sample.rate * sample.channels * 2).to_le_bytes());
    out.extend((sample.channels as u16 * 2).to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(n.to_le_bytes());
    out.extend(&sample.pcm);
    out
}
