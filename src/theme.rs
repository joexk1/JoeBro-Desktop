#![allow(dead_code)]
//! JoeBro design tokens: a port of `JBDesign.swift` (`JB.*`, `JBPalette`,
//! `JBAppearance`, `Font.jb*`). Ink on paper: flat surfaces, 1 px hairlines,
//! Geist + Geist Mono, one accent from the chosen palette, a "live" colour
//! for anything running.
//!
//! `Palette` is the per-frame token set handed to every panel as `sh.palette`
//! (`let p = sh.palette;`). Use the JB names (`surface`, `raised`, `line`,
//! `ink2`, `ink3`, `live`, `warn`, `panel_fill`, ...). The older field names
//! are kept so unported panels still compile; they alias the JB tokens:
//! `bg`=panel_fill, `paper`=raised, `paper_dim`=hover, `ink_dim`=ink2,
//! `ink_faint`=ink3, `edge`/`separator`=line, `success`=live, `warning`=warn,
//! `popover`=surface (opaque), `field`/`control`=raised, `sheet`/`side`=panel_fill.
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Rounding, Stroke, Vec2};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

/// The palette list (JBPalette). `Mono` is the plain mode-aware black/white;
/// every other entry tints the accent and the backdrop glow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JbPalette {
    Mono,
    Ember,
    Phosphor,
    Cobalt,
    Violet,
    Rose,
    Moss,
    Sand,
}

impl JbPalette {
    pub const ALL: [JbPalette; 8] = [
        JbPalette::Mono,
        JbPalette::Ember,
        JbPalette::Phosphor,
        JbPalette::Cobalt,
        JbPalette::Violet,
        JbPalette::Rose,
        JbPalette::Moss,
        JbPalette::Sand,
    ];
    pub fn id(self) -> &'static str {
        match self {
            JbPalette::Mono => "mono",
            JbPalette::Ember => "ember",
            JbPalette::Phosphor => "phosphor",
            JbPalette::Cobalt => "cobalt",
            JbPalette::Violet => "violet",
            JbPalette::Rose => "rose",
            JbPalette::Moss => "moss",
            JbPalette::Sand => "sand",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            JbPalette::Mono => "Mono",
            JbPalette::Ember => "Ember",
            JbPalette::Phosphor => "Phosphor",
            JbPalette::Cobalt => "Cobalt",
            JbPalette::Violet => "Violet",
            JbPalette::Rose => "Rose",
            JbPalette::Moss => "Moss",
            JbPalette::Sand => "Sand",
        }
    }
    /// Parses a palette id, including the old 1.5 preset names.
    pub fn parse(s: &str) -> JbPalette {
        match s.trim() {
            "purple" => JbPalette::Violet,
            "ocean" => JbPalette::Cobalt,
            "forest" => JbPalette::Moss,
            "sunset" => JbPalette::Ember,
            "graphite" | "monochrome" => JbPalette::Mono,
            o => JbPalette::ALL.iter().copied().find(|p| p.id() == o).unwrap_or(JbPalette::Ember),
        }
    }
    /// (light, dark) accent hex. Light values are darkened for contrast on paper.
    pub fn hex(self) -> (u32, u32) {
        match self {
            JbPalette::Mono => (0x151515, 0xEDEDEC),
            JbPalette::Ember => (0xD9501A, 0xFF6A2B),
            JbPalette::Phosphor => (0x0F8A69, 0x5FD1B0),
            JbPalette::Cobalt => (0x2F5BEA, 0x7397FF),
            JbPalette::Violet => (0x6A48DB, 0xA88EFF),
            JbPalette::Rose => (0xD23A68, 0xFF7099),
            JbPalette::Moss => (0x4C7A1E, 0xA6D35F),
            JbPalette::Sand => (0x94661C, 0xE3BD7E),
        }
    }
    pub fn accent(self, dark: bool) -> Color32 {
        let (l, d) = self.hex();
        hex(if dark { d } else { l })
    }
}

