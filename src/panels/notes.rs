//! Shared notes and checklists backed by the JoeBro notes API.
use crate::api::R;
use crate::state::{Job, Shared};
use crate::widgets;
use eframe::egui::{self, Align, Layout, RichText, Ui};
use serde_json::{json, Value};

#[derive(Default)]
pub struct NotesPanel {
    notes: Vec<Value>,
    loaded: bool,
    load: Option<Job<R<Value>>>,
    action: Option<Job<R<Value>>>,
    error: Option<String>,
    form: Option<NoteForm>,
}

#[derive(Clone)]
struct NoteForm {
    id: Option<String>,
    title: String,
    content: String,
    checklist: bool,
    items: Vec<ChecklistItem>,
    error: Option<String>,
}

#[derive(Clone, Default)]
struct ChecklistItem {
    text: String,
    done: bool,
}

impl Default for NoteForm {
    fn default() -> Self {
        Self {
            id: None,
            title: String::new(),
            content: String::new(),
            checklist: false,
            items: vec![ChecklistItem::default()],
            error: None,
        }
    }
}

impl NotesPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        if !self.loaded && self.load.is_none() {
            self.load = Some(sh.spawn(|a| a.get("api/notes")));
        }
        if let Some(job) = &mut self.load {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) => {
                        self.notes = v
                            .get("notes")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        self.loaded = true;
                    }
                    Err(e) => {
                        self.error = Some(format!("Couldn't load notes: {e}"));
                        self.loaded = true;
                    }
                }
                self.load = None;
            }
        }
        if let Some(job) = &mut self.action {
            if let Some(result) = job.take() {
                match result {
                    Ok(_) => {
                        self.error = None;
                        self.loaded = false;
                    }
                    Err(e) => self.error = Some(e),
                }
                self.action = None;
            }
        }

        let p = sh.palette;
        let (refresh, new) = (std::cell::Cell::new(false), std::cell::Cell::new(false));
        widgets::panel_chrome(
            ui,
            &p,
            "Notes",
            "note.text",
            |ui| {
                // right-to-left: plus first (rightmost), then refresh
                new.set(widgets::icon_btn(ui, &p, "plus", 26.0, false).on_hover_text("New note").clicked());
                refresh.set(widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).clicked());
            },
            |ui| {
                if !self.loaded {
                    widgets::panel_loading(ui, &p);
                    return;
                }
                if self.notes.is_empty() {
                    match self.error.clone() {
                        Some(e) => widgets::panel_failure(ui, &p, "Couldn't load notes", Some(&e)),
                        None => crate::ascii::empty_state(ui, &p, crate::ascii::Art::Files, "No notes yet"),
                    }
                    return;
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                        // Adaptive grid: columns of at least 240 pt, 10 pt gaps.
                        let w = ui.available_width();
                        let cols = (((w + 10.0) / 250.0).floor() as usize).max(1);
                        let cw = (w - 10.0 * (cols as f32 - 1.0)) / cols as f32;
                        let notes = self.notes.clone();
                        for chunk in notes.chunks(cols) {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                for note in chunk {
                                    ui.allocate_ui_with_layout(egui::vec2(cw, 0.0), Layout::top_down(Align::Min), |ui| {
                                        ui.set_width(cw);
                                        self.note_card(ui, sh, note);
                                    });
                                }
                            });
                            ui.add_space(10.0);
                        }
                    });
                });
            },
        );
        if refresh.get() {
            self.loaded = false;
            self.error = None;
        }
        if new.get() {
            self.form = Some(NoteForm::default());
        }

        self.form_window(ui, sh);
    }

    fn note_card(&mut self, ui: &mut Ui, sh: &mut Shared, note: &Value) {
        let p = sh.palette;
        let id = str_field(note, "id").to_owned();
        let title = str_field(note, "title");
        let pinned = note.get("pinned").and_then(Value::as_bool).unwrap_or(false);
        let items = note.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        let body = str_field(note, "content");
        let top = ui.cursor().min;
        let rect_id = egui::Id::new(("note-card", &id));
        let prev: Option<egui::Rect> = ui.ctx().data(|d| d.get_temp(rect_id));
        let bg = prev.map(|r| ui.interact(r, rect_id, egui::Sense::click()));
        let cw = ui.available_width();
        widgets::card(ui, &p, 12.0, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                if pinned {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
                    widgets::paint_glyph(ui, r.center(), "\u{f08d}", 9.0, p.accent);
                }
                ui.add(egui::Label::new(crate::theme::ty::sized(if title.is_empty() { "Untitled" } else { title }, 13.0, crate::theme::W::Semibold).color(p.ink)).truncate());
            });
            if !items.is_empty() {
                for (index, item) in items.iter().enumerate() {
                    let text = str_field(item, "text");
                    let done = item.get("done").and_then(Value::as_bool).unwrap_or(false);
                    let r = ui
                        .horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let (gr, _) = ui.allocate_exact_size(egui::vec2(14.0, 16.0), egui::Sense::hover());
                            widgets::paint_glyph(ui, gr.center(), if done { "\u{f058}" } else { "\u{f10c}" }, 12.0, if done { p.accent } else { p.ink3 });
                            let mut t = RichText::new(text).size(12.0).color(if done { p.ink3 } else { p.ink });
                            if done {
                                t = t.strikethrough();
                            }
                            ui.add(egui::Label::new(t).truncate().sense(egui::Sense::click()));
                        })
                        .response
                        .interact(egui::Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if r.clicked() {
                        let i = id.clone();
                        self.action = Some(sh.spawn(move |a| a.post_json(&format!("api/notes/{i}/items/{index}/toggle"), json!({}))));
                    }
                }
            } else if !body.is_empty() {
                let shown: String = body.lines().take(6).collect::<Vec<_>>().join("\n");
                ui.label(RichText::new(shown).size(12.0).color(p.ink3));
            }
            let due = str_field(note, "due_date");
            if !due.is_empty() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (gr, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), egui::Sense::hover());
                    widgets::paint_glyph(ui, gr.center(), "\u{f0f3}", 9.5, p.ink3);
                    let t = widgets::iso_to_secs(due).map(widgets::relative_from_now).unwrap_or_else(|| due.to_string());
                    ui.label(RichText::new(t).size(10.0).color(p.ink3));
                });
            }
        });
        let rect = egui::Rect::from_min_max(top, egui::pos2(top.x + cw, ui.min_rect().bottom()));
        ui.ctx().data_mut(|d| d.insert_temp(rect_id, rect));
        if ui.rect_contains_pointer(rect) {
            let (pin_r, del_r) = (
                egui::Rect::from_min_size(egui::pos2(rect.right() - 12.0 - 54.0, rect.top() + 8.0), egui::Vec2::splat(26.0)),
                egui::Rect::from_min_size(egui::pos2(rect.right() - 12.0 - 26.0, rect.top() + 8.0), egui::Vec2::splat(26.0)),
            );
            let mut act: Option<&str> = None;
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(pin_r), |ui| {
                if widgets::icon_btn(ui, &p, if pinned { "pin.slash" } else { "pin" }, 26.0, false).on_hover_text(if pinned { "Unpin" } else { "Pin" }).clicked() {
                    act = Some("pin");
                }
            });
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(del_r), |ui| {
                if widgets::icon_btn(ui, &p, "trash", 26.0, false).on_hover_text("Delete").clicked() {
                    act = Some("del");
                }
            });
            match act {
                Some("pin") => {
                    let i = id.clone();
                    self.action = Some(sh.spawn(move |a| a.post_json(&format!("api/notes/{i}/pin"), json!({}))));
                }
                Some(_) => {
                    let i = id.clone();
                    self.action = Some(sh.spawn(move |a| a.post_json(&format!("api/notes/{i}/archive"), json!({}))));
                }
                None => {}
            }
        }
        let Some(bg) = bg else { return };
        bg.context_menu(|ui| {
            if ui.button("Edit…").clicked() {
                self.form = Some(NoteForm::from_value(note));
                ui.close_menu();
            }
            if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                let i = id.clone();
                self.action = Some(sh.spawn(move |a| a.post_json(&format!("api/notes/{i}/pin"), json!({}))));
                ui.close_menu();
            }
            if ui.button("Delete").clicked() {
                let i = id.clone();
                self.action = Some(sh.spawn(move |a| a.delete(&format!("api/notes/{i}"))));
                ui.close_menu();
            }
        });
    }

    fn form_window(&mut self, ui: &Ui, sh: &mut Shared) {
        let Some(form) = &mut self.form else {
            return;
        };
        let (mut cancel, mut submit) = (false, false);
        let editing = form.id.is_some();
        let can = !(form.title.trim().is_empty() && form.content.trim().is_empty() && form.items.iter().all(|i| i.text.trim().is_empty()));
        let esc = widgets::dialog(
            ui.ctx(),
            "note_form",
            if editing { "Edit Note" } else { "New Note" },
            None,
            420.0,
            |ui| {
                if widgets::primary(ui, sh, if editing { "Save" } else { "Add" }, can).clicked() {
                    submit = true;
                }
                if widgets::btn(ui, sh, "Cancel").clicked() {
                    cancel = true;
                }
            },
            |ui| {
                widgets::field_label(ui, "Title");
                ui.add(egui::TextEdit::singleline(&mut form.title).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    ui.label("Checklist");
                    widgets::switch(ui, sh, &mut form.checklist);
                });
                if form.checklist {
                    let n = form.items.len();
                    let mut add = false;
                    for (idx, item) in form.items.iter_mut().enumerate() {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut item.text)
                                .hint_text(format!("Item {}", idx + 1))
                                .desired_width(f32::INFINITY),
                        );
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && idx + 1 == n {
                            add = true;
                        }
                    }
                    if add || widgets::btn(ui, sh, "Add item").clicked() {
                        form.items.push(ChecklistItem::default());
                    }
                } else {
                    widgets::field_label(ui, "Content");
                    ui.add(egui::TextEdit::multiline(&mut form.content).desired_rows(7).desired_width(f32::INFINITY));
                }
                if let Some(e) = &form.error {
                    ui.colored_label(sh.palette.danger, e);
                }
            },
        );
        if submit && can {
            let body = json!({
                "title": form.title.trim(),
                "content": if form.checklist { "" } else { form.content.as_str() },
                "note_type": if form.checklist { "checklist" } else { "note" },
                "items": if form.checklist { form.items.iter().filter(|i| !i.text.trim().is_empty()).map(|i| json!({"text": i.text.trim(), "done": i.done})).collect::<Vec<_>>() } else { vec![] },
            });
            let id = form.id.clone();
            self.action = Some(sh.spawn(move |a| match id {
                Some(id) => a.put_json(&format!("api/notes/{id}"), body),
                None => a.post_json("api/notes", body),
            }));
            self.form = None;
        } else if esc || cancel {
            self.form = None;
        }
    }
}

impl NoteForm {
    fn from_value(v: &Value) -> Self {
        let items = v
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Self {
            id: v.get("id").and_then(Value::as_str).map(str::to_owned),
            title: str_field(v, "title").to_owned(),
            content: str_field(v, "content").to_owned(),
            checklist: !items.is_empty() || str_field(v, "note_type") == "checklist",
            items: items
                .iter()
                .map(|i| ChecklistItem {
                    text: str_field(i, "text").to_owned(),
                    done: i.get("done").and_then(Value::as_bool).unwrap_or(false),
                })
                .collect(),
            error: None,
        }
    }
}

fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
