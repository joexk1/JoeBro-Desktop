//! Notifications panel (Mac `InboxPanel`): notices from the phone agent, tasks and agents.
use crate::api::R;
use crate::ascii::{self, Art};
use crate::icons;
use crate::models::InboxItem;
use crate::state::{Job, Shared};
use crate::theme::{ty, Palette, R_CARD, W};
use crate::widgets::{self, BtnKind};
use eframe::egui::{self, Align, Layout, Rect, Sense, Stroke, Ui, Vec2};
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

const UNDO_WINDOW: Duration = Duration::from_secs(4);

enum Items {
    Loading,
    Failed(String),
    Loaded(Vec<InboxItem>),
}

pub struct InboxPanel {
    items: Items,
    started: bool,
    load: Option<Job<R<Vec<InboxItem>>>>,
    selection: BTreeSet<String>,
    hidden: BTreeSet<String>,
    undo: Option<(String, Instant)>,
    /// Fire-and-forget writes (mark read, delete, clear); reload when each finishes.
    writes: Vec<Job<R<serde_json::Value>>>,
    approving: Option<(String, bool, Job<R<serde_json::Value>>)>,
    notes: HashMap<String, String>,
}

impl Default for InboxPanel {
    fn default() -> Self {
        Self {
            items: Items::Loading,
            started: false,
            load: None,
            selection: BTreeSet::new(),
            hidden: BTreeSet::new(),
            undo: None,
            writes: vec![],
            approving: None,
            notes: HashMap::new(),
        }
    }
}

enum Act {
    Refresh,
    SelectAll,
    ClearAll,
    Remove(Vec<String>, String),
    Read(String),
    Toggle(String),
    Approve(String, String, bool),
}

