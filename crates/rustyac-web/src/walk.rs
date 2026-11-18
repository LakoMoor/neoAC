// SPDX-License-Identifier: GPL-3.0-or-later

//! Getting out of the car, as in GTA: the driver on foot, seen through their own eyes. The car
//! stays where it is (its brake held); the walker moves over the track's own surface (the
//! physics' ray cast), is kept out of the car's body, and gets back in near the door.
//!
//! Coordinates are the game's: x to the right, y up, z ahead (a left-handed frame), a world
//! matrix has its right, up and ahead axes in rows 0 to 2 and the position in row 3; a camera's
//! third row is minus the view direction.

use rustyac_game::render::scene::CameraFrame;
use rustyac_game::view::Mat;
use rustyac_physics::track::Track;

/// The eyes above the feet (m).
pub const EYE: f32 = 1.65;
const WALK: f32 = 1.7;
const RUN: f32 = 4.2;
const GRAVITY: f32 = 9.81;
/// The car's body as a box, with the walker's own width on top (half sizes, m).
const CAR_HALF: [f32; 2] = [1.0 + 0.3, 2.35 + 0.3];
/// How near the driver's door the walker must be to sit down again (m).
pub const ENTER_RANGE: f32 = 3.6;

pub mod keys {
    pub const FORWARD: u8 = 1;
    pub const BACK: u8 = 2;
    pub const LEFT: u8 = 4;
    pub const RIGHT: u8 = 8;
    pub const RUN: u8 = 16;
}

