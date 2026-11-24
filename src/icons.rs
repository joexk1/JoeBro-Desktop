//! Icons: SF Symbol names -> Phosphor (regular) via the bundled icon font, plus
//! the 11 custom `JBGlyph`s drawn with the painter on the Mac's 16-unit grid
//! (1.5 stroke, round caps/joins) — a port of `JBIcon` / `JBSymbol`.
//!
//! ```ignore
//! icons::symbol(ui, "gearshape", 14.0, p.ink2);        // JBSymbol: glyph or font icon
//! ui.label(icons::sf_text("plus", 14.0).color(p.ink)); // icon inside a label/button
//! let s: &str = icons::sf("trash");                     // raw private-use char
//! icons::glyph(ui, JbGlyph::Memory, 16.0, p.ink2);      // JBIcon
//! ```
//! Unknown names fall back to a small circle (and `debug_assert`-free: never panics).
#![allow(dead_code)]
use crate::theme;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui, Vec2};
use egui_phosphor::{fill as pf, regular as pr};

/// Concepts that get a JoeBro glyph instead of an Apple-identifiable symbol.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JbGlyph {
    Agent,
    Spark,
    Memory,
    NightShift,
    Skills,
    Tools,
    Coding,
    Research,
    Pulse,
    Plugin,
    Tasks,
}

impl JbGlyph {
    pub const ALL: [JbGlyph; 11] = [
        JbGlyph::Agent,
        JbGlyph::Spark,
        JbGlyph::Memory,
        JbGlyph::NightShift,
        JbGlyph::Skills,
        JbGlyph::Tools,
        JbGlyph::Coding,
        JbGlyph::Research,
        JbGlyph::Pulse,
        JbGlyph::Plugin,
        JbGlyph::Tasks,
    ];
}

/// SF names that `JBSymbol` swaps for a JoeBro glyph.
pub fn glyph_for(name: &str) -> Option<JbGlyph> {
    Some(match name {
        "sparkles" | "wand.and.stars" | "sparkle" => JbGlyph::Spark,
        "brain" | "brain.head.profile" => JbGlyph::Memory,
        "moon.zzz" | "moon.stars" => JbGlyph::NightShift,
        "graduationcap" => JbGlyph::Skills,
        "hammer" | "wrench.and.screwdriver" | "cpu" => JbGlyph::Tools,
        "terminal" => JbGlyph::Coding,
        "puzzlepiece" | "puzzlepiece.extension" => JbGlyph::Plugin,
        "checklist" | "clock.badge.checkmark" => JbGlyph::Tasks,
        "person.3.sequence" | "person.crop.circle" | "agent" => JbGlyph::Agent,
        "antenna.radiowaves.left.and.right" | "sparkle.magnifyingglass" => JbGlyph::Research,
        "waveform.path.ecg" | "pulse" => JbGlyph::Pulse,
        _ => return None,
    })
}

/// Raw icon char for an SF Symbol name (Phosphor regular; `.fill` names use
/// the fill face when one exists — pair with `sf_text`/`symbol`, which pick
/// the right family). Glyph-mapped names return a close Phosphor stand-in.
pub fn sf(name: &str) -> &'static str {
    lookup(name).0
}

/// `(char, is_fill)`.
fn lookup(name: &str) -> (&'static str, bool) {
    if let Some(f) = fill_of(name) {
        return (f, true);
    }
    (regular_of(name), false)
}

fn fill_of(name: &str) -> Option<&'static str> {
    Some(match name {
        "star.fill" => pf::STAR,
        "pin.fill" => pf::PUSH_PIN,
        "play.fill" => pf::PLAY,
        "stop.fill" => pf::STOP,
        "mic.fill" => pf::MICROPHONE,
        "lock.fill" => pf::LOCK_SIMPLE,
        "checkmark.circle.fill" => pf::CHECK_CIRCLE,
        "checkmark.seal.fill" => pf::SEAL_CHECK,
        "xmark.circle.fill" => pf::X_CIRCLE,
        "exclamationmark.triangle.fill" => pf::WARNING,
        "circle.fill" => pf::CIRCLE,
        "heart.fill" => pf::HEART,
        "bolt.fill" => pf::LIGHTNING,
        "folder.fill" => pf::FOLDER_SIMPLE,
        "flag.fill" => pf::FLAG,
        "bookmark.fill" => pf::BOOKMARK_SIMPLE,
        "bell.fill" => pf::BELL,
        _ => return None,
    })
}