impl InboxPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        if !self.started {
            self.started = true;
            self.reload(sh);
        }
        self.poll(sh);
        let p = sh.palette;
        let acts_cell: std::cell::RefCell<Vec<Act>> = std::cell::RefCell::new(vec![]);
        let list: Vec<InboxItem> = match &self.items {
            Items::Loaded(l) => l.iter().filter(|i| !self.hidden.contains(&i.id)).cloned().collect(),
            _ => vec![],
        };
        let has_items = matches!(&self.items, Items::Loaded(l) if !l.is_empty());
        let all_ids: Vec<String> = match &self.items {
            Items::Loaded(l) => l.iter().map(|i| i.id.clone()).collect(),
            _ => vec![],
        };
        widgets::panel_chrome(
            ui,
            &p,
            "Notifications",
            "bell",
            |ui| {
                // right-to-left: last in the Mac HStack is added first
                if has_items {
                    if widgets::button_ex(ui, &p, BtnKind::Ghost, true, true, "Clear all").clicked() {
                        acts_cell.borrow_mut().push(Act::ClearAll);
                    }
                    if widgets::button_ex(ui, &p, BtnKind::Ghost, true, true, "Select all").clicked() {
                        acts_cell.borrow_mut().push(Act::SelectAll);
                    }
                }
                if widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).clicked() {
                    acts_cell.borrow_mut().push(Act::Refresh);
                }
            },
            |ui| match &self.items {
                Items::Loading => widgets::panel_loading(ui, &p),
                Items::Failed(e) => widgets::panel_failure(ui, &p, "Couldn't load the inbox", Some(e)),
                Items::Loaded(_) if list.is_empty() => ascii::empty_state(ui, &p, Art::Done, "Nothing here"),
                Items::Loaded(_) => {
                    if !self.selection.is_empty() {
                        selection_bar(ui, &p, list.len(), &mut self.selection, &all_ids, &mut acts_cell.borrow_mut());
                    }
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        ui.add_space(12.0);
                        ui.spacing_mut().item_spacing.y = 8.0;
                        let selecting = !self.selection.is_empty();
                        for n in &list {
                            let checked = self.selection.contains(&n.id);
                            let busy = self.approving.as_ref().filter(|a| a.0 == n.source).map(|a| a.1);
                            let note = self.notes.get(&n.source).cloned();
                            ui.horizontal(|ui| {
                                ui.add_space(12.0);
                                ui.vertical(|ui| {
                                    ui.set_width(ui.available_width() - 12.0);
                                    row(ui, &p, n, checked, selecting, busy, note.as_deref(), &mut acts_cell.borrow_mut());
                                });
                            });
                        }
                        ui.add_space(12.0);
                    });
                }
            },
        );
        for a in acts_cell.into_inner() {
            self.apply(sh, a);
        }
        if let Some((text, at)) = self.undo.clone() {
            if at.elapsed() >= UNDO_WINDOW {
                self.flush(sh);
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(200));
                if widgets::undo_toast(ui.ctx(), &p, &text) {
                    self.undo = None;
                    self.hidden.clear();
                }
            }
        }
    }

    fn reload(&mut self, sh: &mut Shared) {
        if self.load.is_none() {
            self.load = Some(sh.spawn(|a| a.inbox()));
        }
    }

    fn poll(&mut self, sh: &mut Shared) {
        if let Some(j) = &mut self.load {
            if let Some(r) = j.take() {
                self.load = None;
                match r {
                    Ok(list) => {
                        sh.inbox_unread = list.iter().filter(|i| !i.read).count() as i64;
                        self.items = Items::Loaded(list);
                    }
                    Err(e) => self.items = Items::Failed(e),
                }
            }
        }
        let mut done = false;
        self.writes.retain_mut(|j| match j.take() {
            Some(_) => {
                done = true;
                false
            }
            None => true,
        });
        if done && self.writes.is_empty() {
            self.reload(sh);
            if self.undo.is_none() {
                self.hidden.clear();
            }
        }
        if let Some((src, approve, j)) = &mut self.approving {
            if let Some(r) = j.take() {
                let msg = match r {
                    Ok(v) => {
                        if v.get("applied").and_then(|x| x.as_bool()) == Some(true) {
                            (if *approve { "approved" } else { "denied" }).to_string()
                        } else {
                            v.get("detail").and_then(|x| x.as_str()).unwrap_or("pending").to_string()
                        }
                    }
                    Err(e) => e,
                };
                let src = src.clone();
                self.notes.insert(src.clone(), msg);
                self.approving = None;
                self.reload(sh);
            }
        }
    }

    fn apply(&mut self, sh: &mut Shared, a: Act) {
        match a {
            Act::Refresh => self.reload(sh),
            Act::SelectAll => {
                if let Items::Loaded(l) = &self.items {
                    self.selection = l.iter().map(|i| i.id.clone()).collect();
                }
            }
            Act::ClearAll => {
                if let Items::Loaded(l) = &self.items {
                    self.hidden.extend(l.iter().map(|i| i.id.clone()));
                }
                self.undo = None;
                self.writes.push(sh.spawn(|a| {
                    let _ = a.mark_inbox_read(None);
                    a.clear_inbox()
                }));
            }
            Act::Remove(ids, label) => {
                self.flush(sh);
                for i in &ids {
                    self.selection.remove(i);
                }
                self.hidden.extend(ids);
                self.undo = Some((label, Instant::now()));
            }
            Act::Read(id) => self.writes.push(sh.spawn(move |a| a.mark_inbox_read(Some(&id)))),
            Act::Toggle(id) => {
                if !self.selection.remove(&id) {
                    self.selection.insert(id);
                }
            }
            Act::Approve(agent, _id, approve) => {
                if self.approving.is_none() {
                    let a2 = agent.clone();
                    self.approving = Some((agent, approve, sh.spawn(move |a| a.coding_decide(&a2, approve))));
                }
            }
        }
    }

    /// Delete for real whatever is hidden.
    fn flush(&mut self, sh: &mut Shared) {
        self.undo = None;
        let ids: Vec<String> = self.hidden.iter().cloned().collect();
        if ids.is_empty() {
            return;
        }
        self.writes.push(sh.spawn(move |a| a.delete_inbox(&ids)));
    }
}

fn selection_bar(ui: &mut Ui, p: &Palette, total: usize, sel: &mut BTreeSet<String>, all: &[String], acts: &mut Vec<Act>) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 36.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.accent_soft);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.line));
    ui.allocate_new_ui(
        egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(12.0, 0.0))).layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(ty::sized(format!("{} selected", sel.len()), 11.0, W::Medium).color(p.ink));
            if widgets::button_ex(ui, p, BtnKind::Ghost, true, true, if sel.len() == total { "None" } else { "All" }).clicked() {
                if sel.len() == total {
                    sel.clear();
                } else {
                    *sel = all.iter().cloned().collect();
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if widgets::icon_btn(ui, p, "xmark", 26.0, false).on_hover_text("Clear selection").clicked() {
                    sel.clear();
                }
                if widgets::icon_btn(ui, p, "trash", 26.0, false).on_hover_text("Delete selected").clicked() {
                    let ids: Vec<String> = sel.iter().cloned().collect();
                    let label = format!("Deleted {}", ids.len());
                    sel.clear();
                    acts.push(Act::Remove(ids, label));
                }
            });
        },
    );
}

