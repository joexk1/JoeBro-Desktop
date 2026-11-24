//! JoeBro's ASCII / dither identity kit — a port of `JBAscii.swift`: spinner,
//! scramble text, stream cursor, JoeBro's face, wordmark, dot number, the
//! dithered backdrop and the empty state. Everything respects reduce-motion
//! (`theme::reduce_motion()`) and only requests repaints while on screen.
#![allow(dead_code)]
use crate::icons;
use crate::theme::{self, Palette, JbPalette, W};
use eframe::egui::{self, epaint::PathShape, Color32, Context, FontId, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui, Vec2};

fn now(ctx: &Context) -> f64 {
    ctx.input(|i| i.time)
}

// ---- spinner -------------------------------------------------------------

/// A thin rotating arc for anything the agent is doing (`JBSpinner`).
pub fn spinner(ui: &mut Ui, size: f32, tint: Color32) -> Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return r;
    }
    let t = now(ui.ctx());
    let spin = if theme::reduce_motion() { 0.0 } else { (t / 0.8).fract() as f32 * std::f32::consts::TAU };
    let w = (size * 0.13).max(1.3);
    let rad = (size - w) / 2.0;
    let (a0, a1) = (0.12 * std::f32::consts::TAU, 0.78 * std::f32::consts::TAU);
    let pts: Vec<Pos2> = (0..=28)
        .map(|i| {
            let a = spin + a0 + (a1 - a0) * i as f32 / 28.0;
            rect.center() + Vec2::new(a.cos(), a.sin()) * rad
        })
        .collect();
    ui.painter().add(Shape::Path(PathShape::line(pts.clone(), Stroke::new(w, tint))));
    for q in [pts[0], pts[28]] {
        ui.painter().circle_filled(q, w / 2.0, tint);
    }
    if !theme::reduce_motion() {
        ui.ctx().request_repaint();
    }
    r
}

// ---- scramble text ---------------------------------------------------------

/// Text that shimmers while `active` (thinking labels); otherwise plain
/// (`JBScrambleText`). The first letter is capitalised like the Mac.
pub fn scramble_text(ui: &mut Ui, p: &Palette, text: &str, active: bool, font: FontId, tint: Color32) {
    let mut label = text.to_string();
    if let Some(f) = label.get(0..1) {
        label = f.to_uppercase() + &label[1..];
    }
    if !active || theme::reduce_motion() {
        ui.label(RichText::new(label).font(font).color(tint));
        return;
    }
    let t = now(ui.ctx());
    let n = label.chars().count().max(1) as f64;
    // A soft light sweeps across the label...
    let sweep = (t % 1.8) / 1.8 * 1.45 - 0.45;
    let mut job = egui::text::LayoutJob::default();
    for (i, ch) in label.chars().enumerate() {
        let x = i as f64 / n;
        let near = (1.0 - ((x - sweep).abs() / 0.22)).clamp(0.0, 1.0) as f32;
        let a = 0.55 + 0.45 * near;
        job.append(
            &ch.to_string(),
            0.0,
            egui::TextFormat { font_id: font.clone(), color: theme::with_alpha(tint, a), ..Default::default() },
        );
    }
    // ...and a three-cell dither trail breathes after it.
    job.append("  ", 0.0, egui::TextFormat { font_id: font.clone(), ..Default::default() });
    for i in 0..3 {
        let v = (0.5 + 0.5 * (t * 5.0 - i as f64 * 0.9).sin()) as f32;
        let ch = ["\u{2591}", "\u{2592}", "\u{2593}"][((v * 3.0) as usize).min(2)];
        job.append(
            ch,
            1.0,
            egui::TextFormat { font_id: theme::mono(10.0, W::Regular), color: theme::with_alpha(p.accent, 0.35 + 0.5 * v), ..Default::default() },
        );
    }
    ui.label(job);
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
}

// ---- stream cursor ---------------------------------------------------------

/// The block cursor at the tail of streaming text (`JBStreamCursor`).
pub fn stream_cursor(ui: &mut Ui, tint: Color32, height: f32) -> Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(height * 0.5, height), Sense::hover());
    let a = if theme::reduce_motion() {
        1.0
    } else {
        // easeInOut(0.55) autoreversing between 1 and 0.15
        let ph = ((now(ui.ctx()) / 0.55) % 2.0) as f32;
        let tri = if ph < 1.0 { ph } else { 2.0 - ph };
        let e = tri * tri * (3.0 - 2.0 * tri);
        1.0 - 0.85 * e
    };
    ui.painter().rect_filled(rect, 1.0, theme::with_alpha(tint, a));
    if !theme::reduce_motion() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }
    r
}

// ---- face ------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Idle,
    Thinking,
    Talking,
    Happy,
    Sleeping,
    Alert,
}

