//! Night Shift: queue coding tasks for your agents overnight, review the results in the morning.
//! Mirrors NightShiftPanel.swift (+ NightDiffSheet, NightComposerSheet) and ExtrasKit.swift
//! (`ExtrasShell`, `HoverActionRow`, `CodingTargetFields`).
#![allow(dead_code)]
use super::coding_agents::select;
use crate::api::R;
use crate::ascii::{self, Art};
use crate::icons::{self, JbGlyph};
use crate::models::{NightHost, NightShiftState, NightTask};
use crate::state::{Job, Shared};
use crate::theme::{self, jb, ty, W, R_CARD, R_CTRL, R_PANEL};
use crate::widgets::{self, BtnKind, Level};
use eframe::egui::{self, Align2, Color32, Key, Rect, RichText, Rounding, Sense, Stroke, Ui, Vec2};
use serde_json::{json, Value};

const MODELS: [(&str, &[&str]); 4] = [
    ("claude", &["opus", "sonnet", "haiku"]),
    ("codex", &["gpt-5-codex", "gpt-5"]),
    ("opencode", &["qwen3-coder:30b", "llama3.3:70b", "deepseek-v3.2"]),
    ("agy", &["gemini-4-neon-high", "gemini-4-carbon-medium", "gemini-3.8-flash-high"]),
];

#[derive(Default)]
struct Composer {
    host_choices: Vec<NightHost>,
    repo: String,
    prompt: String,
    title: String,
    agent: String,
    model: String,
    host: String,
    tests: String,
    error: Option<String>,
    save: Option<Job<R<Value>>>,
    pick: Option<Job<Option<String>>>,
}

#[derive(Default)]
pub struct NightShiftPanel {
    review: bool, // false = Review page (default), see `page_queue`
    page_queue: bool,
    state: NightShiftState,
    loaded: bool,
    load: Option<Job<R<NightShiftState>>>,
    act: Option<Job<R<Value>>>,
    timer: f64,
    run_on: String,
    enabled: bool,
    note: Option<String>,
    composer: Option<Composer>,
    follow_up: Option<(NightTask, String)>,
    rejecting: Option<NightTask>,
    diff_task: Option<NightTask>,
    diff: String,
    diff_job: Option<Job<R<String>>>,
}

impl NightShiftPanel {
    fn reload(&mut self) {
        self.timer = 0.0;
    }

    fn multi_host(&self) -> bool {
        self.state.hosts.len() > 1
    }

    /// The chosen computer, else the Mac (the backend's fallback for unpinned tasks).
    fn current_host(&self) -> String {
        if !self.run_on.is_empty() {
            return self.run_on.clone();
        }
        let h = &self.state.hosts;
        h.iter().find(|h| h.label() == "Mac").map(|h| h.id.clone()).or_else(|| h.first().map(|h| h.id.clone())).unwrap_or_default()
    }

    fn meta(&self, t: &NightTask) -> String {
        let mut bits = vec![t.repo_name.clone(), t.agent_label().to_string(), t.model.clone()];
        if self.multi_host() && !t.host.is_empty() {
            bits.push(self.state.hosts.iter().find(|h| h.id == t.host).map(|h| h.label()).unwrap_or_else(|| t.host.clone()));
        }
        bits.into_iter().filter(|b| !b.is_empty()).collect::<Vec<_>>().join(" \u{b7} ")
    }

