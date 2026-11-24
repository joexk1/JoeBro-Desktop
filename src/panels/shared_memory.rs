//! Memory panel (Mac `SharedMemoryPanel`): the one memory surface. What you told JoeBro (You), what
//! every agent learned about a project (Projects), what applies everywhere (Global). Routed for
//! both `Tab::Memory` and `Tab::Brain` (the Mac routes `.brain` and `.memory` to the same panel).
//! The legacy personal-memory list/graph (`brain.rs`) stays compiled but is no longer routed.
use crate::api::R;
use crate::icons::{self, JbGlyph};
use crate::models::{MemFact, MemProject};
use crate::state::{Job, Shared};
use crate::theme::{self, ty, W, R_CTRL, R_PANEL};
use crate::widgets::{self, BtnKind};
use eframe::egui::{self, text::LayoutJob, Align, Color32, FontId, Layout, Rect, Sense, Stroke, TextFormat, Ui, Vec2};

const SCOPES: [&str; 4] = ["All", "You", "Projects", "Global"];
const KINDS: [&str; 6] = ["fact", "decision", "gotcha", "preference", "handoff", "learned"];
const ADD_KINDS: [&str; 4] = ["fact", "decision", "gotcha", "preference"];

type Loaded = R<(Option<Vec<MemProject>>, Vec<MemFact>)>;

#[derive(Default)]
pub struct SharedMemoryPanel {
    #[allow(dead_code)]
    pub legacy: super::brain::BrainPanel,
    scope: usize,
    projects: Vec<MemProject>,
    project: String,
    facts: Vec<MemFact>,
    search: String,
    search_open: bool,
    show_archived: bool,
    selected: Option<String>,
    note: Option<String>,
    load: Option<Job<Loaded>>,
    /// (search, project, archived) the last load was issued for.
    loaded_key: Option<(String, String, bool)>,
    act: Option<Job<R<Option<String>>>>,
    act_select: bool,
    detail: Option<Detail>,
    adding: Option<AddForm>,
}

struct Detail {
    id: String,
    text: String,
    topic: String,
    kind: String,
    history: Vec<MemFact>,
    hist: Option<Job<R<Vec<MemFact>>>>,
    error: Option<String>,
}

struct AddForm {
    text: String,
    target: String,
    kind: String,
    error: Option<String>,
    job: Option<Job<R<serde_json::Value>>>,
}

