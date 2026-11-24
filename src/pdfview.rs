//! Inline PDF viewer. Pages are rendered lazily by poppler's `pdftoppm`
//! (one page per job, at the DPI the current zoom needs), cached as PNGs
//! under `~/.cache/joebro/pdf/<hash>/`, and uploaded as egui textures.
//! Page sizes come from `pdfinfo` up front, so the layout never jumps while
//! pages stream in; a lower-resolution texture stays on screen until the
//! sharper one is ready.

use crate::state::Shared;
use crate::widgets;
use egui::{pos2, vec2, Align, Color32, ColorImage, Key, Rect, RichText, Rounding, Sense, TextureHandle, Ui, Vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

type R<T> = Result<T, String>;
/// The cached PDF path and its page sizes in points.
type Opened = (PathBuf, Vec<Vec2>);

const GAP: f32 = 14.0;
const DPI_STEPS: [u32; 6] = [48, 72, 110, 150, 200, 288];
const MAX_JOBS: usize = 3;
/// Keep textures this many pages either side of the visible ones.
const KEEP_AROUND: usize = 4;
const MIN_SCALE: f32 = 0.2;
const MAX_SCALE: f32 = 6.0;

fn cache_root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("joebro")
        .join("pdf")
}

fn hash_key(bytes: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}-{}", h.finish(), bytes.len())
}

/// Drop cached page renders older than two weeks (best effort, our dir only).
fn prune_cache(root: &Path) {
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(14 * 86_400);
    let Ok(dirs) = std::fs::read_dir(root) else { return };
    for dir in dirs.flatten() {
        let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
        let mut left = 0;
        for f in files.flatten() {
            let old = f
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| t < cutoff)
                .unwrap_or(false);
            if old {
                let _ = std::fs::remove_file(f.path());
            } else {
                left += 1;
            }
        }
        if left == 0 {
            let _ = std::fs::remove_dir(dir.path());
        }
    }
}

fn parse_size(s: &str) -> Option<Vec2> {
    let mut it = s.split_whitespace();
    let w: f32 = it.next()?.parse().ok()?;
    if it.next()? != "x" {
        return None;
    }
    let h: f32 = it.next()?.parse().ok()?;
    (w > 0.0 && h > 0.0).then(|| vec2(w, h))
}

/// Page sizes (in points, rotation applied) from `pdfinfo -f 1 -l N` output.
pub fn parse_pdfinfo(out: &str) -> Vec<Vec2> {
    let mut count = 0usize;
    let mut default = None;
    let mut sizes: HashMap<usize, Vec2> = HashMap::new();
    let mut rots: HashMap<usize, i32> = HashMap::new();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("Pages:") {
            count = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("Page size:") {
            default = parse_size(rest);
        } else if let Some(rest) = line.strip_prefix("Page") {
            let rest = rest.trim_start();
            let Some((num, tail)) = rest.split_once(char::is_whitespace) else { continue };
            let Ok(n) = num.parse::<usize>() else { continue };
            let tail = tail.trim_start();
            if let Some(s) = tail.strip_prefix("size:") {
                if let Some(v) = parse_size(s) {
                    sizes.insert(n, v);
                }
            } else if let Some(r) = tail.strip_prefix("rot:") {
                rots.insert(n, r.trim().parse().unwrap_or(0));
            }
        }
    }
    let count = count.max(sizes.keys().copied().max().unwrap_or(0));
    let fallback = default.unwrap_or(vec2(612.0, 792.0));
    (1..=count)
        .map(|n| {
            let s = sizes.get(&n).copied().unwrap_or(fallback);
            if rots.get(&n).map(|r| r.rem_euclid(180) == 90).unwrap_or(false) {
                vec2(s.y, s.x)
            } else {
                s
            }
        })
        .collect()
}

