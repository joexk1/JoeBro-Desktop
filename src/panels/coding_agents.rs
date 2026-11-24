//! Coding agents as chats. Mirrors CodingAgentConversation.swift (the chat-area view of one agent),
//! AgentChat.swift (transcript, composer, approvals, `AgentPromptCard`, model presets) and
//! NewCodingAgentSheet.swift. State: `sh.agent_nav`; API: `Api::coding_*`, `agent_detail_typed`.
#![allow(dead_code)]
use super::mdview::MdView;
use crate::api::R;
use crate::models::{AgentMessage, AgentPrompt, AgentRun, CodingModel, ResumableSession};
use crate::state::{Job, Shared};
use crate::theme::{self, jb, mono, ty, W, R_CARD, R_CTRL, R_PANEL};
use crate::widgets::{self, BtnKind, Level};
use crate::{ascii, icons, logos};
use eframe::egui::{self, Align2, Color32, Key, Modifiers, Rect, RichText, Rounding, Sense, Stroke, Ui, Vec2};
use serde_json::{json, Value};

// ---- helpers ported from AgentChat.swift ---------------------------------------------------

/// "Message from app via JoeBro: hi" / "[from app] hi" -> "hi".
pub fn handover_stripped(s: &str) -> String {
    let mut t = s.replace("[from app] ", "");
    if let Some(rest) = t.strip_prefix("Message from ") {
        if let Some(i) = rest.find(": ") {
            if i >= 1 && i <= 40 && !rest[..i].contains('\n') {
                t = rest[i + 2..].to_string();
            }
        }
    }
    t
}

/// Delivered-yet check: the feed collapses whitespace and clips long text, so compare normalised prefixes.
pub fn same_message(feed: &str, sent: &str) -> bool {
    fn norm(s: &str) -> String {
        handover_stripped(s).split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
    }
    let (a, b) = (norm(feed), norm(sent));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let n = 80.min(a.chars().count()).min(b.chars().count());
    let pa: String = a.chars().take(n).collect();
    let pb: String = b.chars().take(n).collect();
    a.contains(&pb) || b.starts_with(&pa)
}

/// Default model presets per harness, merged with server-reported options and the active model.
pub fn model_defaults(harness: &str) -> Vec<CodingModel> {
    let l: &[(&str, &str)] = match harness {
        "claude" => &[("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku"), ("default", "Default")],
        "agy" => &[
            ("gemini-4-neon-high", "Gemini 4 Neon (High)"),
            ("gemini-4-neon-medium", "Gemini 4 Neon (Medium)"),
            ("gemini-4-carbon-high", "Gemini 4 Carbon (High)"),
            ("gemini-4-hydrogen-high", "Gemini 4 Hydrogen (High)"),
            ("gemini-pro-orchestrator-neon", "Orchestrator (Neon)"),
            ("claude-opus-5-5-high", "Claude Opus 5.5 (High)"),
            ("claude-sonnet-5-5-high", "Claude Sonnet 5.5 (High)"),
        ],
        "codex" => &[
            ("gpt-6.2-sol", "GPT-6.2 Sol"),
            ("gpt-6.2-astra", "GPT-6.2 Astra"),
            ("gpt-6.1-terra", "GPT-6.1 Terra"),
            ("gpt-5-codex", "GPT-5 Codex"),
            ("gpt-5", "GPT-5"),
        ],
        "opencode" => &[("qwen3-coder:30b", "Qwen3 Coder 30B"), ("llama3.3:70b", "Llama 3.3 70B"), ("deepseek-v3.2", "DeepSeek V3.2")],
        _ => &[],
    };
    l.iter().map(|(i, n)| CodingModel { id: i.to_string(), label: n.to_string() }).collect()
}

pub fn model_options(harness: &str, current: &str, server: &[CodingModel]) -> Vec<CodingModel> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<CodingModel> = vec![];
    let mut add = |m: CodingModel| {
        let k = m.id.trim().to_lowercase();
        if !k.is_empty() && seen.insert(k) {
            out.push(m);
        }
    };
    let cur = current.trim();
    if !cur.is_empty() {
        let pool = if server.is_empty() { model_defaults(harness) } else { server.to_vec() };
        let hit = pool.into_iter().find(|m| m.id.to_lowercase() == cur.to_lowercase());
        add(hit.unwrap_or(CodingModel { id: cur.to_string(), label: cur.rsplit('/').next().unwrap_or(cur).to_string() }));
    }
    for m in server {
        add(m.clone());
    }
    for m in model_defaults(harness) {
        add(m);
    }
    out
}

/// Only the exact model is ticked; a preset alias (`opus`) is ticked only when it is what's selected.
pub fn is_current_model(active: &str, id: &str) -> bool {
    !active.trim().is_empty() && active.trim().eq_ignore_ascii_case(id.trim())
}

pub fn model_label(model: &str, options: &[CodingModel]) -> String {
    if model.is_empty() {
        return "Model".into();
    }
    options
        .iter()
        .find(|m| m.id.to_lowercase() == model.to_lowercase())
        .map(|m| m.label.clone())
        .unwrap_or_else(|| model.rsplit('/').next().unwrap_or(model).to_string())
}

/// Harness slash commands typed into the agent like any message.
pub fn agent_commands(harness: &str) -> Vec<(&'static str, &'static str)> {
    match harness {
        "agy" => vec![
            ("model", "switch model"),
            ("effort", "set reasoning effort (low|medium|high|xhigh)"),
            ("compact", "summarise the conversation so far"),
            ("plan", "draft an implementation plan"),
            ("diff", "view current changes"),
            ("learn", "capture rules or skills from this session"),
            ("usage", "display model quota usage"),
        ],
        "codex" => vec![
            ("model", "switch model"),
            ("compact", "summarise the conversation so far"),
            ("review", "review the current changes"),
            ("diff", "view current diff"),
            ("new", "start a new session"),
        ],
        "opencode" => vec![
            ("models", "choose model"),
            ("compact", "summarise the conversation so far"),
            ("new", "start a new session"),
        ],
        _ => vec![
            ("compact", "summarise the conversation so far"),
            ("clear", "start fresh"),
            ("model", "switch model"),
            ("review", "review the current changes"),
            ("cost", "token usage"),
            ("memory", "edit memory files"),
            ("init", "write a CLAUDE.md for this repo"),
        ],
    }
}

/// Still a conversation you can type into (running, waiting on you, or open between turns).
pub fn is_open_session(r: &AgentRun) -> bool {
    r.open || r.live || r.status == "waiting"
}

pub fn status_colour(p: &theme::Palette, r: &AgentRun) -> Color32 {
    if r.status == "waiting" || r.status == "limited" {
        p.warn
    } else if r.live {
        p.live
    } else {
        p.ink3
    }
}

/// `statusChip`: dot + "Needs you" / "Working" / status.
pub fn status_chip(ui: &mut Ui, p: &theme::Palette, r: &AgentRun) {
    let waiting = r.status == "waiting";
    let color = status_colour(p, r);
    let text = if waiting {
        "Needs you".to_string()
    } else if r.live {
        "Working".to_string()
    } else if r.status == "limited" {
        "Hit usage limit".to_string()
    } else {
        let mut c = r.status.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        widgets::status_dot(ui, color, r.live, 6.0);
        ui.label(ty::sized(text, 12.0, W::Medium).color(color));
    });
}

// ---- prompt card + approvals -----------------------------------------------------------------

/// `AgentPromptCard` state: the prompt shown (steps through multi-question forms), typed text, busy note.
#[derive(Default)]
pub struct PromptCard {
    sig: String,
    shown: Option<AgentPrompt>,
    typed: String,
    job: Option<Job<R<Option<AgentPrompt>>>>,
    note: Option<String>,
}

fn prompt_sig(p: &AgentPrompt) -> String {
    format!("{}|{}|{}", p.header, p.question, p.options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>().join(","))
}

