// SPDX-License-Identifier: GPL-3.0-or-later

//! The debug panel (F10), an immediate-mode window drawn over the game with egui: frame times
//! and draw statistics, the car's telemetry, the collision picture (each tyre's contact point
//! and normal, the velocity, the body's box, the other cars) projected into the view, the
//! walker, and the render settings that can change while the game runs.

use rustyac_game::render::scene::CameraFrame;
use rustyac_game::view::{CarView, Mat};
use rustyac_render::wgpu;
use std::collections::VecDeque;
use std::sync::Arc;
use winit::event::WindowEvent;
use winit::window::Window;

/// What the panel changes in the game: the game reads these back after each frame.
#[derive(Clone, Debug)]
pub struct Controls {
    pub brightness: f32,
    pub lod_distance: f32,
    pub paused: bool,
    /// the one-shot buttons: reset the car to the pits / track
    pub reset: bool,
    pub to_track: bool,
}

/// What the game tells the panel each frame.
pub struct Data<'a> {
    pub view: &'a CarView,
    pub camera: Option<CameraFrame>,
    pub triangles: i32,
    pub draw_calls: i32,
    pub render_size: (u32, u32),
    pub walker: Option<([f32; 3], f32)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Performance,
    Car,
    Collision,
    Render,
}

pub struct DebugUi {
    ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    pub open: bool,
    tab: Tab,
    frames: VecDeque<f32>,
    contacts: bool,
    velocity: bool,
    body_box: bool,
    others: bool,
}

impl DebugUi {
    pub fn new(window: &Arc<Window>, device: &wgpu::Device, format: wgpu::TextureFormat) -> DebugUi {
        let ctx = egui::Context::default();
        let state = egui_winit::State::new(ctx.clone(), egui::ViewportId::ROOT, window.as_ref(), Some(window.scale_factor() as f32), None, None);
        let renderer = egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default());
        DebugUi { ctx, state, renderer, open: false, tab: Tab::Performance, frames: VecDeque::new(), contacts: true, velocity: true, body_box: true, others: false }
    }

    /// An event for the panel: true when it takes it (the game does not see it).
    pub fn event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        if !self.open {
            return false;
        }
        let response = self.state.on_window_event(window, event);
        response.consumed
    }

    /// Does the pointer sit over the panel? (The game keeps its mouse look for itself otherwise.)
    pub fn wants_pointer(&self) -> bool {
        self.open && self.ctx.egui_wants_pointer_input()
    }

    pub fn frame(&mut self, window: &Window, device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::TextureView, size: (u32, u32), dt: f32, data: &Data, controls: &mut Controls) {
        if self.frames.len() >= 240 {
            self.frames.pop_front();
        }
        self.frames.push_back(dt * 1000.0);
        if !self.open {
            return;
        }
        let raw = self.state.take_egui_input(window);
        let mut tab = self.tab;
        let (mut contacts, mut velocity, mut body_box, mut others) = (self.contacts, self.velocity, self.body_box, self.others);
        let frames = &self.frames;
        let out = self.ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            egui::Window::new("rustyAC debug").default_width(340.0).show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    for (t, name) in [(Tab::Performance, "Performance"), (Tab::Car, "Car"), (Tab::Collision, "Collision"), (Tab::Render, "Render")] {
                        ui.selectable_value(&mut tab, t, name);
                    }
                });
                ui.separator();
                match tab {
                    Tab::Performance => performance(ui, frames, data),
                    Tab::Car => car(ui, data.view),
                    Tab::Collision => {
                        ui.checkbox(&mut contacts, "Tyre contact points and normals");
                        ui.checkbox(&mut velocity, "Velocity vector");
                        ui.checkbox(&mut body_box, "Body box");
                        ui.checkbox(&mut others, "Other cars");
                        ui.separator();
                        let v = data.view;
                        ui.label(format!("contacts mask: {:#06b}", v.contacts));
                        for w in 0..4 {
                            ui.label(format!("wheel {w}: load {:.0} N, slip {:.2}, travel {:.3} m, surface {}", v.wheel_load[w], v.wheel_slip[w], v.render.suspension_travel[w], v.surfaces[w].as_str()));
                        }
                        if let Some((pos, yaw)) = data.walker {
                            ui.separator();
                            ui.label(format!("on foot: {:.1} {:.1} {:.1}, yaw {:.0}°", pos[0], pos[1], pos[2], yaw.to_degrees()));
                        }
                    }
                    Tab::Render => {
                        ui.add(egui::Slider::new(&mut controls.brightness, 0.2..=4.0).text("brightness"));
                        ui.add(egui::Slider::new(&mut controls.lod_distance, 0.2..=3.2).text("detail distance"));
                        ui.checkbox(&mut controls.paused, "Pause the simulation");
                        ui.horizontal(|ui| {
                            controls.reset |= ui.button("Reset to pits").clicked();
                            controls.to_track |= ui.button("To track").clicked();
                        });
                        ui.label("(brightness and detail distance are also in the launcher's Shaders Patch page)");
                    }
                }
            });
            // the collision picture, projected into the view
            if let Some(camera) = data.camera {
                let screen = ctx.content_rect();
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("collision")));
                overlay(&painter, screen, &camera, data.view, contacts, velocity, body_box, others);
            }
        });
        (self.tab, self.contacts, self.velocity, self.body_box, self.others) = (tab, contacts, velocity, body_box, others);
        self.state.handle_platform_output(window, out.platform_output);
        let primitives = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                self.renderer.update_texture(device, queue, *id, delta);
            }
        }
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [size.0, size.1], pixels_per_point: out.pixels_per_point };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("debug panel") });
        self.renderer.update_buffers(device, queue, &mut encoder, &primitives, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("debug panel"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer.render(&mut pass.forget_lifetime(), &primitives, &screen);
        }
        queue.submit([encoder.finish()]);
        for id in &out.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}

