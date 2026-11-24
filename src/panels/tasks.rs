//! Tasks: scheduled model prompts, or a coding agent started on a schedule (Night Shift).
//! Mirrors TasksPanel.swift: ExtrasShell header (Tasks / Personal agent scope, search,
//! refresh, new), quick-add bar, selectable rows with a context menu, new/edit sheets.
use crate::api::R;
use crate::icons::{self, JbGlyph};
use crate::models::NightHost;
use crate::panels::workkit as wk;
use crate::state::{Job, Shared};
use crate::theme::{self, ty, W};
use crate::widgets::{self, BtnKind};
use eframe::egui::{self, Ui};
use serde_json::{json, Value};

const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// First non-empty string among `keys` (the backend uses `name`/`scheduled_time`/`repeat_day`).
fn gs<'a>(v: &'a Value, keys: &[&str]) -> &'a str {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
            if !s.is_empty() {
                return s;
            }
        }
    }
    ""
}

fn gi(v: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|k| v.get(*k).and_then(|x| x.as_i64()))
}

/// Body shared by create/update: both the Mac field names and the legacy ones.
fn form_body(f: &TaskForm, personal: bool) -> Value {
    let name = {
        let t = f.title.trim();
        if t.is_empty() { f.prompt.trim().chars().take(40).collect::<String>() } else { t.to_string() }
    };
    let mut b = json!({
        "name": name, "title": name,
        "prompt": f.prompt.trim(),
        "task_type": "llm",
        "schedule": f.schedule,
        "scheduled_time": f.time.trim(), "time": f.time.trim(),
        "permission_mode": f.mode,
        "repeat_day": f.weekday, "weekday": f.weekday,
        "trigger_type": "schedule",
        "output_target": "session",
    });
    if personal {
        b["scope"] = json!("personal");
    }
    if f.id.as_deref().map_or(true, |i| !i.starts_with("ptask_")) && !personal {
        if f.coding {
            b["coding_agent"] = json!(f.agent);
            b["coding_repo"] = json!(f.repo);
            b["coding_host"] = json!(f.host);
            b["coding_model"] = json!(f.model);
        } else {
            for k in ["coding_agent", "coding_repo", "coding_host", "coding_model"] {
                b[k] = json!("");
            }
        }
    }
    b
}

/// State of the New/Edit sheet. `id == None` means creating.
struct TaskForm {
    id: Option<String>,
    title: String,
    prompt: String,
    schedule: String,
    time: String,
    weekday: i64,
    mode: String,
    coding: bool,
    agent: String,
    model: String,
    host: String,
    repo: String,
    allow_coding: bool,
    error: Option<String>,
}

impl TaskForm {
    fn new(personal: bool) -> Self {
        TaskForm {
            id: None,
            title: String::new(),
            prompt: String::new(),
            schedule: "daily".into(),
            time: "09:00".into(),
            weekday: 0,
            mode: "sandbox".into(),
            coding: false,
            agent: "claude".into(),
            model: String::new(),
            host: String::new(),
            repo: String::new(),
            allow_coding: !personal,
            error: None,
        }
    }
    fn from_task(t: &Value) -> Self {
        let id = t.get("id").and_then(|x| x.as_str()).map(str::to_string);
        let or = |s: &str, d: &str| if s.is_empty() { d.to_string() } else { s.to_string() };
        let agent = gs(t, &["coding_agent"]);
        TaskForm {
            allow_coding: !id.as_deref().unwrap_or("").starts_with("ptask_"),
            id,
            title: gs(t, &["name", "title"]).to_string(),
            prompt: gs(t, &["prompt"]).to_string(),
            schedule: or(gs(t, &["schedule"]), "daily"),
            time: or(gs(t, &["scheduled_time", "time"]), "09:00"),
            weekday: gi(t, &["repeat_day", "weekday"]).unwrap_or(0).clamp(0, 6),
            mode: or(gs(t, &["permission_mode"]), "sandbox"),
            coding: !agent.is_empty(),
            agent: or(agent, "claude"),
            model: gs(t, &["coding_model"]).to_string(),
            host: gs(t, &["coding_host"]).to_string(),
            repo: gs(t, &["coding_repo"]).to_string(),
            error: None,
        }
    }
    fn can_save(&self) -> bool {
        !self.prompt.is_empty() && (!self.coding || !self.repo.is_empty())
    }
}