    fn do_act(&mut self, sh: &mut Shared, f: impl FnOnce(&crate::api::Api) -> R<Value> + Send + 'static) {
        self.act = Some(sh.spawn(f));
    }

    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let p = sh.palette;
        let dt = ui.input(|i| i.stable_dt) as f64;
        // ---- data
        if let Some(j) = &mut self.load {
            if let Some(res) = j.take() {
                self.load = None;
                match res {
                    Ok(s) => {
                        self.enabled = s.config.enabled;
                        self.run_on = s.config.default_host.clone();
                        self.state = s;
                        self.note = None;
                        self.loaded = true;
                    }
                    Err(e) if e.contains("404") || e.to_lowercase().contains("not found") => {
                        self.state.tasks.clear();
                        self.note = None;
                        self.loaded = true;
                    }
                    Err(e) => {
                        self.note = Some(e);
                        self.loaded = true;
                    }
                }
            }
        }
        if let Some(j) = &mut self.act {
            if let Some(res) = j.take() {
                self.act = None;
                match res {
                    Ok(_) => self.reload(),
                    Err(e) => self.note = Some(e),
                }
            }
        }
        if let Some(j) = &mut self.diff_job {
            if let Some(res) = j.take() {
                self.diff_job = None;
                self.diff = res.unwrap_or_default();
            }
        }
        self.timer -= dt;
        if self.timer <= 0.0 && self.load.is_none() {
            self.timer = 20.0;
            self.load = Some(sh.spawn(|a| a.night_shift()));
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));

        let reviews: Vec<NightTask> = self.state.tasks.iter().filter(|t| t.needs_review()).cloned().collect();
        let queue: Vec<NightTask> = self.state.tasks.iter().filter(|t| t.is_open()).cloned().collect();

        // ---- header (ExtrasShell)
        let rect = ui.max_rect();
        let hdr = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 46.0));
        ui.painter().extend(widgets::glass_surface(&p, hdr, R_PANEL, widgets::level_fill(&p, Level::Chrome), false));
        let mut refresh = false;
        let mut compose = false;
        let mut set_enabled: Option<bool> = None;
        let mut set_host: Option<String> = None;
        ui.allocate_new_ui(
            egui::UiBuilder::new().max_rect(hdr.shrink2(Vec2::new(16.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                icons::glyph(ui, JbGlyph::NightShift, 16.0, p.accent);
                ui.label(ty::sized("Night Shift", 14.0, W::Semibold).extra_letter_spacing(-0.2).color(p.ink));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).on_hover_text("Refresh").clicked() {
                        refresh = true;
                    }
                    if widgets::icon_btn(ui, &p, "plus", 26.0, false).on_hover_text("Queue a task").clicked() {
                        compose = true;
                    }
                    if self.multi_host() {
                        let cur = self.current_host();
                        let label = self.state.hosts.iter().find(|h| h.id == cur).map(|h| h.label()).unwrap_or_else(|| "Mac".into());
                        let g1 = ui.painter().layout_no_wrap("Runs on".into(), jb(11.5, W::Regular), p.ink3);
                        let g2 = ui.painter().layout_no_wrap(label, jb(11.5, W::Regular), p.ink);
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(g1.size().x + 4.0 + g2.size().x + 4.0 + 10.0, 24.0), Sense::click());
                        let mut x = r.left();
                        let (w1, w2) = (g1.size().x, g2.size().x);
                        ui.painter().galley(egui::pos2(x, r.center().y - g1.size().y / 2.0), g1, p.ink3);
                        x += w1 + 4.0;
                        ui.painter().galley(egui::pos2(x, r.center().y - g2.size().y / 2.0), g2, p.ink);
                        icons::paint_symbol(ui.painter(), egui::pos2(x + w2 + 9.0, r.center().y), "chevron.down", 8.0, p.ink3);
                        let resp = resp.on_hover_text("Where Night Shift agents run");
                        widgets::menu_popup(ui, &p, &resp, |ui| {
                            for h in &self.state.hosts {
                                let lbl = if h.id == cur || h.up { h.label() } else { format!("{} (offline)", h.label()) };
                                if widgets::menu_item(ui, &p, None, &lbl, h.id == cur, true).clicked() {
                                    set_host = Some(h.id.clone());
                                    ui.memory_mut(|m| m.close_popup());
                                }
                            }
                        });
                    }
                    let mut en = self.enabled;
                    if widgets::jb_switch(ui, &p, &mut en).changed() {
                        set_enabled = Some(en);
                    }
                    ui.label(ty::sized("Overnight", 11.0, W::Regular).color(p.ink2));
                });
            },
        );
        if refresh {
            self.reload();
        }
        if compose {
            self.composer = Some(Composer {
                host_choices: self.state.hosts.clone(),
                agent: "claude".into(),
                host: self.current_host(),
                ..Default::default()
            });
        }
        if let Some(e) = set_enabled {
            self.enabled = e;
            self.do_act(sh, move |a| a.night_config_enabled(e));
        }
        if let Some(h) = set_host {
            self.run_on = h.clone();
            self.do_act(sh, move |a| a.night_config_default_host(&h));
        }

        // ---- page tabs
        let body = Rect::from_min_max(egui::pos2(rect.left(), hdr.bottom() + 12.0), rect.max);
        let mut set_page: Option<bool> = None;
        let tabs = Rect::from_min_size(body.min, Vec2::new(body.width(), 28.0));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(tabs).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (q, name, n) in [(false, "Review", reviews.len()), (true, "Queue", queue.len())] {
                if tab_button(ui, &p, name, n, self.page_queue == q).clicked() {
                    set_page = Some(q);
                }
            }
            if let Some(n) = &self.note {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(ty::sized(n, 11.0, W::Regular).color(p.danger)).truncate());
                });
            }
        });
        if let Some(q) = set_page {
            self.page_queue = q;
        }

        // ---- list
        let list = Rect::from_min_max(egui::pos2(body.left(), tabs.bottom() + 12.0), rect.max);
        ui.painter().extend(widgets::glass_surface(&p, list, R_PANEL, p.panel_fill, false));
        let mut decide: Option<(NightTask, &'static str, Option<String>)> = None;
        let mut open_diff: Option<NightTask> = None;
        let mut ask_follow: Option<NightTask> = None;
        let mut ask_reject: Option<NightTask> = None;
        let mut act_run: Option<String> = None;
        let mut act_cancel: Option<String> = None;
        let mut reorder: Option<Vec<String>> = None;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(list), |ui| {
            ui.set_clip_rect(list.intersect(ui.clip_rect()));
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    if !self.page_queue {
                        if reviews.is_empty() {
                            ascii::empty_state(ui, &p, Art::Done, "Nothing to review");
                        }
                        for t in &reviews {
                            self.review_card(ui, &p, t, &mut decide, &mut open_diff, &mut ask_follow, &mut ask_reject);
                        }
                    } else {
                        if queue.is_empty() {
                            ascii::empty_state(ui, &p, Art::Sleep, "Nothing queued");
                        }
                        for (i, t) in queue.iter().enumerate() {
                            self.queue_row(ui, &p, t, i, &queue, &mut act_run, &mut act_cancel, &mut reorder);
                        }
                    }
                });
            });
        });
        if let Some((t, d, text)) = decide {
            let id = t.id.clone();
            self.do_act(sh, move |a| a.night_decide(&id, d, None, text.as_deref()));
        }
        if let Some(t) = open_diff {
            self.diff.clear();
            let id = t.id.clone();
            self.diff_job = Some(sh.spawn(move |a| a.night_diff(&id)));
            self.diff_task = Some(t);
        }
        if let Some(t) = ask_follow {
            self.follow_up = Some((t, String::new()));
        }
        if let Some(t) = ask_reject {
            self.rejecting = Some(t);
        }
        if let Some(id) = act_run {
            self.do_act(sh, move |a| a.night_run(&id));
        }
        if let Some(id) = act_cancel {
            self.do_act(sh, move |a| a.night_cancel(&id));
        }
        if let Some(ids) = reorder {
            self.do_act(sh, move |a| a.night_reorder(&ids));
        }
        ui.advance_cursor_after_rect(rect);

        self.sheets(ui.ctx(), sh);
    }

    #[allow(clippy::too_many_arguments)]
    fn review_card(
        &self,
        ui: &mut Ui,
        p: &theme::Palette,
        t: &NightTask,
        decide: &mut Option<(NightTask, &'static str, Option<String>)>,
        open_diff: &mut Option<NightTask>,
        follow: &mut Option<NightTask>,
        reject: &mut Option<NightTask>,
    ) {
        widgets::card(ui, p, 14.0, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                widgets::status_dot(ui, if t.state() == "failed" { p.warn } else { p.accent }, false, 8.0);
                ui.label(ty::sized(t.name(), 13.5, W::Semibold).extra_letter_spacing(-0.1).color(p.ink));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(ok) = t.result.as_ref().and_then(|r| r.tests.as_ref()).and_then(|x| x.passed) {
                        widgets::chip_tag(ui, p, if ok { "tests ok" } else { "tests failed" }, Some(if ok { p.live } else { p.danger }));
                    }
                });
            });
            ui.label(ty::sized(self.meta(t), 11.0, W::Regular).color(p.ink3));
            if let Some(d) = t.result.as_ref().and_then(|r| r.diff.as_ref()) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.label(ty::sized(format!("{} files", d.files), 11.5, W::Regular).color(p.ink2));
                    ui.label(ty::sized(format!("+{}", d.insertions), 11.5, W::Regular).color(p.live));
                    ui.label(ty::sized(format!("-{}", d.deletions), 11.5, W::Regular).color(p.danger));
                    if let Some(r) = &t.result {
                        if r.duration_s > 0.0 {
                            ui.label(ty::sized(format!("{}m", ((r.duration_s / 60.0) as i64).max(1)), 11.5, W::Regular).color(p.ink3));
                        }
                    }
                });
                for f in d.file_list.iter().take(3) {
                    ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), 15.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.label(ty::sized(format!("+{} -{}", f.added, f.removed), 11.0, W::Regular).color(p.ink3));
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(egui::Label::new(ty::sized(&f.path, 11.0, W::Regular).color(p.ink2)).truncate());
                        });
                    });
                }
            }
            let summary = t.result.as_ref().map(|r| r.summary.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| t.error.clone());
            if !summary.is_empty() {
                ui.add(egui::Label::new(ty::sized(summary, 12.0, W::Regular).color(p.ink2)).wrap());
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if t.state() == "review" && widgets::button_ex(ui, p, BtnKind::Primary, true, true, "Approve").clicked() {
                    *decide = Some((t.clone(), "approve", None));
                }
                if widgets::button_ex(ui, p, BtnKind::Secondary, true, true, "Follow-up").clicked() {
                    *follow = Some(t.clone());
                }
                if widgets::button_ex(ui, p, BtnKind::Danger, true, true, "Reject").clicked() {
                    *reject = Some(t.clone());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button_ex(ui, p, BtnKind::Ghost, true, true, "Diff").clicked() {
                        *open_diff = Some(t.clone());
                    }
                });
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn queue_row(
        &self,
        ui: &mut Ui,
        p: &theme::Palette,
        t: &NightTask,
        index: usize,
        queue: &[NightTask],
        run: &mut Option<String>,
        cancel: &mut Option<String>,
        reorder: &mut Option<Vec<String>>,
    ) {
        let id = egui::Id::new(("night-row", &t.id));
        let prev: Option<Rect> = ui.data(|d| d.get_temp(id));
        let pinned = t.state() == "running";
        let hovering = prev.map_or(false, |r| ui.rect_contains_pointer(r));
        let base = if let Some(r) = prev { Some(ui.interact(r, id.with("ctx"), Sense::click())) } else { None };
        let slot = ui.painter().add(egui::Shape::Noop);
        let w = ui.available_width();
        let inner = egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
            ui.set_width(w - 24.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if pinned {
                    widgets::status_dot(ui, p.live, true, 8.0);
                } else {
                    icons::glyph(ui, JbGlyph::NightShift, 14.0, p.ink3);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add(egui::Label::new(ty::sized(t.name(), 12.5, W::Medium).color(p.ink)).truncate());
                    ui.label(ty::sized(self.meta(t), 11.0, W::Regular).color(p.ink3));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if hovering || pinned {
                        if widgets::icon_btn(ui, p, "xmark", 26.0, false).on_hover_text("Cancel").clicked() {
                            *cancel = Some(t.id.clone());
                        }
                        if t.state() == "queued" {
                            if widgets::icon_btn(ui, p, "play", 26.0, false).on_hover_text("Run now").clicked() {
                                *run = Some(t.id.clone());
                            }
                            let can_down = index + 1 < queue.len();
                            if ui.add_enabled_ui(can_down, |ui| widgets::icon_btn(ui, p, "arrow.down", 26.0, false)).inner.on_hover_text("Move down").clicked() {
                                *reorder = move_ids(queue, index, 1);
                            }
                            let can_up = index > 0 && queue[index - 1].state() == "queued";
                            if ui.add_enabled_ui(can_up, |ui| widgets::icon_btn(ui, p, "arrow.up", 26.0, false)).inner.on_hover_text("Move up").clicked() {
                                *reorder = move_ids(queue, index, -1);
                            }
                        }
                    }
                });
            });
        });
        let rc = inner.response.rect;
        ui.data_mut(|d| d.insert_temp(id, rc));
        let r = Rounding::same(R_CARD);
        let mut shapes = vec![egui::Shape::rect_filled(rc, r, p.raised)];
        if hovering {
            shapes.push(egui::Shape::rect_filled(rc, r, theme::with_alpha(p.hover, 0.35)));
        }
        shapes.push(egui::Shape::rect_stroke(rc.shrink(0.5), r, Stroke::new(1.0, p.line)));
        ui.painter().set(slot, egui::Shape::Vec(shapes));
        if let Some(b) = base {
            b.context_menu(|ui| {
                if t.state() == "queued" && ui.button("Run now").clicked() {
                    *run = Some(t.id.clone());
                    ui.close_menu();
                }
                if ui.button(RichText::new("Cancel").color(p.danger)).clicked() {
                    *cancel = Some(t.id.clone());
                    ui.close_menu();
                }
            });
        }
    }

    // ---- sheets / alerts

    fn sheets(&mut self, ctx: &egui::Context, sh: &mut Shared) {
        let p = sh.palette;
        let any = self.composer.is_some() || self.diff_task.is_some() || self.follow_up.is_some() || self.rejecting.is_some();
        if any {
            let screen = ctx.screen_rect();
            egui::Area::new(egui::Id::new("night-scrim")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
                let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(90));
            });
        }
        // Diff sheet
        if let Some(t) = self.diff_task.clone() {
            let mut done = false;
            sheet_window(ctx, "night-diff", &p, 640.0, |ui| {
                sheet_header(ui, &p, &t.name(), |ui| {
                    if widgets::button_ex(ui, &p, BtnKind::Secondary, true, true, "Done").clicked() {
                        done = true;
                    }
                });
                egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    if let Some(out) = t.result.as_ref().and_then(|r| r.tests.as_ref()).map(|x| x.output_tail.clone()).filter(|o| !o.is_empty()) {
                        widgets::card(ui, &p, 8.0, |ui| {
                            ui.add(egui::Label::new(ty::sized(out, 11.0, W::Regular).color(p.ink2)).wrap());
                        });
                    }
                    for l in t.result.as_ref().map(|r| r.learned.clone()).unwrap_or_default() {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            icons::glyph(ui, JbGlyph::Memory, 13.0, p.accent);
                            ui.add(egui::Label::new(ty::sized(l, 12.0, W::Regular).color(p.ink2)).wrap());
                        });
                    }
                    egui::ScrollArea::vertical().max_height(340.0).min_scrolled_height(300.0).auto_shrink([false, false]).show(ui, |ui| {
                        ui.add(egui::Label::new(ty::sized(&self.diff, 11.0, W::Regular).color(p.ink)).selectable(true));
                    });
                });
            });
            if done || ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.diff_task = None;
            }
        }
        // Follow-up alert
        if let Some((t, mut text)) = self.follow_up.take() {
            let (mut send, mut cancel) = (false, false);
            alert(ctx, "night-follow", &p, "Follow-up", None, |ui| {
                widgets::field(ui, &p, &mut text, "What should change?");
            }, |ui| {
                if widgets::button_ex(ui, &p, BtnKind::Primary, true, true, "Send").clicked() {
                    send = true;
                }
                if widgets::button_ex(ui, &p, BtnKind::Ghost, true, true, "Cancel").clicked() {
                    cancel = true;
                }
            });
            if send {
                let id = t.id.clone();
                self.do_act(sh, move |a| a.night_decide(&id, "followup", None, Some(&text)));
            } else if !cancel && !ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.follow_up = Some((t, text));
            }
        }
        // Reject alert
        if let Some(t) = self.rejecting.take() {
            let (mut go, mut cancel) = (false, false);
            alert(ctx, "night-reject", &p, "Reject this work?", Some("The worktree and branch are removed."), |_| {}, |ui| {
                if widgets::button_ex(ui, &p, BtnKind::Danger, true, true, "Reject").clicked() {
                    go = true;
                }
                if widgets::button_ex(ui, &p, BtnKind::Secondary, true, true, "Keep reviewing").clicked() {
                    cancel = true;
                }
            });
            if go {
                let id = t.id.clone();
                self.do_act(sh, move |a| a.night_decide(&id, "reject", None, None));
            } else if !cancel && !ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.rejecting = Some(t);
            }
        }
        // Composer sheet
        if let Some(mut c) = self.composer.take() {
            let mut keep = self.composer_ui(ctx, sh, &mut c);
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                keep = false;
            }
            if keep {
                self.composer = Some(c);
            }
        }
    }

    /// `NightComposerSheet`. Returns false when it should close.
    fn composer_ui(&mut self, ctx: &egui::Context, sh: &mut Shared, c: &mut Composer) -> bool {
        let p = sh.palette;
        if let Some(j) = &mut c.save {
            if let Some(res) = j.take() {
                c.save = None;
                match res {
                    Ok(_) => {
                        self.reload();
                        return false;
                    }
                    Err(e) => c.error = Some(e),
                }
            }
        }
        if let Some(d) = sh.take_picked("night_repo") {
            c.repo = d.to_string_lossy().into_owned();
        }
        if let Some(j) = &mut c.pick {
            if let Some(res) = j.take() {
                c.pick = None;
                if let Some(d) = res {
                    c.repo = d;
                }
            }
        }
        let mut close = false;
        let mut save = false;
        let can = !c.prompt.trim().is_empty() && !c.repo.is_empty() && c.save.is_none();
        sheet_window(ctx, "night-compose", &p, 540.0, |ui| {
            sheet_header(ui, &p, "Queue a task", |ui| {
                if widgets::button_ex(ui, &p, BtnKind::Primary, true, can, "Queue").clicked() {
                    save = true;
                }
                if widgets::button_ex(ui, &p, BtnKind::Ghost, true, true, "Cancel").clicked() {
                    close = true;
                }
            });
            egui::Frame::none().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 12.0;
                widgets::field_frame(&p, false).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut c.prompt)
                            .hint_text(RichText::new("What should the agent do?").color(p.ink3))
                            .frame(false)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY)
                            .font(jb(13.0, W::Regular)),
                    );
                });
                widgets::field(ui, &p, &mut c.title, "Title");
                // CodingTargetFields
                let multi = c.host_choices.len() > 1;
                let n = if multi { 3.0 } else { 2.0 };
                let cw = (540.0 - 32.0 - 12.0 * (n - 1.0)) / n;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    eyebrow_field(ui, &p, "Agent", |ui| {
                        let opts: Vec<(String, String)> = [("claude", "Claude Code"), ("codex", "Codex"), ("opencode", "OpenCode"), ("agy", "Antigravity"), ("any", "Any")]
                            .iter()
                            .map(|(a, b)| (a.to_string(), b.to_string()))
                            .collect();
                        let cur = opts.iter().find(|o| o.0 == c.agent).map(|o| o.1.clone()).unwrap_or_default();
                        if let Some(v) = select(ui, &p, "n-agent", cw, &cur, &opts, &c.agent) {
                            c.agent = v;
                            c.model.clear();
                        }
                    });
                    eyebrow_field(ui, &p, "Model", |ui| {
                        let mut opts = vec![(String::new(), "Default".to_string())];
                        if let Some((_, l)) = MODELS.iter().find(|(a, _)| *a == c.agent) {
                            opts.extend(l.iter().map(|m| (m.to_string(), m.to_string())));
                        }
                        let cur = if c.model.is_empty() { "Default".to_string() } else { c.model.clone() };
                        if let Some(v) = select(ui, &p, "n-model", cw, &cur, &opts, &c.model) {
                            c.model = v;
                        }
                    });
                    if multi {
                        eyebrow_field(ui, &p, "Runs on", |ui| {
                            let opts: Vec<(String, String)> =
                                c.host_choices.iter().map(|h| (h.id.clone(), if h.up { h.label() } else { format!("{} (offline)", h.label()) })).collect();
                            let cur = opts.iter().find(|o| o.0 == c.host).map(|o| o.1.clone()).unwrap_or_default();
                            if let Some(v) = select(ui, &p, "n-host", cw, &cur, &opts, &c.host) {
                                c.host = v;
                            }
                        });
                    }
                });
                eyebrow_field(ui, &p, "Folder", |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let local = c.host.is_empty() || c.host_choices.iter().find(|h| h.id == c.host).map_or(true, |h| h.local);
                        let w = ui.available_width() - if local { 32.0 } else { 0.0 };
                        ui.allocate_ui(Vec2::new(w, 30.0), |ui| {
                            widgets::field(ui, &p, &mut c.repo, "/path/to/repo");
                        });
                        if local && widgets::icon_btn(ui, &p, "folder", 26.0, false).on_hover_text("Choose folder").clicked() {
                            sh.picker.open("night_repo", "Choose folder", crate::pickers::PickMode::Folder, None);
                        }
                    });
                });
                widgets::field(ui, &p, &mut c.tests, "Test command");
                if let Some(e) = &c.error {
                    ui.add(egui::Label::new(ty::sized(e, 11.0, W::Regular).color(p.danger)).wrap());
                }
            });
        });
        if save {
            let mut body = serde_json::Map::new();
            body.insert("prompt".into(), json!(c.prompt));
            body.insert("agent".into(), json!(c.agent));
            body.insert("repo".into(), json!(c.repo));
            for (k, v) in [("title", &c.title), ("model", &c.model), ("host", &c.host), ("test_command", &c.tests)] {
                if !v.is_empty() {
                    body.insert(k.into(), json!(v));
                }
            }
            c.error = None;
            c.save = Some(sh.spawn(move |a| a.night_add(Value::Object(body))));
        }
        !close
    }
}