/// `RelativeDateTimeFormatter` (short, numeric) over an ISO timestamp.
pub fn relative_iso(iso: &str) -> String {
    let Some(t) = widgets::iso_to_secs(iso) else { return String::new() };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let d = t - now;
    let (n, future) = (d.abs(), d >= 0.0);
    let (v, unit) = if n < 60.0 {
        return "now".into();
    } else if n < 3600.0 {
        ((n / 60.0).round() as i64, "min")
    } else if n < 86400.0 {
        ((n / 3600.0).round() as i64, "hr")
    } else if n < 86400.0 * 7.0 {
        let days = (n / 86400.0).round() as i64;
        (days, if days == 1 { "day" } else { "days" })
    } else if n < 86400.0 * 30.0 {
        ((n / (86400.0 * 7.0)).round() as i64, "wk")
    } else if n < 86400.0 * 365.0 {
        ((n / (86400.0 * 30.0)).round() as i64, "mo")
    } else {
        ((n / (86400.0 * 365.0)).round() as i64, "yr")
    };
    if future { format!("in {v} {unit}") } else { format!("{v} {unit} ago") }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

impl SharedMemoryPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        if self.project.is_empty() {
            self.project = "__all".into();
        }
        self.poll(sh);
        let p = sh.palette;
        let rect = ui.max_rect();

        // ---- ExtrasShell header (chrome surface) ----
        let hdr = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 46.0));
        ui.painter().extend(widgets::glass_surface(&p, hdr, R_PANEL, p.panel_fill, false));
        let (mut refresh, mut add) = (false, false);
        ui.allocate_new_ui(
            egui::UiBuilder::new().max_rect(hdr.shrink2(Vec2::new(16.0, 0.0))).layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                icons::glyph(ui, JbGlyph::Memory, 16.0, p.accent);
                ui.label(ty::sized("Memory", 14.0, W::Semibold).extra_letter_spacing(-0.2).color(p.ink));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    refresh = widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).on_hover_text("Refresh").clicked();
                    add = widgets::icon_btn(ui, &p, "plus", 26.0, false).on_hover_text("Add to memory").clicked();
                    search_box(ui, &p, &mut self.search, &mut self.search_open, "Search memory");
                });
            },
        );
        if refresh {
            self.reload(sh);
        }
        if add {
            self.adding = Some(AddForm {
                text: String::new(),
                target: if self.project != "__all" { self.project.clone() } else { "user".into() },
                kind: "fact".into(),
                error: None,
                job: None,
            });
        }

        // ---- scope row ----
        let row = Rect::from_min_size(egui::pos2(rect.left(), hdr.bottom() + 12.0), Vec2::new(rect.width(), 28.0));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (i, s) in SCOPES.iter().enumerate() {
                let kind = if self.scope == i { BtnKind::Secondary } else { BtnKind::Ghost };
                if widgets::button_ex(ui, &p, kind, true, true, *s).clicked() {
                    self.scope = i;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if let Some(n) = &self.note {
                    ui.label(ty::sized(n.as_str(), 11.0, W::Regular).color(p.danger));
                }
                let k = if self.show_archived { BtnKind::Secondary } else { BtnKind::Ghost };
                if widgets::button_ex(ui, &p, k, true, true, "Archived").clicked() {
                    self.show_archived = !self.show_archived;
                }
                if self.projects.len() > 2 && (self.scope == 0 || self.scope == 2) {
                    let mut items = vec![("__all".to_string(), "Every project".to_string())];
                    items.extend(self.projects.iter().filter(|x| x.key != "global" && x.key != "user").map(|x| (x.key.clone(), x.label())));
                    let cur = items.iter().find(|i| i.0 == self.project).map(|i| i.1.clone()).unwrap_or_else(|| "Every project".into());
                    if let Some(k) = dropdown(ui, &p, "mem-project", &cur, true, &items, &self.project) {
                        self.project = k;
                    }
                }
            });
        });

        // auto-reload when query/project/archived changed
        let key = (self.search.clone(), self.project.clone(), self.show_archived);
        if self.load.is_none() && self.loaded_key.as_ref() != Some(&key) {
            self.reload(sh);
        }

        // ---- list + detail ----
        let body = Rect::from_min_max(egui::pos2(rect.left(), row.bottom() + 12.0), rect.max);
        let left = Rect::from_min_size(body.min, Vec2::new(400.0_f32.min(body.width() * 0.5), body.height()));
        let right = Rect::from_min_max(egui::pos2(left.right() + 12.0, body.top()), body.max);
        ui.painter().extend(widgets::glass_surface(&p, left, R_PANEL, p.panel_fill, false));
        ui.painter().extend(widgets::glass_surface(&p, right, R_PANEL, p.panel_fill, false));
        let shown = self.shown();
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(left.shrink(8.0)), |ui| {
            ui.set_clip_rect(left.intersect(ui.clip_rect()));
            self.list(ui, sh, &shown);
        });
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right), |ui| {
            ui.set_clip_rect(right.intersect(ui.clip_rect()));
            self.detail_ui(ui, sh, right);
        });
        ui.advance_cursor_after_rect(rect);

        self.add_sheet(ui.ctx(), sh);
    }

    fn shown(&self) -> Vec<MemFact> {
        let want = match self.scope {
            1 => Some("user"),
            2 => Some("project"),
            3 => Some("global"),
            _ => None,
        };
        let mut v: Vec<MemFact> = self.facts.iter().filter(|f| want.map_or(true, |w| f.scope == w)).cloned().collect();
        // stable: pinned first, then strength (descending)
        v.sort_by(|a, b| {
            (b.pinned, b.strength_value()).partial_cmp(&(a.pinned, a.strength_value())).unwrap_or(std::cmp::Ordering::Equal)
        });
        v
    }

    fn poll(&mut self, sh: &mut Shared) {
        if let Some(j) = &mut self.load {
            if let Some(r) = j.take() {
                match r {
                    Ok((projects, facts)) => {
                        if let Some(pr) = projects {
                            self.projects = pr;
                        }
                        self.facts = facts;
                        self.note = None;
                    }
                    Err(e) if e.contains("404") => {
                        self.facts.clear();
                        self.note = None;
                    }
                    Err(e) => self.note = Some(e),
                }
                self.load = None;
            }
        }
        if let Some(j) = &mut self.act {
            if let Some(r) = j.take() {
                self.act = None;
                match r {
                    Ok(new_id) => {
                        if let (true, Some(id)) = (self.act_select, new_id) {
                            self.selected = Some(id);
                        }
                        self.reload(sh);
                    }
                    Err(e) => {
                        if let Some(d) = &mut self.detail {
                            d.error = Some(e.clone());
                        }
                        self.note = Some(e);
                    }
                }
            }
        }
        if let Some(d) = &mut self.detail {
            if let Some(j) = &mut d.hist {
                if let Some(r) = j.take() {
                    d.history = r.unwrap_or_default();
                    d.hist = None;
                }
            }
        }
    }

    fn reload(&mut self, sh: &mut Shared) {
        if self.load.is_some() {
            return;
        }
        let (project, status, query) = (self.project.clone(), if self.show_archived { "all" } else { "active" }, self.search.clone());
        let need = self.projects.is_empty();
        self.loaded_key = Some((query.clone(), project.clone(), self.show_archived));
        self.load = Some(sh.spawn(move |a| {
            let projects = if need { Some(a.memory_projects()?) } else { None };
            Ok((projects, a.memory_facts(&project, status, &query)?))
        }));
    }

    fn list(&mut self, ui: &mut Ui, sh: &mut Shared, shown: &[MemFact]) {
        let p = sh.palette;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            if shown.is_empty() {
                ui.allocate_ui(Vec2::new(ui.available_width(), 240.0), |ui| {
                    crate::ascii::empty_state(ui, &p, crate::ascii::Art::Inbox, "Nothing here yet");
                });
            }
            for f in shown {
                let selected = self.selected.as_deref() == Some(f.id.as_str());
                let r = ui
                    .scope(|ui| {
                        if f.is_archived() {
                            ui.set_opacity(0.5);
                        }
                        widgets::hover_row(ui, &p, selected, 8.0, |ui| fact_row(ui, &p, f))
                    })
                    .inner;
                if r.clicked() {
                    self.selected = Some(f.id.clone());
                }
                r.context_menu(|ui| {
                    let id = f.id.clone();
                    if widgets::menu_item(ui, &p, None, if f.pinned { "Unpin" } else { "Pin" }, false, true).clicked() {
                        let pin = !f.pinned;
                        self.act = Some(sh.spawn(move |a| a.memory_pin(&id, pin).map(|_| None)));
                        self.act_select = false;
                        ui.close_menu();
                    } else if f.is_archived() {
                        if widgets::menu_item(ui, &p, None, "Restore", false, true).clicked() {
                            let id = f.id.clone();
                            self.act = Some(sh.spawn(move |a| a.memory_restore(&id).map(|_| None)));
                            self.act_select = false;
                            ui.close_menu();
                        }
                    } else if widgets::menu_item(ui, &p, None, "Archive", false, true).clicked() {
                        let id = f.id.clone();
                        self.act = Some(sh.spawn(move |a| a.memory_archive(&id).map(|_| None)));
                        self.act_select = false;
                        ui.close_menu();
                    }
                });
            }
        });
    }

    fn detail_ui(&mut self, ui: &mut Ui, sh: &mut Shared, rect: Rect) {
        let p = sh.palette;
        let fact = self.selected.as_ref().and_then(|id| self.facts.iter().find(|f| &f.id == id)).cloned();
        let Some(fact) = fact else {
            crate::ascii::empty_state(ui, &p, crate::ascii::Art::Search, "Pick a memory");
            return;
        };
        if self.detail.as_ref().map(|d| d.id.as_str()) != Some(fact.id.as_str()) {
            let id = fact.id.clone();
            self.detail = Some(Detail {
                id: id.clone(),
                text: fact.text.clone(),
                topic: fact.topic.clone(),
                kind: if fact.kind.is_empty() { "fact".into() } else { fact.kind.clone() },
                history: vec![],
                hist: Some(sh.spawn(move |a| a.memory_history(&id))),
                error: None,
            });
        }
        let d = self.detail.as_mut().unwrap();
        let dirty = d.text != fact.text || d.topic != fact.topic || d.kind != (if fact.kind.is_empty() { "fact".to_string() } else { fact.kind.clone() });
        let (mut save, mut toggle_arch, mut toggle_pin) = (false, false, false);
        let bar_h = 50.0;
        let scroll_rect = Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.bottom() - bar_h));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(scroll_rect), |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let w = (ui.available_width() - 40.0).min(720.0).max(100.0);
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        ui.spacing_mut().item_spacing.y = 24.0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let items: Vec<(String, String)> = KINDS.iter().map(|k| (k.to_string(), cap(k))).collect();
                            if let Some(k) = dropdown(ui, &p, "mem-kind", &cap(&d.kind), false, &items, &d.kind) {
                                d.kind = k;
                            }
                            if fact.pinned {
                                icons::symbol(ui, "pin.fill", 11.0, p.accent);
                            }
                            if fact.is_archived() {
                                ui.label(ty::sized("Archived", 12.0, W::Regular).color(p.ink3));
                            }
                        });
                        ui.add(
                            egui::TextEdit::singleline(&mut d.topic)
                                .frame(false)
                                .hint_text(ty::sized("Topic", 20.0, W::Semibold).color(p.ink3))
                                .font(theme::jb(20.0, W::Semibold))
                                .text_color(p.ink)
                                .desired_width(f32::INFINITY),
                        );
                        ui.add(
                            egui::TextEdit::multiline(&mut d.text)
                                .frame(false)
                                .font(theme::jb(15.0, W::Regular))
                                .text_color(p.ink)
                                .desired_rows(6)
                                .desired_width(f32::INFINITY),
                        );
                        widgets::card(ui, &p, 16.0, |ui| {
                            ui.set_width(ui.available_width());
                            egui::Grid::new("mem-stats").num_columns(2).spacing([24.0, 12.0]).min_col_width((w - 32.0 - 24.0) / 2.0).show(ui, |ui| {
                                let prov = fact.provenance_line();
                                stat(ui, &p, "Saved by", if prov.is_empty() { "Unknown" } else { &prov });
                                stat(ui, &p, "Updated", &relative_iso(&fact.updated_at));
                                ui.end_row();
                                stat(ui, &p, "Strength", &format!("{}%", (fact.strength_value() * 100.0) as i64));
                                stat(ui, &p, "Recalled", &format!("{} times", fact.recalls));
                                ui.end_row();
                            });
                        });
                        if d.history.len() > 1 {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 12.0;
                                ui.label(ty::sized("Earlier versions", 13.0, W::Semibold).color(p.ink2));
                                for h in &d.history[..d.history.len() - 1] {
                                    ui.horizontal_top(|ui| {
                                        ui.spacing_mut().item_spacing.x = 10.0;
                                        let (r, _) = ui.allocate_exact_size(Vec2::new(5.0, 20.0), Sense::hover());
                                        ui.painter().circle_filled(egui::pos2(r.center().x, r.top() + 10.0), 2.5, p.ink3);
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 2.0;
                                            ui.label(ty::sized(h.text.as_str(), 13.0, W::Regular).color(p.ink2));
                                            ui.label(ty::sized(relative_iso(&h.updated_at), 11.5, W::Regular).color(p.ink3));
                                        });
                                    });
                                }
                            });
                        }
                        ui.add_space(20.0);
                    });
                });
            });
        });

        // ---- actions bar ----
        let bar = Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - bar_h), rect.max);
        ui.painter().hline(bar.x_range(), bar.top(), Stroke::new(1.0, p.line));
        ui.allocate_new_ui(
            egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(16.0, 0.0))).layout(Layout::right_to_left(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if widgets::button_ex(ui, &p, BtnKind::Primary, false, dirty, "Save").clicked() {
                    save = true;
                }
                if widgets::button(ui, &p, BtnKind::Secondary, if fact.pinned { "Unpin" } else { "Pin" }).clicked() {
                    toggle_pin = true;
                }
                if fact.is_archived() {
                    if widgets::button(ui, &p, BtnKind::Secondary, "Restore").clicked() {
                        toggle_arch = true;
                    }
                } else if widgets::button(ui, &p, BtnKind::Ghost, "Archive").clicked() {
                    toggle_arch = true;
                }
                if let Some(e) = &d.error {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(egui::Label::new(ty::sized(e.as_str(), 12.0, W::Regular).color(p.danger)).truncate());
                    });
                }
            },
        );
        if dirty && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            save = true;
        }
        let id = fact.id.clone();
        if save && dirty {
            let (t, tp, k) = (d.text.clone(), d.topic.clone(), d.kind.clone());
            self.act = Some(sh.spawn(move |a| a.memory_edit(&id, &t, &tp, &k)));
            self.act_select = true;
        } else if toggle_pin {
            let pin = !fact.pinned;
            self.act = Some(sh.spawn(move |a| a.memory_pin(&id, pin).map(|_| None)));
            self.act_select = false;
        } else if toggle_arch {
            let restore = fact.is_archived();
            self.act = Some(sh.spawn(move |a| if restore { a.memory_restore(&id) } else { a.memory_archive(&id) }.map(|_| None)));
            self.act_select = false;
        }
    }

    fn add_sheet(&mut self, ctx: &egui::Context, sh: &mut Shared) {
        let p = sh.palette;
        let projects = self.projects.clone();
        let Some(f) = &mut self.adding else { return };
        if let Some(j) = &mut f.job {
            if let Some(r) = j.take() {
                match r {
                    Ok(_) => {
                        self.adding = None;
                        self.loaded_key = None;
                        return;
                    }
                    Err(e) => {
                        f.error = Some(e);
                        f.job = None;
                    }
                }
            }
        }
        let (mut cancel, mut submit) = (false, false);
        let can = !f.text.trim().is_empty() && f.job.is_none();
        let esc = widgets::dialog(
            ctx,
            "mem_add",
            "Add to memory",
            None,
            480.0,
            |ui| {
                if widgets::button_ex(ui, &p, BtnKind::Primary, true, can, "Add").clicked() {
                    submit = true;
                }
                if widgets::button_ex(ui, &p, BtnKind::Ghost, true, true, "Cancel").clicked() {
                    cancel = true;
                }
            },
            |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut f.text)
                        .hint_text("What should every agent know?")
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.vertical(|ui| {
                        ui.label(ty::sized("Applies to", 11.0, W::Medium).color(p.ink3));
                        let mut items = vec![("user".to_string(), "You".to_string()), ("global".to_string(), "Everywhere".to_string())];
                        items.extend(projects.iter().filter(|x| x.key != "global" && x.key != "user").map(|x| (x.key.clone(), x.label())));
                        let cur = items.iter().find(|i| i.0 == f.target).map(|i| i.1.clone()).unwrap_or_else(|| f.target.clone());
                        if let Some(k) = plain_dropdown(ui, &p, &cur, &items, &f.target) {
                            f.target = k;
                        }
                    });
                    ui.vertical(|ui| {
                        ui.label(ty::sized("Kind", 11.0, W::Medium).color(p.ink3));
                        let items: Vec<(String, String)> = ADD_KINDS.iter().map(|k| (k.to_string(), k.to_string())).collect();
                        if let Some(k) = plain_dropdown(ui, &p, &f.kind.clone(), &items, &f.kind) {
                            f.kind = k;
                        }
                    });
                });
                if let Some(e) = &f.error {
                    ui.label(ty::sized(e.as_str(), 11.0, W::Regular).color(p.danger));
                }
            },
        );
        if submit && can {
            let (t, k, target) = (f.text.trim().to_string(), f.kind.clone(), f.target.clone());
            f.job = Some(sh.spawn(move |a| a.memory_add(&target, &t, &k)));
        } else if cancel || esc {
            self.adding = None;
        }
    }
}