/// Four-line frame. Pure function of mood and time (`JBFace.frame`).
pub fn face_frame(mood: Mood, t: f64) -> [String; 4] {
    let tick = (t / 0.12) as i64;
    let blink = tick.rem_euclid(30) >= 28;
    let glance = (tick / 40).rem_euclid(4);
    let (mut eye_l, mut eye_r, mut mouth) = ("o", "o", "-");
    let mut pad = (2usize, 2usize);
    let mut extra = "   ";
    match mood {
        Mood::Idle => {
            if glance == 1 {
                pad = (1, 3);
            } else if glance == 3 {
                pad = (3, 1);
            }
        }
        Mood::Thinking => {
            eye_l = "\u{b0}";
            eye_r = "\u{b0}";
            mouth = "~";
            pad = (3, 1);
            extra = ["   ", ".  ", ".. ", "..."][(tick / 3).rem_euclid(4) as usize];
        }
        Mood::Talking => mouth = ["-", "o", "O", "o", "-", "o"][tick.rem_euclid(6) as usize],
        Mood::Happy => {
            eye_l = "^";
            eye_r = "^";
            mouth = "w";
        }
        Mood::Sleeping => {
            eye_l = "-";
            eye_r = "-";
            mouth = "_";
            extra = [" z ", " zZ", "zZ "][(tick / 6).rem_euclid(3) as usize];
        }
        Mood::Alert => {
            eye_l = "O";
            eye_r = "O";
            mouth = "o";
        }
    }
    if blink && mood != Mood::Sleeping && mood != Mood::Happy {
        eye_l = "-";
        eye_r = "-";
    }
    let eyes = format!("{}{} {}{}", " ".repeat(pad.0), eye_l, eye_r, " ".repeat(pad.1));
    [
        format!("\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}{extra}"),
        format!("\u{2502}{eyes}\u{2502}"),
        format!("\u{2502}   {mouth}   \u{2502}"),
        "\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}".to_string(),
    ]
}

/// JoeBro's ASCII face: blinks, glances, talks and sleeps (`JBFace`).
pub fn face(ui: &mut Ui, mood: Mood, size: f32, tint: Color32) -> Response {
    let t = if theme::reduce_motion() { 0.0 } else { now(ui.ctx()) };
    let lines = face_frame(mood, t);
    let r = ui
        .vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for l in &lines {
                ui.add(egui::Label::new(RichText::new(l).font(theme::mono(size, W::Medium)).color(tint)).wrap_mode(egui::TextWrapMode::Extend));
            }
        })
        .response;
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(if theme::reduce_motion() { 10_000 } else { 120 }));
    r
}

// ---- wordmark / numbers ------------------------------------------------------

/// "JoeBro" wordmark: Geist semibold at dot*10, tracking -dot*0.35 (`JBWordmark`).
pub fn wordmark(ui: &mut Ui, text: &str, dot: f32, tint: Color32) -> Response {
    ui.label(RichText::new(text).font(theme::jb(dot * 10.0, W::Semibold)).extra_letter_spacing(-dot * 0.35).color(tint))
}

/// Counter numerals (`JBDotNumber`).
pub fn dot_number(ui: &mut Ui, value: &str, dot: f32, tint: Color32) -> Response {
    ui.label(RichText::new(value).font(theme::jb((dot * 6.5).max(12.0), W::Semibold)).color(tint))
}

// ---- dithered backdrop ---------------------------------------------------------

const CELL: f32 = 3.0; // logical px per dither pixel

/// Build the ordered-dither glow bitmap (accent bleeding in from the
/// bottom-right corner), a port of `JBDitherCache.image`.
fn dither_image(w: usize, h: usize, palette: JbPalette, dark: bool) -> egui::ColorImage {
    let (l, d) = palette.hex();
    let hexv = if dark { d } else { l };
    let (r, g, b) = (((hexv >> 16) & 0xff) as f64, ((hexv >> 8) & 0xff) as f64, (hexv & 0xff) as f64);
    let peak = if dark { 0.30 } else { 0.20 };
    let bayer: Vec<f64> = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5].iter().map(|v| (*v as f64 + 0.5) / 16.0).collect();
    let mut px = vec![Color32::TRANSPARENT; w * h];
    let (cx, cy) = (w as f64 * 1.02, h as f64 * 1.04);
    let radius = w.max(h) as f64 * 0.78;
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f64 - cx) / radius;
            let dy = (y as f64 - cy) / (radius * 0.82);
            let dist = (dx * dx + dy * dy).sqrt();
            let v = (1.0 - dist).max(0.0);
            let intensity = v * v * (3.0 - 2.0 * v);
            if intensity <= bayer[(y % 4) * 4 + (x % 4)] * 0.98 {
                continue;
            }
            let a = peak * (0.35 + 0.65 * intensity);
            px[y * w + x] = Color32::from_rgba_premultiplied((r * a) as u8, (g * a) as u8, (b * a) as u8, (255.0 * a) as u8);
        }
    }
    egui::ColorImage { size: [w, h], pixels: px }
}

