//! Launch moment (`JBIntro.swift`): a sphere of dots (a Fibonacci lattice) that breathes in faint
//! and small, swells wide and sparse, settles dense and bright, takes the accent and lifts away.
//! Always on black. Click to skip; skipped under reduce-motion.
use crate::theme::{self, Palette, W};
use eframe::egui::{self, Color32, Context, Pos2, Rect, Vec2};

pub const DURATION: f64 = 3.0;

#[derive(Default)]
pub struct Intro {
    start: Option<f64>,
    pub done: bool,
    points: Vec<[f32; 3]>,
}

fn ease(x: f64) -> f64 {
    let c = x.clamp(0.0, 1.0);
    c * c * (3.0 - 2.0 * c)
}
fn ease_in(x: f64) -> f64 {
    let c = x.clamp(0.0, 1.0);
    c * c * c
}
fn ease_back(x: f64) -> f64 {
    let c = x.clamp(0.0, 1.0);
    let s = 1.4;
    1.0 + (s + 1.0) * (c - 1.0).powi(3) + s * (c - 1.0).powi(2)
}
/// 0 -> 1 between a and b, holds, 1 -> 0 between c and d.
fn window(t: f64, a: f64, b: f64, c: f64, d: f64) -> f64 {
    if t < b { ease((t - a) / (b - a)) } else { 1.0 - ease((t - c) / (d - c)) }
}

impl Intro {
    /// Paint the intro over everything; sets `done` when finished or skipped.
    pub fn ui(&mut self, ctx: &Context, p: &Palette) {
        if self.done {
            return;
        }
        if theme::reduce_motion() {
            self.done = true;
            return;
        }
        let now = ctx.input(|i| i.time);
        let t = now - *self.start.get_or_insert(now);
        if t >= DURATION {
            self.done = true;
            return;
        }
        if self.points.is_empty() {
            let n = 900usize;
            let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
            self.points = (0..n)
                .map(|i| {
                    let y = 1.0 - (i as f64 + 0.5) / n as f64 * 2.0;
                    let r = (1.0 - y * y).sqrt();
                    let th = golden * i as f64;
                    [(th.cos() * r) as f32, y as f32, (th.sin() * r) as f32]
                })
                .collect();
        }
        let screen = ctx.screen_rect();
        let mut skip = false;
        egui::Area::new(egui::Id::new("jb-intro"))
            .order(egui::Order::Tooltip)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                skip = resp.clicked();
                self.paint(ui.painter(), rect, t, p);
            });
        if skip {
            self.done = true;
        }
        ctx.request_repaint();
    }

    fn paint(&self, painter: &egui::Painter, rect: Rect, t: f64, p: &Palette) {
        let m = rect.width().min(rect.height()) as f64;
        // The black ground fades out at the end to reveal the app.
        let ground_a = 1.0 - ease((t - 2.45) / 0.55);
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha((ground_a * 255.0) as u8));
        // Radius: faint seed -> wide breath -> dense settle -> lift away.
        let k = if t < 0.55 {
            0.16 + 0.22 * ease(t / 0.55)
        } else if t < 1.15 {
            0.38 + 0.12 * ease((t - 0.55) / 0.6)
        } else if t < 1.75 {
            0.50 - 0.20 * ease_back((t - 1.15) / 0.6)
        } else if t < 2.45 {
            0.30 - 0.015 * ((t - 1.75) / 0.7 * std::f64::consts::PI).sin()
        } else {
            0.30 + 0.9 * ease_in((t - 2.45) / 0.55)
        };
        let radius = m * k;
        let fade_in = ease((t / 0.6).min(1.0));
        let fade_out = 1.0 - ease((t - 2.5).max(0.0) / 0.5);
        let dot = (m * 0.0042 * (0.62 / k.max(0.3)).sqrt()).max(1.1);
        let spin = t * 0.9;
        let tilt = 0.38f64;
        let (cs, ss, ct, st) = (spin.cos(), spin.sin(), tilt.cos(), tilt.sin());
        let lift = if t > 1.75 { m * 0.035 * ease(((t - 1.75) / 0.4).min(1.0)) } else { 0.0 };
        let centre = Pos2::new(rect.center().x, rect.center().y - lift as f32);
        let accent_mix = ease(((t - 1.6) / 0.6).clamp(0.0, 1.0));
        for q in &self.points {
            let (px, py, pz) = (q[0] as f64, q[1] as f64, q[2] as f64);
            let x1 = px * cs + pz * ss;
            let z1 = -px * ss + pz * cs;
            let y2 = py * ct - z1 * st;
            let z2 = py * st + z1 * ct;
            let depth = (z2 + 1.0) / 2.0;
            let a = (0.18 + 0.82 * depth) * fade_in * fade_out;
            if a <= 0.02 {
                continue;
            }
            let s = dot * (0.55 + 0.6 * depth);
            let pt = Pos2::new(centre.x + (x1 * radius) as f32, centre.y + (y2 * radius) as f32);
            // Front of the sphere takes the accent; the rest stays white.
            let col = if depth > 0.6 && accent_mix > 0.5 {
                theme::with_alpha(p.accent, ((a * 1.15).min(1.0)) as f32)
            } else {
                Color32::from_white_alpha((a * 255.0) as u8)
            };
            painter.circle_filled(pt, (s / 2.0) as f32, col);
        }
        let la = window(t, 1.75, 2.15, 2.5, 2.85) as f32;
        if la > 0.01 {
            painter.text(
                Pos2::new(rect.center().x, rect.center().y + (m * 0.30 + 30.0) as f32),
                egui::Align2::CENTER_CENTER,
                "JoeBro",
                theme::jb(30.0, W::Semibold),
                Color32::from_white_alpha((la * 255.0) as u8),
            );
        }
        let _ = Vec2::ZERO;
    }
}
