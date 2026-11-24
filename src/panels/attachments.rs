//! Chat attachments — the Mac `AttachmentViews`. Images render inline in the
//! bubble (space reserved from the upload's own width/height, so nothing
//! jumps when they land), PDFs as a first-page card, everything else as a
//! glass chip. Clicking any of them opens a preview sheet: the PDF viewer,
//! a zoomable image, plain text, or Office files via LibreOffice → PDF, with
//! Open (default app) and Save… — the QuickLook sheet, on Linux.

use crate::pdfview::{state_message, PdfViewer};
use crate::state::{Job, Shared};
use crate::widgets;
use egui::{vec2, Align, Color32, Key, Layout, Rect, RichText, Rounding, Sense, Ui, Vec2};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

type R<T> = Result<T, String>;

const IMG_MAX: Vec2 = Vec2::new(320.0, 240.0);
const OPEN_ICON: &str = "\u{f08e}";

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Att {
    pub id: String,
    pub name: String,
    pub mime: String,
    /// Pixel size for images, when the backend recorded it.
    pub size: Option<Vec2>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Image,
    Pdf,
    Office,
    Text,
    Other,
}

impl Att {
    pub fn new(id: &str, name: &str, mime: &str) -> Att {
        let mime = if mime.is_empty() { guess_mime(name) } else { mime.to_string() };
        Att { id: id.into(), name: name.into(), mime, size: None }
    }

    pub fn from_json(v: &Value) -> Option<Att> {
        let id = v.get("id").and_then(Value::as_str)?;
        let name = v.get("name").and_then(Value::as_str).unwrap_or("file");
        let mime = v.get("mime").and_then(Value::as_str).unwrap_or("");
        let mut a = Att::new(id, name, mime);
        let w = v.get("width").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let h = v.get("height").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        if w > 0.0 && h > 0.0 {
            a.size = Some(vec2(w, h));
        }
        Some(a)
    }

    /// `metadata.attachments` of a history message.
    pub fn parse_list(meta: Option<&Value>) -> Vec<Att> {
        meta.and_then(|m| m.get("attachments"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Att::from_json).collect())
            .unwrap_or_default()
    }

    pub fn kind(&self) -> Kind {
        let m = self.mime.to_lowercase();
        let n = self.name.to_lowercase();
        let ext = n.rsplit('.').next().unwrap_or("");
        if m.starts_with("image/") && !m.contains("svg") {
            Kind::Image
        } else if m.contains("pdf") || ext == "pdf" {
            Kind::Pdf
        } else if ["doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf"].contains(&ext) {
            Kind::Office
        } else if m.starts_with("text/")
            || m.contains("json")
            || m.contains("xml")
            || m.contains("javascript")
            || [
                "txt", "md", "csv", "tsv", "log", "json", "yaml", "yml", "toml", "py", "rs", "js", "ts", "sh",
                "c", "h", "cpp", "go", "java", "swift", "html", "css", "sql", "ini", "cfg", "svg",
            ]
            .contains(&ext)
        {
            Kind::Text
        } else {
            Kind::Other
        }
    }

    pub fn icon(&self) -> &'static str {
        match self.kind() {
            Kind::Image => "\u{f03e}",
            Kind::Pdf => crate::theme::icon::PDF,
            Kind::Office => "\u{f1c2}",
            Kind::Text => "\u{f15c}",
            Kind::Other => {
                if self.mime.starts_with("audio/") {
                    "\u{f1c7}"
                } else if self.mime.starts_with("video/") {
                    "\u{f1c8}"
                } else if self.mime.contains("zip") || self.mime.contains("tar") {
                    "\u{f1c6}"
                } else {
                    crate::theme::icon::FILE
                }
            }
        }
    }
}