impl PromptCard {
    /// Returns true when the agent was answered with nothing more to ask (caller reloads).
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared, agent_id: &str, prompt: &AgentPrompt) -> bool {
        let p = sh.palette;
        let sig = prompt_sig(prompt);
        if sig != self.sig {
            self.sig = sig;
            self.shown = None;
        }
        let mut done = false;
        if let Some(job) = &mut self.job {
            if let Some(res) = job.take() {
                self.job = None;
                match res {
                    Ok(Some(next)) => {
                        self.typed.clear();
                        self.shown = Some(next);
                    }
                    Ok(None) => {
                        self.typed.clear();
                        self.note = Some("Answered".into());
                        done = true;
                    }
                    Err(e) => self.note = Some(e),
                }
            }
        }
        let busy = self.job.is_some();
        let cur = self.shown.clone().unwrap_or_else(|| prompt.clone());
        let mut answer: Option<(Option<String>, Option<String>)> = None;
        ui.spacing_mut().item_spacing.y = 8.0;
        if !cur.header.is_empty() {
            widgets::eyebrow(ui, &p, &cur.header);
        }
        if !cur.question.is_empty() {
            ui.add(egui::Label::new(ty::sized(&cur.question, 13.0, W::Medium).color(p.ink)).wrap());
        }
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for (i, o) in cur.options.iter().filter(|o| !o.freeform).enumerate() {
                let r = widgets::hover_row(ui, &p, false, R_CTRL, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let (kr, _) = ui.allocate_exact_size(Vec2::new(14.0, 16.0), Sense::hover());
                        let k = if o.key.is_empty() { (i + 1).to_string() } else { o.key.clone() };
                        ui.painter().text(kr.center(), Align2::CENTER_CENTER, k, mono(10.5, W::Regular), p.ink3);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.add(egui::Label::new(ty::sized(&o.label, 12.5, W::Regular).color(p.ink)).truncate());
                            if !o.description.is_empty() && o.description != o.label {
                                ui.add(egui::Label::new(ty::sized(&o.description, 11.0, W::Regular).color(p.ink3)).wrap());
                            }
                        });
                    });
                });
                if r.clicked() && !busy {
                    let key = if o.key.is_empty() { o.label.clone() } else { o.key.clone() };
                    answer = Some((Some(key), None));
                }
            }
        });
        if cur.options.iter().any(|o| o.freeform) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let can = !self.typed.trim().is_empty() && !busy;
                let w = ui.available_width() - 64.0;
                let id = ui.id().with(("prompt-typed", agent_id));
                let focused = ui.memory(|m| m.has_focus(id));
                let mut submit = false;
                widgets::field_frame(&p, focused).inner_margin(egui::Margin::symmetric(8.0, 6.0)).show(ui, |ui| {
                    ui.set_width(w - 16.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.typed)
                            .id(id)
                            .hint_text(RichText::new("Type your own answer").color(p.ink3))
                            .frame(false)
                            .desired_width(f32::INFINITY)
                            .font(jb(12.5, W::Regular)),
                    );
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        submit = true;
                    }
                });
                if widgets::button_ex(ui, &p, BtnKind::Secondary, false, can, "Send").clicked() || (submit && can) {
                    answer = Some((None, Some(self.typed.trim().to_string())));
                }
            });
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if busy {
                ascii::spinner(ui, 12.0, p.ink2);
            }
            if let Some(n) = &self.note {
                ui.add(egui::Label::new(ty::sized(n, 10.5, W::Regular).color(p.ink3)).truncate());
            }
        });
        if let Some((key, text)) = answer {
            self.note = None;
            let id = agent_id.to_string();
            self.job = Some(sh.spawn(move |a| a.coding_answer(&id, key.as_deref(), text.as_deref())));
        }
        done
    }
}

/// `AgentApprovalButtons`: Approve / Deny, reports the outcome in place.
#[derive(Default)]
pub struct Approval {
    job: Option<Job<(bool, R<Value>)>>,
    note: Option<String>,
}

impl Approval {
    /// Returns true once a decision was sent.
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared, agent_id: &str) -> bool {
        let p = sh.palette;
        let mut done = false;
        if let Some(job) = &mut self.job {
            if let Some((approve, res)) = job.take() {
                self.job = None;
                match res {
                    Ok(v) => {
                        let applied = v.get("applied").and_then(Value::as_bool) == Some(true);
                        self.note = Some(if applied {
                            if approve { "approved".into() } else { "denied".into() }
                        } else {
                            v.get("detail").and_then(Value::as_str).unwrap_or("pending").to_string()
                        });
                        done = true;
                    }
                    Err(e) => self.note = Some(e),
                }
            }
        }
        let busy = self.job.is_some();
        let mut decide: Option<bool> = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if widgets::button_ex(ui, &p, BtnKind::Primary, false, !busy, "Approve").clicked() {
                decide = Some(true);
            }
            if widgets::button_ex(ui, &p, BtnKind::Danger, false, !busy, "Deny").clicked() {
                decide = Some(false);
            }
            if busy {
                ascii::spinner(ui, 12.0, p.ink2);
            }
            if let Some(n) = &self.note {
                ui.add(egui::Label::new(ty::sized(n, 10.5, W::Regular).color(p.ink3)).truncate());
            }
        });
        if let Some(a) = decide {
            let id = agent_id.to_string();
            self.job = Some(sh.spawn(move |api| (a, api.coding_decide(&id, a))));
        }
        done
    }
}

// ---- the thread ---------------------------------------------------------------------------------

enum Item<'a> {
    Msg(&'a AgentMessage),
    Tools(Vec<&'a str>),
}

fn fold(messages: &[AgentMessage]) -> Vec<Item<'_>> {
    let mut out: Vec<Item> = vec![];
    for m in messages {
        if !["user", "assistant", "approval", "notice", "event"].contains(&m.role.as_str()) {
            if let Some(Item::Tools(l)) = out.last_mut() {
                l.push(&m.content);
            } else {
                out.push(Item::Tools(vec![&m.content]));
            }
        } else {
            out.push(Item::Msg(m));
        }
    }
    out
}

/// Chat-style view of a coding agent: user prompts as raised cards, replies as plain text,
/// tool calls as one line, and a composer that talks to the agent (`AgentChatThread`).
#[derive(Default)]
pub struct AgentThread {
    draft: String,
    slash_src: super::chat::cui::SlashSource,
    echoes: Vec<(String, String)>,
    error: Option<String>,
    pub show_screen: bool,
    screen: String,
    screen_job: Option<Job<R<String>>>,
    screen_timer: f64,
    send_job: Option<Job<(String, R<String>)>>,
    model_job: Option<Job<(String, R<String>)>>,
    picked_model: Option<String>,
    custom_open: bool,
    custom_text: String,
    slash_index: usize,
    expanded: std::collections::HashSet<usize>,
    prompt: PromptCard,
    approval: Approval,
    md: MdView,
    last_count: usize,
    agent_id: String,
    recording: Option<crate::voice::Recording>,
    transcription: Option<Job<R<String>>>,
}

impl AgentThread {
    fn slash_items(&self, run: &AgentRun) -> Vec<(String, String)> {
        let d = &self.draft;
        if !d.starts_with('/') || d.contains(' ') || d.contains('\n') {
            return vec![];
        }
        let q = d[1..].to_lowercase();
        let mut out: Vec<(String, String)> = agent_commands(&run.harness())
            .into_iter()
            .filter(|(n, _)| q.is_empty() || n.starts_with(&q))
            .map(|(n, l)| (n.to_string(), l.to_string()))
            .collect();
        // Plus your skills (Mac `slash.matches(q).filter { $0.kind == .skill }`).
        out.extend(self.slash_src.skill_matches(&q).into_iter().map(|i| (i.name, i.label)));
        out
    }

