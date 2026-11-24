//! Agents: everything running on its own (chat turns, research, tasks, subagents, coding sessions)
//! from the backend's one registry. Mirrors AgentsPanel.swift: list left (330 pt), the selected
//! run's output on the right; coding sessions open as chats or show their transcript.
#![allow(dead_code)]
use super::coding_agents::{is_open_session, Approval, AgentThread};
use super::mdview::MdView;
use crate::api::R;
use crate::ascii::{self, Art};
use crate::models::{relative_iso, AgentRun, AgentsEnvelope};
use crate::state::{Job, Shared, Tab};
use crate::theme::{self, ty, W, R_CTRL, R_PANEL};
use crate::widgets::{self, BtnKind};
use crate::icons;
use eframe::egui::{self, Rect, RichText, Rounding, Sense, Ui, Vec2};
use serde_json::Value;

const KINDS: [(&str, &str); 6] =
    [("", "All"), ("research", "Research"), ("chat", "Chats"), ("task", "Tasks"), ("subagent", "Subagents"), ("coding", "Coding")];

#[derive(Default)]
pub struct AgentsPanel {
    runs: Option<Result<Vec<AgentRun>, String>>,
    load: Option<Job<R<AgentsEnvelope>>>,
    timer: f64,
    kind: String,
    active_only: bool,
    search: String,
    search_open: bool,
    selected: Option<String>,
    detail: Option<AgentRun>,
    detail_job: Option<Job<R<AgentRun>>>,
    detail_loading: bool,
    cancel_note: Option<String>,
    cancel_job: Option<Job<R<Value>>>,
    resume_job: Option<Job<(String, R<Value>, Option<String>)>>,
    resume_note: Option<String>,
    approval: Approval,
    thread: AgentThread,
    md: MdView,
}

fn colour(p: &theme::Palette, status: &str) -> egui::Color32 {
    match status {
        "running" => p.live,
        "waiting" => p.warn,
        "done" => p.ink2,
        "failed" => p.danger,
        _ => p.ink3,
    }
}

fn date_of(r: &AgentRun) -> String {
    relative_iso(if r.finished_at.is_empty() { &r.created_at } else { &r.finished_at })
}

impl AgentsPanel {
    pub fn demo_select(&mut self, id: &str, sh: &Shared) {
        self.select(id.to_string(), sh);
    }

    fn reload(&mut self) {
        self.timer = 0.0;
    }

    fn select(&mut self, id: String, sh: &Shared) {
        self.selected = Some(id.clone());
        self.cancel_note = None;
        self.detail_loading = true;
        self.fetch_detail(id, sh);
    }

    fn fetch_detail(&mut self, id: String, sh: &Shared) {
        self.detail_job = Some(sh.spawn(move |a| a.agent_detail_typed(&id)));
    }

    fn live_count(&self) -> usize {
        match &self.runs {
            Some(Ok(l)) => l.iter().filter(|r| r.live).count(),
            _ => 0,
        }
    }