pub fn guess_mime(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        "mp3" | "wav" | "ogg" | "m4a" => "audio/*",
        "mp4" | "mov" | "webm" | "mkv" => "video/*",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Fit `size` inside `max`, never upscaling.
fn fit(size: Vec2, max: Vec2) -> Vec2 {
    let s = (max.x / size.x).min(max.y / size.y).min(1.0);
    size * s
}

enum Load {
    Pending(Job<R<Vec<u8>>>),
    Ready(Arc<[u8]>),
    Failed(String),
}

struct Preview {
    att: Att,
    opened: f64,
    /// 0 = fit to window; otherwise the image scale.
    img_zoom: f32,
}

#[derive(Default)]
pub struct Attachments {
    files: HashMap<String, Load>,
    pdfs: HashMap<String, PdfViewer>,
    office: HashMap<String, Job<R<Vec<u8>>>>,
    office_err: HashMap<String, String>,
    preview: Option<Preview>,
    actions: Vec<Job<R<String>>>,
    /// Bytes waiting for the in-app Save sheet to return a path.
    pending_save: Option<Arc<[u8]>>,
}

fn friendly(e: String) -> String {
    let l = e.to_lowercase();
    if l.contains("404") || l.contains("not found") {
        "This file is no longer on the server.".into()
    } else {
        e
    }
}

pub fn safe_name(name: &str) -> String {
    let n: String = name
        .chars()
        .map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c })
        .collect();
    if n.trim().is_empty() {
        "file".into()
    } else {
        n
    }
}

impl Attachments {
    /// Seed the cache with bytes we already have (just-uploaded files).
    pub fn seed(&mut self, id: &str, bytes: Vec<u8>) {
        self.files.entry(id.to_string()).or_insert(Load::Ready(bytes.into()));
    }

    fn bytes(&mut self, sh: &Shared, att: &Att) -> Option<R<Arc<[u8]>>> {
        let id = att.id.clone();
        let entry = self.files.entry(id.clone()).or_insert_with(|| {
            Load::Pending(sh.spawn(move |a| a.get_bytes(&format!("api/upload/{id}"), &[])))
        });
        if let Load::Pending(job) = entry {
            if let Some(res) = job.take() {
                *entry = match res {
                    Ok(b) => Load::Ready(b.into()),
                    Err(e) => Load::Failed(friendly(e)),
                };
            } else if job.done {
                *entry = Load::Failed("Download interrupted".into());
            }
        }
        match entry {
            Load::Ready(b) => Some(Ok(b.clone())),
            Load::Failed(e) => Some(Err(e.clone())),
            Load::Pending(_) => None,
        }
    }

    fn pdf(&mut self, sh: &Shared, att: &Att) -> Option<&mut PdfViewer> {
        if !self.pdfs.contains_key(&att.id) {
            let bytes = self.bytes(sh, att)?.ok()?;
            self.pdfs.insert(att.id.clone(), PdfViewer::new(&sh.ctx, &att.id, bytes.to_vec()));
        }
        self.pdfs.get_mut(&att.id)
    }

    #[cfg(feature = "snapshot")]
    pub fn demo_open(&mut self, ctx: &egui::Context, att: &Att) {
        self.open(ctx, att);
    }

    fn open(&mut self, ctx: &egui::Context, att: &Att) {
        ctx.memory_mut(|m| m.stop_text_input());
        self.preview = Some(Preview { att: att.clone(), opened: ctx.input(|i| i.time), img_zoom: 0.0 });
    }

