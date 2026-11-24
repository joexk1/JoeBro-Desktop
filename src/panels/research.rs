//! Deep Research (Mac ResearchPanel): launcher capsule, live progress card,
//! then either a full-width library (no reports yet) or library | reader.
use super::mdview::MdView;
use crate::api::R;
use crate::state::{Job, Model, Shared};
use crate::icons::{self, JbGlyph};
use crate::panels::workkit as wk;
use crate::theme::{self, ty, W};
use crate::widgets::{self, BtnKind};
use eframe::egui::{self, Align, Layout, Rect, RichText, Sense, Ui, Vec2};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct ResearchPanel {
    pending_export: Option<String>,
    export_job: Option<Job<Result<String, String>>>,
    query: String,
    model: Option<Model>,
    lib: Vec<Value>,
    load: Option<Job<R<Value>>>,
    loaded: bool,
    start_job: Option<Job<R<Value>>>,
    run_id: Option<String>,
    run_query: String,
    run_phase: String,
    run_message: String,
    run_started: f64,
    run_avg: Option<f64>,
    poll: Option<Job<R<Value>>>,
    active_job: Option<Job<R<Value>>>,
    checked_active: bool,
    timer: f64,
    selected: Option<String>,
    detail: Option<Job<R<Value>>>,
    detail_loaded: bool,
    report: String,
    sources: Vec<(String, String)>,
    error: Option<String>,
    action: Option<Job<R<Value>>>,
    cancel_job: Option<Job<R<Value>>>,
    selection: BTreeSet<String>,
    md: MdView,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn mmss(secs: f64) -> String {
    let t = secs.max(0.0) as i64;
    format!("{}:{:02}", t / 60, t % 60)
}

impl ResearchPanel {
    /// Snapshot hook: open a report as if the row was clicked.
    #[cfg(feature = "snapshot")]
    pub fn demo_open(&mut self, id: &str, sh: &Shared) {
        self.open(id, sh);
    }

    fn open(&mut self, id: &str, sh: &Shared) {
        self.selected = Some(id.to_string());
        self.detail_loaded = false;
        self.report.clear();
        self.sources.clear();
        let i = id.to_string();
        self.detail = Some(sh.spawn(move |a| a.research_detail(&i)));
    }

    fn pump(&mut self, ui: &Ui, sh: &mut Shared) {
        if let Some(path) = sh.take_picked("research_export") {
            if let Some(md) = self.pending_export.take() {
                self.export_job = Some(sh.spawn(move |_| {
                    std::fs::write(&path, md).map_err(|e| format!("Save failed: {e}"))?;
                    Ok(format!("Saved {}", path.display()))
                }));
            }
        }
        if let Some(job) = &mut self.export_job {
            if let Some(r) = job.take() {
                self.export_job = None;
                match r {
                    Ok(m) | Err(m) => sh.err(m),
                }
            }
        }
        if !self.loaded && self.load.is_none() {
            self.load = Some(sh.spawn(|a| a.research_library()));
        }
        if let Some(job) = &mut self.load {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) => {
                        self.lib = v
                            .get("research")
                            .and_then(Value::as_array)
                            .cloned()
                            .or_else(|| v.as_array().cloned())
                            .unwrap_or_default();
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
                self.loaded = true;
                self.load = None;
                // Adopt a run the agent started elsewhere (watch only).
                if self.run_id.is_none() && !self.checked_active {
                    self.checked_active = true;
                    self.active_job = Some(sh.spawn(|a| a.research_active()));
                }
            }
        }
        let active = self.active_job.as_mut().and_then(|j| j.take());
        if let Some(res) = active {
            self.active_job = None;
            if let Ok(v) = res {
                let first = v
                    .get("active")
                    .and_then(Value::as_array)
                    .or_else(|| v.as_array())
                    .and_then(|a| a.first().cloned());
                if let (Some(f), None) = (first, &self.run_id) {
                    let id = s(&f, "id");
                    if !id.is_empty() {
                        self.begin_run(id, s(&f, "query"), ui, sh);
                    }
                }
            }
        }
        if let Some(job) = &mut self.start_job {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) => {
                        let id = v.get("id").or_else(|| v.get("session_id")).and_then(Value::as_str).map(String::from);
                        match id {
                            Some(id) => {
                                let q = std::mem::take(&mut self.run_query);
                                self.begin_run(id, q, ui, sh);
                            }
                            None => self.error = Some("The backend did not return a research run ID.".into()),
                        }
                    }
                    Err(e) => self.error = Some(format!("Research start failed: {e}")),
                }
                self.start_job = None;
            }
        }
        if let Some(job) = &mut self.action {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) if v.get("ok").and_then(Value::as_bool) == Some(false) => {
                        self.error = Some(s(&v, "detail").chars().take(200).collect::<String>())
                    }
                    Err(e) => self.error = Some(e),
                    _ => self.error = None,
                }
                self.loaded = false;
                self.action = None;
            }
        }
        if let Some(job) = &mut self.cancel_job {
            if let Some(res) = job.take() {
                if let Err(e) = res {
                    self.error = Some(format!("Cancel failed: {e}"));
                }
                self.cancel_job = None;
            }
        }
        if let Some(job) = &mut self.detail {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) => {
                        self.report = ["report", "summary", "result", "content"]
                            .iter()
                            .map(|k| s(&v, k))
                            .find(|t| !t.is_empty())
                            .unwrap_or_default();
                        self.sources = v
                            .get("sources")
                            .and_then(Value::as_array)
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| {
                                        let u = s(x, "url");
                                        (!u.is_empty()).then(|| (if s(x, "title").is_empty() { u.clone() } else { s(x, "title") }, u))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        self.detail_loaded = true;
                    }
                    Err(e) => {
                        self.error = Some(format!("Couldn't open report: {e}"));
                        self.detail_loaded = true;
                    }
                }
                self.detail = None;
            }
        }
        self.pump_poll(ui, sh);
    }

    fn begin_run(&mut self, id: String, query: String, ui: &Ui, sh: &mut Shared) {
        self.run_id = Some(id);
        self.run_query = query;
        self.run_phase = "planning".into();
        self.run_message.clear();
        self.run_avg = None;
        self.run_started = ui.input(|i| i.time);
        self.timer = 0.3;
        sh.research_running = true;
    }

    fn finish_run(&mut self, sh: &mut Shared) {
        self.run_id = None;
        sh.research_running = false;
        self.loaded = false;
    }

    fn pump_poll(&mut self, ui: &Ui, sh: &mut Shared) {
        if let Some(job) = &mut self.poll {
            if let Some(result) = job.take() {
                match result {
                    Err(e) => self.error = Some(format!("Research status check failed: {e}")),
                    Ok(v) => {
                        let status = s(&v, "status");
                        if let Some(a) = v.get("avg_duration").and_then(Value::as_f64) {
                            self.run_avg = Some(a);
                        }
                        let prog = v.get("progress");
                        if let Some(ph) = prog.and_then(|p| p.get("phase")).and_then(Value::as_str).or_else(|| v.get("phase").and_then(Value::as_str)) {
                            self.run_phase = ph.to_string();
                        }
                        if let Some(m) = prog.and_then(|p| p.get("message")).and_then(Value::as_str) {
                            self.run_message = m.to_string();
                        }
                        if ["completed", "failed", "cancelled", "canceled", "done", "complete", "error"].contains(&status.as_str()) {
                            if status == "failed" || status == "error" {
                                self.error = Some(if self.run_message.is_empty() { "Research failed.".into() } else { self.run_message.clone() });
                            }
                            self.finish_run(sh);
                        }
                    }
                }
                self.poll = None;
            }
        }
        if let Some(id) = self.run_id.clone() {
            self.timer -= ui.input(|i| i.stable_dt) as f64;
            if self.timer <= 0.0 && self.poll.is_none() {
                self.timer = 2.0;
                self.poll = Some(sh.spawn(move |a| a.get(&format!("api/research/status/{id}"))));
            }
            sh.ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn start(&mut self, sh: &Shared) {
        let q = self.query.trim().to_string();
        if q.is_empty() || self.run_id.is_some() || self.start_job.is_some() {
            return;
        }
        self.query.clear();
        self.run_query = q.clone();
        let (eid, m) = self.model.as_ref().map(|m| (m.endpoint_id.clone(), m.model_id.clone())).unwrap_or_default();
        self.start_job = Some(sh.spawn(move |a| a.research_start(&q, &eid, &m)));
    }

    /// Markdown of title + report + numbered sources (Copy / Download).
    fn report_markdown(&self, query: &str) -> String {
        let mut md = format!("# {query}\n\n{}", self.report);
        if !self.sources.is_empty() {
            md.push_str("\n\n## Sources\n");
            for (i, (t, u)) in self.sources.iter().enumerate() {
                md.push_str(&format!("\n{}. [{t}]({u})", i + 1));
            }
        }
        md
    }

    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        self.pump(ui, sh);
        let p = sh.palette;
        let mut refresh = false;
        let mut go = false;
        let rect = ui.max_rect();
        widgets::panel_chrome(
            ui,
            &p,
            "Deep Research",
            "antenna.radiowaves.left.and.right",
            |ui| {
                refresh = widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).on_hover_text("Refresh").clicked();
            },
            |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                // Launcher bar: raised r10, 14 / 10 padding.
                wk::raised_row(ui, &p, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        icons::glyph(ui, JbGlyph::Research, 15.0, p.accent);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let can = !self.query.trim().is_empty() && self.run_id.is_none() && self.start_job.is_none();
                            if widgets::button_ex(ui, &p, BtnKind::Primary, true, can, "Research").clicked() {
                                go = true;
                            }
                            let mut sel = self.model.take().or_else(|| sh.selected_model.clone());
                            widgets::model_picker(ui, "research_model", sh, &mut sel);
                            self.model = sel;
                            let te = ui.add(
                                egui::TextEdit::singleline(&mut self.query)
                                    .frame(false)
                                    .desired_width(ui.available_width().max(60.0))
                                    .hint_text(RichText::new("Research anything").color(p.ink3))
                                    .font(theme::jb(13.0, W::Regular))
                                    .text_color(p.ink),
                            );
                            if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                go = true;
                            }
                        });
                    });
                });
                if self.run_id.is_some() || self.start_job.is_some() {
                    self.progress_card(ui, sh);
                }
                if let Some(e) = &self.error {
                    wk::danger_text(ui, &p, e);
                }
                let has_reports = self.loaded && !self.lib.is_empty();
                let avail = Rect::from_min_max(ui.cursor().min, rect.max);
                ui.allocate_rect(avail, Sense::hover());
                if has_reports {
                    let gap = 12.0;
                    let left_w = 300.0f32.min(avail.width() * 0.45);
                    let left = Rect::from_min_size(avail.min, Vec2::new(left_w, avail.height()));
                    let right = Rect::from_min_max(egui::pos2(avail.left() + left_w + gap, avail.top()), avail.max);
                    self.glass_column(ui, sh, left, |this, ui, sh| this.list_column(ui, sh));
                    self.glass_column(ui, sh, right, |this, ui, sh| this.reader_column(ui, sh));
                } else {
                    self.glass_column(ui, sh, avail, |this, ui, sh| this.list_column(ui, sh));
                }
            },
        );
        if refresh {
            self.loaded = false;
            self.error = None;
        }
        if go {
            self.start(sh);
        }
    }

    fn glass_column(&mut self, ui: &mut Ui, sh: &mut Shared, rect: Rect, add: impl FnOnce(&mut Self, &mut Ui, &mut Shared)) {
        let pal = sh.palette;
        wk::box_in_rect(ui, &pal, rect, 0.0, |ui| add(self, ui, sh));
    }

    fn progress_card(&mut self, ui: &mut Ui, sh: &Shared) {
        let p = sh.palette;
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 96.0), Sense::hover());
        let now = ui.input(|i| i.time);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        let elapsed = if self.run_id.is_some() { now - self.run_started } else { 0.0 };
        let mut cancel = false;
        wk::box_in_rect(ui, &p, rect, 0.0, |ui| {
            egui::Frame::none().inner_margin(egui::Margin::symmetric(16.0, 12.0)).show(ui, |ui| {
                ui.set_width(rect.width() - 32.0);
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    widgets::status_dot(ui, p.live, true, 6.0);
                    ui.label(ty::eyebrow(&self.run_phase).color(p.ink2));
                    if !self.run_message.is_empty() {
                        ui.add(egui::Label::new(ty::sized(&self.run_message, 11.0, W::Regular).color(p.ink3)).truncate());
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        crate::ascii::spinner(ui, 12.0, p.ink2);
                    });
                });
                ui.add(egui::Label::new(ty::sized(&self.run_query, 14.0, W::Semibold).color(p.ink)).truncate());
                let frac = match self.run_avg {
                    Some(a) if a > 0.0 => (elapsed / a).min(0.97),
                    _ => (elapsed / 300.0).min(0.95),
                } as f32;
                let (br, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 3.0), Sense::hover());
                ui.painter().rect_filled(br, 1.5, p.line);
                let mut fill = br;
                fill.set_width(br.width() * frac);
                ui.painter().rect_filled(fill, 1.5, p.live);
                ui.horizontal(|ui| {
                    let mut t = mmss(elapsed);
                    if let Some(a) = self.run_avg {
                        t.push_str(&format!(" / ~{}", mmss(a)));
                    }
                    ui.label(ty::sized(t, 10.5, W::Regular).color(p.ink3));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button_ex(ui, &p, BtnKind::Ghost, true, true, "Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        });
        if cancel {
            if let Some(rid) = self.run_id.clone() {
                self.cancel_job = Some(sh.spawn(move |a| a.research_cancel(&rid)));
            }
            self.run_id = None;
            self.run_query.clear();
        }
    }

    fn list_column(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let p = sh.palette;
        if !self.loaded {
            widgets::panel_loading(ui, &p);
            return;
        }
        if self.lib.is_empty() {
            if let Some(e) = self.error.clone() {
                widgets::panel_failure(ui, &p, "Couldn't load reports", Some(&e));
            } else {
                crate::ascii::empty_state(ui, &p, crate::ascii::Art::Search, "No research yet");
            }
            return;
        }
        let ids: Vec<String> = self.lib.iter().map(|r| s(r, "id")).collect();
        if !self.selection.is_empty() {
            match wk::selection_bar(ui, &p, self.selection.len(), ids.len()) {
                wk::SelAct::All => self.selection = ids.iter().cloned().collect(),
                wk::SelAct::Clear => self.selection.clear(),
                wk::SelAct::Delete => {
                    let del: Vec<String> = std::mem::take(&mut self.selection).into_iter().collect();
                    if let Some(sel) = &self.selected {
                        if del.contains(sel) {
                            self.selected = None;
                            self.report.clear();
                        }
                    }
                    self.action = Some(sh.spawn(move |a| {
                        let mut first_err = None;
                        for id in &del {
                            if let Err(e) = a.research_delete(id) {
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
        let lib = self.lib.clone();
        egui::ScrollArea::vertical().id_salt("rs_list").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(8.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - 16.0);
                    for r in &lib {
                        self.row(ui, sh, r);
                    }
                });
            });
            ui.add_space(8.0);
        });
    }

    fn row(&mut self, ui: &mut Ui, sh: &mut Shared, r: &Value) {
        let p = sh.palette;
        let id = s(r, "id");
        let q = if s(r, "query").is_empty() { "(untitled)".to_string() } else { s(r, "query") };
        let selected = self.selected.as_deref() == Some(&id);
        let checked = self.selection.contains(&id);
        let out = wk::row_shell(ui, &p, egui::Id::new(("rs_row", &id)), checked, !self.selection.is_empty(), |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add(egui::Label::new(ty::sized(&q, 12.5, if selected { W::Semibold } else { W::Regular }).color(p.ink)).wrap());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.spacing_mut().interact_size.y = 24.0;
                let cat = s(r, "category");
                if !cat.is_empty() {
                    widgets::chip_tag(ui, &p, &cat, None);
                }
                if let Some(n) = r.get("source_count").and_then(Value::as_i64) {
                    ui.label(ty::sized(format!("{n} sources"), 10.5, W::Regular).color(p.ink3));
                }
                let done = r
                    .get("completed_at")
                    .and_then(Value::as_f64)
                    .filter(|d| *d > 0.0)
                    .or_else(|| widgets::iso_to_secs(&s(r, "created_at")));
                if let Some(d) = done {
                    ui.label(ty::sized(widgets::fmt_date_abbr(d), 10.5, W::Regular).color(p.ink3));
                }
            });
        });
        if selected {
            let r = out.resp.rect;
            ui.painter().rect_filled(Rect::from_min_max(egui::pos2(r.left() - 1.0, r.top() + 8.0), egui::pos2(r.left() + 1.0, r.bottom() - 8.0)), 1.0, p.accent);
        }
        if out.toggled {
            if !self.selection.remove(&id) {
                self.selection.insert(id.clone());
            }
        } else if out.clicked {
            if self.selection.is_empty() {
                self.open(&id, sh);
            } else if !self.selection.remove(&id) {
                self.selection.insert(id.clone());
            }
        }
        out.resp.context_menu(|ui| {
            if ui.button("Copy Report").clicked() {
                if self.selected.as_deref() == Some(&id) {
                    widgets::set_clipboard(&sh.ctx, &self.report_markdown(&q));
                } else {
                    self.open(&id, sh);
                }
                ui.close_menu();
            }
            if ui.button("Download Markdown").clicked() {
                if self.selected.as_deref() == Some(&id) {
                    let safe: String = q.chars().map(|c| if "/\\:?%*|\"<>".contains(c) { ' ' } else { c }).take(120).collect();
                    self.pending_export = Some(self.report_markdown(&q));
                    sh.picker.open_save("research_export", "Download Markdown", &format!("{}.md", safe.trim()), None);
                } else {
                    self.open(&id, sh);
                }
                ui.close_menu();
            }
            if ui.button("Delete").clicked() {
                let i = id.clone();
                if self.selected.as_deref() == Some(&id) {
                    self.selected = None;
                    self.report.clear();
                }
                self.action = Some(sh.spawn(move |a| a.research_delete(&i)));
                ui.close_menu();
            }
        });
    }

    fn reader_column(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let p = sh.palette;
        let Some(sel) = self.selected.clone() else {
            crate::ascii::empty_state(ui, &p, crate::ascii::Art::Files, "Select a report");
            return;
        };
        if !self.detail_loaded {
            widgets::panel_loading(ui, &p);
            return;
        }
        let q = self.lib.iter().find(|r| s(r, "id") == sel).map(|r| s(r, "query")).unwrap_or_default();
        egui::ScrollArea::vertical().id_salt("rs_reader").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::none().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 12.0;
                ui.horizontal_top(|ui| {
                    let w = ui.available_width() - 32.0;
                    ui.allocate_ui_with_layout(Vec2::new(w, 0.0), Layout::top_down(Align::Min), |ui| {
                        ui.set_width(w);
                        ui.add(egui::Label::new(ty::sized(&q, 20.0, W::Semibold).extra_letter_spacing(-0.3).color(p.ink)).wrap());
                    });
                    let md = self.report_markdown(&q);
                    widgets::copy_check_button(ui, &p, &md, 24.0);
                });
                if self.report.is_empty() {
                    ui.label(ty::sized("Empty report", 12.0, W::Regular).color(p.ink3));
                } else {
                    let report = self.report.clone();
                    self.md.show(ui, sh, &report);
                }
                if !self.sources.is_empty() {
                    let (hr, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                    ui.painter().rect_filled(hr, 0.0, p.line);
                    widgets::section_header(ui, &p, "Sources", Some(self.sources.len()), |_| {});
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for (i, (title, url)) in self.sources.iter().take(30).enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(ty::sized(format!("{}.", i + 1), 10.5, W::Regular).color(p.ink3));
                            ui.add(egui::Hyperlink::from_label_and_url(RichText::new(title).size(12.0).color(p.ink2), url).open_in_new_tab(true));
                        });
                    }
                }
            });
        });
    }
}