    /// Draw the thread in `ui.max_rect()`. Returns true when the run changed and should be reloaded.
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared, run: &AgentRun, read_only: bool) -> bool {
        if self.agent_id != run.id {
            let slash_src = std::mem::take(&mut self.slash_src);
            *self = AgentThread { agent_id: run.id.clone(), slash_src, ..Default::default() };
        }
        if self.draft.starts_with('/') {
            self.slash_src.refresh(sh);
        }
        let mut changed = false;
        // ---- jobs
        if let Some(j) = &mut self.send_job {
            if let Some((text, res)) = j.take() {
                self.send_job = None;
                let working = run.live && run.status != "waiting";
                match res {
                    Ok(how) => {
                        if let Some(e) = self.echoes.iter_mut().rev().find(|e| e.0 == text) {
                            e.1 = if how == "terminal" && !working { "sent".into() } else { "queued".into() };
                        }
                        self.error = None;
                        changed = true;
                    }
                    Err(e) => {
                        self.echoes.retain(|x| x.0 != text);
                        self.draft = text;
                        self.error = Some(e);
                    }
                }
            }
        }
        if let Some(j) = &mut self.model_job {
            if let Some((model, res)) = j.take() {
                self.model_job = None;
                match res {
                    Ok(_) => {
                        self.picked_model = Some(model);
                        self.error = None;
                        changed = true;
                    }
                    Err(e) => self.error = Some(format!("Model change needs the agent idle in a Kitty/tmux terminal: {e}")),
                }
            }
        }
        if let Some(j) = &mut self.transcription {
            if let Some(res) = j.take() {
                self.transcription = None;
                match res {
                    Ok(t) if !t.trim().is_empty() => {
                        if self.draft.is_empty() {
                            self.draft = t;
                        } else {
                            self.draft = format!("{} {}", self.draft, t);
                        }
                    }
                    Err(e) => self.error = Some(e),
                    _ => {}
                }
            }
        }
        // screen polling every 2 s
        if self.show_screen {
            if let Some(j) = &mut self.screen_job {
                if let Some(Ok(s)) = j.take() {
                    self.screen = s;
                }
                if j.done {
                    self.screen_job = None;
                }
            }
            self.screen_timer -= ui.input(|i| i.stable_dt) as f64;
            if self.screen_timer <= 0.0 && self.screen_job.is_none() {
                self.screen_timer = 2.0;
                let id = run.id.clone();
                self.screen_job = Some(sh.spawn(move |a| a.coding_screen(&id, 40)));
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }

        let can_send = !read_only && run.kind == "coding" && run.status != "ended";
        let working = run.live && run.status != "waiting";
        let rect = ui.max_rect();
        let col_w = rect.width().min(860.0);
        let col_x = rect.center().x - col_w / 2.0;

        // ---- composer (bottom), measured the frame before
        let mut comp_top = rect.bottom();
        if can_send {
            let hid = ui.id().with(("comp-h", &run.id));
            let h: f32 = ui.data(|d| d.get_temp(hid)).unwrap_or(90.0);
            let crect = Rect::from_min_max(egui::pos2(col_x + 24.0, rect.bottom() - 12.0 - h), egui::pos2(col_x + col_w - 24.0, rect.bottom() - 12.0));
            let used = ui.allocate_new_ui(egui::UiBuilder::new().max_rect(crect), |ui| {
                self.composer(ui, sh, run, &mut changed);
            });
            let nh = used.response.rect.height();
            if (nh - h).abs() > 0.5 {
                ui.data_mut(|d| d.insert_temp(hid, nh));
                ui.ctx().request_repaint();
            }
            comp_top = crect.bottom() - nh - 12.0;
        }

        // ---- transcript
        let top = Rect::from_min_max(rect.min, egui::pos2(rect.right(), comp_top));
        let count = run.messages.len() + self.echoes.len();
        let grew = count != self.last_count;
        self.last_count = count;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(top), |ui| {
            egui::ScrollArea::vertical().id_salt(("thread", &run.id)).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                let pad_x = ((ui.available_width() - col_w) / 2.0).max(0.0) + 24.0;
                egui::Frame::none().inner_margin(egui::Margin::symmetric(pad_x, 16.0)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    self.transcript(ui, sh, run, working, &mut changed);
                });
                if grew {
                    ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                }
            });
        });
        changed
    }

    fn transcript(&mut self, ui: &mut Ui, sh: &mut Shared, run: &AgentRun, working: bool, changed: &mut bool) {
        let p = sh.palette;
        let items = fold(&run.messages);
        let n = items.len();
        for (i, item) in items.iter().enumerate() {
            match item {
                Item::Msg(m) => {
                    ui.add_space(7.0);
                    self.bubble(ui, sh, run, m, i == n - 1, changed);
                    ui.add_space(7.0);
                }
                Item::Tools(lines) => self.tool_group(ui, &p, i, lines, working && i == n - 1),
            }
        }
        let echoes: Vec<(String, String)> = self
            .echoes
            .iter()
            .filter(|e| !run.messages.iter().any(|m| m.role == "user" && same_message(&m.content, &e.0)))
            .cloned()
            .collect();
        for (t, via) in echoes {
            ui.add_space(7.0);
            user_card(ui, &p, &t, Some(&via));
            ui.add_space(7.0);
        }
        if working {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ascii::spinner(ui, 12.0, p.ink2);
                ascii::stream_cursor(ui, p.ink2, 14.0);
            });
        }
        if run.status == "waiting" && !run.messages.iter().any(|m| m.role == "approval") {
            self.approval_card(ui, sh, run, "needs your approval", true, changed);
        }
        if self.show_screen {
            ui.add_space(8.0);
            widgets::card(ui, &p, 12.0, |ui| {
                ui.set_width(ui.available_width());
                let t = if self.screen.is_empty() { " " } else { self.screen.as_str() };
                ui.add(egui::Label::new(ty::mono(t, 10.5, W::Regular).color(p.ink2)).selectable(true));
            });
        }
    }

    fn bubble(&mut self, ui: &mut Ui, sh: &mut Shared, run: &AgentRun, m: &AgentMessage, last: bool, changed: &mut bool) {
        let p = sh.palette;
        match m.role.as_str() {
            "user" => user_card(ui, &p, &handover_stripped(&m.content), None),
            "assistant" => {
                let w = ui.available_width();
                ui.set_width(w);
                ui.set_max_width(w);
                self.md.show(ui, sh, &super::mdview::restore_structure(&m.content));
            }
            "approval" => self.approval_card(ui, sh, run, &m.content, last && run.status == "waiting", changed),
            "notice" => {
                let r = Rounding::same(R_CTRL);
                let slot = ui.painter().add(egui::Shape::Noop);
                let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(12.0, 8.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        icons::symbol(ui, "exclamationmark.triangle.fill", 11.0, p.warn);
                        ui.add(egui::Label::new(ty::sized(&m.content, 12.5, W::Medium).color(p.ink)).wrap());
                    });
                });
                let rc = inner.response.rect;
                ui.painter().set(
                    slot,
                    egui::Shape::Vec(vec![
                        egui::Shape::rect_filled(rc, r, theme::with_alpha(p.warn, 0.12)),
                        egui::Shape::rect_stroke(rc.shrink(0.5), r, Stroke::new(1.0, theme::with_alpha(p.warn, 0.35))),
                    ]),
                );
            }
            "event" => {
                let fits = ui.painter().layout_no_wrap(m.content.clone(), jb(11.5, W::Regular), p.ink3).size().x + 48.0 <= ui.available_width();
                if !fits {
                    ui.add(egui::Label::new(ty::sized(&m.content, 11.5, W::Regular).color(p.ink3)).wrap());
                    return;
                }
                ui.horizontal(|ui| {
                    let galley = ui.painter().layout_no_wrap(m.content.clone(), jb(11.5, W::Regular), p.ink3);
                    let side = ((ui.available_width() - galley.size().x - 16.0) / 2.0).max(8.0);
                    let (l, _) = ui.allocate_exact_size(Vec2::new(side, 14.0), Sense::hover());
                    ui.painter().hline(l.x_range(), l.center().y, Stroke::new(1.0, p.line));
                    let (t, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
                    ui.painter().galley(t.min, galley, p.ink3);
                    let (r, _) = ui.allocate_exact_size(Vec2::new(side, 14.0), Sense::hover());
                    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, p.line));
                });
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    widgets::status_dot(ui, p.ink3, false, 5.0);
                    ui.add(egui::Label::new(ty::sized(&m.content, 11.0, W::Regular).color(p.ink2)).truncate());
                });
            }
        }
    }

    fn approval_card(&mut self, ui: &mut Ui, sh: &mut Shared, run: &AgentRun, text: &str, active: bool, changed: &mut bool) {
        let p = sh.palette;
        let r = Rounding::same(R_CARD);
        let slot = ui.painter().add(egui::Shape::Noop);
        let w = ui.available_width();
        let inner = egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
            ui.set_width(w - 24.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                widgets::status_dot(ui, p.warn, active, 6.0);
                widgets::eyebrow(ui, &p, "Needs you");
            });
            match (&run.prompt, active) {
                (Some(prompt), true) if !prompt.options.is_empty() => {
                    if self.prompt.ui(ui, sh, &run.id, prompt) {
                        *changed = true;
                    }
                }
                _ => {
                    ui.add(egui::Label::new(ty::sized(text.replace("Question: ", ""), 11.5, W::Regular).color(p.ink)).wrap());
                    if active && self.approval.ui(ui, sh, &run.id) {
                        *changed = true;
                    }
                }
            }
        });
        let rc = inner.response.rect;
        let mut shapes = vec![];
        if active {
            shapes.push(egui::Shape::rect_filled(rc, r, theme::with_alpha(p.surface, 0.96)));
        }
        shapes.push(egui::Shape::rect_filled(rc, r, p.raised));
        shapes.push(egui::Shape::rect_stroke(rc.shrink(0.5), r, Stroke::new(1.0, p.line)));
        if active {
            shapes.push(egui::Shape::rect_stroke(rc.shrink(0.5), r, Stroke::new(1.0, theme::with_alpha(p.warn, 0.5))));
        }
        ui.painter().set(slot, egui::Shape::Vec(shapes));
    }

    fn tool_group(&mut self, ui: &mut Ui, p: &theme::Palette, idx: usize, lines: &[&str], running: bool) {
        let exp = self.expanded.contains(&idx);
        let slot = ui.painter().add(egui::Shape::Noop);
        let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(10.0, 5.0)).show(ui, |ui| {
            let head = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.set_min_height(22.0);
                    widgets::status_dot(ui, if running { p.live } else { p.ink3 }, running, 6.0);
                    let maxw = (ui.available_width() - 60.0).max(40.0);
                    ui.scope(|ui| {
                        ui.set_max_width(maxw);
                        ui.add(egui::Label::new(ty::sized(lines.last().copied().unwrap_or(""), 12.0, W::Regular).color(p.ink2)).truncate());
                    });
                    if lines.len() > 1 {
                        ui.label(ty::sized(lines.len().to_string(), 11.0, W::Regular).color(p.ink3));
                    }
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    icons::paint_symbol(ui.painter(), r.center(), if exp { "chevron.down" } else { "chevron.right" }, 8.0, p.ink3);
                })
                .response;
            if exp {
                ui.add_space(4.0);
                ui.indent(("tools", idx), |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    for l in lines {
                        ui.add(egui::Label::new(ty::mono(*l, 11.0, W::Regular).color(p.ink2)).selectable(true));
                    }
                });
            }
            head
        });
        let rc = inner.response.rect;
        let rr = Rounding::same(R_CTRL + 1.0);
        ui.painter().set(
            slot,
            egui::Shape::Vec(vec![
                egui::Shape::rect_filled(rc, rr, p.panel_fill),
                egui::Shape::rect_stroke(rc.shrink(0.5), rr, Stroke::new(1.0, p.line)),
            ]),
        );
        let head_rect = Rect::from_min_size(rc.min, Vec2::new(rc.width(), 32.0));
        if ui.interact(head_rect, ui.id().with(("toolhead", idx)), Sense::click()).clicked() {
            if exp {
                self.expanded.remove(&idx);
            } else {
                self.expanded.insert(idx);
            }
        }
    }

    // ---- composer

    fn submit(&mut self, sh: &mut Shared, run: &AgentRun) {
        let text = self.draft.trim().to_string();
        if text.is_empty() || self.send_job.is_some() {
            return;
        }
        self.draft.clear();
        let working = run.live && run.status != "waiting";
        self.echoes.push((text.clone(), if working { "queued".into() } else { "sending".into() }));
        let id = run.id.clone();
        self.send_job = Some(sh.spawn(move |a| {
            let r = a.coding_send(&id, &text, "auto");
            (text, r)
        }));
    }

    fn composer(&mut self, ui: &mut Ui, sh: &mut Shared, run: &AgentRun, changed: &mut bool) {
        let p = sh.palette;
        let _ = changed;
        let te_id = ui.id().with(("agent-composer", &run.id));
        let focused = ui.memory(|m| m.has_focus(te_id));
        let items = self.slash_items(run);
        let mut recall = false;
        let mut send_now = false;
        if focused {
            if !items.is_empty() {
                let i = self.slash_index.min(items.len() - 1);
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                    self.slash_index = (i + items.len() - 1) % items.len();
                }
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                    self.slash_index = (i + 1) % items.len();
                }
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::NONE, Key::Tab)) {
                    self.draft = format!("/{} ", items[i].0);
                }
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                    self.draft.clear();
                }
            } else {
                let shift = ui.input(|i| i.modifiers.shift);
                if !shift && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                    send_now = true;
                }
                if self.draft.is_empty() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                    recall = true;
                }
            }
        }
        if recall {
            let last = self
                .echoes
                .last()
                .map(|e| e.0.clone())
                .or_else(|| run.messages.iter().rev().find(|m| m.role == "user").map(|m| handover_stripped(&m.content)));
            if let Some(l) = last {
                self.draft = l;
            }
        }
        if send_now {
            self.submit(sh, run);
        }

        let slot = ui.painter().add(egui::Shape::Noop);
        let inner = egui::Frame::none().inner_margin(egui::Margin { left: 14.0, right: 14.0, top: 12.0, bottom: 8.0 }).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.set_width(ui.available_width());
            let title = if run.title.is_empty() { "agent" } else { run.title.as_str() };
            let rows = self.draft.lines().count().clamp(1, 8);
            let before = self.draft.clone();
            ui.scope(|ui| {
                ui.visuals_mut().override_text_color = Some(p.ink);
                ui.add(
                    egui::TextEdit::multiline(&mut self.draft)
                        .id(te_id)
                        .hint_text(RichText::new(format!("Message {title}")).color(p.ink3))
                        .frame(false)
                        .desired_rows(rows)
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(7.0, 2.0))
                        .font(jb(14.0, W::Regular)),
                );
            });
            if self.draft != before {
                self.slash_index = 0;
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if !run.harness().is_empty() {
                    self.model_menu(ui, sh, run);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let can = !self.draft.trim().is_empty() && self.send_job.is_none();
                    if widgets::icon_btn(ui, &p, "arrow.up", 30.0, !self.draft.is_empty()).clicked() && can {
                        self.submit(sh, run);
                    }
                    ui.add_space(0.0);
                    if self.transcription.is_some() {
                        ascii::spinner(ui, 12.0, p.ink2);
                    } else {
                        let rec = self.recording.is_some();
                        let r = widgets::icon_btn(ui, &p, if rec { "mic.fill" } else { "mic" }, 26.0, false)
                            .on_hover_text(if rec { "Stop recording" } else { "Voice input" });
                        if rec {
                            ui.painter().circle_filled(r.rect.right_top() + Vec2::new(-4.0, 4.0), 2.5, p.danger);
                        }
                        if r.clicked() {
                            if let Some(recording) = self.recording.take() {
                                self.transcription = Some(sh.spawn(move |_| recording.transcribe()));
                            } else {
                                match crate::voice::Recording::start() {
                                    Ok(rc) => self.recording = Some(rc),
                                    Err(e) => self.error = Some(e),
                                }
                            }
                        }
                    }
                });
            });
        });
        let rc = inner.response.rect;
        let rr = Rounding::same(R_PANEL);
        let ring = if focused { theme::with_alpha(p.accent, 0.7) } else { p.line };
        ui.painter().set(
            slot,
            egui::Shape::Vec(vec![egui::Shape::rect_filled(rc, rr, p.raised), egui::Shape::rect_stroke(rc.shrink(0.5), rr, Stroke::new(1.0, ring))]),
        );
        if let Some(e) = &self.error {
            ui.add_space(6.0);
            ui.add(egui::Label::new(ty::sized(e, 11.0, W::Regular).color(p.danger)).wrap());
        }
        // slash popover above the composer
        if !items.is_empty() {
            let sel = self.slash_index.min(items.len() - 1);
            let mut pick: Option<usize> = None;
            egui::Area::new(egui::Id::new(("slash", &run.id)))
                .order(egui::Order::Foreground)
                .pivot(Align2::LEFT_BOTTOM)
                .fixed_pos(egui::pos2(rc.left(), rc.top() - 8.0))
                .show(ui.ctx(), |ui| {
                    widgets::floating_frame(&p, R_CARD).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
                        ui.set_width(360.0);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (i, (name, label)) in items.iter().enumerate() {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
                            if i == sel || resp.hovered() {
                                ui.painter().rect_filled(r, Rounding::same(R_CTRL - 2.0), p.hover);
                            }
                            let ng = ui.painter().layout_no_wrap(format!("/{name}"), mono(12.0, W::Medium), p.ink);
                            let lx = (r.left() + 96.0).max(r.left() + 8.0 + ng.size().x + 12.0);
                            ui.painter().galley(egui::pos2(r.left() + 8.0, r.center().y - ng.size().y / 2.0), ng, p.ink);
                            ui.painter().with_clip_rect(r.shrink2(Vec2::new(4.0, 0.0))).text(egui::pos2(lx, r.center().y), Align2::LEFT_CENTER, label, jb(11.5, W::Regular), p.ink3);
                            if resp.hovered() {
                                self.slash_index = i;
                            }
                            if resp.clicked() {
                                pick = Some(i);
                            }
                        }
                    });
                });
            if let Some(i) = pick {
                self.draft = format!("/{} ", items[i].0);
                ui.memory_mut(|m| m.request_focus(te_id));
            }
        }
    }

    fn model_menu(&mut self, ui: &mut Ui, sh: &mut Shared, run: &AgentRun) {
        let p = sh.palette;
        let harness = run.harness();
        let active = self.picked_model.clone().unwrap_or_else(|| run.model.clone());
        let opts = model_options(&harness, &active, &run.available_models);
        let label = model_label(&active, &opts);
        let busy = self.model_job.is_some();
        let galley = ui.painter().layout_no_wrap(label, jb(11.5, W::Regular), p.ink2);
        let size = Vec2::new(11.0 + 5.0 + if busy { 17.0 } else { 0.0 } + galley.size().x + 5.0 + 8.0 + 12.0, 26.0);
        let (rect, resp) = ui.allocate_exact_size(size, if busy { Sense::hover() } else { Sense::click() });
        if resp.hovered() {
            ui.painter().rect_filled(rect, Rounding::same(R_CTRL), p.hover);
        }
        let mut x = rect.left() + 6.0;
        let lr = Rect::from_center_size(egui::pos2(x + 5.5, rect.center().y), Vec2::splat(11.0));
        logos::paint_logo(ui, lr, if active.is_empty() { &harness } else { &active }, p.ink2);
        x += 16.0;
        if busy {
            let sr = Rect::from_center_size(egui::pos2(x + 6.0, rect.center().y), Vec2::splat(12.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(sr), |ui| ascii::spinner(ui, 12.0, p.ink2));
            x += 17.0;
        }
        let gw = galley.size().x;
        ui.painter().galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, p.ink2);
        icons::paint_symbol(ui.painter(), egui::pos2(x + gw + 10.0, rect.center().y), "chevron.down", 8.0, p.ink3);

        let id = resp.id.with("jb-menu");
        if resp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        let mut pick: Option<String> = None;
        let mut custom = false;
        egui::popup::popup_above_or_below_widget(ui, id, &resp, egui::AboveOrBelow::Above, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            widgets::floating_frame(&p, R_CARD).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
                ui.set_min_width(200.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let cur = active.to_lowercase();
                for m in &opts {
                    let is_cur = is_current_model(&cur, &m.id);
                    if widgets::menu_item(ui, &p, None, &m.label, is_cur, true).clicked() {
                        pick = Some(m.id.clone());
                        ui.memory_mut(|mm| mm.close_popup());
                    }
                }
                widgets::menu_separator(ui, &p);
                if widgets::menu_item(ui, &p, None, "Custom model\u{2026}", false, true).clicked() {
                    custom = true;
                    ui.memory_mut(|mm| mm.close_popup());
                }
            });
        });
        if let Some(m) = pick {
            self.switch_model(sh, run, m);
        }
        if custom {
            self.custom_text = active;
            self.custom_open = true;
        }
        if self.custom_open {
            let mut go = false;
            let mut cancel = false;
            let can_switch = !self.custom_text.trim().is_empty();
            let esc = widgets::dialog(
                ui.ctx(),
                "custom-model",
                "Switch model",
                None,
                380.0,
                |ui| {
                    if widgets::button_ex(ui, &p, BtnKind::Primary, true, can_switch, "Switch").clicked() {
                        go = true;
                    }
                    if widgets::button_ex(ui, &p, BtnKind::Ghost, true, true, "Cancel").clicked() {
                        cancel = true;
                    }
                },
                |ui| {
                    ui.label(ty::sized("Enter the model slug to pass to /model.", 12.0, W::Regular).color(p.ink2));
                    widgets::field(ui, &p, &mut self.custom_text, "Model id (e.g. provider/model)");
                },
            );
            if go {
                let m = self.custom_text.trim().to_string();
                self.custom_open = false;
                if !m.is_empty() {
                    self.switch_model(sh, run, m);
                }
            } else if cancel || esc {
                self.custom_open = false;
            }
        }
    }

    fn switch_model(&mut self, sh: &mut Shared, run: &AgentRun, model: String) {
        let id = run.id.clone();
        let harness = run.harness();
        self.model_job = Some(sh.spawn(move |a| {
            let r = a.coding_set_model(&id, &model, &harness);
            (model, r)
        }));
    }
}