/// The token set. Construct with `Palette::for_state(aero, dark)` (reads the
/// global palette + solidity) or `Palette::new(palette, dark, solidity)`.
#[derive(Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub palette: JbPalette,
    /// Kept for source compatibility: always true.
    pub aero: bool,

    // ---- JB tokens ----
    /// Ground behind everything (`JB.bg`).
    pub ground: Color32,
    /// Opaque surface (`JB.surface`): menus, popovers, dialogs.
    pub surface: Color32,
    /// Surface at the user's panel opacity (`JB.panelFill`).
    pub panel_fill: Color32,
    /// Ink at 3.5 % / 4.5 % (`JB.raised`): cards, fields, chips.
    pub raised: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub ink: Color32,
    pub ink2: Color32,
    pub ink3: Color32,
    /// Ink on the inverse ground (text on an ink-filled control).
    pub inverse: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    /// Accent at 14 % (`JB.accentSoft`).
    pub accent_soft: Color32,
    /// "Alive / running". The accent, except Mono keeps phosphor green.
    pub live: Color32,
    pub warn: Color32,
    pub danger: Color32,
    /// Panel opacity 0.35..1.
    pub solidity: f32,

    // ---- legacy aliases (see module docs) ----
    pub bg: Color32,
    pub paper: Color32,
    pub paper_dim: Color32,
    pub ink_dim: Color32,
    pub ink_faint: Color32,
    pub accent_hi: Color32,
    pub edge: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub window: Color32,
    pub side: Color32,
    pub sheet: Color32,
    pub popover: Color32,
    pub field: Color32,
    pub control: Color32,
    pub rim_top: Color32,
    pub rim_bottom: Color32,
    pub specular: Color32,
    pub separator: Color32,
    pub shadow: Color32,
}

pub fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// CSS-style rgba (gamma-space premultiply). NOT `from_rgba_unmultiplied`:
/// that premultiplies in linear light, which — since the framebuffer blends in
/// gamma space — makes a 7.5 % white wash land like ~30 %.
pub fn rgba(v: u32, a: f32) -> Color32 {
    let a = a.clamp(0.0, 1.0);
    let m = |c: u32| ((c & 0xff) as f32 * a).round() as u8;
    Color32::from_rgba_premultiplied(m(v >> 16), m(v >> 8), m(v), (a * 255.0).round() as u8)
}

/// A colour at an alpha (gamma-space), keeping its RGB.
pub fn with_alpha(c: Color32, a: f32) -> Color32 {
    rgba(((c.r() as u32) << 16) | ((c.g() as u32) << 8) | c.b() as u32, a)
}

// ---- appearance globals (JBAppearance) -----------------------------------

static PALETTE: AtomicU8 = AtomicU8::new(1); // ember
static SOLIDITY: AtomicU32 = AtomicU32::new(0x3f60_a3d7); // 0.88
static REDUCE_MOTION: AtomicBool = AtomicBool::new(false);

pub fn palette() -> JbPalette {
    JbPalette::ALL[(PALETTE.load(Ordering::Relaxed) as usize).min(7)]
}

/// Set the palette and persist it. Re-run `theme::apply` + rebuild `sh.palette` afterwards.
pub fn set_palette(p: JbPalette) {
    PALETTE.store(JbPalette::ALL.iter().position(|x| *x == p).unwrap_or(1) as u8, Ordering::Relaxed);
    write_pref("palette", p.id());
}

/// Panel opacity over the backdrop, 0.35 (see-through) .. 1 (solid).
pub fn solidity() -> f32 {
    f32::from_bits(SOLIDITY.load(Ordering::Relaxed))
}

pub fn set_solidity(s: f32) {
    SOLIDITY.store(s.clamp(0.35, 1.0).to_bits(), Ordering::Relaxed);
}

pub fn save_solidity(s: f32) {
    set_solidity(s);
    write_pref("solidity", &format!("{:.2}", solidity()));
}

/// Reduce motion: no spinners/pulses/rises (the Mac's accessibilityReduceMotion).
pub fn reduce_motion() -> bool {
    REDUCE_MOTION.load(Ordering::Relaxed)
}

pub fn set_reduce_motion(on: bool) {
    REDUCE_MOTION.store(on, Ordering::Relaxed);
    write_pref("reduce-motion", if on { "1" } else { "0" });
}

// Old names (the glass era's "reduce glass" == reduce motion/effects).
pub fn reduce_glass() -> bool {
    reduce_motion()
}
pub fn set_reduce_glass_quiet(on: bool) {
    REDUCE_MOTION.store(on, Ordering::Relaxed);
}
pub fn set_reduce_glass(on: bool) {
    set_reduce_motion(on);
    crate::config::update(|c| c.reduce_glass = on);
}