/// One `MemoryRow`.
fn fact_row(ui: &mut Ui, p: &crate::theme::Palette, f: &MemFact) {
    ui.add_space(7.0);
    ui.spacing_mut().item_spacing.y = 5.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let t = if f.topic.is_empty() { f.kind_label() } else { f.topic.clone() };
        ui.add(egui::Label::new(ty::sized(t, 12.0, W::Medium).color(p.ink2)).truncate());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(ty::sized(relative_iso(&f.updated_at), 12.0, W::Regular).color(p.ink3));
            if f.pinned {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(5.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 2.5, p.accent);
            }
        });
    });
    clamped(ui, &f.text, theme::jb(14.0, W::Regular), p.ink, 4);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (r, _) = ui.allocate_exact_size(Vec2::new(36.0, 14.0), Sense::hover());
        let bar = Rect::from_center_size(r.center(), Vec2::new(36.0, 2.0));
        ui.painter().rect_filled(bar, 1.0, p.line);
        let fill = Rect::from_min_size(bar.min, Vec2::new(36.0 * f.strength_value() as f32, 2.0));
        ui.painter().rect_filled(fill, 1.0, p.accent_a(0.8));
        let prov = f.provenance_line();
        if !prov.is_empty() {
            ui.add(egui::Label::new(ty::sized(prov, 12.0, W::Regular).color(p.ink3)).truncate());
        }
    });
    ui.add_space(7.0);
}