/// Same bubble as your messages in a JoeBro chat (right-aligned), with "Queued" under it until delivered.
pub fn user_card(ui: &mut Ui, p: &theme::Palette, text: &str, tag: Option<&str>) {
    let w = ui.available_width();
    ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
        let slot = ui.painter().add(egui::Shape::Noop);
        let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(14.0, 10.0)).show(ui, |ui| {
            ui.set_max_width((w - 80.0 - 28.0).max(60.0));
            ui.add(egui::Label::new(RichText::new(text).font(jb(14.0, W::Regular)).color(p.ink)).selectable(true));
        });
        let rc = inner.response.rect;
        let r = Rounding::same(R_CARD);
        ui.painter().set(
            slot,
            egui::Shape::Vec(vec![egui::Shape::rect_filled(rc, r, p.raised), egui::Shape::rect_stroke(rc.shrink(0.5), r, Stroke::new(1.0, p.line))]),
        );
        if let Some(t) = tag {
            ui.add_space(2.0);
            let mut c = t.chars();
            let cap = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
            ui.label(ty::sized(cap, 11.0, W::Medium).color(p.ink2));
        }
    });
}

// ---- the chat-area view of one coding agent -----------------------------------------------

#[derive(Default)]
pub struct CodingAgentConversation {
    run: Option<AgentRun>,
    for_id: String,
    job: Option<Job<R<AgentRun>>>,
    timer: f64,
    stop_job: Option<Job<R<Value>>>,
    follow_job: Option<Job<R<AgentRun>>>,
    thread: AgentThread,
}