fn open_pdf(bytes: &[u8]) -> R<Opened> {
    if !bytes.starts_with(b"%PDF") && !bytes.windows(4).take(1024).any(|w| w == b"%PDF") {
        return Err("Not a PDF file".into());
    }
    let root = cache_root();
    prune_cache(&root);
    let dir = root.join(hash_key(bytes));
    std::fs::create_dir_all(&dir).map_err(|e| format!("PDF cache: {e}"))?;
    let path = dir.join("doc.pdf");
    if std::fs::metadata(&path).map(|m| m.len() as usize != bytes.len()).unwrap_or(true) {
        let tmp = dir.join(format!("doc.{}.tmp", std::process::id()));
        std::fs::write(&tmp, bytes).map_err(|e| format!("PDF cache: {e}"))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("PDF cache: {e}"))?;
    }
    let out = crate::platform::poppler("pdfinfo")
        .args(["-f", "1", "-l", "100000"])
        .arg(&path)
        .output()
        .map_err(|e| format!("pdfinfo is not available (install poppler): {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("This PDF could not be read").to_string());
    }
    let pages = parse_pdfinfo(&String::from_utf8_lossy(&out.stdout));
    if pages.is_empty() {
        return Err("This PDF has no pages".into());
    }
    Ok((path, pages))
}

