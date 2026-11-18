// SPDX-License-Identifier: GPL-3.0-or-later

//! `launch_debug <car> <track> [seconds]`: full throttle by the keyboard from standstill; prints the pedals, the
//! revs, the gear, the wheels' speed and the car's speed every half second. For eyes.

use rustyac_web::session::Session;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let car = args.first().map(String::as_str).unwrap_or("ks_nissan_gtr");
    let track = args.get(1).map(String::as_str).unwrap_or("drift");
    let seconds: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8.0);
    let mut s = Session::with_assists(car, track, "", true, "hotlap", None, None, Some("[ASSISTS]\nABS=2\nTRACTION_CONTROL=2\nSTABILITY_CONTROL=0\nAUTO_SHIFTER=1\nAUTO_CLUTCH=1\nAUTO_BLIP=1\nDAMAGE=0\nFUEL_RATE=0\nTYRE_WEAR=0\nSLIPSTREAM=0\n")).expect("session");
    let dt = 1.0 / 60.0;
    for _ in 0..120 {
        s.advance(dt).expect("advance");
    }
    {
        let mut i = s.input.lock().unwrap();
        i.keys.clear();
        i.keys.insert(0x26 as _);
    }
    let mut t = 0.0;
    let mut n = 0;
    while t < seconds {
        s.advance(dt).expect("advance");
        t += dt;
        n += 1;
        if n % 30 == 0 {
            let v = s.view();
            println!("{t:5.1}s gas {:.2} clutch {:.2} brake {:.2} hand {:.2} gear {} rpm {:6.0}/{:.0} wheels {:.1} {:.1} {:.1} {:.1} speed {:.1} km/h lights {}", v.gas, v.clutch, v.brake, v.hand_brake, v.gear - 1, v.rpm, v.rpm_limit, v.wheel_angular_speed[0], v.wheel_angular_speed[1], v.wheel_angular_speed[2], v.wheel_angular_speed[3], v.speed_kmh, v.lights);
        }
    }
}