fn performance(ui: &mut egui::Ui, frames: &VecDeque<f32>, data: &Data) {
    let last = frames.back().copied().unwrap_or(0.0);
    let avg = frames.iter().sum::<f32>() / frames.len().max(1) as f32;
    let max = frames.iter().copied().fold(0.0, f32::max);
    ui.label(format!("{:.0} fps   frame {last:.1} ms   avg {avg:.1} ms   worst {max:.1} ms", 1000.0 / avg.max(0.001)));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(200.0), 60.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(120));
    let top = 33.3f32;
    let n = frames.len().max(2) as f32;
    let points: Vec<egui::Pos2> = frames.iter().enumerate().map(|(i, ms)| egui::pos2(rect.left() + rect.width() * i as f32 / (n - 1.0), rect.bottom() - rect.height() * (ms / top).min(1.0))).collect();
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, egui::Color32::LIGHT_GREEN)));
    ui.label(format!("{} triangles, {} draw calls, drawn at {}x{}", data.triangles, data.draw_calls, data.render_size.0, data.render_size.1));
}

fn car(ui: &mut egui::Ui, v: &CarView) {
    ui.label(format!("{:.0} km/h   {:.0} / {:.0} rpm   gear {}", v.speed_kmh, v.rpm, v.rpm_limit, v.gear - 1));
    ui.label(format!("gas {:.2}   brake {:.2}   clutch {:.2}   steer {:.2} ({:.0}°)   handbrake {:.2}", v.gas, v.brake, v.clutch, v.steer, v.steer_deg, v.hand_brake));
    ui.label(format!("g: lat {:.2}  vert {:.2}  long {:.2}", v.acc_g[0], v.acc_g[1], v.acc_g[2]));
    ui.label(format!("TC {}   ABS {}   fuel {:.1} l   damage {:.2?}", v.tc_in_action, v.abs_in_action, v.fuel, v.damage));
    let r = &v.render;
    ui.separator();
    ui.label("tyre: slip angle / slip ratio / core temp / pressure");
    for w in 0..4 {
        ui.label(format!("{}: {:+.1}°  {:+.2}  {:.0} °C  {:.1} psi", ["FL", "FR", "RL", "RR"][w], r.slip_angle[w], r.slip_ratio[w], r.core_temps[w], r.dynamic_pressure[w]));
    }
    ui.label(format!("velocity {:.1} {:.1} {:.1} m/s", r.velocity[0], r.velocity[1], r.velocity[2]));
}