    fn pump(&mut self, ui: &Ui, sh: &mut Shared) {
        if let Some(j) = &mut self.load {
            if let Some(res) = j.take() {
                self.load = None;
                match res {
                    Ok(env) => {
                        sh.agents_running = env.agents.iter().any(|r| r.live);
                        if let Some(id) = self.selected.clone() {
                            if env.agents.iter().find(|r| r.id == id).map_or(false, |r| r.live) && self.detail_job.is_none() {
                                self.fetch_detail(id, sh);
                            }
                        }
                        self.runs = Some(Ok(env.agents));
                    }
                    Err(e) => self.runs = Some(Err(e)),
                }
            }
        }
        if let Some(j) = &mut self.detail_job {
            if let Some(res) = j.take() {
                self.detail_job = None;
                self.detail_loading = false;
                match res {
                    Ok(r) if self.selected.as_deref() == Some(r.id.as_str()) => self.detail = Some(r),
                    Ok(_) => {}
                    Err(e) => self.cancel_note = Some(e),
                }
            }
        }
        if let Some(j) = &mut self.cancel_job {
            if let Some(res) = j.take() {
                self.cancel_job = None;
                self.cancel_note = Some(match res {
                    Ok(v) => v.get("detail").or_else(|| v.get("message")).and_then(Value::as_str).unwrap_or("Stop requested").to_string(),
                    Err(e) => e,
                });
                self.reload();
                if let Some(id) = self.selected.clone() {
                    self.fetch_detail(id, sh);
                }
            }
        }
        if let Some(j) = &mut self.resume_job {
            if let Some((id, res, back)) = j.take() {
                self.resume_job = None;
                match res {
                    Ok(v) if v.get("ok").and_then(Value::as_bool) != Some(true) => {
                        self.resume_note = Some(v.get("detail").and_then(Value::as_str).unwrap_or("Couldn't resume").to_string());
                    }
                    Ok(v) if v.get("started").and_then(Value::as_bool) != Some(true) => {
                        self.resume_note = Some("Queued \u{2014} it starts when that computer is online".into());
                    }
                    Ok(_) => {
                        sh.agent_nav.timer = 0.0;
                        sh.agent_nav.open(&back.unwrap_or(id));
                        sh.goto_tab = Some(Tab::Chats);
                    }
                    Err(e) => self.resume_note = Some(e),
                }
            }
        }
        self.timer -= ui.input(|i| i.stable_dt) as f64;
        if self.timer <= 0.0 && self.load.is_none() {
            self.timer = if self.live_count() > 0 { 3.0 } else { 20.0 };
            let kind = self.kind.clone();
            self.load = Some(sh.spawn(move |a| a.agents_typed(&kind, "")));
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
    }

    fn filtered(&self, all: &[AgentRun]) -> Vec<AgentRun> {
        let q = self.search.trim().to_lowercase();
        all.iter()
            .filter(|r| !self.active_only || r.live || (r.kind == "coding" && is_open_session(r)))
            .filter(|r| q.is_empty() || format!("{} {} {}", r.title, r.progress, r.kind_label()).to_lowercase().contains(&q))
            .cloned()
            .collect()
    }

    /// Children listed right under their parent (when it is in the list); the rest keep order.
    fn nested(list: &[AgentRun]) -> Vec<(AgentRun, usize)> {
        let ids: std::collections::HashSet<&str> = list.iter().map(|r| r.id.as_str()).collect();
        let sessions: std::collections::HashMap<&str, &str> =
            list.iter().filter(|r| !r.session_id.is_empty()).map(|r| (r.session_id.as_str(), r.id.as_str())).collect();
        let parent = |r: &AgentRun| -> Option<String> {
            if r.parent_id.is_empty() || r.parent_id == r.id {
                None
            } else if ids.contains(r.parent_id.as_str()) {
                Some(r.parent_id.clone())
            } else {
                sessions.get(r.parent_id.as_str()).map(|s| s.to_string())
            }
        };
        let mut out = vec![];
        for r in list.iter().filter(|r| parent(r).is_none()) {
            out.push((r.clone(), 0));
            for k in list.iter().filter(|k| parent(k).as_deref() == Some(r.id.as_str())) {
                out.push((k.clone(), 1));
            }
        }
        out
    }

    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        self.pump(ui, sh);
        let p = sh.palette;
        let rect = ui.max_rect();
        ui.painter().extend(widgets::glass_surface(&p, rect, R_PANEL, p.panel_fill, false));
        let hdr = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 44.0));
        let (mut refresh, mut plus, mut kind_changed) = (false, false, false);
        ui.allocate_new_ui(
            egui::UiBuilder::new().max_rect(hdr.shrink2(Vec2::new(16.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                icons::symbol(ui, "person.3.sequence", 15.0, p.ink2);
                ui.label(ty::sized("Agents", 15.0, W::Semibold).extra_letter_spacing(-0.2).color(p.ink));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    refresh = widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).on_hover_text("Refresh").clicked();
                    plus = widgets::icon_btn(ui, &p, "plus", 26.0, false).on_hover_text("New coding agent").clicked();
                    widgets::search_field(ui, sh, &mut self.search, &mut self.search_open, "Search agents");
                });
            },
        );
        ui.painter().hline(rect.x_range(), hdr.bottom(), egui::Stroke::new(1.0, p.line));
        if refresh {
            self.reload();
        }
        if plus {
            sh.new_coding_agent_requested = true;
        }

        // kind tabs + live + active only
        let tabs = Rect::from_min_size(egui::pos2(rect.left() + 12.0, hdr.bottom() + 12.0), Vec2::new(rect.width() - 24.0, 26.0));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(tabs).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (k, label) in KINDS {
                let on = self.kind == k;
                let g = ui.painter().layout_no_wrap(label.to_string(), theme::jb(11.0, W::Medium), if on { p.ink } else { p.ink2 });
                let (r, resp) = ui.allocate_exact_size(g.size() + Vec2::new(18.0, 10.0), Sense::click());
                if on {
                    ui.painter().rect_filled(r, Rounding::same(R_CTRL), p.accent_soft);
                }
                ui.painter().galley(r.center() - g.size() / 2.0, g, p.ink);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !on {
                    self.kind = k.to_string();
                    kind_changed = true;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                widgets::jb_switch(ui, &p, &mut self.active_only);
                ui.label(ty::sized("Active only", 11.0, W::Regular).color(p.ink));
                ui.add_space(4.0);
                widgets::eyebrow(ui, &p, "live");
                let n = self.live_count();
                ascii::dot_number(ui, &n.to_string(), 2.0, if n > 0 { p.live } else { p.ink3 });
            });
        });
        if kind_changed {
            self.runs = None;
            self.reload();
        }

        let body_top = tabs.bottom() + 12.0;
        let list_r = Rect::from_min_max(egui::pos2(rect.left(), body_top), egui::pos2(rect.left() + 330.0, rect.bottom()));
        let det_r = Rect::from_min_max(egui::pos2(list_r.right() + 12.0, body_top), rect.max);
        for r in [list_r, det_r] {
            ui.painter().extend(widgets::glass_surface(&p, r, R_PANEL, p.panel_fill, false));
        }
        let mut picked: Option<AgentRun> = None;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(list_r), |ui| {
            ui.set_clip_rect(list_r.intersect(ui.clip_rect()));
            self.list(ui, sh, &mut picked);
        });
        if let Some(r) = picked {
            if r.kind == "coding" && is_open_session(&r) {
                sh.agent_nav.open(&r.id);
                sh.goto_tab = Some(Tab::Chats);
            } else {
                self.select(r.id.clone(), sh);
            }
        }
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(det_r), |ui| {
            ui.set_clip_rect(det_r.intersect(ui.clip_rect()));
            self.detail_pane(ui, sh);
        });
        ui.advance_cursor_after_rect(rect);
    }

    fn list(&mut self, ui: &mut Ui, sh: &mut Shared, picked: &mut Option<AgentRun>) {
        let p = sh.palette;
        let all = match &self.runs {
            None => {
                ui.centered_and_justified(|ui| {
                    ascii::spinner(ui, 14.0, p.ink2);
                });
                return;
            }
            Some(Err(_)) => {
                ascii::empty_state(ui, &p, Art::Plug, "Couldn't load agents");
                return;
            }
            Some(Ok(l)) => l.clone(),
        };
        let shown = self.filtered(&all);
        if shown.is_empty() {
            ascii::empty_state(ui, &p, Art::Sleep, "Nothing running");
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::none().inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (run, depth) in Self::nested(&shown) {
                    if depth > 0 {
                        ui.add_space(0.0);
                    }
                    ui.horizontal(|ui| {
                        if depth > 0 {
                            ui.add_space(18.0);
                        }
                        let sel = self.selected.as_deref() == Some(run.id.as_str());
                        let r = widgets::hover_row(ui, &p, sel, R_CTRL, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 9.0;
                                ui.add_space(0.0);
                                ui.vertical(|ui| {
                                    ui.add_space(5.0);
                                    widgets::status_dot(ui, colour(&p, &run.status), run.live && run.status != "waiting", 7.0);
                                });
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 3.0;
                                    let title = if run.title.is_empty() { run.kind_label() } else { run.title.clone() };
                                    ui.add(egui::Label::new(ty::sized(title, 12.5, if sel { W::Semibold } else { W::Regular }).color(p.ink)).wrap());
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;
                                        let c = if run.status == "waiting" { p.warn } else { p.ink2 };
                                        icons::symbol(ui, run.kind_icon(), 11.0, c);
                                        let t = if run.live && !run.progress.is_empty() { run.progress.clone() } else { run.status.clone() };
                                        ui.add(egui::Label::new(ty::sized(t, 10.5, W::Regular).color(c)).truncate());
                                    });
                                    if run.parent_id.is_empty() {
                                        let kids: Vec<&AgentRun> = all.iter().filter(|k| k.parent_id == run.id).collect();
                                        if !kids.is_empty() {
                                            ui.label(ty::sized(format!("{} sub \u{b7} {} live", kids.len(), kids.iter().filter(|k| k.live).count()), 10.0, W::Regular).color(p.ink3));
                                        }
                                    }
                                    ui.label(ty::sized(date_of(&run), 10.0, W::Regular).color(p.ink3));
                                });
                            });
                        });
                        if r.clicked() {
                            *picked = Some(run.clone());
                        }
                    });
                }
            });
        });
    }

    fn detail_pane(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let p = sh.palette;
        let Some(run) = self.detail.clone().filter(|_| self.selected.is_some()) else {
            ascii::empty_state(ui, &p, Art::Inbox, "Select an agent");
            return;
        };
        let rect = ui.max_rect();
        let open = is_open_session(&run);
        let coding = run.kind == "coding";
        let mut stop = false;
        let mut resume = false;
        let mut open_chat = false;
        let mut open_session = false;
        let mut decided = false;
        let hdr = ui
            .scope(|ui| {
                egui::Frame::none().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        icons::symbol(ui, run.kind_icon(), 13.0, p.ink2);
                        let title = if run.title.is_empty() { run.kind_label() } else { run.title.clone() };
                        ui.add(egui::Label::new(ty::sized(title, 15.0, W::Semibold).extra_letter_spacing(-0.2).color(p.ink)).truncate());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if !run.session_id.is_empty() && !coding && widgets::button(ui, &p, BtnKind::Secondary, "Open chat").clicked() {
                                open_session = true;
                            }
                            if coding && open && widgets::button(ui, &p, BtnKind::Secondary, "Open chat").clicked() {
                                open_chat = true;
                            }
                            if coding && run.parent_id.is_empty() && !open {
                                let busy = self.resume_job.is_some();
                                if widgets::button_ex(ui, &p, BtnKind::Primary, false, !busy, if busy { "Resuming\u{2026}" } else { "Resume chat" }).clicked() {
                                    resume = true;
                                }
                                if let Some(n) = &self.resume_note {
                                    ui.label(ty::sized(n, 11.5, W::Regular).color(p.ink2));
                                }
                            }
                            if run.live && widgets::button(ui, &p, BtnKind::Secondary, "Stop").on_hover_text("Ask this agent to stop").clicked() {
                                stop = true;
                            }
                            if coding && run.status == "waiting" && self.approval.ui(ui, sh, &run.id) {
                                decided = true;
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        widgets::status_dot(ui, colour(&p, &run.status), run.live && run.status != "waiting", 7.0);
                        let mut t = if run.live && !run.progress.is_empty() { format!("{} \u{b7} {}", run.status, run.progress) } else { run.status.clone() };
                        if !run.model.is_empty() {
                            t += &format!(" \u{b7} {}", run.model);
                        }
                        if run.result_chars > 0 {
                            t += &format!(" \u{b7} {} chars", run.result_chars);
                        }
                        t += &format!(" \u{b7} {}", date_of(&run));
                        ui.add(egui::Label::new(ty::sized(t, 11.0, W::Regular).color(p.ink2)).truncate());
                    });
                });
            })
            .response
            .rect;
        ui.painter().hline(rect.x_range(), hdr.bottom(), egui::Stroke::new(1.0, p.line));
        if self.detail_loading {
            ascii::spinner(&mut ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(rect.right_top() + Vec2::new(-26.0, 10.0), Vec2::splat(14.0)))), 14.0, p.ink2);
        }
        let body = Rect::from_min_max(egui::pos2(rect.left(), hdr.bottom() + 1.0), rect.max);
        if coding {
            let changed = ui
                .allocate_new_ui(egui::UiBuilder::new().max_rect(body), |ui| {
                    ui.set_clip_rect(body.intersect(ui.clip_rect()));
                    self.thread.ui(ui, sh, &run, !open)
                })
                .inner;
            if changed || decided {
                self.reload();
                self.fetch_detail(run.id.clone(), sh);
            }
        } else {
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body), |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::none().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 12.0;
                        if !run.error.is_empty() {
                            ui.add(egui::Label::new(ty::sized(&run.error, 11.5, W::Regular).color(if run.status == "failed" { p.danger } else { p.ink2 })).wrap());
                        }
                        if let Some(n) = &self.cancel_note {
                            ui.label(ty::sized(n, 11.0, W::Regular).color(p.ink2));
                        }
                        if run.result.is_empty() {
                            if run.live {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    ascii::spinner(ui, 12.0, p.ink2);
                                    ascii::stream_cursor(ui, p.ink2, 14.0);
                                });
                            } else {
                                ui.label(ty::sized("No output", 11.0, W::Regular).color(p.ink3));
                            }
                        } else {
                            self.md.show(ui, sh, &run.result);
                        }
                        if !run.messages.is_empty() {
                            ui.add_space(8.0);
                            widgets::eyebrow(ui, &p, "Transcript");
                            for m in run.messages.iter().take(12) {
                                let t: String = m.content.chars().take(600).collect();
                                if m.role == "user" {
                                    widgets::card(ui, &p, 12.0, |ui| {
                                        ui.add(egui::Label::new(RichText::new(t).font(theme::jb(13.0, W::Regular)).color(p.ink)).selectable(true));
                                    });
                                } else {
                                    ui.add(egui::Label::new(RichText::new(t).font(theme::jb(13.0, W::Regular)).color(p.ink)).selectable(true));
                                }
                            }
                        }
                    });
                });
            });
        }
        if stop {
            let id = run.id.clone();
            self.cancel_job = Some(sh.spawn(move |a| a.agent_cancel(&id)));
        }
        if resume {
            self.resume_note = None;
            let id = run.id.clone();
            self.resume_job = Some(sh.spawn(move |a| {
                let r = a.coding_resume(&id);
                let started = r.as_ref().map(|v| v.get("ok").and_then(Value::as_bool) == Some(true) && v.get("started").and_then(Value::as_bool) == Some(true)).unwrap_or(false);
                let mut back = None;
                if started {
                    // Mac: poll up to 10x (1.5 s apart) for the session to come back open, or a fresh live run.
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                    for _ in 0..10 {
                        if let Ok(env) = a.agents_typed("coding", "") {
                            let open = |r: &crate::models::AgentRun| r.open || r.live || r.status == "waiting";
                            back = env.agents.iter().find(|r| r.id == id && open(r)).map(|r| r.id.clone()).or_else(|| {
                                env.agents.iter().find(|r| r.live && crate::widgets::iso_to_secs(&r.started_at).map_or(false, |t| t > now - 60.0)).map(|r| r.id.clone())
                            });
                        }
                        if back.is_some() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1500));
                    }
                }
                (id.clone(), r, back)
            }));
        }
        if open_chat {
            sh.agent_nav.open(&run.id);
            sh.goto_tab = Some(Tab::Chats);
        }
        if open_session {
            sh.current_sid = Some(run.session_id.clone());
            if !run.title.is_empty() {
                sh.current_title = Some(run.title.clone());
            }
            sh.goto_tab = Some(Tab::Chats);
        }
    }
}