impl CodingAgentConversation {
    fn reload(&mut self) {
        self.timer = 0.0;
    }

    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared, agent_id: &str) {
        let p = sh.palette;
        if self.for_id != agent_id {
            *self = CodingAgentConversation { for_id: agent_id.to_string(), ..Default::default() };
        }
        // ---- polling (3 s while live, else 10 s)
        let dt = ui.input(|i| i.stable_dt) as f64;
        if let Some(j) = &mut self.job {
            if let Some(res) = j.take() {
                self.job = None;
                match res {
                    Ok(r) => {
                        if r.id != agent_id {
                            sh.agent_nav.open(&r.id);
                        }
                        self.run = Some(r);
                    }
                    Err(_) => self.follow_replacement(sh, agent_id),
                }
            }
        }
        if let Some(j) = &mut self.follow_job {
            if let Some(res) = j.take() {
                self.follow_job = None;
                if let Ok(r) = res {
                    sh.agent_nav.open(&r.id);
                    self.run = Some(r);
                }
            }
        }
        if let Some(j) = &mut self.stop_job {
            if j.take().is_some() {
                self.stop_job = None;
                self.reload();
            }
        }
        self.timer -= dt;
        if self.timer <= 0.0 && self.job.is_none() {
            self.timer = if self.run.as_ref().map_or(false, |r| r.live) { 3.0 } else { 10.0 };
            let target = self.run.as_ref().map(|r| r.id.clone()).unwrap_or_else(|| agent_id.to_string());
            self.job = Some(sh.spawn(move |a| a.agent_detail_typed(&target)));
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));

        // ---- layout: 46 pt chrome header, 12 pt gap, thread
        let rect = ui.max_rect();
        let hdr = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 46.0));
        ui.painter().extend(widgets::glass_surface(&p, hdr, R_PANEL, widgets::level_fill(&p, Level::Chrome), false));
        let mut stop = false;
        ui.allocate_new_ui(
            egui::UiBuilder::new().max_rect(hdr.shrink2(Vec2::new(12.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if widgets::icon_btn(ui, &p, "sidebar.left", 26.0, false).on_hover_text("Toggle sidebar").clicked() {
                    sh.sidebar_hidden = !sh.sidebar_hidden;
                }
                icons::symbol(ui, "chevron.left.forwardslash.chevron.right", 13.0, p.ink2);
                let title = self.run.as_ref().filter(|r| !r.title.is_empty()).map(|r| r.title.clone()).unwrap_or_else(|| "Coding agent".into());
                let maxw = (ui.available_width() - 200.0).max(60.0);
                ui.scope(|ui| {
                    ui.set_max_width(maxw);
                    ui.add(egui::Label::new(ty::sized(title, 14.0, W::Semibold).color(p.ink)).truncate());
                });
                if let Some(r) = &self.run {
                    status_chip(ui, &p, r);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(r) = &self.run {
                        if r.live {
                            let stopping = self.stop_job.is_some();
                            if widgets::button_ex(ui, &p, BtnKind::Secondary, false, !stopping, if stopping { "Stopping\u{2026}" } else { "Stop" }).clicked() {
                                stop = true;
                            }
                        }
                    }
                });
            },
        );
        if stop {
            if let Some(r) = &self.run {
                let id = r.id.clone();
                self.stop_job = Some(sh.spawn(move |a| a.agent_cancel(&id)));
            }
        }
        let body = Rect::from_min_max(egui::pos2(rect.left(), hdr.bottom() + 12.0), rect.max);
        ui.painter().extend(widgets::glass_surface(&p, body, R_PANEL, widgets::level_fill(&p, Level::Panel), false));
        match self.run.clone() {
            Some(run) => {
                let changed = ui
                    .allocate_new_ui(egui::UiBuilder::new().max_rect(body), |ui| {
                        ui.set_clip_rect(body.intersect(ui.clip_rect()));
                        self.thread.ui(ui, sh, &run, false)
                    })
                    .inner;
                if changed {
                    self.reload();
                }
            }
            None => {
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body), |ui| {
                    ui.centered_and_justified(|ui| {
                        ascii::spinner(ui, 18.0, p.ink2);
                    });
                });
            }
        }
        ui.advance_cursor_after_rect(rect);
    }

    /// An adopted pid* placeholder replaced by its real session id: follow it.
    fn follow_replacement(&mut self, sh: &mut Shared, agent_id: &str) {
        let target = self.run.as_ref().map(|r| r.id.clone()).unwrap_or_else(|| agent_id.to_string());
        let parts: Vec<&str> = target.split(':').collect();
        let want_host = match &self.run {
            Some(r) if !r.host.is_empty() => r.host.clone(),
            _ if parts.first() == Some(&"code") && parts.len() >= 2 => parts[1].to_string(),
            _ => String::new(),
        };
        let want_harness = match &self.run {
            Some(r) if !r.harness().is_empty() => r.harness(),
            _ if parts.first() == Some(&"code") && parts.len() >= 3 => parts[2].to_string(),
            _ => String::new(),
        };
        let cands: Vec<&AgentRun> = sh
            .agent_nav
            .runs
            .iter()
            .filter(|r| r.id != target && (want_harness.is_empty() || r.harness() == want_harness) && (want_host.is_empty() || r.host.is_empty() || r.host == want_host))
            .collect();
        let pick = |f: &dyn Fn(&&AgentRun) -> bool| {
            let m: Vec<&&AgentRun> = cands.iter().filter(|r| f(r)).collect();
            if m.len() == 1 { Some(m[0].id.clone()) } else { None }
        };
        let rep = match &self.run {
            Some(r) if !r.cwd.is_empty() => {
                let c = r.cwd.clone();
                pick(&|x: &&AgentRun| x.cwd == c)
            }
            Some(r) if !r.title.is_empty() => {
                let t = r.title.clone();
                pick(&|x: &&AgentRun| x.title == t)
            }
            _ => None,
        };
        if let Some(id) = rep {
            self.follow_job = Some(sh.spawn(move |a| a.agent_detail_typed(&id)));
        }
        sh.agent_nav.timer = 0.0;
    }
}

