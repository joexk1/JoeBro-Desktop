//! Skills (Mac SkillsPanel): search/refresh/+ header, quick-add capsule that
//! drafts a skill with AI, glass list of selectable skill cards, and the
//! New/Edit, Generate and Learn-from-Sources sheets.
use crate::api::R;
use crate::state::{Job, Shared};
use crate::panels::workkit as wk;
use crate::theme::{self, ty, W};
use crate::widgets;
use eframe::egui::{self, Align, Layout, RichText, Ui, Vec2};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct SkillsPanel {
    skills: Vec<Value>,
    load: Option<Job<R<Value>>>,
    loaded: bool,
    action: Option<Job<R<Value>>>,
    search: String,
    search_open: bool,
    selection: BTreeSet<String>,
    quick: String,

    // New / Edit sheet
    show_form: bool,
    edit_id: Option<String>,
    f_name: String,
    f_desc: String,
    f_cat: String,
    f_when: String,
    f_proc: String,
    form_error: Option<String>,
    // Generate sheet
    show_generate: bool,
    generate_prompt: String,
    generate_job: Option<Job<R<Value>>>,
    generate_error: Option<String>,
    load_error: Option<String>,
    action_error: Option<String>,

    // Learn-from-sources sheet
    show_learn: bool,
    learn_folder: Option<String>,
    learn_job: Option<Job<R<Value>>>,
    learn_poll: Option<Job<R<Value>>>,
    learn_id: Option<String>,
    learn_status: String,
    learn_made: Vec<String>,
    learn_error: Option<String>,
    learn_timer: f64,
}