/// One `SkillRowShell` row of notification content.
#[allow(clippy::too_many_arguments)]
fn row(ui: &mut Ui, p: &Palette, n: &InboxItem, checked: bool, selecting: bool, busy: Option<bool>, note: Option<&str>, acts: &mut Vec<Act>) {
    let slot = ui.painter().add(egui::Shape::Noop);
    let w = ui.available_width();
    let hovered_before = ui.rect_contains_pointer(Rect::from_min_size(ui.cursor().min, Vec2::new(w, 60.0)));
    let (mut check_rect, mut trash_rect) = (Rect::NOTHING, Rect::NOTHING);
    let inner = egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
        ui.set_width(w - 24.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let show = selecting || hovered_before || checked;
            let (r, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
            check_rect = r;
            if show {
                icons::paint_symbol(
                    ui.painter(),
                    r.center() - Vec2::new(0.0, 3.0),
                    if checked { "checkmark.circle.fill" } else { "circle" },
                    15.0,
                    if checked { p.accent } else { p.ink3 },
                );
            }
            let (ir, _) = ui.allocate_exact_size(Vec2::new(24.0, 18.0), Sense::hover());
            icons::paint_symbol(ui.painter(), ir.center(), n.icon(), 13.0, if n.read { p.ink3 } else { p.accent });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if !n.read {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 3.0, p.accent);
                    }
                    let t = if n.title.is_empty() { "Notice" } else { n.title.as_str() };
                    ui.label(ty::sized(t, 13.0, if n.read { W::Regular } else { W::Semibold }).color(if n.read { p.ink2 } else { p.ink }));
                });
                if !n.body.is_empty() {
                    ui.add(egui::Label::new(ty::sized(n.body.as_str(), 12.0, W::Regular).color(p.ink2)).selectable(true));
                }
                let fresh = widgets::iso_to_secs(&n.created_at).map_or(false, |t| {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                    now - t < 15.0 * 60.0
                });
                if n.kind == "coding_approval" && !n.source.is_empty() && !n.read && fresh {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.add_enabled_ui(busy.is_none(), |ui| {
                            if widgets::button(ui, p, BtnKind::Primary, "Approve").clicked() {
                                acts.push(Act::Approve(n.source.clone(), n.id.clone(), true));
                                acts.push(Act::Read(n.id.clone()));
                            }
                            if widgets::button(ui, p, BtnKind::Danger, "Deny").clicked() {
                                acts.push(Act::Approve(n.source.clone(), n.id.clone(), false));
                                acts.push(Act::Read(n.id.clone()));
                            }
                        });
                        if busy.is_some() {
                            ascii::spinner(ui, 12.0, p.ink3);
                        }
                        if let Some(nt) = note {
                            ui.label(ty::sized(nt, 10.5, W::Regular).color(p.ink3));
                        }
                    });
                }
                let rel = widgets::iso_to_secs(&n.created_at).map(widgets::relative_from_now).unwrap_or_default();
                ui.label(ty::sized(rel, 10.5, W::Regular).color(p.ink3));
            });
        });
    });
    let rect = inner.response.rect;
    let resp = ui.interact(rect, ui.id().with(("inbox-row", &n.id)), Sense::click());
    let hov = resp.hovered();
    if hov {
        // hover action: trash, vertically centred at the trailing edge
        trash_rect = Rect::from_center_size(egui::pos2(rect.right() - 12.0 - 13.0, rect.center().y), Vec2::splat(26.0));
        let over = ui.rect_contains_pointer(trash_rect);
        let col = if over { p.ink } else { p.ink2 };
        if over {
            ui.painter().rect_filled(trash_rect, crate::theme::R_CTRL, p.hover);
        }
        icons::paint_symbol(ui.painter(), trash_rect.center(), "trash", 13.0, col);
    }
    let fill = if checked { p.accent_soft } else if hov { p.hover } else { p.raised };
    let stroke = if checked { p.accent_a(0.4) } else { p.line };
    ui.painter().set(
        slot,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(rect, R_CARD, fill),
            egui::Shape::rect_stroke(rect.shrink(0.5), R_CARD, Stroke::new(1.0, stroke)),
        ]),
    );
    if resp.clicked() {
        let pos = resp.interact_pointer_pos().unwrap_or_default();
        if check_rect.expand(2.0).contains(pos) && (selecting || hovered_before || checked) {
            acts.push(Act::Toggle(n.id.clone()));
        } else if trash_rect.contains(pos) {
            acts.push(Act::Remove(vec![n.id.clone()], "Deleted".into()));
        } else if !n.read {
            acts.push(Act::Read(n.id.clone()));
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::Default);
}