// ---- "New coding agent" sheet ---------------------------------------------------------------------

pub struct NewCodingAgentSheet {
    pub open: bool,
    hosts: Vec<crate::models::NightHost>,
    sessions: Vec<ResumableSession>,
    load: Option<Job<(Vec<crate::models::NightHost>, Vec<ResumableSession>)>>,
    loaded: bool,
    host: String,
    repo: String,
    query: String,
    agent: String,
    permission: String,
    prompt: String,
    busy: Option<Job<R<Value>>>,
    error: Option<String>,
}

impl Default for NewCodingAgentSheet {
    fn default() -> Self {
        NewCodingAgentSheet {
            open: false,
            hosts: vec![],
            sessions: vec![],
            load: None,
            loaded: false,
            host: String::new(),
            repo: String::new(),
            query: String::new(),
            agent: "claude".into(),
            permission: "edits".into(),
            prompt: String::new(),
            busy: None,
            error: None,
        }
    }
}

fn fetch_sessions(a: &crate::api::Api) -> (Vec<crate::models::NightHost>, Vec<ResumableSession>) {
    let hosts = a.night_hosts();
    let mut sessions = a.resumable_sessions(50).unwrap_or_default();
    // Coding sessions JoeBro watched (hook-fed) are resumable too, by their folder.
    if let Ok(v) = a.agents_with_coding("coding", "") {
        let known: std::collections::HashSet<String> = sessions.iter().map(|s| s.resume_id.clone()).collect();
        for x in crate::models::arr(&v, "agents") {
            let g = |k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let (id, cwd, sid) = (g("id"), g("cwd"), g("session_id"));
            if id.is_empty() || cwd.is_empty() || !g("parent_id").is_empty() || sid.is_empty() || known.contains(&sid) {
                continue;
            }
            let at = if g("started_at").is_empty() { g("created_at") } else { g("started_at") };
            sessions.push(ResumableSession { id: id.clone(), repo: cwd, host: g("host"), agent: g("source"), summary: g("title"), branch: String::new(), at, resume_id: id });
        }
        sessions.sort_by(|a, b| b.at.cmp(&a.at));
    }
    (hosts, sessions)
}