/// A world point to the screen: None behind the camera.
fn project(camera: &CameraFrame, screen: egui::Rect, p: [f32; 3]) -> Option<egui::Pos2> {
    let m = &camera.matrix;
    let d = [p[0] - m[3][0], p[1] - m[3][1], p[2] - m[3][2]];
    let dot = |r: usize| d[0] * m[r][0] + d[1] * m[r][1] + d[2] * m[r][2];
    let (x, y, z) = (dot(0), dot(1), -dot(2));
    if z < camera.near.max(0.05) {
        return None;
    }
    let t = (camera.fov.to_radians() * 0.5).tan();
    let aspect = screen.width() / screen.height().max(1.0);
    let (sx, sy) = (x / (z * t * aspect), y / (z * t));
    Some(egui::pos2(screen.center().x + sx * screen.width() * 0.5, screen.center().y - sy * screen.height() * 0.5))
}

fn line(painter: &egui::Painter, camera: &CameraFrame, screen: egui::Rect, a: [f32; 3], b: [f32; 3], stroke: egui::Stroke) {
    if let (Some(a), Some(b)) = (project(camera, screen, a), project(camera, screen, b)) {
        painter.line_segment([a, b], stroke);
    }
}

fn at(m: &Mat, local: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| m[3][i] + local[0] * m[0][i] + local[1] * m[1][i] + local[2] * m[2][i])
}

fn box_lines(painter: &egui::Painter, camera: &CameraFrame, screen: egui::Rect, body: &Mat, half: [f32; 3], stroke: egui::Stroke) {
    let corner = |i: usize| at(body, [if i & 1 == 0 { -half[0] } else { half[0] }, if i & 2 == 0 { -half[1] } else { half[1] }, if i & 4 == 0 { -half[2] } else { half[2] }]);
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                line(painter, camera, screen, corner(i), corner(i | bit), stroke);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn overlay(painter: &egui::Painter, screen: egui::Rect, camera: &CameraFrame, v: &CarView, contacts: bool, velocity: bool, body_box: bool, others: bool) {
    let green = egui::Stroke::new(1.5, egui::Color32::LIGHT_GREEN);
    if body_box {
        box_lines(painter, camera, screen, &v.body, [0.95, 0.4, 2.3], egui::Stroke::new(1.0, egui::Color32::YELLOW));
    }
    if others {
        for body in v.other_cars.iter().take(v.other_car_count as usize) {
            box_lines(painter, camera, screen, body, [0.95, 0.4, 2.3], egui::Stroke::new(1.0, egui::Color32::LIGHT_BLUE));
        }
    }
    if contacts {
        for w in 0..4 {
            let (p, n) = (v.render.contact_point[w], v.render.contact_normal[w]);
            if p == [0.0; 3] {
                continue;
            }
            let tip = [p[0] + n[0] * 0.6, p[1] + n[1] * 0.6, p[2] + n[2] * 0.6];
            line(painter, camera, screen, p, tip, egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 80, 80)));
            if let Some(c) = project(camera, screen, p) {
                painter.circle_filled(c, 3.5, if v.render.surface_is_valid_track[w] { egui::Color32::LIGHT_GREEN } else { egui::Color32::ORANGE });
            }
        }
    }
    if velocity {
        let c = [v.body[3][0], v.body[3][1], v.body[3][2]];
        let vel = v.render.velocity;
        line(painter, camera, screen, c, [c[0] + vel[0] * 0.3, c[1] + vel[1] * 0.3, c[2] + vel[2] * 0.3], green);
    }
}