    /// The attachments of one message. `right` aligns them for user bubbles.
    pub fn show(&mut self, ui: &mut Ui, sh: &Shared, atts: &[Att], right: bool) {
        if atts.is_empty() {
            return;
        }
        let layout = if right { Layout::top_down(Align::Max) } else { Layout::top_down(Align::Min) };
        ui.with_layout(layout, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for att in atts.iter().filter(|a| a.kind() == Kind::Image) {
                self.image_tile(ui, sh, att);
            }
            for att in atts.iter().filter(|a| a.kind() == Kind::Pdf) {
                self.pdf_card(ui, sh, att);
            }
            let rest: Vec<&Att> = atts.iter().filter(|a| !matches!(a.kind(), Kind::Image | Kind::Pdf)).collect();
            if !rest.is_empty() {
                let dir = if right { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
                ui.with_layout(dir.with_main_wrap(true), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for att in rest {
                        let p = sh.palette;
                        let slot = ui.painter().add(egui::Shape::Noop);
                        let fr = egui::Frame::none().inner_margin(egui::Margin::symmetric(8.0, 4.0)).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.horizontal(|ui| {
                                crate::icons::symbol(ui, "paperclip", 11.0, p.ink3);
                                ui.label(egui::RichText::new(&att.name).font(crate::theme::jb(11.0, crate::theme::W::Regular)).color(p.ink2));
                            });
                        });
                        let rr = Rounding::same(6.0);
                        ui.painter().set(slot, egui::Shape::Vec(vec![
                            egui::Shape::rect_filled(fr.response.rect, rr, p.raised),
                            egui::Shape::rect_stroke(fr.response.rect.shrink(0.5), rr, egui::Stroke::new(1.0, p.line)),
                        ]));
                        let r = fr.response.interact(Sense::click()).on_hover_text(format!("Preview {}", att.name)).on_hover_cursor(egui::CursorIcon::PointingHand);
                        if r.clicked() {
                            self.open(ui.ctx(), att);
                        }
                    }
                });
            }
        });
    }

    fn image_tile(&mut self, ui: &mut Ui, sh: &Shared, att: &Att) {
        let p = sh.palette;
        let known = att.size.map(|s| fit(s, IMG_MAX));
        let rounding = Rounding::same(crate::theme::R_CARD);
        match self.bytes(sh, att) {
            Some(Ok(bytes)) => {
                let img = egui::Image::from_bytes(format!("bytes://att/{}", att.id), egui::load::Bytes::Shared(bytes))
                    .rounding(rounding)
                    .sense(Sense::click());
                let img = match known {
                    Some(sz) => img.fit_to_exact_size(sz),
                    None => img.max_size(IMG_MAX),
                };
                let r = ui.add(img).on_hover_cursor(egui::CursorIcon::PointingHand);
                let hov = ui.ctx().animate_bool_with_time(r.id.with("hov"), r.hovered(), 0.15);
                ui.painter().rect_stroke(r.rect.shrink(0.5), rounding, egui::Stroke::new(1.0, p.line));
                if hov > 0.0 {
                    ui.painter().rect_stroke(r.rect, rounding, egui::Stroke::new(1.5, p.accent.gamma_multiply(0.7 * hov)));
                }
                if r.clicked() {
                    self.open(ui.ctx(), att);
                }
            }
            Some(Err(e)) => {
                widgets::btn_icon(ui, sh, "\u{f071}", &att.name).on_hover_text(e);
            }
            None => {
                let sz = known.unwrap_or(vec2(200.0, 130.0));
                let (rect, _) = ui.allocate_exact_size(sz, Sense::hover());
                ui.painter().rect_filled(rect, rounding, p.control);
                let t = ui.input(|i| i.time) as f32;
                let a = 0.4 + 0.3 * (t * 3.0).sin();
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "\u{f03e}",
                    egui::FontId::new(22.0, crate::theme::icon_font()),
                    p.ink_faint.gamma_multiply(a + 0.3),
                );
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
            }
        }
    }

    fn pdf_card(&mut self, ui: &mut Ui, sh: &Shared, att: &Att) {
        let p = sh.palette;
        let size = vec2(280.0, 92.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("Open {}", att.name));
        let hov = ui.ctx().animate_bool_with_time(resp.id.with("hov"), resp.hovered(), 0.15);
        let fill = if resp.is_pointer_button_down_on() { p.pressed } else { widgets::mix(p.control, p.hover, hov) };
        let mut shapes = widgets::glass_surface(&p, rect, 14.0, fill, false);
        if hov > 0.0 {
            shapes.push(egui::Shape::rect_stroke(rect, Rounding::same(14.0), egui::Stroke::new(1.0, p.accent.gamma_multiply(0.5 * hov))));
        }
        ui.painter().extend(shapes);
        let thumb = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(56.0, 72.0));
        let mut info = "PDF".to_string();
        let err = match self.bytes(sh, att) {
            Some(Err(e)) => Some(e),
            _ => None,
        };
        if let Some(v) = self.pdf(sh, att) {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(thumb));
            v.thumbnail(&mut child, sh, thumb.size());
            if let Some(e) = v.error() {
                info = e.to_string();
            } else if v.page_count() > 0 {
                let n = v.page_count();
                info = format!("PDF · {n} page{}", if n == 1 { "" } else { "s" });
            } else {
                info = "PDF · opening…".into();
            }
        } else {
            ui.painter().rect_filled(thumb, Rounding::same(6.0), p.paper);
            ui.painter().text(
                thumb.center(),
                egui::Align2::CENTER_CENTER,
                crate::theme::icon::PDF,
                egui::FontId::new(20.0, crate::theme::icon_font()),
                p.danger.gamma_multiply(0.8),
            );
            if let Some(e) = err {
                info = e;
            } else {
                info.push_str(" · downloading…");
            }
        }
        let text_x = thumb.right() + 12.0;
        let max_w = rect.right() - text_x - 10.0;
        let name = ui.painter().layout(
            att.name.clone(),
            egui::FontId::new(13.0, crate::theme::medium_font()),
            p.ink,
            max_w,
        );
        let name_h = name.size().y.min(36.0);
        let painter = ui.painter().with_clip_rect(Rect::from_min_max(egui::pos2(text_x, rect.top()), rect.max));
        painter.galley(egui::pos2(text_x, rect.top() + 18.0), name, p.ink);
        painter.text(
            egui::pos2(text_x, rect.top() + 24.0 + name_h),
            egui::Align2::LEFT_TOP,
            info,
            egui::FontId::proportional(11.5),
            p.ink_dim,
        );
        if resp.clicked() {
            self.open(ui.ctx(), att);
        }
    }

    /// Land background save/open results; call once per frame.
    pub fn pump(&mut self, sh: &mut Shared) {
        if let Some(path) = sh.take_picked("att_save") {
            if let Some(bytes) = self.pending_save.take() {
                self.actions.push(sh.spawn(move |_| {
                    std::fs::write(&path, &bytes[..]).map_err(|e| format!("Save failed: {e}"))?;
                    Ok(format!("Saved {}", path.display()))
                }));
            }
        }
        self.actions.retain_mut(|job| match job.take() {
            Some(Ok(msg)) => {
                if !msg.is_empty() {
                    sh.err(msg);
                }
                false
            }
            Some(Err(e)) => {
                sh.err(e);
                false
            }
            None => !job.done,
        });
        let landed: Vec<(String, R<Vec<u8>>)> = self
            .office
            .iter_mut()
            .filter_map(|(id, job)| job.take().map(|r| (id.clone(), r)))
            .collect();
        for (id, res) in landed {
            self.office.remove(&id);
            match res {
                Ok(pdf) => {
                    self.pdfs.insert(id.clone(), PdfViewer::new(&sh.ctx, &format!("office-{id}"), pdf));
                }
                Err(e) => {
                    self.office_err.insert(id, e);
                }
            }
        }
    }

    fn open_external(&mut self, sh: &Shared, att: &Att, bytes: Arc<[u8]>) {
        let name = format!("{}-{}", att.id.chars().take(12).collect::<String>(), safe_name(&att.name));
        self.actions.push(sh.spawn(move |_| {
            let dir = dirs::cache_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("joebro/attachments");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join(name);
            std::fs::write(&path, &bytes[..]).map_err(|e| e.to_string())?;
            let ok = crate::platform::open_path(&path).is_ok();
            if ok {
                Ok(String::new())
            } else {
                Err("No app is set up to open this file type".into())
            }
        }));
    }

    fn save_as(&mut self, sh: &mut Shared, att: &Att, bytes: Arc<[u8]>) {
        self.pending_save = Some(bytes);
        sh.picker.open_save("att_save", "Save attachment", &safe_name(&att.name), None);
    }

    /// The preview sheet (dimmed backdrop + centred glass card). Call every
    /// frame before the panels so it can claim Esc.
    pub fn preview_ui(&mut self, ctx: &egui::Context, sh: &mut Shared) {
        self.pump(sh);
        let Some(pv) = &self.preview else { return };
        let att = pv.att.clone();
        let t = ((ctx.input(|i| i.time) - pv.opened) / 0.18).clamp(0.0, 1.0) as f32;
        let ease = 1.0 - (1.0 - t).powi(3);
        if t < 1.0 {
            ctx.request_repaint();
        }
        let mut close = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let p = sh.palette;
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("att_preview_backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha((110.0 * ease) as u8));
                if resp.clicked() {
                    close = true;
                }
            });
        let size = vec2((screen.width() - 48.0).min(1100.0), (screen.height() - 48.0).min(940.0));
        let pos = screen.center() - size / 2.0 + vec2(0.0, 10.0 * (1.0 - ease));
        let card = Rect::from_min_size(pos, size);
        let area = egui::Area::new(egui::Id::new("att_preview_card"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                ui.multiply_opacity(ease);
                ui.painter().extend(widgets::glass_surface(&p, card, crate::theme::R_WIN, p.popover, true));
                let inner = card.shrink2(vec2(16.0, 14.0));
                let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
                ui.set_clip_rect(inner.expand(2.0));
                self.preview_body(&mut ui, sh, &att, &mut close);
            });
        ctx.move_to_top(area.response.layer_id);
        if close {
            self.preview = None;
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("composer_input")));
        }
    }

    fn preview_body(&mut self, ui: &mut Ui, sh: &mut Shared, att: &Att, close: &mut bool) {
        let p = sh.palette;
        let bytes = self.bytes(sh, att);
        // Header: icon · name · Open · Save… · ✕
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(widgets::ico(att.icon(), 14.0, sh).color(p.accent_hi));
            let ready = bytes.as_ref().and_then(|b| b.as_ref().ok()).cloned();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_chip(ui, crate::theme::icon::CLOSE, false, sh).on_hover_text("Close (Esc)").clicked() {
                    *close = true;
                }
                ui.add_enabled_ui(ready.is_some(), |ui| {
                    if widgets::btn_icon(ui, sh, crate::theme::icon::SAVE, "Save…").clicked() {
                        if let Some(b) = ready.clone() {
                            self.save_as(sh, att, b);
                        }
                    }
                    if widgets::btn_icon(ui, sh, OPEN_ICON, "Open")
                        .on_hover_text("Open in the default app")
                        .clicked()
                    {
                        if let Some(b) = ready.clone() {
                            self.open_external(sh, att, b);
                        }
                    }
                });
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&att.name).size(14.0).family(crate::theme::display_font()).color(p.ink))
                            .truncate(),
                    );
                });
            });
        });
        ui.add_space(8.0);
        let bytes = match bytes {
            None => {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.4);
                    ui.add(egui::Spinner::new().size(22.0).color(p.ink_faint));
                    ui.label(RichText::new("Downloading…").size(12.5).color(p.ink_dim));
                });
                return;
            }
            Some(Err(e)) => {
                state_message(ui, sh, "\u{f071}", "Couldn't load this file", &e);
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    if widgets::btn(ui, sh, "Try again").clicked() {
                        self.files.remove(&att.id);
                    }
                });
                return;
            }
            Some(Ok(b)) => b,
        };
        match att.kind() {
            Kind::Pdf => {
                if let Some(v) = self.pdf(sh, att) {
                    v.ui(ui, sh, true, |_| {});
                }
            }
            Kind::Office => {
                if let Some(v) = self.pdfs.get_mut(&att.id) {
                    v.ui(ui, sh, true, |_| {});
                } else if let Some(e) = self.office_err.get(&att.id) {
                    state_message(ui, sh, "\u{f071}", "No preview for this document", e);
                } else {
                    if !self.office.contains_key(&att.id) {
                        let ext = att.name.rsplit('.').next().unwrap_or("docx").to_lowercase();
                        let b = bytes.clone();
                        self.office
                            .insert(att.id.clone(), sh.spawn(move |_| crate::panels::editor::office_to_pdf(&b, &ext)));
                    }
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.4);
                        ui.add(egui::Spinner::new().size(22.0).color(p.ink_faint));
                        ui.label(RichText::new("Rendering with LibreOffice…").size(12.5).color(p.ink_dim));
                    });
                }
            }
            Kind::Image => self.image_view(ui, sh, att, bytes),
            Kind::Text => {
                let text = String::from_utf8_lossy(&bytes[..bytes.len().min(512 * 1024)]).into_owned();
                egui::Frame::none()
                    .fill(p.field)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(text).monospace().size(12.5))
                                    .wrap_mode(egui::TextWrapMode::Extend)
                                    .selectable(true),
                            );
                        });
                    });
            }
            Kind::Other => {
                state_message(
                    ui,
                    sh,
                    att.icon(),
                    "No preview for this file type",
                    "Open it in its default app, or save a copy.",
                );
            }
        }
    }

    fn image_view(&mut self, ui: &mut Ui, sh: &Shared, att: &Att, bytes: Arc<[u8]>) {
        let p = sh.palette;
        let Some(pv) = self.preview.as_mut() else { return };
        // zoom: Ctrl+scroll, Ctrl +/−, Ctrl 0 = fit
        use egui::gui_zoom::kb_shortcuts as z;
        let (zin, zout, zfit, wheel) = ui.ctx().input_mut(|i| {
            (
                i.consume_shortcut(&z::ZOOM_IN) || i.consume_shortcut(&z::ZOOM_IN_SECONDARY),
                i.consume_shortcut(&z::ZOOM_OUT),
                i.consume_shortcut(&z::ZOOM_RESET),
                i.zoom_delta(),
            )
        });
        let avail = ui.available_size();
        let img = egui::Image::from_bytes(format!("bytes://att/{}", att.id), egui::load::Bytes::Shared(bytes));
        let natural = match att.size {
            Some(s) => s,
            None => match img.load_for_size(ui.ctx(), avail).ok().and_then(|t| t.size()) {
                Some(s) => s,
                None => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(avail.y * 0.4);
                        ui.add(egui::Spinner::new().size(22.0).color(p.ink_faint));
                    });
                    return;
                }
            },
        };
        let fit_scale = (avail.x / natural.x).min(avail.y / natural.y).min(1.0);
        let cur = if pv.img_zoom <= 0.0 { fit_scale } else { pv.img_zoom };
        let mut next = cur;
        if zin {
            next *= 1.25;
        }
        if zout {
            next /= 1.25;
        }
        if (wheel - 1.0).abs() > 1e-3 {
            next *= wheel;
        }
        if next != cur {
            pv.img_zoom = next.clamp(0.05, 8.0);
        }
        if zfit {
            pv.img_zoom = 0.0;
        }
        let scale = if pv.img_zoom <= 0.0 { fit_scale } else { pv.img_zoom };
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            let sz = natural * scale;
            let w = ui.available_width().max(sz.x);
            let h = ui.available_height().max(sz.y);
            let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
            let r = Rect::from_center_size(rect.center(), sz);
            img.paint_at(ui, r);
            if resp.double_clicked() {
                pv.img_zoom = if pv.img_zoom <= 0.0 { 1.0 } else { 0.0 };
            }
        });
        let label = format!("{:.0}%", scale * 100.0);
        ui.painter().text(
            ui.max_rect().right_bottom() - vec2(8.0, 6.0),
            egui::Align2::RIGHT_BOTTOM,
            label,
            egui::FontId::proportional(11.0),
            p.ink_faint,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_history_attachment_metadata() {
        let meta = serde_json::json!({"attachments": [
            {"id": "abc.png", "name": "image.png", "mime": "image/png", "width": 492, "height": 684},
            {"id": "d.pdf", "name": "Report.pdf", "mime": ""},
            {"name": "no id"}
        ]});
        let a = Att::parse_list(Some(&meta));
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].kind(), Kind::Image);
        assert_eq!(a[0].size, Some(vec2(492.0, 684.0)));
        assert_eq!(a[1].mime, "application/pdf");
        assert_eq!(a[1].kind(), Kind::Pdf);
        assert_eq!(Att::new("x", "notes.docx", "").kind(), Kind::Office);
        assert_eq!(Att::new("x", "main.rs", "").kind(), Kind::Text);
    }

    #[test]
    fn images_fit_without_upscaling() {
        assert_eq!(fit(vec2(100.0, 50.0), IMG_MAX), vec2(100.0, 50.0));
        assert_eq!(fit(vec2(640.0, 240.0), IMG_MAX), vec2(320.0, 120.0));
    }
}