#[derive(Clone, Debug)]
pub struct Walker {
    /// The feet.
    pub pos: [f32; 3],
    /// Radians: 0 looks along +z, positive turns to the right.
    pub yaw: f32,
    /// Radians: positive looks up.
    pub pitch: f32,
    /// [`keys`] held.
    pub keys: u8,
    /// Mouse movement since the last step (pixels).
    pub look: [f32; 2],
    vy: f32,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normal(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn row(m: &Mat, r: usize) -> [f32; 3] {
    [m[r][0], m[r][1], m[r][2]]
}

impl Walker {
    /// The driver stepping out on the left (the door of the driver's side), looking where the
    /// car points.
    pub fn leave_car(body: &Mat) -> Walker {
        let (right, ahead, centre) = (row(body, 0), row(body, 2), row(body, 3));
        let flat = |v: [f32; 3]| normal([v[0], 0.0, v[2]]);
        let (right, ahead) = (flat(right), flat(ahead));
        let pos = [centre[0] - right[0] * 1.75 + ahead[0] * 0.3, centre[1] - 0.3, centre[2] - right[2] * 1.75 + ahead[2] * 0.3];
        Walker { pos, yaw: ahead[0].atan2(ahead[2]), pitch: 0.0, keys: 0, look: [0.0; 2], vy: 0.0 }
    }

    /// Near enough to the car to sit down in it?
    pub fn near_car(&self, body: &Mat) -> bool {
        let c = row(body, 3);
        let (dx, dz) = (self.pos[0] - c[0], self.pos[2] - c[2]);
        (dx * dx + dz * dz).sqrt() < ENTER_RANGE + 1.0
    }

    /// One frame of `dt` seconds: the mouse turns, the keys walk, the ground holds, the car is
    /// not walked through.
    pub fn step(&mut self, dt: f32, track: Option<&Track>, body: &Mat) {
        let dt = dt.clamp(0.0, 0.1);
        self.yaw += self.look[0] * 0.0024;
        self.pitch = (self.pitch - self.look[1] * 0.0024).clamp(-1.45, 1.45);
        self.look = [0.0; 2];
        let (sin, cos) = self.yaw.sin_cos();
        let ahead = [sin, cos];
        let right = [cos, -sin];
        let k = |bit: u8| (self.keys & bit != 0) as i32 as f32;
        let (f, s) = (k(keys::FORWARD) - k(keys::BACK), k(keys::RIGHT) - k(keys::LEFT));
        let mut wish = [ahead[0] * f + right[0] * s, ahead[1] * f + right[1] * s];
        let len = (wish[0] * wish[0] + wish[1] * wish[1]).sqrt();
        if len > 1.0 {
            wish = [wish[0] / len, wish[1] / len];
        }
        let speed = if self.keys & keys::RUN != 0 { RUN } else { WALK };
        self.pos[0] += wish[0] * speed * dt;
        self.pos[2] += wish[1] * speed * dt;
        self.keep_out_of(body);
        // the ground under the feet: a ray from a little above them
        let ground = track.and_then(|t| {
            let org = rustyac_physics::vecmath::Vec3f { x: self.pos[0], y: self.pos[1] + 0.7, z: self.pos[2] };
            let dir = rustyac_physics::vecmath::Vec3f { x: 0.0, y: -1.0, z: 0.0 };
            t.ray_cast_hit(&org, &dir, 4.0).map(|h| h.contact.pos[1])
        });
        match ground {
            Some(g) => {
                self.vy -= GRAVITY * dt;
                self.pos[1] += self.vy * dt;
                if self.pos[1] <= g + 0.02 {
                    self.pos[1] = g;
                    self.vy = 0.0;
                }
            }
            None => self.vy = 0.0,
        }
    }

    /// The car's body is a box the walker cannot enter: pushed out through the nearest face.
    fn keep_out_of(&mut self, body: &Mat) {
        let (right, ahead, c) = (normal(row(body, 0)), normal(row(body, 2)), row(body, 3));
        let d = sub(self.pos, c);
        let (x, z) = (dot(d, [right[0], 0.0, right[2]]) / dot([right[0], 0.0, right[2]], [right[0], 0.0, right[2]]).max(1e-6), dot(d, [ahead[0], 0.0, ahead[2]]) / dot([ahead[0], 0.0, ahead[2]], [ahead[0], 0.0, ahead[2]]).max(1e-6));
        if x.abs() < CAR_HALF[0] && z.abs() < CAR_HALF[1] && (self.pos[1] - c[1]).abs() < 1.6 {
            let (px, pz) = (CAR_HALF[0] - x.abs(), CAR_HALF[1] - z.abs());
            if px < pz {
                let push = px * x.signum();
                self.pos[0] += right[0] * push;
                self.pos[2] += right[2] * push;
            } else {
                let push = pz * z.signum();
                self.pos[0] += ahead[0] * push;
                self.pos[2] += ahead[2] * push;
            }
        }
    }

    /// The view: `base` with the walker's eyes in place of its matrix (its lens kept).
    pub fn frame(&self, base: CameraFrame) -> CameraFrame {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let ahead = [cp * sy, sp, cp * cy];
        let right = normal(cross([0.0, 1.0, 0.0], ahead));
        let up = cross(ahead, right);
        let mut m = base.matrix;
        m[0] = [right[0], right[1], right[2], 0.0];
        m[1] = [up[0], up[1], up[2], 0.0];
        m[2] = [-ahead[0], -ahead[1], -ahead[2], 0.0];
        m[3] = [self.pos[0], self.pos[1] + EYE, self.pos[2], 1.0];
        CameraFrame { matrix: m, ..base }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustyac_game::view::IDENTITY;

    fn car_at_origin() -> Mat {
        IDENTITY
    }

    #[test]
    fn steps_out_on_the_left_and_walks_where_it_looks() {
        let body = car_at_origin();
        let mut w = Walker::leave_car(&body);
        assert!(w.pos[0] < -1.5, "the driver's side is the car's left: {:?}", w.pos);
        assert!(w.near_car(&body));
        w.keys = keys::FORWARD;
        let z0 = w.pos[2];
        for _ in 0..60 {
            w.step(1.0 / 60.0, None, &body);
        }
        assert!((w.pos[2] - z0 - WALK).abs() < 0.05, "one second of walking ahead: {}", w.pos[2] - z0);
        w.keys = keys::RIGHT;
        let x0 = w.pos[0];
        for _ in 0..60 {
            w.step(1.0 / 60.0, None, &body);
        }
        // (the car stands in the way after half a metre: that is its box)
        assert!(w.pos[0] > x0 + 0.3, "strafing right goes towards +x");
    }

    #[test]
    fn the_car_is_not_walked_through() {
        let body = car_at_origin();
        let mut w = Walker::leave_car(&body);
        w.pos = [-3.0, 0.0, 0.0];
        w.keys = keys::RIGHT;
        for _ in 0..240 {
            w.step(1.0 / 60.0, None, &body);
            let inside = w.pos[0].abs() < CAR_HALF[0] - 0.05 && w.pos[2].abs() < CAR_HALF[1] - 0.05;
            assert!(!inside, "inside the car at {:?}", w.pos);
        }
    }

    #[test]
    fn the_mouse_turns_and_the_view_follows() {
        let body = car_at_origin();
        let mut w = Walker::leave_car(&body);
        w.look = [400.0, 0.0];
        w.step(0.016, None, &body);
        assert!(w.yaw > 0.5, "moving the mouse right turns right");
        let frame = w.frame(CameraFrame { matrix: IDENTITY, fov: 60.0, near: 0.1 });
        let ahead = [-frame.matrix[2][0], -frame.matrix[2][1], -frame.matrix[2][2]];
        assert!(ahead[0] > 0.4, "looking towards +x after turning right: {ahead:?}");
        assert!((frame.matrix[3][1] - (w.pos[1] + EYE)).abs() < 1e-5);
        w.look = [0.0, -10_000.0];
        w.step(0.016, None, &body);
        assert!(w.pitch > 1.4, "the pitch is clamped short of straight up");
    }
}