fn regular_of(name: &str) -> &'static str {
    match name {
        // arrows
        "arrow.clockwise" => pr::ARROW_CLOCKWISE,
        "arrow.counterclockwise" => pr::ARROW_COUNTER_CLOCKWISE,
        "arrow.down" => pr::ARROW_DOWN,
        "arrow.up" => pr::ARROW_UP,
        "arrow.left" => pr::ARROW_LEFT,
        "arrow.right" => pr::ARROW_RIGHT,
        "arrow.up.right" => pr::ARROW_UP_RIGHT,
        "arrow.down.right.and.arrow.up.left" => pr::ARROWS_IN_SIMPLE,
        "arrow.up.left.and.arrow.down.right" => pr::ARROWS_OUT_SIMPLE,
        "arrow.right.circle" => pr::ARROW_CIRCLE_RIGHT,
        "arrow.down.circle" => pr::ARROW_CIRCLE_DOWN,
        "arrowshape.turn.up.left" => pr::ARROW_BEND_UP_LEFT,
        "arrowshape.turn.up.left.2" => pr::ARROW_BEND_DOUBLE_UP_LEFT,
        "arrowshape.turn.up.right" => pr::ARROW_BEND_UP_RIGHT,
        "arrow.triangle.branch" => pr::GIT_BRANCH,
        "arrow.triangle.2.circlepath" => pr::ARROWS_CLOCKWISE,
        "arrow.up.doc" => pr::FILE_ARROW_UP,
        "arrow.up.forward.app" => pr::ARROW_SQUARE_OUT,
        "arrow.uturn.forward" => pr::ARROW_U_UP_RIGHT,
        "arrow.uturn.backward" => pr::ARROW_U_UP_LEFT,
        "square.and.arrow.up" => pr::EXPORT,
        "square.and.arrow.down" => pr::DOWNLOAD_SIMPLE,
        "shuffle" => pr::SHUFFLE,
        "switch.2" => pr::ARROWS_LEFT_RIGHT,
        // chevrons / marks
        "chevron.down" => pr::CARET_DOWN,
        "chevron.up" => pr::CARET_UP,
        "chevron.left" => pr::CARET_LEFT,
        "chevron.right" => pr::CARET_RIGHT,
        "chevron.up.chevron.down" => pr::CARET_UP_DOWN,
        "chevron.left.forwardslash.chevron.right" => pr::CODE,
        "checkmark" => pr::CHECK,
        "checkmark.circle" => pr::CHECK_CIRCLE,
        "checkmark.seal" => pr::SEAL_CHECK,
        "checkmark.shield" => pr::SHIELD_CHECK,
        "xmark" => pr::X,
        "xmark.circle" => pr::X_CIRCLE,
        "plus" => pr::PLUS,
        "plus.circle" => pr::PLUS_CIRCLE,
        "minus" => pr::MINUS,
        "ellipsis" => pr::DOTS_THREE,
        "ellipsis.circle" => pr::DOTS_THREE_CIRCLE,
        "circle" => pr::CIRCLE,
        "circle.lefthalf.filled" => pr::CIRCLE_HALF,
        "line.3.horizontal" => pr::LIST,
        // docs / files
        "doc" => pr::FILE,
        "doc.text" | "doc.richtext" => pr::FILE_TEXT,
        "doc.badge.plus" => pr::FILE_PLUS,
        "doc.on.doc" | "copy" => pr::COPY,
        "doc.on.clipboard" => pr::CLIPBOARD,
        "folder" => pr::FOLDER_SIMPLE,
        "folder.badge.plus" => pr::FOLDER_SIMPLE_PLUS,
        "archivebox" => pr::ARCHIVE,
        "photo" => pr::IMAGE,
        "paperclip" => pr::PAPERCLIP,
        "note.text" => pr::NOTE,
        "newspaper" => pr::NEWSPAPER,
        "books.vertical" => pr::BOOKS,
        "tray" | "tray.full" => pr::TRAY,
        // comms
        "envelope" | "envelope.badge" => pr::ENVELOPE_SIMPLE,
        "envelope.open" => pr::ENVELOPE_SIMPLE_OPEN,
        "bubble.left" | "bubble.right" => pr::CHAT,
        "bubble.left.and.bubble.right" => pr::CHATS,
        "paperplane" => pr::PAPER_PLANE_TILT,
        "bell" => pr::BELL,
        "alarm" => pr::ALARM,
        "mic" => pr::MICROPHONE,
        "speaker.wave.2" => pr::SPEAKER_HIGH,
        // edit
        "pencil" => pr::PENCIL_SIMPLE,
        "pencil.line" => pr::PENCIL_SIMPLE_LINE,
        "square.and.pencil" => pr::NOTE_PENCIL,
        "trash" => pr::TRASH,
        "scissors" => pr::SCISSORS,
        "text.append" => pr::TEXT_ALIGN_LEFT,
        "text.quote" => pr::QUOTES,
        "textformat" => pr::TEXT_T,
        "textformat.abc.dottedunderline" => pr::TEXT_AA,
        "list.bullet" => pr::LIST_BULLETS,
        "list.number" => pr::LIST_NUMBERS,
        "bold" => pr::TEXT_B,
        "italic" => pr::TEXT_ITALIC,
        "number" => pr::HASH,
        "at" => pr::AT,
        // ui / system
        "gearshape" | "settings" => pr::GEAR,
        "magnifyingglass" => pr::MAGNIFYING_GLASS,
        "sidebar.left" | "sidebar.trailing" => pr::SIDEBAR_SIMPLE,
        "square.grid.2x2" => pr::SQUARES_FOUR,
        "macwindow.on.rectangle" => pr::BROWSERS,
        "rectangle.on.rectangle.slash" => pr::SQUARE,
        "slider.horizontal.3" => pr::SLIDERS_HORIZONTAL,
        "palette" => pr::PALETTE,
        "paintbrush" => pr::PAINT_BRUSH,
        "lock" => pr::LOCK_SIMPLE,
        "lock.open" => pr::LOCK_SIMPLE_OPEN,
        "key" => pr::KEY,
        "shield" => pr::SHIELD,
        "eye" => pr::EYE,
        "eye.slash" => pr::EYE_SLASH,
        "link" => pr::LINK,
        "power" => pr::POWER,
        "pin" | "pin.fill" => pr::PUSH_PIN,
        "pin.slash" => pr::PUSH_PIN_SLASH,
        "star" => pr::STAR,
        "heart" => pr::HEART,
        "flag" => pr::FLAG,
        "bookmark" => pr::BOOKMARK_SIMPLE,
        "tag" => pr::TAG,
        "lightbulb" => pr::LIGHTBULB,
        "info.circle" => pr::INFO,
        "questionmark.circle" => pr::QUESTION,
        "exclamationmark.triangle" => pr::WARNING,
        "exclamationmark.circle" => pr::WARNING_CIRCLE,
        // time / places / things
        "calendar" => pr::CALENDAR_BLANK,
        "calendar.badge.plus" => pr::CALENDAR_PLUS,
        "clock" => pr::CLOCK,
        "timer" => pr::TIMER,
        "hourglass" => pr::HOURGLASS,
        "moon" => pr::MOON,
        "sun.max" => pr::SUN,
        "globe" => pr::GLOBE,
        "mappin" => pr::MAP_PIN,
        "house" => pr::HOUSE,
        "person" => pr::USER,
        "person.2" => pr::USERS,
        "hand.raised" => pr::HAND,
        "iphone" => pr::DEVICE_MOBILE,
        "laptopcomputer" => pr::LAPTOP,
        "externaldrive" | "internaldrive" => pr::HARD_DRIVE,
        "wifi" => pr::WIFI_HIGH,
        "apple.logo" => pr::APPLE_LOGO,
        "camera" => pr::CAMERA,
        "play" => pr::PLAY,
        "stop" => pr::STOP,
        "pause" => pr::PAUSE,
        "chart.bar" => pr::CHART_BAR,
        "wrench.adjustable" => pr::WRENCH,
        "flame" => pr::FLAME,
        "leaf" => pr::LEAF,
        "face.smiling" => pr::SMILEY,
        "bolt" => pr::LIGHTNING,
        // glyph-mapped names (stand-ins when drawn as text)
        "sparkles" | "wand.and.stars" | "sparkle" => pr::SPARKLE,
        "brain" | "brain.head.profile" => pr::BRAIN,
        "moon.zzz" | "moon.stars" => pr::MOON_STARS,
        "graduationcap" => pr::GRADUATION_CAP,
        "hammer" | "wrench.and.screwdriver" | "cpu" => pr::HAMMER,
        "terminal" => pr::TERMINAL_WINDOW,
        "puzzlepiece" | "puzzlepiece.extension" => pr::PUZZLE_PIECE,
        "checklist" | "clock.badge.checkmark" => pr::LIST_CHECKS,
        "person.3.sequence" | "person.crop.circle" | "agent" => pr::USER_CIRCLE,
        "antenna.radiowaves.left.and.right" | "sparkle.magnifyingglass" => pr::BROADCAST,
        "waveform.path.ecg" | "pulse" => pr::HEARTBEAT,
        _ => pr::CIRCLE,
    }
}