fn host_label(h: &crate::models::NightHost) -> String {
    if h.up { h.label() } else { format!("{} (offline)", h.label()) }
}

/// A `select` field: raised trigger with chevron, `menu_popup` of options. Returns the picked value.
pub fn select(ui: &mut Ui, p: &theme::Palette, id: &str, width: f32, title: &str, options: &[(String, String)], current: &str) -> Option<String> {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 32.0), Sense::click());
    let focused = ui.memory(|m| m.is_popup_open(resp.id.with("jb-menu")));
    ui.painter().rect_filled(rect, Rounding::same(R_CTRL), p.raised);
    ui.painter().rect_stroke(rect.shrink(0.5), Rounding::same(R_CTRL), Stroke::new(1.0, if focused { theme::with_alpha(p.accent, 0.7) } else { p.line }));
    let galley = ui.painter().layout(title.to_string(), jb(13.0, W::Regular), p.ink, (width - 36.0).max(10.0));
    ui.painter().galley(egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0), galley, p.ink);
    icons::paint_symbol(ui.painter(), egui::pos2(rect.right() - 14.0, rect.center().y), "chevron.down", 9.0, p.ink3);
    let _ = id;
    let mut pick = None;
    widgets::menu_popup(ui, p, &resp, |ui| {
        ui.set_min_width(width - 12.0);
        for (v, l) in options {
            if widgets::menu_item(ui, p, None, l, v == current, true).clicked() {
                pick = Some(v.clone());
                ui.memory_mut(|m| m.close_popup());
            }
        }
    });
    pick
}

fn labelled(ui: &mut Ui, p: &theme::Palette, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(ty::sized(title, 12.0, W::Semibold).color(p.ink2));
        add(ui);
    });
}

fn last_component(p: &str) -> String {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or(p).to_string()
}

impl NewCodingAgentSheet {
    fn folders(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        self.sessions
            .iter()
            .filter(|s| s.host == self.host && !s.repo.is_empty() && !s.repo.starts_with("/tmp/") && !s.repo.contains("/scratchpad"))
            .map(|s| s.repo.clone())
            .filter(|r| seen.insert(r.clone()))
            .collect()
    }

    fn start(&mut self, sh: &mut Shared, resume: Option<String>) {
        let mut body = serde_json::Map::new();
        body.insert("repo".into(), json!(self.repo.trim()));
        body.insert("host".into(), json!(self.host));
        body.insert("agent".into(), json!(self.agent));
        body.insert("permission".into(), json!(self.permission));
        if resume.is_none() && !self.prompt.trim().is_empty() {
            body.insert("prompt".into(), json!(self.prompt));
        }
        body.insert("resume".into(), match resume {
            Some(r) => json!(r),
            None => json!(false),
        });
        self.error = None;
        self.busy = Some(sh.spawn(move |a| a.coding_start(Value::Object(body))));
    }