fn cfg_dir() -> std::path::PathBuf {
    crate::config::dir()
}

fn read_pref(name: &str) -> Option<String> {
    std::fs::read_to_string(cfg_dir().join(name)).ok().map(|s| s.trim().to_string())
}

fn write_pref(name: &str, v: &str) {
    let dir = cfg_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join(name), v);
    }
}

/// Load persisted palette / solidity / reduce-motion into the globals.
pub fn load_prefs() {
    if let Some(p) = read_pref("palette") {
        let p = JbPalette::parse(&p);
        PALETTE.store(JbPalette::ALL.iter().position(|x| *x == p).unwrap_or(1) as u8, Ordering::Relaxed);
    }
    if let Some(v) = read_pref("solidity").and_then(|s| s.parse::<f32>().ok()) {
        set_solidity(v);
    }
    if let Some(v) = read_pref("reduce-motion") {
        REDUCE_MOTION.store(v == "1", Ordering::Relaxed);
    }
}

/// Old name.
pub fn load_solidity() {
    load_prefs();
}

impl Palette {
    pub fn new(palette: JbPalette, dark: bool, solidity: f32) -> Self {
        let pick = |l: u32, d: u32| hex(if dark { d } else { l });
        // `jbInkAlpha`: black in light mode, white in dark.
        let ink_a = |l: f32, d: f32| if dark { rgba(0xffffff, d) } else { rgba(0x000000, l) };
        let solidity = solidity.clamp(0.35, 1.0);
        let surface = pick(0xFCFBF9, 0x131314);
        let ground = pick(0xF5F4F1, 0x0A0A0B);
        let inverse = pick(0xFCFBF9, 0x0A0A0B);
        let ink = pick(0x151515, 0xEDEDEC);
        let ink2 = pick(0x66655F, 0x9A9A9F);
        let ink3 = pick(0xA09E98, 0x5E5E63);
        let accent = palette.accent(dark);
        let mono = palette == JbPalette::Mono;
        let live = if mono { pick(0x0F8A69, 0x5FD1B0) } else { accent };
        let on_accent = if mono { inverse } else { pick(0xFFFFFF, 0x0A0A0B) };
        let raised = ink_a(0.035, 0.045);
        let hover = ink_a(0.06, 0.07);
        let pressed = ink_a(0.09, 0.10);
        let line = ink_a(0.08, 0.075);
        let line_strong = ink_a(0.15, 0.13);
        let surf_rgb = if dark { 0x131314 } else { 0xFCFBF9 };
        let panel_fill = rgba(surf_rgb, solidity);
        let accent_soft = with_alpha(accent, 0.14);
        Palette {
            dark,
            palette,
            aero: true,
            ground,
            surface,
            panel_fill,
            raised,
            hover,
            pressed,
            line,
            line_strong,
            ink,
            ink2,
            ink3,
            inverse,
            accent,
            on_accent,
            accent_soft,
            live,
            warn: pick(0xB97A06, 0xF5B83D),
            danger: pick(0xD13A2C, 0xFF5A4A),
            solidity,
            bg: panel_fill,
            paper: raised,
            paper_dim: hover,
            ink_dim: ink2,
            ink_faint: ink3,
            accent_hi: accent,
            edge: line,
            success: live,
            warning: pick(0xB97A06, 0xF5B83D),
            window: ground,
            side: panel_fill,
            sheet: panel_fill,
            popover: surface,
            field: raised,
            control: raised,
            rim_top: Color32::TRANSPARENT,
            rim_bottom: Color32::TRANSPARENT,
            specular: Color32::TRANSPARENT,
            separator: line,
            shadow: rgba(0x000000, 0.28),
        }
    }

    pub fn light() -> Self {
        Self::new(palette(), false, solidity())
    }

    pub fn dark() -> Self {
        Self::new(palette(), true, solidity())
    }

    pub fn for_state(_aero: bool, dark: bool) -> Self {
        Self::new(palette(), dark, solidity())
    }

    /// The window clear colour (the ground) as gamma floats.
    pub fn clear_rgba(&self) -> [f32; 4] {
        self.ground.to_normalized_gamma_f32()
    }

    /// Accent at an alpha (`JB.accent.opacity(a)`).
    pub fn accent_a(&self, a: f32) -> Color32 {
        with_alpha(self.accent, a)
    }