#[derive(Default)]
pub struct TasksPanel {
    tasks: Vec<Value>,
    hosts: Vec<NightHost>,
    host_job: Option<Job<Vec<NightHost>>>,
    load: Option<Job<R<Value>>>,
    loaded: bool,
    load_err: Option<String>,
    note: Option<String>,
    search: String,
    search_open: bool,
    phone_scope: bool,

    form: Option<TaskForm>,
    save: Option<Job<R<Value>>>,
    action: Option<Job<R<Value>>>,
    run_job: Option<Job<R<Value>>>,

    quick: String,
    gen_job: Option<Job<R<Value>>>,
    gen_err: Option<String>,

    selection: std::collections::HashSet<String>,
}

impl TasksPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        self.drain(sh);
        if !self.loaded && self.load.is_none() {
            self.reload(sh);
        }
        let p = sh.palette;
        let (mut refresh, mut new, mut scope, mut starter) = (false, false, None, None);
        let phone = self.phone_scope;
        let sq = std::cell::RefCell::new((std::mem::take(&mut self.search), self.search_open));
        wk::shell(
            ui,
            &p,
            "Tasks",
            JbGlyph::Tasks,
            |ui| {
                new = widgets::icon_btn(ui, &p, "plus", 26.0, false).on_hover_text("New task").clicked();
                refresh = widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).on_hover_text("Refresh").clicked();
                {
                    let mut g = sq.borrow_mut();
                    let (t, o) = &mut *g;
                    wk::search(ui, &p, t, o, "Search tasks");
                }
                if phone {
                    let t = icons::glyph(ui, JbGlyph::Spark, 14.0, p.ink2);
                    let t = ui.interact(t.rect.expand(6.0), t.id.with("st"), egui::Sense::click()).on_hover_text("Starter tasks");
                    widgets::menu_popup(ui, &p, &t, |ui| {
                        if widgets::menu_item(ui, &p, None, "Morning briefing", false, true).clicked() {
                            starter = Some("morning");
                            ui.close_menu();
                        }
                        if widgets::menu_item(ui, &p, None, "Evening wrap-up", false, true).clicked() {
                            starter = Some("evening");
                            ui.close_menu();
                        }
                    });
                }
                scope = wk::seg_rtl(ui, &p, &["Tasks", "Personal agent"], phone as usize);
            },
            |ui| {
                let q = sq.borrow().0.clone();
                self.content(ui, sh, &q)
            },
        );
        let (t, o) = sq.into_inner();
        self.search = t;
        self.search_open = o;
        if let Some(s) = scope {
            if (s == 1) != self.phone_scope {
                self.phone_scope = s == 1;
                self.selection.clear();
                self.loaded = false;
                self.tasks.clear();
                self.reload(sh);
            }
        }
        if let Some(id) = starter {
            self.action = Some(sh.spawn(move |a| a.create_starter_task(id)));
        }
        if refresh {
            self.reload(sh);
        }
        if new {
            self.form = Some(TaskForm::new(self.phone_scope));
        }
        self.form_window(ui, sh);
    }

    fn content(&mut self, ui: &mut Ui, sh: &mut Shared, search: &str) {
        let p = sh.palette;
        ui.spacing_mut().item_spacing.y = theme::S3;
        if !self.phone_scope {
            let busy = self.gen_job.is_some();
            let err = self.gen_err.clone();
            if let Some(text) = wk::quick_add(ui, &p, &mut self.quick, "Describe a new task", "arrow.up", busy, None) {
                self.quick = text;
                self.generate(sh);
            }
            let _ = err;
        }
        if let Some(m) = self.gen_err.clone().or_else(|| self.note.clone()) {
            wk::danger_text(ui, &p, &m);
        }
        let q = search.trim().to_lowercase();
        let visible: Vec<Value> = self
            .tasks
            .iter()
            .filter(|t| q.is_empty() || format!("{} {}", gs(t, &["name", "title"]), gs(t, &["prompt"])).to_lowercase().contains(&q))
            .cloned()
            .collect();
        let rect = egui::Rect::from_min_max(ui.cursor().min, ui.max_rect().max);
        ui.scope(|ui| widgets::surface(ui, &p, widgets::Level::Panel, Some(theme::R_PANEL), |ui| {
            ui.set_min_size(rect.size());
            ui.set_max_size(rect.size());
            if self.load.is_some() && !self.loaded {
                widgets::panel_loading(ui, &p);
            } else if let Some(e) = self.load_err.clone() {
                crate::ascii::empty_state(ui, &p, crate::ascii::Art::Plug, &e);
            } else if self.tasks.is_empty() {
                crate::ascii::empty_state(ui, &p, crate::ascii::Art::Inbox, if self.phone_scope { "No personal-agent tasks" } else { "No tasks" });
            } else {
                if !self.selection.is_empty() {
                    self.selection_bar(ui, sh, &visible);
                }
                egui::ScrollArea::vertical().id_salt("tasks_scroll").auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::none().inner_margin(egui::Margin::same(theme::S3)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = theme::S2;
                        for t in &visible {
                            self.task_row(ui, sh, t);
                        }
                    });
                });
            }
        }));
    }

    // ---- jobs ---------------------------------------------------------------

    fn drain(&mut self, sh: &mut Shared) {
        if let Some(job) = &mut self.host_job {
            if let Some(h) = job.take() {
                self.hosts = h;
                self.host_job = None;
            }
        }
        if let Some(job) = &mut self.load {
            if let Some(res) = job.take() {
                match res {
                    Ok(v) => {
                        self.tasks = v.get("tasks").and_then(|x| x.as_array()).cloned().or_else(|| v.as_array().cloned()).unwrap_or_default();
                        self.load_err = None;
                    }
                    Err(e) => self.load_err = Some(e),
                }
                self.loaded = true;
                self.load = None;
            }
        }
        if let Some(job) = &mut self.action {
            if let Some(res) = job.take() {
                self.action = None;
                match res {
                    Err(e) => self.gen_err = Some(e),
                    Ok(v) if v.get("ok").and_then(Value::as_bool) == Some(false) => {
                        self.gen_err = Some(v.get("detail").and_then(Value::as_str).unwrap_or("Task action failed").to_owned());
                    }
                    Ok(_) => self.gen_err = None,
                }
                self.reload(sh);
            }
        }
        if let Some(job) = &mut self.save {
            if let Some(res) = job.take() {
                self.save = None;
                match res {
                    Ok(_) => {
                        self.form = None;
                        self.reload(sh);
                    }
                    Err(e) => {
                        if let Some(f) = &mut self.form {
                            f.error = Some(e);
                        }
                    }
                }
            }
        }
        if let Some(job) = &mut self.run_job {
            if let Some(res) = job.take() {
                self.run_job = None;
                self.note = res.err();
                self.reload(sh);
            }
        }
        if let Some(job) = &mut self.gen_job {
            if let Some(res) = job.take() {
                self.gen_job = None;
                match res {
                    Ok(_) => {
                        self.gen_err = None;
                        self.reload(sh);
                    }
                    Err(e) => self.gen_err = Some(e),
                }
            }
        }
    }

    fn reload(&mut self, sh: &Shared) {
        let personal = self.phone_scope;
        self.load = Some(sh.spawn(move |a| if personal { a.personal_tasks() } else { a.tasks() }));
        if !personal && self.hosts.is_empty() && self.host_job.is_none() {
            self.host_job = Some(sh.spawn(|a| a.night_hosts()));
        }
    }

    /// Same throwaway-session pattern as the macOS app: draft on the user's
    /// DEFAULT model, parse strict JSON, create the task, clean up the session.
    fn generate(&mut self, sh: &mut Shared) {
        let described = self.quick.trim().to_string();
        if described.is_empty() || self.gen_job.is_some() {
            return;
        }
        self.gen_err = None;
        let (model, ep_url, ep_id) = default_model(sh);
        self.gen_job = Some(sh.spawn(move |a| {
            let sess = a.create_session("__taskgen__", &model, &ep_url, &ep_id)?;
            let sid = sess
                .get("id")
                .and_then(|x| x.as_str())
                .ok_or_else(|| "no session id".to_string())?
                .to_string();
            let instruction = format!(
                "Turn this into a scheduled AI task: \"{described}\".\n\
                 Reply with STRICT JSON only, no prose, shaped exactly:\n\
                 {{\"name\": \"short title\", \"prompt\": \"what the assistant should do each run\", \
                 \"schedule\": \"daily|weekly|monthly\", \"time\": \"HH:MM 24h\"}}"
            );
            let r = a.post_form("api/chat", &[("message", &instruction), ("session", &sid)]);
            let _ = a.delete_session(&sid);
            let r = r?;
            let text = r
                .get("response")
                .and_then(|x| x.as_str())
                .ok_or_else(|| "bad response".to_string())?;
            let start = text.find('{');
            let end = text.rfind('}');
            let obj: Value = match (start, end) {
                (Some(s), Some(e)) if e > s => serde_json::from_str(&text[s..=e])
                    .map_err(|_| "Model didn't return a valid task".to_string())?,
                _ => return Err("Model didn't return a valid task".to_string()),
            };
            let title = obj
                .get("name")
                .and_then(|x| x.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| described.chars().take(40).collect());
            let prompt = obj
                .get("prompt")
                .and_then(|x| x.as_str())
                .unwrap_or(&described)
                .to_string();
            let schedule = match obj.get("schedule").and_then(|x| x.as_str()) {
                Some(s @ ("daily" | "weekly" | "monthly")) => s,
                _ => "daily",
            };
            let time = obj.get("time").and_then(|x| x.as_str()).unwrap_or("09:00");
            let mut f = TaskForm::new(false);
            f.title = title;
            f.prompt = prompt;
            f.schedule = schedule.to_string();
            f.time = time.to_string();
            a.post_json("api/tasks", form_body(&f, false))
        }));
    }

    fn selection_bar(&mut self, ui: &mut Ui, sh: &mut Shared, visible: &[Value]) {
        let p = sh.palette;
        let all_ids: Vec<String> = visible.iter().filter_map(|t| t.get("id").and_then(|x| x.as_str()).map(str::to_string)).collect();
        match wk::selection_bar(ui, &p, self.selection.len(), all_ids.len()) {
            wk::SelAct::All => self.selection = all_ids.iter().cloned().collect(),
            wk::SelAct::Clear => self.selection.clear(),
            wk::SelAct::Delete => {
                let ids: Vec<String> = self.selection.drain().collect();
                self.action = Some(sh.spawn(move |a| {
                    for id in &ids {
                        a.delete_task(id)?;
                    }
                    Ok(Value::Null)
                }));
            }
            wk::SelAct::None => {}
        }
    }

    fn host_label(&self, t: &Value) -> Option<String> {
        let h = gs(t, &["coding_host"]);
        if self.hosts.len() <= 1 || h.is_empty() {
            return None;
        }
        Some(self.hosts.iter().find(|x| x.id == h).map(|x| x.name.clone()).unwrap_or_else(|| h.to_string()))
    }

    fn task_row(&mut self, ui: &mut Ui, sh: &mut Shared, t: &Value) {
        let p = sh.palette;
        let id = t.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let name = { let n = gs(t, &["name", "title"]); if n.is_empty() { "Task".to_string() } else { n.to_string() } };
        let paused = gs(t, &["status"]) == "paused";
        let prompt = gs(t, &["prompt"]).to_string();
        let schedule = gs(t, &["schedule"]).to_string();
        let time = gs(t, &["scheduled_time", "time"]).to_string();
        let coding_agent = gs(t, &["coding_agent"]).to_string();
        let is_coding = !coding_agent.is_empty();
        let coding_status = gs(t, &["coding_status"]).to_string();
        let checked = self.selection.contains(&id);
        let selecting = !self.selection.is_empty();

        let mut meta: Vec<String> = Vec::new();
        if !schedule.is_empty() {
            meta.push(if time.is_empty() { schedule.clone() } else { format!("{schedule} {time}") });
        }
        if let Some(s) = widgets::iso_to_secs(gs(t, &["next_run"])) {
            meta.push(format!("next {}", widgets::relative_from_now(s)));
        }
        if is_coding {
            let a = if coding_agent == "any" { "any agent".to_string() } else { coding_agent.clone() };
            let parts: Vec<String> = [Some(a), self.host_label(t)].into_iter().flatten().filter(|s| !s.is_empty()).collect();
            meta.push(parts.join(" \u{b7} "));
        }

        let out = ui
            .scope(|ui| {
                if paused {
                    ui.set_opacity(0.55);
                }
                wk::row_shell(ui, &p, egui::Id::new(("task_row", &id)), checked, selecting, |ui| {
                    let w = ui.available_width();
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 18.0), egui::Sense::hover());
                        let col = if paused { p.ink3 } else { p.accent };
                        icons::paint_glyph(ui.painter(), r.center(), if is_coding { JbGlyph::Coding } else { JbGlyph::Tasks }, 15.0, col);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            ui.spacing_mut().interact_size.y = 0.0;
                            ui.set_max_width((w - 22.0 - 10.0 - if paused || !coding_status.is_empty() { 90.0 } else { 0.0 }).max(40.0));
                            ui.add(egui::Label::new(ty::sized(&name, 13.0, W::Medium).color(p.ink)).truncate());
                            if !meta.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    ui.spacing_mut().interact_size.y = 0.0;
                                    for m in &meta {
                                        ui.label(ty::sized(m, 11.0, W::Regular).color(p.ink3));
                                    }
                                });
                            }
                            if !prompt.is_empty() {
                                ui.add(egui::Label::new(ty::sized(&prompt, 11.5, W::Regular).color(p.ink2)).wrap().truncate());
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            if paused {
                                widgets::chip_tag(ui, &p, "paused", None);
                            }
                            if !coding_status.is_empty() {
                                let tint = match coding_status.as_str() {
                                    "running" => p.live,
                                    "review" => p.accent,
                                    "failed" => p.danger,
                                    _ => p.ink2,
                                };
                                widgets::chip_tag(ui, &p, &coding_status, Some(tint));
                                if coding_status == "running" {
                                    widgets::status_dot(ui, p.live, true, 6.0);
                                }
                            }
                        });
                    });
                })
            })
            .inner;
        let resp = out.resp.clone();
        if out.toggled || (out.clicked && selecting) {
            if !self.selection.remove(&id) {
                self.selection.insert(id.clone());
            }
        } else if resp.double_clicked() {
            self.form = Some(TaskForm::from_task(t));
        }

        resp.context_menu(|ui| {
            if ui.button("Run now").clicked() {
                let tid = id.clone();
                self.note = None;
                self.run_job = Some(sh.spawn(move |a| a.run_task(&tid).map(|v| v)));
                ui.close_menu();
            }
            if ui.button("Edit").clicked() {
                self.form = Some(TaskForm::from_task(t));
                ui.close_menu();
            }
            if ui.button(if paused { "Resume" } else { "Pause" }).clicked() {
                let tid = id.clone();
                let act = if paused { "resume" } else { "pause" };
                self.action = Some(sh.spawn(move |a| a.task_action(&tid, act)));
                ui.close_menu();
            }
            if ui.button("Delete").clicked() {
                let tid = id.clone();
                self.action = Some(sh.spawn(move |a| a.delete_task(&tid)));
                ui.close_menu();
            }
        });
    }

    // ---- new/edit sheet -------------------------------------------------------

    fn form_window(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let Some(mut form) = self.form.take() else { return };
        let p = sh.palette;
        let editing = form.id.is_some();
        let saving = self.save.is_some();
        let (mut submit, mut cancel) = (false, false);
        let can_save = form.can_save() && !saving;
        let hosts = self.hosts.clone();
        let esc = wk::sheet(
            ui.ctx(),
            &p,
            "task_form",
            if editing { "Edit task" } else { "New task" },
            500.0,
            |ui| {
                if saving {
                    crate::ascii::spinner(ui, 12.0, p.ink2);
                } else if widgets::button_ex(ui, &p, BtnKind::Primary, true, can_save, if editing { "Save" } else { "Create" }).clicked() {
                    submit = true;
                }
                if widgets::button_ex(ui, &p, BtnKind::Ghost, true, true, "Cancel").clicked() {
                    cancel = true;
                }
            },
            |ui| {
                if form.allow_coding {
                    if let Some(i) = wk::seg(ui, &p, &["AI task", "Coding agent"], form.coding as usize) {
                        form.coding = i == 1;
                    }
                }
                widgets::field(ui, &p, &mut form.title, "Name");
                let hint = if form.coding { "What should the agent do?" } else { "What should JoeBro do?" };
                widgets::field_frame(&p, false).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut form.prompt).frame(false).desired_rows(3).desired_width(f32::INFINITY)
                            .hint_text(egui::RichText::new(hint).color(p.ink3)).font(theme::jb(13.0, W::Regular)),
                    );
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme::S3;
                    wk::eyebrow_field(ui, &p, "Repeat", |ui| {
                        wk::picker(ui, &p, "task_schedule", &mut form.schedule, &[("daily", "Daily"), ("weekly", "Weekly"), ("monthly", "Monthly")]);
                    });
                    if form.schedule == "weekly" {
                        wk::eyebrow_field(ui, &p, "On", |ui| {
                            let mut d = form.weekday.clamp(0, 6).to_string();
                            let opts: Vec<(String, &str)> = WEEKDAYS.iter().enumerate().map(|(i, w)| (i.to_string(), *w)).collect();
                            let o: Vec<(&str, &str)> = opts.iter().map(|(a, b)| (a.as_str(), *b)).collect();
                            wk::picker(ui, &p, "task_weekday", &mut d, &o);
                            form.weekday = d.parse().unwrap_or(0);
                        });
                    }
                    wk::eyebrow_field(ui, &p, "Time", |ui| {
                        widgets::field_frame(&p, false).show(ui, |ui| {
                            ui.add(egui::TextEdit::singleline(&mut form.time).frame(false).desired_width(60.0).hint_text("09:00").font(theme::jb(13.0, W::Regular)));
                        });
                    });
                });
                if form.coding {
                    coding_fields(ui, &p, &mut form, &hosts);
                } else {
                    wk::eyebrow_field(ui, &p, "File access", |ui| {
                        wk::picker(ui, &p, "task_mode", &mut form.mode, &[("sandbox", "Bound folder"), ("readonly", "Read-only"), ("full", "Full access")]);
                    });
                }
                if let Some(e) = &form.error {
                    wk::danger_text(ui, &p, e);
                }
            },
        );
        if esc || cancel {
            return;
        }
        if submit && !saving {
            form.error = None;
            let personal = self.phone_scope;
            let body = form_body(&form, personal);
            if let Some(id) = form.id.clone() {
                self.save = Some(sh.spawn(move |a| a.update_task(&id, body)));
            } else {
                self.save = Some(sh.spawn(move |a| a.post_json("api/tasks", body)));
            }
        }
        self.form = Some(form);
    }
}