    /// Call every frame; opens when `sh.new_coding_agent_requested` is set.
    pub fn ui(&mut self, ctx: &egui::Context, sh: &mut Shared) {
        if std::mem::take(&mut sh.new_coding_agent_requested) {
            self.open = true;
            self.loaded = false;
            self.error = None;
            self.prompt.clear();
            self.query.clear();
        }
        if !self.open {
            return;
        }
        let p = sh.palette;
        if !self.loaded && self.load.is_none() {
            self.load = Some(sh.spawn(fetch_sessions));
        }
        if let Some(j) = &mut self.load {
            if let Some((hosts, sessions)) = j.take() {
                self.load = None;
                self.loaded = true;
                self.hosts = hosts;
                self.sessions = sessions;
                // Default to the computer you used last.
                let hs = &self.hosts;
                self.host = self
                    .sessions
                    .iter()
                    .find(|s| hs.iter().any(|h| h.id == s.host))
                    .map(|s| s.host.clone())
                    .or_else(|| hs.iter().find(|h| h.label() == "Mac").map(|h| h.id.clone()))
                    .or_else(|| hs.first().map(|h| h.id.clone()))
                    .unwrap_or_default();
                self.repo = self.folders().first().cloned().unwrap_or_default();
            }
        }
        if let Some(j) = &mut self.busy {
            if let Some(res) = j.take() {
                self.busy = None;
                match res {
                    Ok(v) if v.get("ok").and_then(Value::as_bool) == Some(false) => {
                        self.error = Some(v.get("detail").and_then(Value::as_str).unwrap_or("Couldn't start").to_string());
                    }
                    Ok(_) => {
                        sh.agent_nav.timer = 0.0;
                        self.open = false;
                        return;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
        // scrim
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("new-agent-scrim")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(90));
        });
        let folders = self.folders();
        let folder = self.repo.trim().to_string();
        let q = self.query.trim().to_lowercase();
        let matches: Vec<String> = folders.iter().filter(|f| q.is_empty() || f.to_lowercase().contains(&q)).take(6).cloned().collect();
        let typed_path = {
            let t = self.query.trim();
            if (t.starts_with('/') || t.starts_with('~')) && !folders.iter().any(|f| f == t) { Some(t.to_string()) } else { None }
        };
        let norm = |s: &str| if s.len() > 1 && s.ends_with('/') { s[..s.len() - 1].to_string() } else { s.to_string() };
        let resumable: Vec<ResumableSession> = self
            .sessions
            .iter()
            .filter(|s| s.host == self.host && norm(&s.repo) == norm(&folder) && !s.resume_id.is_empty())
            .take(3)
            .cloned()
            .collect();
        let busy = self.busy.is_some();
        let mut close = false;
        let mut start: Option<Option<String>> = None;
        let mut new_host: Option<String> = None;
        let mut set_repo: Option<String> = None;

        egui::Window::new("new-coding-agent")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(widgets::floating_frame(&p, R_PANEL).inner_margin(egui::Margin::same(24.0)))
            .fixed_size(Vec2::new(540.0, 0.0))
            .show(ctx, |ui| {
                ui.set_width(540.0);
                ui.spacing_mut().item_spacing.y = 16.0;
                ui.label(ty::sized("New coding agent", 17.0, W::Semibold).extra_letter_spacing(-0.2).color(p.ink));
                let multi = self.hosts.len() > 1;
                let n = if multi { 3.0 } else { 2.0 };
                let cw = (540.0 - 12.0 * (n - 1.0)) / n;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if multi {
                        labelled(ui, &p, "Device", |ui| {
                            let opts: Vec<(String, String)> = self.hosts.iter().map(|h| (h.id.clone(), host_label(h))).collect();
                            let cur = self.hosts.iter().find(|h| h.id == self.host).map(host_label).unwrap_or_else(|| "Choose".into());
                            if let Some(v) = select(ui, &p, "device", cw, &cur, &opts, &self.host) {
                                new_host = Some(v);
                            }
                        });
                    }
                    labelled(ui, &p, "Agent", |ui| {
                        let opts: Vec<(String, String)> = [("claude", "Claude Code"), ("codex", "Codex"), ("opencode", "OpenCode"), ("agy", "Antigravity")]
                            .iter()
                            .map(|(a, b)| (a.to_string(), b.to_string()))
                            .collect();
                        let cur = opts.iter().find(|o| o.0 == self.agent).map(|o| o.1.clone()).unwrap_or_else(|| self.agent.clone());
                        if let Some(v) = select(ui, &p, "agent", cw, &cur, &opts, &self.agent) {
                            self.agent = v;
                        }
                    });
                    labelled(ui, &p, "Permissions", |ui| {
                        let opts: Vec<(String, String)> = [("ask", "Ask first"), ("edits", "Accept edits"), ("full", "Full access")]
                            .iter()
                            .map(|(a, b)| (a.to_string(), b.to_string()))
                            .collect();
                        let cur = opts.iter().find(|o| o.0 == self.permission).map(|o| o.1.clone()).unwrap_or_else(|| self.permission.clone());
                        if let Some(v) = select(ui, &p, "perm", cw, &cur, &opts, &self.permission) {
                            self.permission = v;
                        }
                    });
                });

                labelled(ui, &p, "Folder", |ui| {
                    let slot = ui.painter().add(egui::Shape::Noop);
                    let inner = ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.allocate_ui_with_layout(Vec2::new(540.0, 34.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add_space(10.0);
                            ui.spacing_mut().item_spacing.x = 8.0;
                            icons::symbol(ui, "magnifyingglass", 12.0, p.ink3);
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut self.query)
                                    .hint_text(RichText::new("Search recent folders or paste a path").color(p.ink3))
                                    .frame(false)
                                    .desired_width(540.0 - 50.0)
                                    .font(jb(13.0, W::Regular)),
                            );
                            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                                if let Some(t) = &typed_path {
                                    set_repo = Some(t.clone());
                                } else if let Some(f) = matches.first() {
                                    set_repo = Some(f.clone());
                                }
                            }
                        });
                        let (lr, _) = ui.allocate_exact_size(Vec2::new(540.0, 1.0), Sense::hover());
                        ui.painter().rect_filled(lr, 0.0, p.line);
                        egui::Frame::none().inner_margin(egui::Margin::same(4.0)).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.set_width(532.0);
                            let mut row = |ui: &mut Ui, f: &str, title: Option<&str>| {
                                let sel = self.repo == f;
                                let (r, resp) = ui.allocate_exact_size(Vec2::new(532.0, 30.0), Sense::click());
                                let hov = ui.ctx().animate_bool_with_time(resp.id.with("fr"), resp.hovered() && !sel, theme::FADE);
                                let shapes = widgets::hover_row_shapes(&p, r, R_CTRL, sel, hov);
                                ui.painter().extend(shapes);
                                icons::paint_symbol(ui.painter(), egui::pos2(r.left() + 17.0, r.center().y), if sel { "folder.fill" } else { "folder" }, 13.0, if sel { p.accent } else { p.ink3 });
                                let t = title.map(String::from).unwrap_or_else(|| last_component(f));
                                let tg = ui.painter().layout_no_wrap(t, jb(13.0, W::Medium), p.ink);
                                let tw = tg.size().x;
                                ui.painter().galley(egui::pos2(r.left() + 36.0, r.center().y - tg.size().y / 2.0), tg, p.ink);
                                let avail = (r.right() - 30.0 - (r.left() + 36.0 + tw + 10.0)).max(20.0);
                                let pg = ui.painter().layout(f.to_string(), jb(11.5, W::Regular), p.ink3, f32::INFINITY);
                                let shown = if pg.size().x > avail { format!("\u{2026}{}", f.chars().rev().take((avail / 6.0) as usize).collect::<String>().chars().rev().collect::<String>()) } else { f.to_string() };
                                let pg = ui.painter().layout_no_wrap(shown, jb(11.5, W::Regular), p.ink3);
                                ui.painter().galley(egui::pos2(r.left() + 36.0 + tw + 10.0, r.center().y - pg.size().y / 2.0), pg, p.ink3);
                                if sel {
                                    icons::paint_symbol(ui.painter(), egui::pos2(r.right() - 16.0, r.center().y), "checkmark", 11.0, p.accent);
                                }
                                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    set_repo = Some(f.to_string());
                                }
                            };
                            if let Some(t) = &typed_path {
                                row(ui, t, Some("Use this path"));
                            }
                            for f in &matches {
                                row(ui, f, None);
                            }
                            if matches.is_empty() && typed_path.is_none() {
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(6.0);
                                    ui.label(ty::sized(
                                        if folders.is_empty() { "No recent folders on this device. Paste a path." } else { "No match. Paste a full path." },
                                        12.0,
                                        W::Regular,
                                    ).color(p.ink3));
                                });
                                ui.add_space(6.0);
                            }
                        });
                    });
                    let rc = inner.response.rect;
                    let rr = Rounding::same(R_CARD);
                    ui.painter().set(slot, egui::Shape::Vec(vec![egui::Shape::rect_filled(rc, rr, p.raised), egui::Shape::rect_stroke(rc.shrink(0.5), rr, Stroke::new(1.0, p.line))]));
                });

                labelled(ui, &p, "First message", |ui| {
                    widgets::field_frame(&p, false).show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.prompt)
                                .hint_text(RichText::new("Optional").color(p.ink3))
                                .frame(false)
                                .desired_rows(1)
                                .desired_width(540.0 - 20.0)
                                .font(jb(13.0, W::Regular)),
                        );
                    });
                });

                if !resumable.is_empty() {
                    labelled(ui, &p, "Or resume a session here", |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for s in &resumable {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(540.0, 30.0), if busy { Sense::hover() } else { Sense::click() });
                            let hov = ui.ctx().animate_bool_with_time(resp.id.with("rs"), resp.hovered(), theme::FADE);
                            ui.painter().extend(widgets::hover_row_shapes(&p, r, R_CTRL, false, hov));
                            icons::paint_symbol(ui.painter(), egui::pos2(r.left() + 16.0, r.center().y), "arrow.uturn.forward", 11.0, p.ink3);
                            let rel = crate::models::relative_iso(&s.at);
                            let rg = ui.painter().layout_no_wrap(rel, jb(11.5, W::Regular), p.ink3);
                            let rw = rg.size().x;
                            ui.painter().galley(egui::pos2(r.right() - 8.0 - rw, r.center().y - rg.size().y / 2.0), rg, p.ink3);
                            let txt = if s.summary.is_empty() { "Session".to_string() } else { s.summary.clone() };
                            let tg = ui.painter().layout(txt, jb(13.0, W::Regular), p.ink, r.width() - 52.0 - rw);
                            let clipped = ui.painter().with_clip_rect(Rect::from_min_max(r.min + Vec2::new(34.0, 0.0), egui::pos2(r.right() - 16.0 - rw, r.bottom())));
                            clipped.galley(egui::pos2(r.left() + 34.0, r.center().y - 8.0), tg, p.ink);
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !busy {
                                start = Some(Some(s.resume_id.clone()));
                            }
                        }
                    });
                }

                if let Some(e) = &self.error {
                    ui.add(egui::Label::new(ty::sized(e, 12.0, W::Regular).color(p.danger)).wrap());
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if busy {
                        ascii::spinner(ui, 12.0, p.ink2);
                        ui.label(ty::sized("Starting\u{2026}", 12.0, W::Regular).color(p.ink2));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let ok = !busy && !self.host.is_empty() && !folder.is_empty();
                        if widgets::button_ex(ui, &p, BtnKind::Primary, false, ok, "Start").clicked() {
                            start = Some(None);
                        }
                        if widgets::button(ui, &p, BtnKind::Ghost, "Cancel").clicked() {
                            close = true;
                        }
                    });
                });
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
        if let Some(h) = new_host {
            self.host = h;
            self.repo = self.folders().first().cloned().unwrap_or_default();
            self.query.clear();
        }
        if let Some(r) = set_repo {
            self.repo = r;
        }
        if let Some(s) = start {
            self.start(sh, s);
        }
        if close {
            self.open = false;
        }
    }
}

#[cfg(test)]
mod model_tick_tests {
    use super::is_current_model;
    #[test]
    fn only_exact_model_is_ticked() {
        assert!(is_current_model("claude-opus-5-5", "claude-opus-5-5"));
        assert!(!is_current_model("claude-opus-5-5", "opus"));
        assert!(is_current_model("Opus", "opus"));
        assert!(!is_current_model("", "opus"));
    }
}