    /// Ink at an alpha (`JB.ink.opacity(a)`).
    pub fn ink_a(&self, a: f32) -> Color32 {
        with_alpha(self.ink, a)
    }
}

// ---- radii / spacing ----------------------------------------------------

/// Controls: buttons, fields, rows (`JB.rControl`).
pub const R_CTRL: f32 = 7.0;
/// Cards, bubbles, menus (`JB.rCard`).
pub const R_CARD: f32 = 10.0;
/// Panels (`JB.rPanel`).
pub const R_PANEL: f32 = 14.0;
/// Dialogs / floating panels (old name, = R_PANEL).
pub const R_WIN: f32 = 14.0;
/// 4 pt spacing grid (`JB.s1..s6`).
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S5: f32 = 24.0;
pub const S6: f32 = 32.0;

// ---- motion ---------------------------------------------------------------

/// Durations (s). The Mac uses springs; these are the settle times of the same
/// springs (snap = response .26, smooth = .42, gentle .22, fade .16).
pub const SNAP: f32 = 0.20;
pub const SMOOTH: f32 = 0.34;
pub const GENTLE: f32 = 0.22;
pub const FADE: f32 = 0.16;

/// Spring-ish ease used for snap/smooth: ease-out cubic.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// 0..1 animated toward `on` over `dur` seconds (instant under reduce-motion).
pub fn anim(ctx: &egui::Context, id: egui::Id, on: bool, dur: f32) -> f32 {
    ctx.animate_bool_with_time(id, on, if reduce_motion() { 0.0 } else { dur })
}

fn mtime(p: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// (glass, dark). Dark/light follows the desktop (`~/.config/themes/mode`,
/// live-watched). The in-app toggle writes `joebro-personal/appearance`; the
/// most recently written of the two wins, so a desktop flip always carries
/// through while a manual override sticks until the desktop changes again.
/// `JOEBRO_THEME_MODE=light|dark` forces a mode (snapshot harness).
pub fn desktop_state(sys_dark: Option<bool>) -> (bool, bool) {
    if let Ok(m) = std::env::var("JOEBRO_THEME_MODE") {
        return (true, m.trim() != "light");
    }
    let read = |p: &std::path::Path| {
        std::fs::read_to_string(p)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| s == "light" || s == "dark")
    };
    let mine = cfg_dir().join("appearance");
    // Production: a manual choice from the sidebar toggle wins; otherwise
    // follow the OS light/dark setting (dark if the OS can't say).
    if !crate::config::PERSONAL {
        let pick = read(&mine).unwrap_or_else(|| match sys_dark {
            Some(false) => "light".into(),
            _ => "dark".into(),
        });
        return (true, pick != "light");
    }
    let desk = dirs::config_dir()
        .unwrap_or_default()
        .join("themes/mode");
    let pick = match (read(&desk), read(&mine)) {
        (Some(d), Some(m)) => {
            if mtime(&mine) > mtime(&desk) {
                m
            } else {
                d
            }
        }
        (Some(d), None) => d,
        (None, Some(m)) => m,
        (None, None) => "dark".into(),
    };
    (true, pick != "light")
}

/// Escape hatch: `JOEBRO_OPAQUE=1` or a `joebro-personal/opaque` file paints
/// an opaque window (no compositor glass) in case a driver misbehaves with
/// alpha surfaces.
pub fn opaque_window() -> bool {
    // The JB look paints its own ground; an opaque window is the portable choice.
    true
}

pub fn request_mode(dark: bool) {
    let dir = cfg_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("appearance"), if dark { "dark" } else { "light" });
    }
}


// ---- fonts ----------------------------------------------------------------

static GEIST_LIGHT: &[u8] = include_bytes!("../assets/fonts/Geist-Light.ttf");
static GEIST_REGULAR: &[u8] = include_bytes!("../assets/fonts/Geist-Regular.ttf");
static GEIST_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Geist-Medium.ttf");
static GEIST_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Geist-SemiBold.ttf");
static GEIST_BOLD: &[u8] = include_bytes!("../assets/fonts/Geist-Bold.ttf");
static MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/GeistMono-Regular.ttf");
static MONO_MEDIUM: &[u8] = include_bytes!("../assets/fonts/GeistMono-Medium.ttf");
static MONO_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/GeistMono-SemiBold.ttf");
static MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/GeistMono-Bold.ttf");

/// Font weights available in Geist / Geist Mono (`Font.Weight` buckets of `JBFonts`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum W {
    Light,
    Regular,
    Medium,
    Semibold,
    Bold,
}