fn move_ids(queue: &[NightTask], index: usize, delta: i64) -> Option<Vec<String>> {
    let mut ids: Vec<String> = queue.iter().filter(|t| t.state() == "queued").map(|t| t.id.clone()).collect();
    let running = queue.len() - ids.len();
    let i = index as i64 - running as i64;
    let j = i + delta;
    if i < 0 || j < 0 || i as usize >= ids.len() || j as usize >= ids.len() {
        return None;
    }
    ids.swap(i as usize, j as usize);
    Some(ids)
}

/// Review / Queue page button (`JBButtonStyle` compact, count in accent when active).
fn tab_button(ui: &mut Ui, p: &theme::Palette, name: &str, n: usize, active: bool) -> egui::Response {
    let g1 = ui.painter().layout_no_wrap(name.to_string(), jb(12.0, W::Medium), p.ink);
    let g2 = (n > 0).then(|| ui.painter().layout_no_wrap(n.to_string(), jb(10.5, W::Regular), if active { p.accent } else { p.ink3 }));
    let w = g1.size().x + g2.as_ref().map_or(0.0, |g| g.size().x + 6.0) + 18.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
    let hov = resp.hovered();
    let fill = if active { p.raised } else if hov { p.hover } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, Rounding::same(R_CTRL), fill);
    if active {
        ui.painter().rect_stroke(rect.shrink(0.5), Rounding::same(R_CTRL), Stroke::new(1.0, p.line));
    }
    let mut x = rect.left() + 9.0;
    let w1 = g1.size().x;
    ui.painter().galley(egui::pos2(x, rect.center().y - g1.size().y / 2.0), g1, p.ink);
    x += w1 + 6.0;
    if let Some(g) = g2 {
        ui.painter().galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, p.ink3);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn eyebrow_field(ui: &mut Ui, p: &theme::Palette, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 5.0;
        widgets::eyebrow(ui, p, &label.to_uppercase());
        add(ui);
    });
}