fn render_page(pdf: &Path, page: usize, dpi: u32) -> R<ColorImage> {
    let dir = pdf.parent().ok_or("bad cache path")?;
    let out = dir.join(format!("p{page}-{dpi}.png"));
    if !out.exists() {
        let stem = dir.join(format!("tmp-{page}-{dpi}-{:?}", std::thread::current().id()).replace(['(', ')'], ""));
        let status = crate::platform::poppler("pdftoppm")
            .args(["-png", "-singlefile", "-r", &dpi.to_string()])
            .args(["-f", &page.to_string(), "-l", &page.to_string()])
            .arg(pdf)
            .arg(&stem)
            .output()
            .map_err(|e| format!("{} ({e})", crate::platform::install_hint("pdftoppm (poppler)")))?;
        let produced = stem.with_extension("png");
        if !status.status.success() || !produced.exists() {
            return Err(format!("Page {page} could not be rendered"));
        }
        std::fs::rename(&produced, &out).map_err(|e| e.to_string())?;
    } else {
        // touch, so pruning keeps pages that are still being read
        let _ = std::fs::File::open(&out).and_then(|f| f.set_modified(std::time::SystemTime::now()));
    }
    let img = image::open(&out).map_err(|e| format!("Page {page}: {e}"))?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

pub struct PdfViewer {
    id: egui::Id,
    opening: Option<Receiver<R<Opened>>>,
    path: Option<PathBuf>,
    pages: Vec<Vec2>,
    error: Option<String>,
    tex: HashMap<usize, (u32, TextureHandle)>,
    jobs: HashMap<usize, (u32, Receiver<R<ColorImage>>)>,
    page_errors: HashMap<usize, String>,
    /// Fit the widest page to the viewer width (recomputed every frame).
    pub fit: bool,
    /// Logical pixels per PDF point.
    scale: f32,
    current: usize,
    goto: Option<usize>,
    page_field: String,
    editing_page: bool,
    last_offset: f32,
    last_content_h: f32,
}

impl PdfViewer {
    /// Start opening `bytes` in the background. `key` scopes widget ids.
    pub fn new(ctx: &egui::Context, key: &str, bytes: Vec<u8>) -> Self {
        let (tx, rx) = channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(open_pdf(&bytes));
            c.request_repaint();
        });
        Self {
            id: egui::Id::new(("pdfview", key.to_string())),
            opening: Some(rx),
            path: None,
            pages: vec![],
            error: None,
            tex: HashMap::new(),
            jobs: HashMap::new(),
            page_errors: HashMap::new(),
            fit: true,
            scale: 1.0,
            current: 0,
            goto: None,
            page_field: String::new(),
            editing_page: false,
            last_offset: 0.0,
            last_content_h: 0.0,
        }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.opening {
            if let Ok(res) = rx.try_recv() {
                match res {
                    Ok((path, pages)) => {
                        self.path = Some(path);
                        self.pages = pages;
                    }
                    Err(e) => self.error = Some(e),
                }
                self.opening = None;
            }
        }
        let mut done = vec![];
        for (&page, (dpi, rx)) in &self.jobs {
            if let Ok(res) = rx.try_recv() {
                done.push((page, *dpi, res));
            }
        }
        for (page, dpi, res) in done {
            self.jobs.remove(&page);
            match res {
                Ok(img) => {
                    // Trust the rendered aspect (covers odd /Rotate + crop boxes).
                    if let Some(sz) = self.pages.get_mut(page) {
                        let (w, h) = (img.size[0] as f32, img.size[1] as f32);
                        if w > 0.0 && h > 0.0 && ((w / h) - (sz.x / sz.y)).abs() > 0.02 {
                            *sz = vec2(sz.x, sz.x * h / w);
                        }
                    }
                    let tex = ctx.load_texture(
                        format!("{:?}-p{page}-{dpi}", self.id),
                        img,
                        egui::TextureOptions::LINEAR,
                    );
                    self.tex.insert(page, (dpi, tex));
                }
                Err(e) => {
                    self.page_errors.insert(page, e);
                }
            }
        }
    }

    /// Make sure `page` has (or is getting) a texture at least `want` DPI.
    fn ensure(&mut self, ctx: &egui::Context, page: usize, want: u32) {
        let have = self.tex.get(&page).map(|t| t.0).unwrap_or(0);
        if have >= want || self.page_errors.contains_key(&page) {
            return;
        }
        if let Some((dpi, _)) = self.jobs.get(&page) {
            if *dpi >= want {
                return;
            }
        }
        if self.jobs.len() >= MAX_JOBS {
            return;
        }
        let Some(path) = self.path.clone() else { return };
        let (tx, rx) = channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(render_page(&path, page + 1, want));
            c.request_repaint();
        });
        self.jobs.insert(page, (want, rx));
    }

    fn dpi_for(&self, page: usize, scale: f32, ppp: f32) -> u32 {
        let need = 72.0 * scale * ppp;
        let size = self.pages.get(page).copied().unwrap_or(vec2(612.0, 792.0));
        // keep textures under ~4k px on the long side
        let cap = (4096.0 * 72.0 / size.x.max(size.y)).max(48.0);
        let mut pick = DPI_STEPS[0];
        for d in DPI_STEPS {
            pick = d;
            if d as f32 >= need {
                break;
            }
        }
        pick.min(cap as u32).max(DPI_STEPS[0])
    }

    fn set_scale(&mut self, s: f32) {
        self.fit = false;
        self.scale = s.clamp(MIN_SCALE, MAX_SCALE);
    }

    pub fn go_to(&mut self, page: usize) {
        if !self.pages.is_empty() {
            self.goto = Some(page.min(self.pages.len() - 1));
        }
    }

    fn keys(&mut self, ui: &Ui) {
        use egui::gui_zoom::kb_shortcuts as z;
        let n = self.pages.len();
        if n == 0 {
            return;
        }
        let (zin, zout, zreset) = ui.ctx().input_mut(|i| {
            (
                i.consume_shortcut(&z::ZOOM_IN) || i.consume_shortcut(&z::ZOOM_IN_SECONDARY),
                i.consume_shortcut(&z::ZOOM_OUT),
                i.consume_shortcut(&z::ZOOM_RESET),
            )
        });
        if zin {
            self.set_scale(self.scale * 1.2);
        }
        if zout {
            self.set_scale(self.scale / 1.2);
        }
        if zreset {
            self.fit = true;
        }
        if ui.ctx().wants_keyboard_input() {
            return;
        }
        let (prev, next, home, end) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::PageUp),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::PageDown),
                i.key_pressed(Key::Home),
                i.key_pressed(Key::End),
            )
        });
        if prev {
            self.go_to(self.current.saturating_sub(1));
        }
        if next {
            self.go_to((self.current + 1).min(n - 1));
        }
        if home {
            self.go_to(0);
        }
        if end {
            self.go_to(n - 1);
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, sh: &Shared, extra: impl FnOnce(&mut Ui)) {
        let p = sh.palette;
        let n = self.pages.len();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let can = n > 0;
            if widgets::icon_chip(ui, "\u{f053}", false, sh)
                .on_hover_text("Previous page (←)")
                .clicked()
                && can
            {
                self.go_to(self.current.saturating_sub(1));
            }
            // "3 / 12" — click the number to type a page.
            let field_id = self.id.with("page_field");
            if self.editing_page {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.page_field)
                        .id(field_id)
                        .desired_width(34.0)
                        .horizontal_align(Align::Center),
                );
                r.request_focus();
                if r.lost_focus() {
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        if let Ok(v) = self.page_field.trim().parse::<usize>() {
                            self.go_to(v.max(1) - 1);
                        }
                    }
                    self.editing_page = false;
                }
            } else {
                let label = if can { format!("{}", self.current + 1) } else { "–".into() };
                let r = widgets::chip(ui, &label, false, sh).on_hover_text("Go to page…");
                if r.clicked() && can {
                    self.page_field = (self.current + 1).to_string();
                    self.editing_page = true;
                }
            }
            ui.label(RichText::new(format!("of {n}")).size(12.0).color(p.ink_dim));
            if widgets::icon_chip(ui, "\u{f054}", false, sh)
                .on_hover_text("Next page (→)")
                .clicked()
                && can
            {
                self.go_to((self.current + 1).min(n.saturating_sub(1)));
            }
            ui.add_space(8.0);
            if widgets::icon_chip(ui, "\u{f068}", false, sh)
                .on_hover_text("Zoom out (Ctrl −)")
                .clicked()
            {
                self.set_scale(self.scale / 1.2);
            }
            let pct = (self.scale * 100.0 / (96.0 / 72.0)).round();
            ui.add_sized(
                vec2(44.0, 20.0),
                egui::Label::new(RichText::new(format!("{pct:.0}%")).size(12.0).color(p.ink_dim)),
            );
            if widgets::icon_chip(ui, "\u{f067}", false, sh)
                .on_hover_text("Zoom in (Ctrl +)")
                .clicked()
            {
                self.set_scale(self.scale * 1.2);
            }
            let mut fit = self.fit;
            if widgets::chip(ui, "Fit width", fit, sh)
                .on_hover_text("Fit the page to the window (Ctrl 0)")
                .clicked()
            {
                fit = !fit;
            }
            self.fit = fit;
            ui.with_layout(egui::Layout::right_to_left(Align::Center), extra);
        });
    }

    /// The full viewer: toolbar + scrolling pages. `extra` adds buttons on
    /// the toolbar's right. Keyboard shortcuts act while the pointer is over
    /// the viewer, or always when `keys_always` (e.g. a modal preview).
    pub fn ui(&mut self, ui: &mut Ui, sh: &Shared, keys_always: bool, extra: impl FnOnce(&mut Ui)) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.toolbar(ui, sh, extra);
        ui.add_space(6.0);
        let p = sh.palette;
        if let Some(e) = &self.error {
            state_message(ui, sh, "\u{f071}", "Couldn't open this PDF", e);
            return;
        }
        if self.pages.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                ui.add(egui::Spinner::new().size(20.0).color(p.ink_faint));
                ui.label(RichText::new("Opening PDF…").size(12.5).color(p.ink_dim));
            });
            return;
        }
        let area = ui.available_rect_before_wrap();
        if keys_always || ui.rect_contains_pointer(area) {
            self.keys(ui);
            let zd = ui.input(|i| i.zoom_delta());
            if (zd - 1.0).abs() > 1e-3 && ui.rect_contains_pointer(area) {
                self.set_scale(self.scale * zd);
            }
        }
        let maxw = self.pages.iter().map(|s| s.x).fold(1.0f32, f32::max);
        let old_scale = self.scale;
        if self.fit {
            self.scale = ((area.width() - 2.0 * GAP - 12.0) / maxw).clamp(MIN_SCALE, MAX_SCALE);
        }
        let scale = self.scale;
        let total_h: f32 = self.pages.iter().map(|s| s.y * scale + GAP).sum::<f32>() + GAP;
        let mut sa = egui::ScrollArea::both()
            .id_salt(self.id.with("scroll"))
            .auto_shrink([false, false]);
        // Keep the reading position when the zoom changes.
        if (old_scale - scale).abs() > 1e-4 && self.last_content_h > 0.0 && self.goto.is_none() {
            let frac = self.last_offset / self.last_content_h;
            sa = sa.vertical_scroll_offset(frac * total_h);
        }
        let ppp = ctx.pixels_per_point();
        let goto = self.goto.take();
        let mut wanted: Vec<(usize, u32)> = vec![];
        let mut visible: Vec<usize> = vec![];
        let out = sa.show(ui, |ui| {
            let content_w = (maxw * scale + 2.0 * GAP).max(ui.available_width());
            let (rect, _) = ui.allocate_exact_size(vec2(content_w, total_h), Sense::hover());
            let clip = ui.clip_rect();
            let prefetch = clip.expand2(vec2(0.0, clip.height() * 0.75));
            let painter = ui.painter();
            let mut y = rect.top() + GAP;
            let mut best = (f32::INFINITY, self.current);
            for (i, sz) in self.pages.iter().enumerate() {
                let (w, h) = (sz.x * scale, sz.y * scale);
                let pr = Rect::from_min_size(pos2(rect.center().x - w / 2.0, y), vec2(w, h));
                y += h + GAP;
                if goto == Some(i) {
                    ui.scroll_to_rect(pr.translate(vec2(0.0, -GAP / 2.0)), Some(Align::TOP));
                }
                let d = (pr.center().y - clip.center().y).abs() - h / 2.0;
                if d < best.0 {
                    best = (d, i);
                }
                if !pr.intersects(prefetch) {
                    continue;
                }
                if pr.intersects(clip) {
                    visible.push(i);
                }
                painter.add(egui::Shadow {
                    offset: vec2(0.0, 4.0),
                    blur: 16.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(if p.dark { 90 } else { 40 }),
                }
                .as_shape(pr, Rounding::same(3.0)));
                painter.rect_filled(pr, Rounding::same(3.0), Color32::WHITE);
                if let Some((_, tex)) = self.tex.get(&i) {
                    painter.image(
                        tex.id(),
                        pr,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else if let Some(e) = self.page_errors.get(&i) {
                    painter.text(pr.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(12.0), Color32::from_gray(120));
                } else {
                    let t = ui.input(|i| i.time) as f32;
                    let a = 0.35 + 0.25 * (t * 3.0).sin();
                    painter.text(
                        pr.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("Page {}", i + 1),
                        egui::FontId::proportional(13.0),
                        Color32::from_gray(150).gamma_multiply(a + 0.4),
                    );
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                }
                wanted.push((i, self.dpi_for(i, scale, ppp)));
            }
            self.current = best.1;
        });
        self.last_offset = out.state.offset.y;
        self.last_content_h = out.content_size.y.max(1.0);
        // Nearest pages first.
        wanted.sort_by_key(|(i, _)| (*i as i64 - self.current as i64).abs());
        for (i, dpi) in wanted {
            self.ensure(&ctx, i, dpi);
        }
        if let (Some(&lo), Some(&hi)) = (visible.iter().min(), visible.iter().max()) {
            self.tex
                .retain(|&i, _| i + KEEP_AROUND >= lo && i <= hi + KEEP_AROUND);
        }
    }

    /// First-page thumbnail letterboxed into a fixed `size` box (the box
    /// never changes, so cards don't jump when the real page size lands).
    pub fn thumbnail(&mut self, ui: &mut Ui, sh: &Shared, size: Vec2) -> egui::Response {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let (bx, resp) = ui.allocate_exact_size(size, Sense::click());
        let page = self.pages.first().copied().unwrap_or(vec2(612.0, 792.0));
        let s = (size.x / page.x).min(size.y / page.y);
        let rect = Rect::from_center_size(bx.center(), page * s);
        let painter = ui.painter();
        painter.add(egui::Shadow {
            offset: vec2(0.0, 2.0),
            blur: 6.0,
            spread: 0.0,
            color: Color32::from_black_alpha(if sh.palette.dark { 80 } else { 30 }),
        }
        .as_shape(rect, Rounding::same(3.0)));
        painter.rect_filled(rect, Rounding::same(3.0), Color32::WHITE);
        if let Some((_, tex)) = self.tex.get(&0) {
            painter.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            let glyph = if self.error.is_some() { "PDF" } else { "…" };
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(13.0), Color32::from_gray(140));
        }
        if !self.pages.is_empty() {
            let dpi = self.dpi_for(0, s, ctx.pixels_per_point());
            self.ensure(&ctx, 0, dpi);
        }
        resp
    }
}