/// Geist (bundled) for the UI in five weights, Geist Mono for labels/meta/code,
/// Phosphor for icons (family "phosphor" / "phosphor-fill"), and the system
/// Nerd Font + Noto Symbols as legacy glyph fallbacks (the old `icon::*` codes).
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let load = |paths: &[&str]| -> Option<Vec<u8>> { paths.iter().find_map(|p| std::fs::read(p).ok()) };
    for (name, bytes) in [
        ("geist-light", GEIST_LIGHT),
        ("geist", GEIST_REGULAR),
        ("geist-medium", GEIST_MEDIUM),
        ("geist-semibold", GEIST_SEMIBOLD),
        ("geist-bold", GEIST_BOLD),
        ("geist-mono", MONO_REGULAR),
        ("geist-mono-medium", MONO_MEDIUM),
        ("geist-mono-semibold", MONO_SEMIBOLD),
        ("geist-mono-bold", MONO_BOLD),
    ] {
        fonts.font_data.insert(name.into(), FontData::from_static(bytes));
    }
    fonts
        .font_data
        .insert("phosphor".into(), egui_phosphor::Variant::Regular.font_data());
    fonts
        .font_data
        .insert("phosphor-fill".into(), egui_phosphor::Variant::Fill.font_data());
    let mut fallbacks: Vec<String> = vec![];
    if let Some(b) = load(crate::platform::symbol_fonts()) {
        fonts.font_data.insert("symbols".into(), FontData::from_owned(b));
        fallbacks.push("symbols".into());
    }
    let have_nerd = if let Some(b) = load(&[
        "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
        "/usr/share/fonts/TTF/JetBrainsMonoNerdFontMono-Regular.ttf",
    ]) {
        fonts.font_data.insert("icons".into(), FontData::from_owned(b));
        fallbacks.push("icons".into());
        true
    } else {
        false
    };
    let builtin_prop = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let builtin_mono = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let fam = |first: &str, mono: bool| -> Vec<String> {
        let mut v = vec![first.to_string()];
        if mono {
            v.push("geist".into()); // glyphs Geist Mono lacks
        }
        v.extend(fallbacks.iter().cloned());
        v.extend(if mono { builtin_mono.iter() } else { builtin_prop.iter() }.cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, fam("geist", false));
    fonts.families.insert(FontFamily::Monospace, fam("geist-mono", true));
    for (n, f, mono) in [
        ("light", "geist-light", false),
        ("medium", "geist-medium", false),
        ("semibold", "geist-semibold", false),
        ("bold", "geist-bold", false),
        ("mono-medium", "geist-mono-medium", true),
        ("mono-semibold", "geist-mono-semibold", true),
        ("mono-bold", "geist-mono-bold", true),
        // legacy names some panels still ask for
        ("display", "geist-semibold", false),
        ("body", "geist", false),
        ("aero", "geist", false),
    ] {
        fonts.families.insert(FontFamily::Name(n.into()), fam(f, mono));
    }
    fonts
        .families
        .insert(FontFamily::Name("phosphor".into()), vec!["phosphor".into()]);
    fonts
        .families
        .insert(FontFamily::Name("phosphor-fill".into()), vec!["phosphor-fill".into()]);
    // Legacy FontAwesome-in-Nerd-Font family (old `icon::*` constants).
    fonts.families.insert(
        FontFamily::Name("icons".into()),
        if have_nerd {
            vec!["icons".into(), "geist".into()]
        } else {
            fonts.families[&FontFamily::Proportional].clone()
        },
    );
    ctx.set_fonts(fonts);
}

/// Geist at `size`: `Font.jb(size, weight)`.
pub fn jb(size: f32, w: W) -> FontId {
    let fam = match w {
        W::Regular => FontFamily::Proportional,
        W::Light => FontFamily::Name("light".into()),
        W::Medium => FontFamily::Name("medium".into()),
        W::Semibold => FontFamily::Name("semibold".into()),
        W::Bold => FontFamily::Name("bold".into()),
    };
    FontId::new(size, fam)
}

/// Geist Mono at `size`: `Font.jbMono(size, weight)`. Light maps to Regular.
pub fn mono(size: f32, w: W) -> FontId {
    let fam = match w {
        W::Regular | W::Light => FontFamily::Monospace,
        W::Medium => FontFamily::Name("mono-medium".into()),
        W::Semibold => FontFamily::Name("mono-semibold".into()),
        W::Bold => FontFamily::Name("mono-bold".into()),
    };
    FontId::new(size, fam)
}