/// An icon as `RichText` in the right icon family: use in labels, buttons, menus.
pub fn sf_text(name: &str, size: f32) -> RichText {
    let (c, fill) = lookup(name);
    RichText::new(c).font(theme::icon_font_id(size, fill))
}

/// Paint an SF-named icon (or its JB glyph) centred at `center` (`JBSymbol`).
pub fn paint_symbol(painter: &Painter, center: Pos2, name: &str, size: f32, color: Color32) {
    if let Some(g) = glyph_for(name) {
        paint_glyph(painter, center, g, size, color);
    } else {
        let (c, fill) = lookup(name);
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            c,
            theme::icon_font_id((size - 1.0).max(8.0) * 1.05, fill),
            color,
        );
    }
}

/// `JBSymbol(name:size:)` as a widget: allocates `size` x `size`.
pub fn symbol(ui: &mut Ui, name: &str, size: f32, color: Color32) -> Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(size.max(14.0)), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_symbol(ui.painter(), rect.center(), name, size, color);
    }
    r
}

/// `JBIcon(glyph, size:)` as a widget.
pub fn glyph(ui: &mut Ui, g: JbGlyph, size: f32, color: Color32) -> Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_glyph(ui.painter(), rect.center(), g, size, color);
    }
    r
}

/// Draw a `JbGlyph` centred at `center` on a 16-unit grid scaled to `size`, 1.5 stroke.
pub fn paint_glyph(painter: &Painter, center: Pos2, g: JbGlyph, size: f32, color: Color32) {
    paint_glyph_w(painter, center, g, size, color, 1.5)
}

