# Sound on macOS and Linux without FMOD (started)

AC's FMOD 1.08.12 is a Windows library (its macOS build is x86_64 only), and FMOD 2.x cannot be
shipped. The chosen way is a mixer of our own that plays the game's banks directly.

## What is done

`crates/rustyac-audio/src/bank.rs`, `examples/bank_dump.rs` (`cargo run --release -p rustyac-audio
--example bank_dump -- <file.bank> [--tree] [--wav <folder>]`):

- A bank is RIFF `FEV `: `FMT `, a `LIST PROJ` (all event data) and `SND `, which holds an
  **FSB5** after zero padding (it starts on a 32-byte boundary of the file).
- AC's banks hold **uncompressed PCM16** (`mode` 2): no Vorbis, no decoder. Sample header (u64):
  bit 0 extra chunks, bits 1-4 rate (table: 4000 .. 48000), bit 5 stereo, bits 6-33 data offset
  / 16, bits 34-63 frames. Sample data starts at `60 + header size + name table size`.
- `ks_nissan_gtr.bank`: 35 samples, 44.1 kHz, the engine as RPM-banded loops (`ext_5141a`,
  `mid_5867b`, `idle_1884`, `*_off` for lift-off), plus `limiter`, `turbo`, `horn`, doors, tyres,
  wind, skid, gear shifts. They play correctly as WAV.

## Not done: the events

The `LIST PROJ` holds what FMOD Studio's runtime does with those samples. Forms seen in a car
bank: `EVNT` (the event: `EVTB` starts with its GUID), `TMLN` (timeline), `WAIT` (a sound module:
which sample, start, loop), `PMLO` / `PARM` (a parameter and what it modulates), `CRVS` (the
curves of the automation), `TRNS` / `TRAN` (transitions), `BEFF` / `BEFX` / `PEFF` (effects: the
3D spatializer, gain, the game's two plug-ins), `GBUS` / `MBUS` / `IBUS` (buses), `PRPS`
(properties). An engine event crossfades the RPM-banded loops by the RPM parameter and pitches
them: that is the part to rebuild, from the bytes, with the call logs of the Windows port
(`tools/audio_oracle`) as the reference for every event.

Also needed: an event's path to its GUID (`GUIDs.txt` beside each car's bank: the copy of the game
used so far has none, `common.strings.bank` gives the master bank's), then the mixer thread on
the sound card (cpal), 3D position, the listener, reverb.

Steps: 1. read `EVNT` / `TMLN` / `WAIT` / `PMLO` / `CRVS` (a dump tool first); 2. play one event
by parameter value; 3. the `FMOD_*` calls of `fmod/raw.rs` implemented on that (so `rustyac-audio`
above it stays the port's); 4. cpal output; 5. proofs against the Windows call logs.

## What a public FMOD bank parser added (FModBankParser, Apache-2.0)

* INST (file version 0x50): timeline guid 0, volume 16, pitch 20, loop count 24, flags 28, 3D offset 29,
  trigger chance 33, trigger delay 37, quantization 45, autopitch parameter 53, autopitch reference 69,
  seek 73, polyphony 77, routable 81 (i16 size, **bus guid at 83**), polyphony behaviour 107, left trim 111,
  seek percent 115, autopitch at minimum 119. Placement (PMLB): guid, position, length.
* Curves on the **group buses** (GBSB: guid 0, its parent bus at 18) are what `throttle`, `decay`,
  `speed` and `boost` do to the engine, limiter, tyres and turbo: an instrument's gain is its own curves
  times its bus's and its parents'. -42 dB in a curve is "off".
* The recorded rpm of a loop is in its name and its autopitch reference, except `ext_idle1036` (reference
  1648, recorded at 1040 by its firing frequency): the recording is trusted.