fn st(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn is_active(s: &Value) -> bool {
    let status = s.get("status").and_then(Value::as_str).unwrap_or("active");
    status != "disabled" && status != "archived"
}

fn row_id(s: &Value) -> String {
    let id = st(s, "id");
    if id.is_empty() { st(s, "name") } else { id }
}

impl SkillsPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        self.pump(ui, sh);
        let p = sh.palette;
        let (mut refresh, mut pick) = (false, None);
        let sq = std::cell::RefCell::new((std::mem::take(&mut self.search), self.search_open));
        let mut quick = std::mem::take(&mut self.quick);
        let mut submitted = None;
        let err = self.action_error.clone();
        let rect = ui.max_rect();
        widgets::panel_chrome(
            ui,
            &p,
            "Skills",
            "graduationcap",
            |ui| {
                let plus = widgets::icon_btn(ui, &p, "plus", 26.0, false).on_hover_text("Add skill");
                widgets::menu_popup(ui, &p, &plus, |ui| {
                    let items = [("square.and.pencil", "Write manually\u{2026}"), ("arrow.up.doc", "Upload markdown\u{2026}"), ("sparkles", "Generate with AI\u{2026}"), ("books.vertical", "Learn from sources\u{2026}")];
                    for (i, (ic, l)) in items.iter().enumerate() {
                        if widgets::menu_item(ui, &p, Some(ic), l, false, true).clicked() {
                            pick = Some(i);
                            ui.close_menu();
                        }
                    }
                });
                refresh = widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).on_hover_text("Refresh").clicked();
                let mut g = sq.borrow_mut();
                let (t, o) = &mut *g;
                wk::search(ui, &p, t, o, "Search skills");
            },
            |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                submitted = wk::quick_add(ui, &p, &mut quick, "Describe a new skill\u{2026}", "sparkles", false, err.as_deref());
                let r = egui::Rect::from_min_max(ui.cursor().min, rect.max);
                ui.scope(|ui| {
                    widgets::surface(ui, &p, widgets::Level::Panel, Some(theme::R_PANEL), |ui| {
                        ui.set_min_size(r.size());
                        ui.set_max_size(r.size());
                        let q = sq.borrow().0.clone();
                        self.list(ui, sh, &q);
                    })
                });
            },
        );
        let (t, o) = sq.into_inner();
        self.search = t;
        self.search_open = o;
        self.quick = quick;
        if refresh {
            self.load_error = None;
            self.loaded = false;
        }
        match pick {
            Some(0) => self.open_form(None),
            Some(1) => sh.picker.open("skills_md", "Upload markdown", crate::pickers::PickMode::File(&["md", "markdown", "txt"]), dirs::document_dir()),
            Some(2) => {
                self.generate_prompt.clear();
                self.generate_error = None;
                self.show_generate = true;
            }
            Some(3) => {
                self.learn_made.clear();
                self.learn_error = None;
                self.show_learn = true;
            }
            _ => {}
        }
        if let Some(text) = submitted {
            self.generate_prompt = text;
            self.generate_error = None;
            self.show_generate = true;
            self.generate_skill(sh);
        }

        self.form_window(ui, sh);
        self.generate_window(ui, sh);
        self.learn_window(ui, sh);
    }

    fn filtered(&self, q: &str) -> Vec<Value> {
        let q = q.trim().to_lowercase();
        self.skills
            .iter()
            .filter(|s| {
                q.is_empty()
                    || format!("{} {} {}", st(s, "name"), st(s, "description"), st(s, "content")).to_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    }

    fn list(&mut self, ui: &mut Ui, sh: &mut Shared, q: &str) {
        let p = sh.palette;
        if !self.loaded {
            widgets::panel_loading(ui, &p);
            return;
        }
        if let Some(e) = self.load_error.clone() {
            widgets::panel_failure(ui, &p, "Couldn't load skills", Some(&e));
            return;
        }
        if self.skills.is_empty() {
            crate::ascii::empty_state(ui, &p, crate::ascii::Art::Files, "No skills yet");
            return;
        }
        let shown = self.filtered(q);
        if !self.selection.is_empty() {
            let ids: Vec<String> = shown.iter().map(row_id).collect();
            match wk::selection_bar(ui, &p, self.selection.len(), shown.len()) {
                wk::SelAct::All => self.selection = ids.into_iter().collect(),
                wk::SelAct::Clear => self.selection.clear(),
                wk::SelAct::Delete => {
                    let del: Vec<String> = std::mem::take(&mut self.selection).into_iter().collect();
                    self.action = Some(sh.spawn(move |a| {
                        let mut first_err = None;
                        for id in &del {
                            if let Err(e) = a.delete_skill(id) {
                                first_err.get_or_insert(e);
                            }
                        }
                        if let Some(e) = first_err {
                            return Err(e);
                        }
                        Ok(json!({"ok": true}))
                    }));
                }
                wk::SelAct::None => {}
            }
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 8.0;
                for s in &shown {
                    self.skill_row(ui, sh, s);
                }
            });
        });
    }

    fn skill_row(&mut self, ui: &mut Ui, sh: &mut Shared, s: &Value) {
        let p = sh.palette;
        let id = row_id(s);
        let active = is_active(s);
        let checked = self.selection.contains(&id);
        let selecting = !self.selection.is_empty();
        let name = if st(s, "name").is_empty() { "Unnamed skill".to_string() } else { st(s, "name") };
        let out = ui.scope(|ui| {
            if !active {
                ui.set_opacity(0.65);
            }
            wk::row_shell(ui, &p, egui::Id::new(("skill_row", &id)), checked, selecting, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.spacing_mut().interact_size.y = 0.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 7.0;
                    ui.spacing_mut().interact_size.y = 24.0;
                    let (dr, _) = ui.allocate_exact_size(Vec2::new(6.0, 6.0), egui::Sense::hover());
                    let lr = ui.label(ty::sized(&name, 13.0, W::Semibold).color(if active { p.ink } else { p.ink2 }));
                    ui.painter().circle_filled(egui::pos2(dr.center().x, lr.rect.center().y), 3.0, if active { p.live } else { p.ink3 });
                    let cat = st(s, "category");
                    if !cat.is_empty() {
                        widgets::chip_tag(ui, &p, &cat, None);
                    }
                });
                let desc = st(s, "description");
                if !desc.is_empty() {
                    ui.add(egui::Label::new(ty::sized(desc, 12.0, W::Regular).color(p.ink2)).truncate());
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.spacing_mut().interact_size.y = 0.0;
                    let mut meta = |t: String| {
                        ui.label(ty::sized(t, 10.5, W::Regular).color(p.ink3));
                    };
                    if let Some(u) = s.get("uses").and_then(Value::as_i64) {
                        meta(format!("{u} uses"));
                    }
                    if let Some(c) = s.get("confidence").and_then(Value::as_f64) {
                        meta(format!("{}%", c as i64));
                    }
                    let src = st(s, "source");
                    if !src.is_empty() {
                        meta(src);
                    }
                });
            })
        });
        let out = out.inner;
        let over = ui.input(|i| i.pointer.hover_pos()).map_or(false, |pp| out.resp.rect.contains(pp));
        if over && !selecting {
            let r = out.resp.rect;
            let bar = egui::Rect::from_min_size(egui::pos2(r.right() - 12.0 - 90.0, r.center().y - 15.0), egui::vec2(90.0, 30.0));
            ui.painter().rect_filled(bar, egui::Rounding::same(theme::R_CTRL + 1.0), p.surface);
            ui.painter().rect_stroke(bar.shrink(0.5), egui::Rounding::same(theme::R_CTRL + 1.0), egui::Stroke::new(1.0, p.line));
            let mut c = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(egui::vec2(2.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
            c.spacing_mut().item_spacing.x = 0.0;
            if widgets::icon_btn(&mut c, &p, "pencil", 26.0, false).on_hover_text("Edit").clicked() {
                self.open_form(Some(s));
            }
            if widgets::icon_btn(&mut c, &p, "power", 26.0, active).on_hover_text(if active { "Disable" } else { "Activate" }).clicked() {
                let (i, ns) = (id.clone(), if active { "disabled" } else { "active" });
                self.action = Some(sh.spawn(move |a| a.update_skill(&i, json!({ "status": ns }))));
            }
            if widgets::icon_btn(&mut c, &p, "trash", 26.0, false).on_hover_text("Delete").clicked() {
                let i = id.clone();
                self.action = Some(sh.spawn(move |a| a.delete_skill(&i)));
            }
        }
        if out.resp.double_clicked() {
            self.open_form(Some(s));
        }
        if out.toggled || (out.clicked && selecting) {
            if !self.selection.remove(&id) {
                self.selection.insert(id.clone());
            }
        }
        out.resp.context_menu(|ui| {
            if ui.button("Edit…").clicked() {
                self.open_form(Some(s));
                ui.close_menu();
            }
            if ui.button(if active { "Disable" } else { "Activate" }).clicked() {
                let (i, ns) = (id.clone(), if active { "disabled" } else { "active" });
                self.action = Some(sh.spawn(move |a| a.update_skill(&i, json!({ "status": ns }))));
                ui.close_menu();
            }
            if ui.button("Delete").clicked() {
                let i = id.clone();
                self.action = Some(sh.spawn(move |a| a.delete_skill(&i)));
                ui.close_menu();
            }
        });
    }

    fn pump(&mut self, ui: &Ui, sh: &mut Shared) {
        if !self.loaded && self.load.is_none() {
            self.load = Some(sh.spawn(|a| a.skills()));
        }
        if let Some(job) = &mut self.load {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) => {
                        self.skills = v.get("skills").and_then(Value::as_array).cloned().unwrap_or_default();
                        self.load_error = None;
                    }
                    Err(e) => self.load_error = Some(e),
                }
                self.loaded = true;
                self.load = None;
            }
        }
        if let Some(job) = &mut self.action {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) if v.get("ok").and_then(Value::as_bool) == Some(false) => {
                        self.action_error = Some(v.get("detail").and_then(Value::as_str).unwrap_or("Skill action failed").to_owned())
                    }
                    Err(e) => self.action_error = Some(e),
                    _ => self.action_error = None,
                }
                self.loaded = false;
                self.action = None;
            }
        }
        if let Some(job) = &mut self.generate_job {
            if let Some(result) = job.take() {
                match result {
                    Ok(_) => {
                        self.loaded = false;
                        self.generate_error = None;
                        self.generate_prompt.clear();
                        self.show_generate = false;
                    }
                    Err(e) => self.generate_error = Some(e),
                }
                self.generate_job = None;
            }
        }
        self.pump_learn(ui, sh);
    }

    fn open_form(&mut self, existing: Option<&Value>) {
        self.form_error = None;
        match existing {
            Some(s) => {
                self.edit_id = Some(row_id(s)).filter(|i| !i.is_empty());
                self.f_name = st(s, "name");
                self.f_desc = st(s, "description");
                self.f_cat = if st(s, "category").is_empty() { "general".into() } else { st(s, "category") };
                self.f_when = st(s, "when_to_use");
                self.f_proc = if st(s, "procedure").is_empty() { st(s, "content") } else { st(s, "procedure") };
            }
            None => {
                self.edit_id = None;
                self.f_name.clear();
                self.f_desc.clear();
                self.f_cat = "general".into();
                self.f_when.clear();
                self.f_proc.clear();
            }
        }
        self.show_form = true;
    }

    fn generate_skill(&mut self, sh: &Shared) {
        let prompt = self.generate_prompt.trim().to_owned();
        if prompt.is_empty() || self.generate_job.is_some() {
            return;
        }
        let selected = sh.selected_model.clone();
        let models = sh.models.clone();
        self.generate_error = None;
        self.generate_job = Some(sh.spawn(move |a| {
            let prefs = a.prefs()?;
            let default_model = prefs.get("default_model").and_then(Value::as_str).unwrap_or("");
            let default_endpoint = prefs.get("default_endpoint_id").and_then(Value::as_str).unwrap_or("");
            let model = models.iter().find(|m| m.model_id == default_model && m.endpoint_id == default_endpoint)
                .cloned().or(selected);
            let (model_id, endpoint_id, endpoint_url) = model.map(|m| (m.model_id, m.endpoint_id, m.endpoint_url)).unwrap_or_default();
            let session = a.create_session("__skillgen__", &model_id, &endpoint_url, &endpoint_id)?;
            let sid = session.get("id").and_then(Value::as_str)
                .ok_or_else(|| "Temporary skill generation session was not created".to_owned())?.to_owned();
            let result = (|| {
                let instruction = format!("Write a reusable agent skill for this request: {prompt:?}. Reply with strict JSON only, exactly shaped as {{\"name\":\"...\",\"description\":\"...\",\"category\":\"...\",\"when_to_use\":\"...\",\"procedure\":\"... markdown steps ...\"}}.");
                let reply = a.post_form("api/chat", &[("message", &instruction), ("session", &sid)])?;
                let text = reply.get("response").and_then(Value::as_str).ok_or_else(|| "The model did not return a response".to_owned())?;
                let start = text.find('{').ok_or_else(|| "The model didn't return JSON".to_owned())?;
                let end = text.rfind('}').ok_or_else(|| "The model didn't return a complete JSON object".to_owned())?;
                let mut body: Value = serde_json::from_str(&text[start..=end]).map_err(|e| format!("Couldn't parse generated skill: {e}"))?;
                let obj = body.as_object_mut().ok_or_else(|| "Generated skill must be a JSON object".to_owned())?;
                obj.insert("source".into(), json!("ai-generated"));
                a.add_skill(body)
            })();
            let cleanup = a.delete_session(&sid);
            match (result, cleanup) {
                (Err(e), _) => Err(e),
                (Ok(_), Err(e)) => Err(format!("Skill was created, but the temporary chat could not be removed: {e}")),
                (Ok(v), Ok(_)) => Ok(v),
            }
        }));
    }

    fn form_window(&mut self, ui: &Ui, sh: &mut Shared) {
        if !self.show_form {
            return;
        }
        let editing = self.edit_id.is_some();
        let (mut cancel, mut save) = (false, false);
        let can = !self.f_name.trim().is_empty() && !self.f_proc.trim().is_empty();
        let esc = wk::dialog(ui.ctx(), &sh.palette, "skill_form", if editing { "Edit Skill" } else { "New Skill" }, 520.0,
            |ui| {
                if wk::primary(ui, &sh.palette, if editing { "Save" } else { "Add" }, can).clicked() {
                    save = true;
                }
                if wk::ghost(ui, &sh.palette, "Cancel").clicked() {
                    cancel = true;
                }
            },
            |ui| {
                wk::label(ui, &sh.palette, "Name");
                ui.add(egui::TextEdit::singleline(&mut self.f_name).desired_width(f32::INFINITY));
                wk::label(ui, &sh.palette, "Description");
                ui.add(egui::TextEdit::singleline(&mut self.f_desc).desired_width(f32::INFINITY));
                wk::label(ui, &sh.palette, "Category");
                ui.add(egui::TextEdit::singleline(&mut self.f_cat).desired_width(f32::INFINITY));
                wk::label(ui, &sh.palette, "When to use");
                ui.add(egui::TextEdit::multiline(&mut self.f_when).desired_rows(2).desired_width(f32::INFINITY));
                wk::label(ui, &sh.palette, "Procedure (markdown)");
                egui::ScrollArea::vertical().id_salt("skill_proc").max_height(240.0).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.f_proc)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(12)
                            .desired_width(f32::INFINITY),
                    );
                });
                if let Some(e) = &self.form_error {
                    ui.colored_label(sh.palette.danger, e);
                }
            },
        );
        if esc || cancel {
            self.show_form = false;
        }
        if save && can {
            let body = json!({
                "name": self.f_name, "description": self.f_desc, "category": self.f_cat,
                "when_to_use": self.f_when, "procedure": self.f_proc, "source": "user",
            });
            self.action = Some(match self.edit_id.clone() {
                Some(id) => sh.spawn(move |a| a.update_skill(&id, body)),
                None => sh.spawn(move |a| a.add_skill(body)),
            });
            self.show_form = false;
        }
    }

    fn generate_window(&mut self, ui: &Ui, sh: &mut Shared) {
        if !self.show_generate {
            return;
        }
        let busy = self.generate_job.is_some();
        let (mut cancel, mut go) = (false, false);
        let can = !busy && !self.generate_prompt.trim().is_empty();
        let esc = widgets::dialog(
            ui.ctx(),
            "skill_generate",
            "Generate Skill",
            Some("\u{f0e7}"),
            480.0,
            |ui| {
                if wk::primary(ui, &sh.palette, if busy { "Generating…" } else { "Generate" }, can).clicked() {
                    go = true;
                }
                if wk::ghost(ui, &sh.palette, "Cancel").clicked() {
                    cancel = true;
                }
            },
            |ui| {
                ui.add_enabled(
                    !busy,
                    egui::TextEdit::multiline(&mut self.generate_prompt)
                        .hint_text("Describe the skill — e.g. \"how to review a pull request\"")
                        .desired_rows(5)
                        .desired_width(f32::INFINITY),
                );
                ui.label(RichText::new("Your default model drafts the skill, then it's saved to the library.").size(11.0).color(sh.palette.ink_dim));
                if let Some(e) = &self.generate_error {
                    ui.colored_label(sh.palette.danger, e);
                }
            },
        );
        if (esc || cancel) && !busy {
            self.show_generate = false;
        }
        if go {
            self.generate_skill(sh);
        }
    }

    fn learn_window(&mut self, ui: &Ui, sh: &mut Shared) {
        if !self.show_learn {
            return;
        }
        let running = self.learn_job.is_some() || self.learn_id.is_some();
        let done = !self.learn_made.is_empty();
        let (mut close, mut start, mut pick) = (false, false, false);
        let can = self.learn_folder.is_some() && !running;
        let esc = widgets::dialog(
            ui.ctx(),
            "skill_learn",
            "Learn from Sources",
            Some("\u{f02d}"),
            500.0,
            |ui| {
                if !done && wk::primary(ui, &sh.palette, if running { "Learning…" } else { "Learn" }, can).clicked() {
                    start = true;
                }
                if wk::ghost(ui, &sh.palette, if done { "Done" } else { "Cancel" }).clicked() {
                    close = true;
                }
            },
            |ui| {
                let p = sh.palette;
                ui.horizontal(|ui| {
                    let t = self.learn_folder.clone().unwrap_or_else(|| "No folder selected".into());
                    ui.add(egui::Label::new(RichText::new(t).color(if self.learn_folder.is_some() { p.ink } else { p.ink_dim })).truncate());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_enabled_ui(!running, |ui| {
                            if wk::ghost(ui, &sh.palette, "Choose folder…").clicked() {
                                pick = true;
                            }
                        });
                    });
                });
                ui.label(RichText::new("Your default model studies the folder's documents — exercises, analyses, structures — and saves skill(s) that teach it to do the same kind of work.").size(11.0).color(p.ink_dim));
                if running {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(14.0).color(p.accent_hi));
                        let t = if self.learn_status.is_empty() { "Working…" } else { &self.learn_status };
                        ui.label(RichText::new(t).size(11.0));
                    });
                }
                if done {
                    ui.label(RichText::new(format!("\u{2713} Learned: {}", self.learn_made.join(", "))).size(11.0).color(p.success));
                }
                if let Some(e) = &self.learn_error {
                    ui.colored_label(p.danger, e);
                }
            },
        );
        if pick {
            sh.picker.open("skills_learn", "Choose folder", crate::pickers::PickMode::Folder, None);
        }
        if let Some(dir) = sh.take_picked("skills_learn") {
            self.learn_folder = dir.to_str().map(str::to_string);
        }
        if start {
            self.learn_error = None;
            if let Some(path) = self.learn_folder.clone() {
                self.learn_status = "reading the sources…".into();
                self.learn_job = Some(sh.spawn(move |a| a.learn_skills(&path, &crate::config::device_id())));
            }
        }
        if (esc || close) && !running {
            self.show_learn = false;
        }
    }

    /// Upload markdown: opens the editor prefilled (Mac behaviour), not a direct add.
    fn upload_md(&mut self, path: std::path::PathBuf) {
        {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Skill").to_string();
                let desc = text
                    .lines()
                    .find(|l| l.starts_with('#'))
                    .map(|l| l.trim_start_matches(|c| c == '#' || c == ' ').trim().to_string())
                    .unwrap_or_default();
                self.open_form(None);
                self.f_name = name;
                self.f_desc = desc;
                self.f_proc = text;
            }
        }
    }

    fn pump_learn(&mut self, ui: &Ui, sh: &mut Shared) {
        if let Some(path) = sh.take_picked("skills_md") {
            self.upload_md(path);
        }
        if let Some(job) = &mut self.learn_job {
            if let Some(res) = job.take() {
                match res {
                    Ok(v) => {
                        self.learn_id = v.get("job").and_then(|x| x.as_str()).map(String::from);
                        self.learn_timer = 2.0;
                    }
                    Err(e) => {
                        self.learn_error = Some(e);
                        self.learn_status.clear();
                    }
                }
                self.learn_job = None;
            }
        }
        if let Some(job) = &mut self.learn_poll {
            if let Some(result) = job.take() {
                let v = match result {
                    Ok(v) => v,
                    Err(e) => {
                        self.learn_error = Some(e);
                        self.learn_status.clear();
                        self.learn_id = None;
                        self.learn_poll = None;
                        return;
                    }
                };
                match v
                    .get("status")
                    .and_then(|x| x.as_str())
                    .unwrap_or("running")
                {
                    "done" => {
                        self.learn_made = v
                            .get("skills")
                            .and_then(|x| x.as_array())
                            .map(|a| {
                                a.iter()
                                    .map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.get("name").and_then(Value::as_str).unwrap_or("skill").to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        self.learn_status.clear();
                        self.learn_id = None;
                        self.loaded = false;
                    }
                    "error" => {
                        self.learn_error = Some(
                            v.get("error").and_then(|x| x.as_str()).filter(|e| !e.is_empty()).unwrap_or("Learning failed.").to_string(),
                        );
                        self.learn_status.clear();
                        self.learn_id = None;
                    }
                    _ => {
                        self.learn_status = v
                            .get("detail")
                            .and_then(|x| x.as_str())
                            .unwrap_or("working…")
                            .to_string()
                    }
                }
                self.learn_poll = None;
            }
        }
        if let Some(id) = self.learn_id.clone() {
            self.learn_timer -= ui.input(|i| i.stable_dt) as f64;
            if self.learn_timer <= 0.0 && self.learn_poll.is_none() {
                self.learn_timer = 2.0;
                self.learn_poll = Some(sh.spawn(move |a| a.learn_skills_status(&id)));
            }
            sh.ctx
                .request_repaint_after(std::time::Duration::from_millis(500));
        }
    }
}