pub fn paint_glyph_w(painter: &Painter, center: Pos2, g: JbGlyph, size: f32, color: Color32, line: f32) {
    let k = size / 16.0;
    let o = center - Vec2::splat(size / 2.0);
    let w = line * k;
    let p = |x: f32, y: f32| Pos2::new(o.x + x * k, o.y + y * k);
    let stroke = Stroke::new(w, color);
    // Round-capped polyline.
    let line_path = |pts: &[(f32, f32)], closed: bool, sw: f32| {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| p(*x, *y)).collect();
        let st = Stroke::new(sw, color);
        let n = v.len();
        let segs = if closed { n } else { n - 1 };
        for i in 0..segs {
            painter.line_segment([v[i], v[(i + 1) % n]], st);
        }
        let caps: Box<dyn Iterator<Item = &Pos2>> = if closed { Box::new(v.iter()) } else { Box::new(v.iter()) };
        for q in caps {
            painter.circle_filled(*q, sw / 2.0, color);
        }
    };
    let rr = |x: f32, y: f32, wd: f32, ht: f32, r: f32| {
        painter.rect_stroke(
            Rect::from_min_size(p(x, y), Vec2::new(wd * k, ht * k)),
            egui::Rounding::same(r * k),
            stroke,
        );
    };
    let dot = |x: f32, y: f32, r: f32| {
        painter.circle_filled(p(x, y), r * k, color);
    };
    match g {
        JbGlyph::Agent => {
            rr(2.5, 3.0, 11.0, 10.0, 3.0);
            dot(6.0, 7.6, 1.1);
            dot(10.0, 7.6, 1.1);
            line_path(&[(6.5, 10.4), (9.5, 10.4)], false, w);
        }
        JbGlyph::Spark => {
            line_path(&[(8.0, 2.5), (8.0, 13.5)], false, w);
            line_path(&[(3.2, 5.25), (12.8, 10.75)], false, w);
            line_path(&[(3.2, 10.75), (12.8, 5.25)], false, w);
        }
        JbGlyph::Memory => {
            line_path(&[(4.0, 11.5), (8.0, 4.5), (12.0, 11.5)], true, w * 0.8);
            for (x, y) in [(4.0, 11.5), (8.0, 4.5), (12.0, 11.5)] {
                dot(x, y, 2.0);
            }
        }
        JbGlyph::NightShift => {
            // Big circle minus an offset circle, outlined.
            let (c1, r1) = ((7.6f32, 8.6f32), 5.4f32);
            let (c2, r2) = ((10.4f32, 5.9f32), 4.3f32);
            let (dx, dy) = (c2.0 - c1.0, c2.1 - c1.1);
            let d = (dx * dx + dy * dy).sqrt();
            let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
            let h = (r1 * r1 - a * a).max(0.0).sqrt();
            let (mx, my) = (c1.0 + a * dx / d, c1.1 + a * dy / d);
            let i1 = (mx + h * dy / d, my - h * dx / d);
            let i2 = (mx - h * dy / d, my + h * dx / d);
            let ang = |c: (f32, f32), q: (f32, f32)| (q.1 - c.1).atan2(q.0 - c.0);
            let arc = |c: (f32, f32), r: f32, a0: f32, a1: f32, keep: &dyn Fn((f32, f32)) -> bool| -> Vec<(f32, f32)> {
                let tau = std::f32::consts::TAU;
                for dir in [1.0f32, -1.0] {
                    let mut span = (a1 - a0) * dir;
                    while span < 0.0 {
                        span += tau;
                    }
                    let mid = a0 + dir * span / 2.0;
                    if keep((c.0 + r * mid.cos(), c.1 + r * mid.sin())) {
                        return (0..=24)
                            .map(|i| {
                                let t = a0 + dir * span * i as f32 / 24.0;
                                (c.0 + r * t.cos(), c.1 + r * t.sin())
                            })
                            .collect();
                    }
                }
                vec![]
            };
            let dist = |a: (f32, f32), b: (f32, f32)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
            let mut pts = arc(c1, r1, ang(c1, i1), ang(c1, i2), &|q| dist(q, c2) > r2);
            pts.extend(arc(c2, r2, ang(c2, i2), ang(c2, i1), &|q| dist(q, c1) < r1));
            line_path(&pts, true, w);
            dot(13.4, 2.8, 0.9);
        }
        JbGlyph::Skills => {
            line_path(&[(8.0, 2.8), (13.5, 5.8), (8.0, 8.8), (2.5, 5.8)], true, w);
            line_path(&[(2.5, 9.0), (8.0, 12.0), (13.5, 9.0)], false, w);
        }
        JbGlyph::Tools => {
            line_path(&[(3.5, 12.5), (10.0, 6.0)], false, w);
            line_path(&[(12.5, 12.5), (6.0, 6.0)], false, w);
            rr(9.0, 2.5, 4.5, 4.5, 1.0);
            rr(2.5, 2.5, 4.5, 4.5, 1.0);
        }
        JbGlyph::Coding => {
            line_path(&[(3.0, 4.5), (6.5, 8.0), (3.0, 11.5)], false, w);
            line_path(&[(8.5, 11.5), (13.0, 11.5)], false, w);
        }
        JbGlyph::Research => {
            painter.circle_stroke(p(6.75, 6.75), 4.25 * k, stroke);
            line_path(&[(10.0, 10.0), (13.5, 13.5)], false, w);
            dot(6.75, 6.75, 1.2);
        }
        JbGlyph::Pulse => {
            line_path(&[(1.8, 8.5), (5.0, 8.5), (6.8, 4.0), (9.2, 12.5), (11.0, 8.5), (14.2, 8.5)], false, w);
        }
        JbGlyph::Plugin => {
            rr(3.0, 6.0, 10.0, 7.5, 2.0);
            line_path(&[(6.0, 2.5), (6.0, 6.0)], false, w);
            line_path(&[(10.0, 2.5), (10.0, 6.0)], false, w);
        }
        JbGlyph::Tasks => {
            rr(2.5, 2.5, 11.0, 11.0, 2.5);
            line_path(&[(5.5, 8.2), (7.3, 10.0), (10.8, 6.2)], false, w);
        }
    }
    let _ = Shape::Noop;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every SF Symbol name used by the Mac app's panels/tabs must resolve to a real icon.
    #[test]
    fn every_mac_symbol_resolves() {
        let names = [
            "arrow.clockwise", "arrow.counterclockwise", "arrow.down", "arrow.down.right.and.arrow.up.left", "arrow.right.circle",
            "arrow.triangle.branch", "arrow.up", "arrow.up.doc", "arrow.up.forward.app", "arrow.uturn.forward", "arrowshape.turn.up.left",
            "arrowshape.turn.up.left.2", "arrowshape.turn.up.right", "bell", "books.vertical", "bubble.left", "bubble.left.and.bubble.right",
            "calendar", "checkmark", "checkmark.circle.fill", "checkmark.seal", "checkmark.seal.fill", "checkmark.shield", "chevron.down",
            "chevron.left", "chevron.left.forwardslash.chevron.right", "chevron.right", "chevron.up", "circle.lefthalf.filled", "doc",
            "doc.badge.plus", "doc.on.doc", "doc.richtext", "doc.text", "ellipsis", "envelope", "envelope.badge", "envelope.open",
            "exclamationmark.triangle", "exclamationmark.triangle.fill", "eye", "folder", "folder.badge.plus", "folder.fill", "gearshape",
            "iphone", "key", "laptopcomputer", "lightbulb", "link", "list.bullet", "list.number", "lock", "macwindow.on.rectangle",
            "magnifyingglass", "mic.fill", "minus", "moon", "newspaper", "note.text", "palette", "paperclip", "paperplane", "pencil",
            "pencil.line", "photo", "pin.fill", "pin.slash", "play", "play.fill", "plus", "plus.circle", "power",
            "rectangle.on.rectangle.slash", "shuffle", "sidebar.left", "sidebar.trailing", "square.and.pencil", "square.grid.2x2",
            "star.fill", "stop.fill", "switch.2", "text.append", "text.quote", "textformat.abc.dottedunderline", "trash", "tray",
            "waveform.path.ecg", "wrench.adjustable", "xmark", "xmark.circle.fill", "alarm", "apple.logo", "heart",
        ];
        for n in names {
            assert!(
                glyph_for(n).is_some() || fill_of(n).is_some() || regular_of(n) != pr::CIRCLE,
                "no icon mapped for SF symbol {n}"
            );
        }
        for t in crate::state::Tab::ALL {
            let n = t.icon();
            assert!(glyph_for(n).is_some() || regular_of(n) != pr::CIRCLE, "tab icon {n}");
        }
    }

    #[test]
    fn eleven_glyphs() {
        assert_eq!(JbGlyph::ALL.len(), 11);
    }
}