fn clamped(ui: &mut Ui, text: &str, font: FontId, color: Color32, rows: usize) {
    let mut job = LayoutJob::single_section(text.to_string(), TextFormat { font_id: font, color, ..Default::default() });
    job.wrap.max_width = ui.available_width();
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('\u{2026}');
    ui.add(egui::Label::new(job).wrap());
}

fn stat(ui: &mut Ui, p: &crate::theme::Palette, k: &str, v: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.label(ty::sized(k, 12.0, W::Regular).color(p.ink3));
        ui.add(egui::Label::new(ty::sized(v, 14.0, W::Medium).color(p.ink)).truncate());
    });
}

/// `PanelSearchField`: icon button that expands into a field.
pub fn search_box(ui: &mut Ui, p: &crate::theme::Palette, text: &mut String, open: &mut bool, hint: &str) {
    if !*open {
        if widgets::icon_btn(ui, p, "magnifyingglass", 26.0, false).on_hover_text("Search").clicked() {
            *open = true;
            ui.memory_mut(|m| m.request_focus(egui::Id::new(("panel-search", hint))));
        }
        return;
    }
    let slot = ui.painter().add(egui::Shape::Noop);
    let id = egui::Id::new(("panel-search", hint));
    let mut focused = false;
    let mut close = false;
    let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(10.0, 5.0)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            icons::symbol(ui, "magnifyingglass", 11.0, p.ink3);
            let r = ui.add(
                egui::TextEdit::singleline(text)
                    .id(id)
                    .frame(false)
                    .hint_text(hint)
                    .font(theme::jb(12.0, W::Regular))
                    .desired_width(150.0),
            );
            focused = r.has_focus();
            if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            if icons::symbol(ui, "xmark", 10.0, p.ink3).interact(Sense::click()).clicked() {
                close = true;
            }
        });
    });
    let r = inner.response.rect;
    ui.painter().set(
        slot,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(r, R_CTRL, p.raised),
            egui::Shape::rect_stroke(r.shrink(0.5), R_CTRL, Stroke::new(1.0, if focused { p.accent_a(0.7) } else { p.line })),
        ]),
    );
    if close {
        text.clear();
        *open = false;
    }
}