/// Centered icon + title + detail line — empty / error states.
pub fn state_message(ui: &mut Ui, sh: &Shared, glyph: &str, title: &str, detail: &str) {
    let p = sh.palette;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.3).max(12.0));
        ui.label(widgets::ico(glyph, 26.0, sh).color(p.ink_faint));
        ui.add_space(8.0);
        ui.label(RichText::new(title).size(15.0).family(crate::theme::medium_font()).color(p.ink));
        if !detail.is_empty() {
            ui.add_space(2.0);
            ui.label(RichText::new(detail).size(12.0).color(p.ink_dim));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdfinfo_per_page_sizes_and_rotation() {
        let out = "Title: x\nPages:          3\nPage    1 size: 612 x 792 pts (letter)\nPage    1 rot:  0\nPage    2 size: 612 x 792 pts (letter)\nPage    2 rot:  90\nPage    3 size: 595.28 x 841.89 pts (A4)\nPage    3 rot:  0\n";
        let v = parse_pdfinfo(out);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], vec2(612.0, 792.0));
        assert_eq!(v[1], vec2(792.0, 612.0));
        assert!((v[2].x - 595.28).abs() < 0.01);
    }

    /// A minimal valid PDF: one 200×100 pt page with a filled square.
    fn tiny_pdf() -> Vec<u8> {
        let content = "0 0 1 rg 20 20 60 60 re f";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offs = vec![];
        for (i, o) in objs.iter().enumerate() {
            offs.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let x = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
        for o in offs {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).bytes());
        out
    }

    #[test]
    fn renders_a_real_page_through_poppler() {
        if crate::platform::poppler("pdftoppm").arg("-v").output().is_err() {
            return; // poppler not installed on this machine
        }
        let (path, pages) = open_pdf(&tiny_pdf()).expect("open");
        assert_eq!(pages, vec![vec2(200.0, 100.0)]);
        let img = render_page(&path, 1, 72).expect("render");
        assert_eq!(img.size, [200, 100]);
        // the blue square is where we drew it (PDF y is bottom-up)
        let px = img.pixels[50 * 200 + 50];
        assert!(px.b() > 200 && px.r() < 60, "{px:?}");
        // second call hits the PNG cache
        assert_eq!(render_page(&path, 1, 72).unwrap().size, [200, 100]);
        assert!(open_pdf(b"not a pdf").is_err());
    }

    #[test]
    fn pdfinfo_single_page_size_fallback() {
        let v = parse_pdfinfo("Pages:          2\nPage size:      300 x 400 pts\n");
        assert_eq!(v, vec![vec2(300.0, 400.0), vec2(300.0, 400.0)]);
    }
}