/// Phosphor icon font at `size` (see `icons.rs`).
pub fn icon_font_id(size: f32, fill: bool) -> FontId {
    FontId::new(size, FontFamily::Name(if fill { "phosphor-fill" } else { "phosphor" }.into()))
}

/// Text styles (`Font.jbTitle2` ...), macOS point sizes. Each returns a
/// `RichText` with the right font; colour is left to the caller / visuals.
pub mod ty {
    use super::{jb, mono as mono_font, W};
    use eframe::egui::RichText;
    fn t(s: impl Into<String>, size: f32, w: W) -> RichText {
        RichText::new(s).font(jb(size, w))
    }
    pub fn large_title(s: impl Into<String>) -> RichText { t(s, 26.0, W::Semibold) }
    pub fn title(s: impl Into<String>) -> RichText { t(s, 22.0, W::Semibold) }
    pub fn title2(s: impl Into<String>) -> RichText { t(s, 17.0, W::Semibold) }
    pub fn title3(s: impl Into<String>) -> RichText { t(s, 15.0, W::Medium) }
    pub fn headline(s: impl Into<String>) -> RichText { t(s, 13.0, W::Semibold) }
    pub fn body(s: impl Into<String>) -> RichText { t(s, 13.0, W::Regular) }
    pub fn callout(s: impl Into<String>) -> RichText { t(s, 12.0, W::Regular) }
    pub fn subheadline(s: impl Into<String>) -> RichText { t(s, 11.0, W::Regular) }
    pub fn footnote(s: impl Into<String>) -> RichText { t(s, 10.5, W::Regular) }
    pub fn caption(s: impl Into<String>) -> RichText { t(s, 10.5, W::Regular) }
    pub fn caption2(s: impl Into<String>) -> RichText { t(s, 10.0, W::Regular) }
    /// `.jbEyebrow()`: 12 semibold (colour ink2 — pass `p.ink2`).
    pub fn eyebrow(s: impl Into<String>) -> RichText { t(s, 12.0, W::Semibold) }
    /// Explicit size / weight: `Font.jb(size, weight)`.
    pub fn sized(s: impl Into<String>, size: f32, w: W) -> RichText { t(s, size, w) }
    /// `Font.jbMono(size, weight)`.
    pub fn mono(s: impl Into<String>, size: f32, w: W) -> RichText {
        RichText::new(s).font(mono_font(size, w))
    }
    /// Mono at a text style's size x 0.94 (`Font.jbMono(.caption)` -> `mono_style(10.5)`).
    pub fn mono_style(s: impl Into<String>, style_size: f32) -> RichText {
        mono(s, style_size * 0.94, W::Regular)
    }
}

/// Titles — Geist SemiBold.
pub fn display_font() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn medium_font() -> FontFamily {
    FontFamily::Name("medium".into())
}

pub fn light_font() -> FontFamily {
    FontFamily::Name("light".into())
}

/// Legacy FontAwesome (Nerd Font) glyph family — prefer `icons::sf`.
pub fn icon_font() -> FontFamily {
    FontFamily::Name("icons".into())
}

/// FontAwesome codepoints from the Nerd Font.
pub mod icon {
    pub const SETTINGS: &str = "\u{f013}";
    pub const FOLDER: &str = "\u{f07b}";
    pub const FOLDER_OPEN: &str = "\u{f07c}";
    pub const FILE: &str = "\u{f15b}";
    pub const SEARCH: &str = "\u{f002}";
    pub const STAR: &str = "\u{f005}";
    pub const PENCIL: &str = "\u{f044}";
    pub const SUN: &str = "\u{f185}";
    pub const MOON: &str = "\u{f186}";
    pub const NEW: &str = "\u{f067}";
    pub const CLOSE: &str = "\u{f00d}";
    pub const COPY: &str = "\u{f0c5}";
    pub const BRAIN: &str = "\u{f09d1}";
    pub const REFRESH: &str = "\u{f021}";
    pub const TERMINAL: &str = "\u{f120}";
    pub const LOCK: &str = "\u{f023}";
    pub const PAPERCLIP: &str = "\u{f0c6}";
    pub const CHECK_SQUARE: &str = "\u{f14a}";
    pub const SIDEBAR: &str = "\u{f0db}"; // columns
    pub const MIC: &str = "\u{f130}";
    pub const STOP: &str = "\u{f04d}";
    pub const ARROW_UP: &str = "\u{f062}";
    pub const SPARKLES: &str = "\u{f0e7}"; // bolt
    pub const CHEVRON_DOWN: &str = "\u{f078}";
    pub const GLOBE: &str = "\u{f0ac}";
    pub const PDF: &str = "\u{f1c1}";
    pub const SAVE: &str = "\u{f0c7}";
    pub const COMPRESS: &str = "\u{f066}";
}