/// A `Picker`/`JBMenu` trigger + menu. `tinted` = accent-soft pill, otherwise a field-style box.
/// Returns the chosen key.
fn dropdown(ui: &mut Ui, p: &crate::theme::Palette, id: &str, label: &str, tinted: bool, items: &[(String, String)], cur: &str) -> Option<String> {
    let font = theme::jb(12.0, if tinted { W::Regular } else { W::Semibold });
    let color = p.accent;
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let size = Vec2::new(galley.size().x + 9.0 * 2.0 + 14.0, if tinted { 28.0 } else { 24.0 });
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let r = if tinted { 8.0 } else { rect.height() / 2.0 };
    ui.painter().rect_filled(rect, r, p.accent_soft);
    ui.painter().galley(egui::pos2(rect.left() + 9.0, rect.center().y - galley.size().y / 2.0), galley, color);
    icons::paint_symbol(
        ui.painter(),
        egui::pos2(rect.right() - 12.0, rect.center().y),
        if tinted { "chevron.up.chevron.down" } else { "chevron.down" },
        if tinted { 11.0 } else { 8.0 },
        color,
    );
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let mut out = None;
    let _ = id;
    widgets::menu_popup(ui, p, &resp, |ui| {
        for (k, l) in items {
            if widgets::menu_item(ui, p, None, l, k == cur, true).clicked() {
                out = Some(k.clone());
                ui.memory_mut(|m| m.close_popup());
            }
        }
    });
    out
}

/// A plain `Picker` (macOS pop-up button): raised field box, ink label, up/down chevrons.
fn plain_dropdown(ui: &mut Ui, p: &crate::theme::Palette, label: &str, items: &[(String, String)], cur: &str) -> Option<String> {
    let galley = ui.painter().layout_no_wrap(label.to_string(), theme::jb(12.5, W::Regular), p.ink);
    let size = Vec2::new((galley.size().x + 10.0 + 22.0).max(120.0), 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.hovered() { p.hover } else { p.raised };
    ui.painter().rect_filled(rect, R_CTRL, fill);
    ui.painter().rect_stroke(rect.shrink(0.5), R_CTRL, Stroke::new(1.0, p.line));
    ui.painter().galley(egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0), galley, p.ink);
    icons::paint_symbol(ui.painter(), egui::pos2(rect.right() - 12.0, rect.center().y), "chevron.up.chevron.down", 9.0, p.ink2);
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let mut out = None;
    widgets::menu_popup(ui, p, &resp, |ui| {
        for (k, l) in items {
            if widgets::menu_item(ui, p, None, l, k == cur, true).clicked() {
                out = Some(k.clone());
                ui.memory_mut(|m| m.close_popup());
            }
        }
    });
    out
}