/// Paint the default backdrop into `rect` on the background layer: flat
/// ground + a 1-bit dithered accent glow (`JBDitherBackdrop`). Mono has no glow.
pub fn dither_backdrop(ctx: &Context, p: &Palette, rect: Rect) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.rect_filled(rect, 0.0, p.ground);
    if p.palette == JbPalette::Mono || rect.width() < 2.0 || rect.height() < 2.0 {
        return;
    }
    // Bucket to 64 pt so live resizing reuses bitmaps.
    let bw = (rect.width() / 64.0).ceil() as usize * 64;
    let bh = (rect.height() / 64.0).ceil() as usize * 64;
    let key = egui::Id::new(("jb-dither", p.palette.id(), p.dark, bw, bh));
    let tex: egui::TextureHandle = match ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        Some(t) => t,
        None => {
            let img = dither_image(((bw as f32 / CELL) as usize).max(1), ((bh as f32 / CELL) as usize).max(1), p.palette, p.dark);
            let t = ctx.load_texture("jb-dither", img, egui::TextureOptions::NEAREST);
            ctx.data_mut(|d| d.insert_temp(key, t.clone()));
            t
        }
    };
    painter.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
}

// ---- empty state -------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Art {
    Inbox,
    Search,
    Done,
    Sleep,
    Plug,
    Files,
}

impl Art {
    pub fn symbol(self) -> &'static str {
        match self {
            Art::Inbox => "tray",
            Art::Search => "magnifyingglass",
            Art::Done => "checkmark",
            Art::Sleep => "moon",
            Art::Plug => "link",
            Art::Files => "doc.text",
        }
    }
    /// The ASCII drawing for each art (used by onboarding/marketing surfaces).
    pub fn drawing(self) -> &'static str {
        match self {
            Art::Inbox => "\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}\n\u{2502}            \u{2502}\n\u{251c}\u{2500}\u{2500}\u{256e}      \u{256d}\u{2500}\u{2500}\u{2524}\n\u{2502}  \u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}  \u{2502}\n\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}",
            Art::Search => "  \u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}\n  \u{2502} \u{b7}\u{b7} \u{2502}\n  \u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}\\\n         \\\n          \u{b7}",
            Art::Done => "\u{256d}\u{2500}\u{2500}\u{2500}\u{256e}\n\u{2502} \u{2022} \u{2502}\n\u{2570}\u{2500}\u{2500}\u{2500}\u{256f}",
            Art::Sleep => "      z Z\n\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}\n\u{2502} -   - \u{2502}\n\u{2502}   _   \u{2502}\n\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}",
            Art::Plug => "  \u{2502}   \u{2502}\n\u{256d}\u{2500}\u{2534}\u{2500}\u{2500}\u{2500}\u{2534}\u{2500}\u{256e}\n\u{2502}       \u{2502}\n\u{2570}\u{2500}\u{2500}\u{2500}\u{252c}\u{2500}\u{2500}\u{2500}\u{256f}\n    \u{2502}",
            Art::Files => "\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}\u{2500}\u{2500}\u{256e}\n\u{2502} \u{2500}\u{2500}\u{2500}\u{2500} \u{2502}  \u{2502}\n\u{2502} \u{2500}\u{2500}\u{2500}\u{2500} \u{2502}  \u{2502}\n\u{2502} \u{2500}\u{2500}   \u{2502}\u{2500}\u{2500}\u{256f}\n\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}",
        }
    }
}

/// Just the circled icon, for empty states that lay out their own text (`JBEmptyState.icon`).
pub fn empty_icon(ui: &mut Ui, p: &Palette, art: Art) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 22.0, p.raised);
    ui.painter().circle_stroke(rect.center(), 21.5, Stroke::new(1.0, p.line));
    icons::paint_symbol(ui.painter(), rect.center(), art.symbol(), 17.0, p.ink3);
}

/// One small icon + one line, centred in the available space (`JBEmptyState`). No subtitles.
pub fn empty_state(ui: &mut Ui, p: &Palette, art: Art, text: &str) {
    let avail = ui.available_size();
    ui.allocate_ui_with_layout(
        Vec2::new(avail.x, avail.y.max(120.0)),
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(((avail.y - 90.0) / 2.0).max(8.0));
                empty_icon(ui, p, art);
                ui.add_space(12.0);
                ui.label(RichText::new(text).font(theme::jb(14.0, W::Regular)).color(p.ink2));
            });
        },
    );
}