const AGENT_MODELS: [(&str, &[&str]); 5] = [
    ("claude", &["opus", "sonnet", "haiku"]),
    ("codex", &["gpt-5-codex", "gpt-5"]),
    ("opencode", &["qwen3-coder:30b", "llama3.3:70b", "deepseek-v3.2"]),
    ("agy", &["gemini-4-neon-high", "gemini-4-carbon-medium", "gemini-3.8-flash-high"]),
    ("any", &[]),
];

/// `CodingTargetFields`: agent / model / computer / folder.
fn coding_fields(ui: &mut Ui, p: &theme::Palette, f: &mut TaskForm, hosts: &[NightHost]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::S3;
        wk::eyebrow_field(ui, p, "Agent", |ui| {
            wk::picker(ui, p, "task_agent", &mut f.agent, &[("claude", "Claude Code"), ("codex", "Codex"), ("opencode", "OpenCode"), ("agy", "Antigravity"), ("any", "Any")]);
        });
        let models = AGENT_MODELS.iter().find(|m| m.0 == f.agent).map(|m| m.1).unwrap_or(&[]);
        if !models.is_empty() {
            wk::eyebrow_field(ui, p, "Model", |ui| {
                let mut opts: Vec<(&str, &str)> = vec![("", "Default")];
                opts.extend(models.iter().map(|m| (*m, *m)));
                wk::picker(ui, p, "task_model", &mut f.model, &opts);
            });
        }
        if hosts.len() > 1 {
            wk::eyebrow_field(ui, p, "Computer", |ui| {
                let mut opts: Vec<(&str, &str)> = vec![("", "Any")];
                opts.extend(hosts.iter().map(|h| (h.id.as_str(), h.name.as_str())));
                wk::picker(ui, p, "task_host", &mut f.host, &opts);
            });
        }
    });
    wk::eyebrow_field(ui, p, "Folder", |ui| {
        widgets::field(ui, p, &mut f.repo, "/path/to/repo");
    });
}

/// The user's DEFAULT model (prefs default_model/default_endpoint_id matched
/// against the loaded model list), falling back to the picker selection —
/// mirrors the macOS quick-add behaviour.
fn default_model(sh: &Shared) -> (String, String, String) {
    let pref = |k: &str| -> String {
        let v = sh.prefs.get(k);
        v.and_then(|x| x.as_str())
            .or_else(|| v.and_then(|x| x.get("value")).and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_string()
    };
    let dm = pref("default_model");
    let dep = pref("default_endpoint_id");
    if !dm.is_empty() {
        if let Some(m) = sh
            .models
            .iter()
            .find(|m| m.model_id == dm && m.endpoint_id == dep)
        {
            return (
                m.model_id.clone(),
                m.endpoint_url.clone(),
                m.endpoint_id.clone(),
            );
        }
    }
    sh.selected_model
        .as_ref()
        .map(|m| {
            (
                m.model_id.clone(),
                m.endpoint_url.clone(),
                m.endpoint_id.clone(),
            )
        })
        .unwrap_or_default()
}