/// Apply the palette to egui's visuals + style.
/// Push the tokens into egui's Visuals/Style. Call at launch and whenever the
/// palette, mode or solidity changes.
pub fn apply(ctx: &egui::Context, p: Palette) {
    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.override_text_color = Some(p.ink);
    // Panels paint their own fill over the backdrop.
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = p.surface;
    v.extreme_bg_color = p.raised;
    v.code_bg_color = p.raised;
    v.faint_bg_color = p.raised;
    v.window_stroke = Stroke::new(1.0, p.line_strong);
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.danger;
    v.window_rounding = Rounding::same(R_CARD + 4.0);
    v.menu_rounding = Rounding::same(R_CARD);
    v.window_highlight_topmost = false;
    // Floating surfaces are the only ones that cast a shadow (black .28, r24, y12).
    v.window_shadow = egui::epaint::Shadow { offset: Vec2::new(0.0, 12.0), blur: 24.0, spread: 0.0, color: p.shadow };
    v.popup_shadow = v.window_shadow;
    v.striped = false;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);

    let w = &mut v.widgets;
    for st in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        st.rounding = Rounding::same(R_CTRL);
        st.expansion = 0.0;
    }
    w.noninteractive.bg_fill = p.raised;
    w.noninteractive.weak_bg_fill = p.raised;
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.ink);
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.line);

    w.inactive.bg_fill = p.raised;
    w.inactive.weak_bg_fill = p.raised;
    w.inactive.fg_stroke = Stroke::new(1.0, p.ink);
    w.inactive.bg_stroke = Stroke::new(1.0, p.line);

    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.fg_stroke = Stroke::new(1.0, p.ink);
    w.hovered.bg_stroke = Stroke::new(1.0, p.line);

    // NOTE fg_stroke stays `ink`: egui derives strong_text_color() from it.
    w.active.bg_fill = p.pressed;
    w.active.weak_bg_fill = p.pressed;
    w.active.fg_stroke = Stroke::new(1.0, p.ink);
    w.active.bg_stroke = Stroke::new(1.0, p.line);

    w.open.bg_fill = p.hover;
    w.open.weak_bg_fill = p.hover;
    w.open.fg_stroke = Stroke::new(1.0, p.ink);
    w.open.bg_stroke = Stroke::new(1.0, p.line);

    v.selection.bg_fill = with_alpha(p.accent, if p.dark { 0.40 } else { 0.28 });
    v.selection.stroke = Stroke::new(1.0, p.accent);
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    use egui::TextStyle;
    style.text_styles = [
        (TextStyle::Heading, jb(17.0, W::Semibold)),
        (TextStyle::Body, jb(13.0, W::Regular)),
        (TextStyle::Button, jb(13.0, W::Medium)),
        (TextStyle::Small, jb(10.5, W::Regular)),
        (TextStyle::Monospace, mono(12.2, W::Regular)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(12.0, 7.0);
    style.spacing.window_margin = egui::Margin::same(16.0);
    style.spacing.menu_margin = egui::Margin::same(6.0);
    style.spacing.interact_size.y = 28.0;
    style.spacing.combo_height = 320.0;
    style.spacing.text_edit_width = 240.0;
    style.spacing.slider_rail_height = 4.0;
    let mut scroll = egui::style::ScrollStyle::floating();
    scroll.bar_width = 7.0;
    scroll.floating_allocated_width = 0.0;
    style.spacing.scroll = scroll;
    style.animation_time = if reduce_motion() { 0.0 } else { FADE };
    ctx.set_style(style);
}

impl Palette {
    /// The `hover` token's base colour used for fading the hover wash in/out.
    pub fn hover_base(&self) -> Color32 {
        self.hover
    }
}