fn sheet_window(ctx: &egui::Context, id: &str, p: &theme::Palette, width: f32, add: impl FnOnce(&mut Ui)) {
    egui::Window::new(id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(widgets::floating_frame(p, R_PANEL).inner_margin(egui::Margin::ZERO))
        .fixed_size(Vec2::new(width, 0.0))
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
}

/// `ExtrasSheetHeader`: title left, actions right, hairline beneath.
fn sheet_header(ui: &mut Ui, p: &theme::Palette, title: &str, actions: impl FnOnce(&mut Ui)) {
    egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(26.0);
            ui.add(egui::Label::new(ty::sized(title, 14.0, W::Semibold).color(p.ink)).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                actions(ui)
            });
        });
    });
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, p.line);
}

/// A small centred alert (title, optional message, body, button row).
fn alert(ctx: &egui::Context, id: &str, p: &theme::Palette, title: &str, message: Option<&str>, body: impl FnOnce(&mut Ui), buttons: impl FnOnce(&mut Ui)) {
    egui::Window::new(id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(widgets::floating_frame(p, R_PANEL).inner_margin(egui::Margin::same(20.0)))
        .fixed_size(Vec2::new(320.0, 0.0))
        .show(ctx, |ui| {
            ui.set_width(320.0);
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.label(ty::sized(title, 14.0, W::Semibold).color(p.ink));
            if let Some(m) = message {
                ui.add(egui::Label::new(ty::sized(m, 12.0, W::Regular).color(p.ink2)).wrap());
            }
            body(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                buttons(ui)
            });
        });
}
