#![allow(dead_code)] // no longer routed: the Mac folds Brain into Memory (shared_memory.rs)
//! Brain: long-term memory as a searchable list or an Obsidian-style force
//! graph. Add/edit/pin/delete. Monochrome nodes; size = connectivity.
use crate::api::R;
use crate::state::{Job, Shared};
use crate::widgets::{self, SelAction};
use std::collections::BTreeSet;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use serde_json::Value;

#[derive(Default)]
pub struct BrainPanel {
    mems: Vec<Value>,
    load: Option<Job<R<Value>>>,
    loaded: bool,
    action: Option<Job<R<Value>>>,
    search: String,
    search_open: bool,
    add_text: String,
    map: bool,
    map_init: bool,
    selection: BTreeSet<String>,
    hover: Option<usize>,
    // graph layout
    pos: Vec<Pos2>,
    vel: Vec<Vec2>,
    zoom: f32,
    pan: Vec2,
    dragging: Option<usize>,
    edit: Option<(String, String, String)>, // id, text, category
    error: Option<String>,
}

impl BrainPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        if !self.loaded && self.load.is_none() {
            self.load = Some(sh.spawn(|a| a.memory()));
        }
        if let Some(job) = &mut self.load {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) => {
                        self.mems = v
                            .get("memory")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        self.loaded = true;
                        self.error = None;
                        self.init_layout();
                    }
                    Err(e) => {
                        self.error = Some(format!("Couldn't load memory: {e}"));
                        self.loaded = true;
                    }
                }
                self.load = None;
            }
        }
        if let Some(job) = &mut self.action {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) if v.get("ok").and_then(Value::as_bool) == Some(false) => {
                        self.error = Some(
                            v.get("detail")
                                .and_then(Value::as_str)
                                .unwrap_or("Memory action failed")
                                .to_owned(),
                        )
                    }
                    Err(e) => self.error = Some(e),
                    _ => {
                        self.error = None;
                        self.loaded = false;
                    }
                }
                self.action = None;
            }
        }

        if !self.map_init {
            // Mac default: map mode on.
            self.map = true;
            self.map_init = true;
        }
        let p = sh.palette;
        let (mut toggle, mut refresh) = (false, false);
        widgets::page_header(ui, sh, "\u{f09d1}", "Brain", |ui| {
            refresh = widgets::glyph_button(ui, crate::theme::icon::REFRESH, 13.0, None).on_hover_text("Refresh").clicked();
            widgets::search_field(ui, sh, &mut self.search, &mut self.search_open, "Search memories");
            toggle = widgets::glyph_button(ui, if self.map { "\u{f03a}" } else { "\u{f279}" }, 13.0, None)
                .on_hover_text(if self.map { "List view" } else { "Map view" })
                .clicked();
        });
        if refresh {
            self.error = None;
            self.loaded = false;
        }
        if toggle {
            self.map = !self.map;
        }
        if let Some(error) = &self.error {
            ui.colored_label(p.warning, error);
        }
        if let Some(t) = widgets::quick_add(ui, sh, &mut self.add_text, "Remember something new…", "\u{f055}", false, None) {
            self.add_text.clear();
            self.action = Some(sh.spawn(move |a| a.add_memory(&t, "fact")));
        }

        let pal = sh.palette;
        widgets::card_area(ui, &pal, |ui| {
            if !self.loaded {
                ui.centered_and_justified(|ui| ui.add(egui::Spinner::new().color(p.accent_hi)));
            } else if self.mems.is_empty() && self.error.is_some() {
                let e = self.error.clone().unwrap_or_default();
                widgets::empty_state(ui, sh, "\u{f09d1}", "Couldn't load memories", &e);
            } else if self.filtered().is_empty() {
                widgets::empty_state(ui, sh, "\u{f09d1}", "No memories", "");
            } else if self.map {
                self.map_view(ui, sh);
            } else {
                self.list_view(ui, sh);
            }
        });

        // Edit Memory sheet.
        if let Some((id, mut text, mut cat)) = self.edit.take() {
            let (mut save, mut cancel) = (false, false);
            let can = !text.trim().is_empty();
            let esc = widgets::dialog(
                ui.ctx(),
                "edit_memory",
                "Edit Memory",
                None,
                420.0,
                |ui| {
                    if widgets::primary(ui, sh, "Save", can).clicked() {
                        save = true;
                    }
                    if widgets::btn(ui, sh, "Cancel").clicked() {
                        cancel = true;
                    }
                },
                |ui| {
                    widgets::field_label(ui, "Memory");
                    ui.add(egui::TextEdit::multiline(&mut text).desired_width(f32::INFINITY).desired_rows(5));
                    ui.horizontal(|ui| {
                        ui.label("Category");
                        egui::ComboBox::from_id_salt("mem_cat")
                            .selected_text(capitalize(&cat))
                            .show_ui(ui, |ui| {
                                for c in CATEGORIES {
                                    ui.selectable_value(&mut cat, c.to_string(), capitalize(c));
                                }
                            });
                    });
                },
            );
            if save && can {
                let (i, t, c) = (id.clone(), text.clone(), cat.clone());
                self.action = Some(sh.spawn(move |a| a.update_memory(&i, &t, &c)));
            } else if !cancel && !esc {
                self.edit = Some((id, text, cat));
            }
        }
    }

    fn filtered(&self) -> Vec<usize> {
        let q = self.search.to_lowercase();
        (0..self.mems.len())
            .filter(|&i| q.is_empty() || mem_text(&self.mems[i]).to_lowercase().contains(&q))
            .collect()
    }

    fn list_view(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let p = sh.palette;
        let shown = self.filtered();
        if !self.selection.is_empty() {
            let ids: Vec<String> = shown.iter().map(|&i| mid(&self.mems[i])).collect();
            match widgets::selection_bar(ui, sh, self.selection.len(), shown.len()) {
                SelAction::ToggleAll => {
                    self.selection = if self.selection.len() == shown.len() { BTreeSet::new() } else { ids.into_iter().collect() };
                }
                SelAction::Clear => self.selection.clear(),
                SelAction::Delete => {
                    let del: Vec<String> = std::mem::take(&mut self.selection).into_iter().collect();
                    self.action = Some(sh.spawn(move |a| {
                        let mut first_err = None;
                        for id in &del {
                            if let Err(e) = a.delete_memory(id) {
                                first_err.get_or_insert(e);
                            }
                        }
                        if let Some(e) = first_err {
                            return Err(e);
                        }
                        Ok(serde_json::json!({"ok": true}))
                    }));
                }
                SelAction::None => {}
            }
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 8.0;
                for i in shown {
                    let m = self.mems[i].clone();
                    let id = mid(&m);
                    let text = mem_text(&m);
                    let cat = m.get("category").and_then(|x| x.as_str()).filter(|c| !c.is_empty()).unwrap_or("fact").to_string();
                    let pinned = m.get("pinned").and_then(|x| x.as_bool()).unwrap_or(false);
                    let when = m.get("created_at").and_then(Value::as_str).and_then(widgets::iso_to_secs).map(widgets::relative_from_now).unwrap_or_default();
                    let checked = self.selection.contains(&id);
                    let selecting = !self.selection.is_empty();
                    let out = widgets::row_shell(ui, sh, egui::Id::new(("mem_row", &id)), checked, selecting, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            let (gr, _) = ui.allocate_exact_size(egui::vec2(14.0, 18.0), Sense::hover());
                            widgets::paint_glyph(ui, gr.center(), if pinned { "\u{f08d}" } else { "\u{f005}" }, 11.0, if pinned { p.accent } else { p.ink_dim });
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 4.0;
                                ui.add(egui::Label::new(RichText::new(&text).size(13.0)).wrap().selectable(true));
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    widgets::tag(ui, sh, &cat);
                                    if !when.is_empty() {
                                        ui.label(RichText::new(when.clone()).size(10.0).color(p.ink_faint));
                                    }
                                });
                            });
                        });
                    });
                    if out.toggled || (out.clicked && selecting) {
                        if !self.selection.remove(&id) {
                            self.selection.insert(id.clone());
                        }
                    }
                    out.resp.context_menu(|ui| {
                        if ui.button("Edit…").clicked() {
                            self.edit = Some((id.clone(), text.clone(), cat.clone()));
                            ui.close_menu();
                        }
                        if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                            let i = id.clone();
                            self.action = Some(sh.spawn(move |a| a.pin_memory(&i)));
                            ui.close_menu();
                        }
                        if ui.button("Delete").clicked() {
                            let i = id.clone();
                            self.action = Some(sh.spawn(move |a| a.delete_memory(&i)));
                            ui.close_menu();
                        }
                    });
                }
            });
        });
    }

    fn init_layout(&mut self) {
        let n = self.mems.len();
        self.pos = (0..n)
            .map(|i| {
                let a = i as f32 * 2.399963; // golden-angle spread
                Pos2::new(
                    300.0 + 180.0 * a.cos() * (i as f32).sqrt() * 0.3,
                    300.0 + 180.0 * a.sin() * (i as f32).sqrt() * 0.3,
                )
            })
            .collect();
        self.vel = vec![Vec2::ZERO; n];
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
    }

    /// Bottom-right zoom capsule + bottom-left hover card (Mac MemoryMapView).
    fn map_overlays(&mut self, ui: &mut Ui, sh: &Shared, rect: Rect) {
        let p = sh.palette;
        let cap = Rect::from_min_size(Pos2::new(rect.right() - 10.0 - 108.0, rect.bottom() - 10.0 - 30.0), Vec2::new(108.0, 30.0));
        ui.painter().extend(widgets::glass_surface(&p, cap, 15.0, p.control, false));
        let mut zoom_to = None;
        for (i, (g, tip)) in [("\u{f010}", "Zoom out"), ("\u{f05b}", "Reset view"), ("\u{f00e}", "Zoom in")].iter().enumerate() {
            let r = Rect::from_center_size(Pos2::new(cap.left() + 18.0 + i as f32 * 36.0, cap.center().y), Vec2::splat(28.0));
            let resp = ui.interact(r, ui.id().with(("zoomctl", i)), Sense::click()).on_hover_text(*tip);
            if resp.hovered() {
                ui.painter().circle_filled(r.center(), 13.0, p.ink.gamma_multiply(0.1));
            }
            widgets::paint_glyph(ui, r.center(), g, 13.0, p.ink_dim);
            if resp.clicked() {
                zoom_to = Some(i);
            }
        }
        match zoom_to {
            Some(0) => self.zoom = (self.zoom / 1.25).clamp(0.1, 8.0),
            Some(1) => {
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
            }
            Some(2) => self.zoom = (self.zoom * 1.25).clamp(0.1, 8.0),
            _ => {}
        }
        if let Some(i) = self.hover.filter(|&i| i < self.mems.len()) {
            let m = &self.mems[i];
            let text = mem_text(m);
            let cat = m.get("category").and_then(Value::as_str).filter(|c| !c.is_empty()).unwrap_or("fact");
            let when = m.get("created_at").and_then(Value::as_str).and_then(widgets::iso_to_secs).map(widgets::relative_from_now).unwrap_or_default();
            let g = ui.painter().layout(text, egui::FontId::new(12.0, egui::FontFamily::Proportional), p.ink, 240.0);
            let h = g.size().y + 36.0;
            let card = Rect::from_min_size(Pos2::new(rect.left() + 10.0, rect.bottom() - 10.0 - h), Vec2::new(260.0, h));
            ui.painter().extend(widgets::glass_surface(&p, card, 10.0, p.popover, true));
            ui.painter().galley(card.min + Vec2::splat(10.0), g, p.ink);
            ui.painter().text(
                Pos2::new(card.left() + 10.0, card.bottom() - 10.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{cat}  {when}"),
                egui::FontId::new(10.0, crate::theme::medium_font()),
                p.ink_faint,
            );
        }
    }

    fn map_view(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let n = self.mems.len();
        if self.pos.len() != n {
            self.init_layout();
        }
        // edges by shared category/topic words
        let words: Vec<std::collections::HashSet<String>> = self
            .mems
            .iter()
            .map(|m| {
                mem_text(m)
                    .to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| w.len() > 4)
                    .map(String::from)
                    .collect()
            })
            .collect();
        let cats: Vec<String> = self
            .mems
            .iter()
            .map(|m| {
                m.get("category")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        let mut edges: Vec<(usize, usize, f32)> = vec![];
        for i in 0..n {
            for j in (i + 1)..n {
                let shared = words[i].intersection(&words[j]).count();
                let same_cat = !cats[i].is_empty() && cats[i] == cats[j];
                if shared >= 1 || same_cat {
                    let strength = shared as f32 + if same_cat { 1.0 } else { 0.0 };
                    edges.push((i, j, strength));
                }
            }
        }
        let mut degree = vec![0.0f32; n];
        for &(i, j, s) in &edges {
            degree[i] += s;
            degree[j] += s;
        }

        // one physics step
        let mut force = vec![Vec2::ZERO; n];
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let d = self.pos[i] - self.pos[j];
                let dist = d.length().max(1.0);
                force[i] += d / dist * (2200.0 / (dist * dist));
            }
        }
        for &(i, j, s) in &edges {
            let d = self.pos[j] - self.pos[i];
            let dist = d.length().max(1.0);
            let f = d / dist * (dist - 90.0) * 0.02 * (1.0 + s * 0.3);
            force[i] += f;
            force[j] -= f;
        }
        for i in 0..n {
            if Some(i) == self.dragging {
                continue;
            }
            self.vel[i] = (self.vel[i] + force[i] * 0.02) * 0.85;
            self.pos[i] += self.vel[i];
        }
        sh.ctx.request_repaint();

        // canvas
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        // zoom + pan (apply BEFORE building the transform closure).
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                self.zoom = (self.zoom * (1.0 + scroll * 0.001)).clamp(0.2, 4.0);
            }
        }
        if resp.dragged() && self.dragging.is_none() {
            self.pan += resp.drag_delta();
        }
        let zoom = self.zoom;
        let origin = resp.rect.center().to_vec2() + self.pan;
        let tf = |p: Pos2| -> Pos2 {
            (p.to_vec2() * zoom + origin - Vec2::splat(300.0 * zoom)).to_pos2()
        };
        let ink = sh.palette.ink;
        let faint = sh.palette.ink_faint;
        for &(i, j, s) in &edges {
            painter.line_segment(
                [tf(self.pos[i]), tf(self.pos[j])],
                Stroke::new((s * 0.4).min(2.0), faint),
            );
        }
        let pointer = resp.hover_pos();
        let mut hovered_node = None;
        for i in 0..n {
            let pinned = self.mems[i]
                .get("pinned")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let r = (2.5 + degree[i].min(12.0) * 0.55 + if pinned { 1.5 } else { 0.0 }) * self.zoom;
            let c = tf(self.pos[i]);
            let hovered = pointer.map(|p| (p - c).length() < r + 3.0).unwrap_or(false);
            if hovered {
                hovered_node = Some(i);
            }
            let fill = if hovered || pinned {
                ink
            } else {
                sh.palette.paper
            };
            painter.circle(c, r, fill, Stroke::new(if pinned { 2.0 } else { 1.0 }, ink));
            if hovered {
                let text = mem_text(&self.mems[i]);
                painter.rect_filled(
                    Rect::from_min_size(c + Vec2::new(8.0, 8.0), Vec2::new(260.0, 20.0)),
                    if sh.aero {
                        egui::Rounding::same(crate::theme::R_CTRL)
                    } else {
                        egui::Rounding::ZERO
                    },
                    sh.palette.paper,
                );
                painter.text(
                    c + Vec2::new(10.0, 10.0),
                    egui::Align2::LEFT_TOP,
                    truncate(&text, 40),
                    egui::FontId::monospace(11.0),
                    ink,
                );
                // drag start
                if resp.drag_started() {
                    self.dragging = Some(i);
                }
            }
        }
        if let Some(i) = self.dragging {
            if let Some(p) = pointer {
                self.pos[i] =
                    ((p - origin + Vec2::splat(300.0 * self.zoom)).to_vec2() / self.zoom).to_pos2();
            }
            if resp.drag_stopped() {
                self.dragging = None;
            }
        }
        resp.context_menu(|ui| {
            if let Some(i) = hovered_node {
                let m = &self.mems[i];
                let id = m.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
                let text = mem_text(m);
                let pinned = m.get("pinned").and_then(Value::as_bool).unwrap_or(false);
                let cat = m.get("category").and_then(Value::as_str).filter(|c| !c.is_empty()).unwrap_or("fact").to_string();
                if ui.button("Edit…").clicked() {
                    self.edit = Some((id.clone(), text, cat));
                    ui.close_menu();
                }
                if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                    let pin_id = id.clone();
                    self.action = Some(sh.spawn(move |a| a.pin_memory(&pin_id)));
                    ui.close_menu();
                }
                if ui.button("Delete").clicked() {
                    self.action = Some(sh.spawn(move |a| a.delete_memory(&id)));
                    ui.close_menu();
                }
            } else {
                ui.label("Right-click a node for memory actions");
            }
        });
        self.hover = hovered_node;
        self.map_overlays(ui, sh, resp.rect);
        let _ = Color32::TRANSPARENT;
    }
}

const CATEGORIES: [&str; 7] = ["fact", "preference", "person", "project", "event", "skill", "other"];

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn mid(m: &Value) -> String {
    m.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn mem_text(m: &Value) -> String {
    m.get("text")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n).collect::<String>())
    } else {
        s.to_string()
    }
}
