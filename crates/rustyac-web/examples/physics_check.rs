// SPDX-License-Identifier: GPL-3.0-or-later

//! `physics_check <car> <track>`: drives the car by the keyboard keys in a fixed script and prints
//! what a real car does for comparison: the launch (0-100 km/h, 0-200), gear changes, the braking
//! distance from 100 km/h, the lateral g in a held turn. Needs `AC_ROOT`. For eyes, not a test.

use rustyac_web::session::Session;

const UP: u32 = 0x26;
const DOWN: u32 = 0x28;
const LEFT: u32 = 0x25;
const RIGHT: u32 = 0x27;

/// The car's heading (rad): 0 along +z.
fn heading(v: &rustyac_game::view::CarView) -> f32 {
    v.body[2][0].atan2(v.body[2][2])
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let car = args.first().map(String::as_str).unwrap_or("ks_nissan_gtr");
    let track = args.get(1).map(String::as_str).unwrap_or("drift");
    let mut s = Session::with_assists(car, track, "", true, "hotlap", None, None, Some("[ASSISTS]\nABS=2\nTRACTION_CONTROL=2\nSTABILITY_CONTROL=0\nAUTO_SHIFTER=1\nAUTO_CLUTCH=1\nAUTO_BLIP=1\nDAMAGE=0\nFUEL_RATE=0\nTYRE_WEAR=0\nSLIPSTREAM=0\n")).expect("session");
    let hold = |s: &Session, keys: &[u32]| {
        let mut i = s.input.lock().unwrap();
        i.keys.clear();
        for k in keys {
            i.keys.insert(*k as _);
        }
    };
    let dt = 1.0 / 60.0;
    let t = std::cell::Cell::new(0.0f64);
    let run = |s: &mut Session, seconds: f64, keys: &[u32], log: &mut dyn FnMut(f64, &rustyac_game::view::CarView)| {
        let end = t.get() + seconds;
        let h0 = heading(&s.view());
        while t.get() < end {
            // (0 is the script's own flag, not a key)
            let mut k: Vec<u32> = keys.iter().copied().filter(|&x| x != 0).collect();
            // keys with the straight flag (0) steer the car to its starting heading
            if keys.contains(&0) {
                let e = heading(&s.view()) - h0;
                if e > 0.01 {
                    k.push(LEFT);
                } else if e < -0.01 {
                    k.push(RIGHT);
                }
            }
            hold(s, &k);
            s.advance(dt).expect("advance");
            t.set(t.get() + dt);
            let v = s.view();
            log(t.get(), &v);
        }
    };
    // settle
    run(&mut s, 2.0, &[], &mut |_, _| {});
    // launch
    let (mut t100, mut t200, mut last_gear, mut shifts) = (None, None, 0, Vec::new());
    let start = t.get();
    let mut dist = 0.0f64;
    run(&mut s, 25.0, &[UP, 0], &mut |now, v| {
        dist += v.speed_ms as f64 / 60.0;
        if t100.is_none() && v.speed_kmh >= 100.0 {
            t100 = Some(now - start);
        }
        if t200.is_none() && v.speed_kmh >= 200.0 {
            t200 = Some(now - start);
        }
        if v.gear != last_gear {
            shifts.push(format!("gear {} at {:.1}s {:.0} km/h {:.0} rpm", v.gear - 1, now - start, v.speed_kmh, v.rpm));
            last_gear = v.gear;
        }
    });
    let v = s.view();
    println!("launch: 0-100 km/h {}, 0-200 km/h {}, after 25 s {:.0} km/h in gear {} ({:.0} m)", t100.map_or("n/a".into(), |t| format!("{t:.2} s")), t200.map_or("n/a".into(), |t| format!("{t:.2} s")), v.speed_kmh, v.gear - 1, dist);
    for line in shifts.iter().take(10) {
        println!("  {line}");
    }
    // braking from here: until stopped
    let (mut bdist, mut bt, mut peak_decel, mut prev) = (0.0f64, 0.0f64, 0.0f32, v.speed_ms);
    let mut at100 = None;
    run(&mut s, 20.0, &[DOWN, 0], &mut |_, v| {
        if v.speed_kmh <= 100.0 && at100.is_none() {
            at100 = Some((0.0f64, 0.0f64));
        }
        if let Some((d, tt)) = at100.as_mut() {
            if v.speed_ms > 0.5 {
                *d += v.speed_ms as f64 / 60.0;
                *tt += 1.0 / 60.0;
            }
        }
        peak_decel = peak_decel.max((prev - v.speed_ms) * 60.0 / 9.81);
        prev = v.speed_ms;
        let _ = (&mut bdist, &mut bt);
    });
    if let Some((d, tt)) = at100 {
        println!("braking: 100 -> 0 km/h in {d:.1} m, {tt:.2} s; peak deceleration {peak_decel:.2} g");
    }
    // held turn at ~80 km/h: gas to speed, then steer
    run(&mut s, 12.0, &[UP, 0], &mut |_, _| {});
    let mut peak_lat = 0.0f32;
    let mut speed_in = 0.0;
    run(&mut s, 0.3, &[], &mut |_, v| speed_in = v.speed_kmh);
    run(&mut s, 5.0, &[LEFT], &mut |_, v| peak_lat = peak_lat.max(v.acc_g[0].abs()));
    println!("turn: held steering from {speed_in:.0} km/h, peak lateral {peak_lat:.2} g");
}
