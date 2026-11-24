//! Settings — a like-for-like translation of the Mac `SettingsView`:
//! four tabs (General, Models, Appearance, Message Bot), each a stack of
//! grouped sections (header, rounded card of rows, caption help text).
//!
//! General:    Privacy, Email, Calendar, Server, Documents, Updates
//! Models:     Defaults, Subagents, Endpoints & API Keys
//! Appearance: Glass, Background, Wallpaper
//! Message Bot: intro, Telegram, Model, System prompt, Show in replies
//!
//! Backend-held settings are prefs (`PUT api/prefs/<key>`) exactly as on the
//! Mac; Server/Appearance live in the local config (`config.rs`).
use crate::api::R;
use crate::backdrop::{self, Backdrop};
use crate::config;
use crate::state::{Job, Model, Shared};
use crate::{theme, widgets};
use eframe::egui::{self, Align, Color32, Context, Layout, RichText, Stroke, Ui};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    General,
    Appearance,
    Models,
    Agents,
    Mobile,
    Integrations,
    Bot,
    Advanced,
}

impl Tab {
    const ALL: [Tab; 8] = [Tab::General, Tab::Appearance, Tab::Models, Tab::Agents, Tab::Mobile, Tab::Integrations, Tab::Bot, Tab::Advanced];
    fn title(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Appearance => "Appearance",
            Tab::Models => "Models & Keys",
            Tab::Agents => "Agents",
            Tab::Mobile => "Mobile",
            Tab::Integrations => "Integrations",
            Tab::Bot => "Message Bot",
            Tab::Advanced => "Advanced",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Tab::General => "switch.2",
            Tab::Appearance => "circle.lefthalf.filled",
            Tab::Models => "key",
            Tab::Agents => "terminal",
            Tab::Mobile => "iphone",
            Tab::Integrations => "link",
            Tab::Bot => "paperplane",
            Tab::Advanced => "wrench.adjustable",
        }
    }
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Sheet {
    #[default]
    None,
    Email,
    CalDav,
    Endpoint,
}

#[derive(Default)]
pub struct SettingsWindow {
    tab: Tab,
    loaded: bool,
    load: Option<Job<R<Value>>>,
    prefs: Value,
    error: Option<String>,
    /// Fire-and-forget pref writes (errors surface in the banner).
    actions: Vec<Job<R<Value>>>,

    // General
    cloak: bool,
    email_load: i64,
    email_info: Value,
    email_status: Option<Job<R<Value>>>,
    cal_info: Value,
    cal_status: Option<Job<R<Value>>>,
    integ_job: Option<Job<R<Value>>>,
    integration_error: Option<String>,
    conn_url: String,
    conn_token: String,
    conn_seeded: bool,
    bundled: crate::backend_ui::BundledPanel,
    conn_test: Option<Job<R<String>>>,
    conn_verdict: Option<R<String>>,
    doc_approval: bool,
    ask_cmds: bool,
    max_tools: i64,

    // Models
    endpoints: Vec<Value>,
    ep_load: Option<Job<R<Value>>>,
    ep_models: HashMap<String, Vec<(String, String)>>,
    ep_jobs: HashMap<String, Job<R<Value>>>,
    hidden: HashMap<String, HashSet<String>>,
    sub_auto: bool,

    // Sheets
    sheet: Sheet,
    sheet_job: Option<Job<R<Value>>>,
    sheet_err: Option<String>,
    imap_mode: bool,
    em_email: String,
    em_pw: String,
    imap_host: String,
    imap_port: String,
    smtp_host: String,
    smtp_port: String,
    cd_url: String,
    cd_principal: String,
    cd_pw: String,
    ep_name: String,
    ep_url: String,
    ep_key: String,

    // Appearance

    backdrop_note: Option<String>,
    backdrops_scanned: bool,
    user_backdrops: Vec<std::path::PathBuf>,
    mobile: crate::panels::mobile_settings::MobileSettingsTab,
    bot: crate::panels::message_bot::MessageBotTab,
}

// ---- layout helpers (macOS grouped-Form look, JB tokens) ----------------------

/// Placeholder text, dimmer than typed text.
pub fn hint(text: &str) -> RichText {
    RichText::new(text).color(Color32::from_gray(128).gamma_multiply(0.8))
}

fn first_id() -> egui::Id {
    egui::Id::new("settings_form_first")
}

/// Hairline between rows of a section (not before the first one).
pub(crate) fn item_sep(ui: &mut Ui, p: theme::Palette) {
    let first: bool = ui.data(|d| d.get_temp(first_id())).unwrap_or(true);
    if first {
        ui.data_mut(|d| d.insert_temp(first_id(), false));
        ui.add_space(8.0);
        return;
    }
    ui.add_space(8.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 0.0, p.line);
    ui.add_space(8.0);
}

/// A caption row (`.jbCaption`, ink3).
pub(crate) fn help(ui: &mut Ui, p: theme::Palette, text: &str) {
    item_sep(ui, p);
    ui.add(egui::Label::new(theme::ty::caption(text).color(p.ink3)).wrap());
}

/// A caption row in a given colour.
pub(crate) fn note(ui: &mut Ui, p: theme::Palette, text: &str, col: Color32) {
    item_sep(ui, p);
    ui.add(egui::Label::new(theme::ty::caption(text).color(col)).wrap());
}

/// One grouped section: small header above, flat card holding the rows.
pub(crate) fn section(ui: &mut Ui, p: theme::Palette, title: Option<&str>, add: impl FnOnce(&mut Ui)) {
    if let Some(t) = title {
        ui.add_space(2.0);
        ui.label(theme::ty::sized(t, 12.0, theme::W::Semibold).color(p.ink2));
        ui.add_space(6.0);
    }
    ui.data_mut(|d| d.insert_temp(first_id(), true));
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(14.0, 6.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        add(ui);
        ui.add_space(8.0);
    });
    let r = inner.response.rect;
    ui.painter().set(
        slot,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(r, theme::R_CARD, p.raised),
            egui::Shape::rect_stroke(r, theme::R_CARD, Stroke::new(1.0, p.line)),
        ]),
    );
    ui.add_space(18.0);
}

/// Label on the left, control(s) on the right.
pub(crate) fn row(ui: &mut Ui, p: theme::Palette, label: &str, add: impl FnOnce(&mut Ui)) {
    item_sep(ui, p);
    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        ui.label(theme::ty::body(label).color(p.ink));
        ui.with_layout(Layout::right_to_left(Align::Center), add);
    });
}

/// Toggle row: label left, switch right.
pub(crate) fn toggle(ui: &mut Ui, p: theme::Palette, label: &str, on: &mut bool) -> bool {
    let mut changed = false;
        row(ui, p, label, |ui| changed = widgets::jb_switch(ui, &p, on).changed());
    changed
}

/// Plain content row (no label).
pub(crate) fn item(ui: &mut Ui, p: theme::Palette, add: impl FnOnce(&mut Ui)) {
    item_sep(ui, p);
    add(ui);
}

pub(crate) fn capsule(ui: &mut Ui, p: theme::Palette, text: &str) {
        egui::Frame::none()
        .fill(p.hover)
        .rounding(egui::Rounding::same(20.0))
        .inner_margin(egui::Margin::symmetric(6.0, 1.0))
        .show(ui, |ui| ui.label(theme::ty::mono(text, 10.0, theme::W::Regular).color(p.ink2)));
}

pub(crate) fn btn(ui: &mut Ui, p: theme::Palette, text: &str) -> bool {
    widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, true, text).clicked()
}

pub(crate) fn danger_btn(ui: &mut Ui, p: theme::Palette, text: &str) -> bool {
    widgets::button_ex(ui, &p, widgets::BtnKind::Danger, true, true, text).clicked()
}

/// Model dropdown with a leading "none" row, endpoint-grouped like the Mac's
/// `ModelPickerItems`. `Some(None)` = none chosen, `Some(Some(m))` = model.
pub(crate) fn model_menu(
    ui: &mut Ui,
    sh: &Shared,
    id: &str,
    none_label: &str,
    cur_model: &str,
    cur_ep: &str,
) -> Option<Option<Model>> {
    let cur = sh
        .models
        .iter()
        .find(|m| !cur_model.is_empty() && m.model_id == cur_model && (cur_ep.is_empty() || m.endpoint_id == cur_ep))
        .or_else(|| sh.models.iter().find(|m| !cur_model.is_empty() && m.model_id == cur_model));
    let text = cur.map(|m| m.display_with_tag()).unwrap_or_else(|| none_label.to_string());
    let mut out = None;
    egui::ComboBox::from_id_salt(id).width(230.0).selected_text(text).show_ui(ui, |ui| {
        if ui.selectable_label(cur.is_none(), none_label).clicked() {
            out = Some(None);
        }
        for (endpoint, models) in sh.grouped_models() {
            ui.label(theme::ty::mono(endpoint.to_uppercase(), 10.5, theme::W::Regular).color(sh.palette.ink3));
            for m in &models {
                let sel = cur.map(|c| c.id == m.id).unwrap_or(false);
                if ui.selectable_label(sel, m.display_with_tag()).clicked() {
                    out = Some(Some(m.clone()));
                }
            }
        }
    });
    out
}

pub(crate) fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
pub(crate) fn b(v: &Value, k: &str, d: bool) -> bool {
    match v.get(k) {
        Some(Value::Bool(x)) => *x,
        Some(Value::String(x)) => x.eq_ignore_ascii_case("true"),
        _ => d,
    }
}
pub(crate) fn arr(v: &Value, key: &str) -> Vec<Value> {
    v.as_array()
        .cloned()
        .or_else(|| v.get(key).and_then(Value::as_array).cloned())
        .unwrap_or_default()
}
/// A backend reply that failed in-band (`{"ok":false,"detail":..}`).
pub(crate) fn failure(v: &Value) -> Option<String> {
    (v.get("ok").and_then(Value::as_bool) == Some(false))
        .then(|| s(v, "detail").to_string())
        .map(|d| if d.is_empty() { "The backend rejected that.".into() } else { d })
}

impl SettingsWindow {
    /// Write a pref to the backend and mirror it into the shared copy.
    fn pref(&mut self, sh: &mut Shared, key: &str, val: Value) {
        if !sh.prefs.is_object() {
            sh.prefs = json!({});
        }
        sh.prefs[key] = val.clone();
        let k = key.to_string();
        self.actions.push(sh.spawn(move |a| a.set_pref(&k, val)));
    }

    fn poll(&mut self, sh: &mut Shared) {
        if !self.loaded && self.load.is_none() {
            self.load = Some(sh.spawn(|a| a.prefs()));
            self.email_status = Some(sh.spawn(|a| a.email_status()));
            self.cal_status = Some(sh.spawn(|a| a.calendar_status()));
            self.ep_load = Some(sh.spawn(|a| a.model_endpoints()));
        }
        if !self.conn_seeded {
            self.conn_seeded = true;
            self.conn_url = config::server_url();
            self.conn_token = config::load_token().unwrap_or_default();
        }
        if let Some(job) = &mut self.load {
            if let Some(res) = job.take() {
                self.load = None;
                self.loaded = true;
                match res {
                    Err(e) => self.error = Some(format!("Couldn't load settings: {e}")),
                    Ok(v) => {
                        self.cloak = b(&v, "cloak_mode", true);
                        self.email_load = v.get("email_load_count").and_then(Value::as_i64).unwrap_or(50);
                        self.doc_approval = b(&v, "require_doc_approval", true);
                        self.ask_cmds = b(&v, "ask_before_commands", false);
                        self.max_tools = v.get("max_tool_calls").and_then(Value::as_i64).unwrap_or(0).max(0);
                        self.sub_auto = b(&v, "subagent_model_auto", false);
                        self.prefs = v;
                    }
                }
            }
        }
        macro_rules! land {
            ($job:expr, $on_ok:expr, $label:literal) => {
                if let Some(j) = &mut $job {
                    if let Some(res) = j.take() {
                        match res {
                            Ok(v) => $on_ok(v),
                            Err(e) => self.error = Some(format!("{}: {e}", $label)),
                        }
                        $job = None;
                    }
                }
            };
        }
        land!(self.email_status, |v| self.email_info = v, "Email status");
        land!(self.cal_status, |v| self.cal_info = v, "Calendar status");
        land!(self.ep_load, |v: Value| self.endpoints = arr(&v, "endpoints"), "Model endpoints");
        if let Some(j) = &mut self.integ_job {
            if let Some(res) = j.take() {
                self.integ_job = None;
                match res {
                    Ok(v) => self.integration_error = failure(&v),
                    Err(e) => self.integration_error = Some(e),
                }
                self.email_status = Some(sh.spawn(|a| a.email_status()));
                self.cal_status = Some(sh.spawn(|a| a.calendar_status()));
            }
        }
        if let Some(j) = &mut self.conn_test {
            if let Some(res) = j.take() {
                self.conn_verdict = Some(res);
                self.conn_test = None;
            }
        }
        let mut finished = vec![];
        for (i, a) in self.actions.iter_mut().enumerate() {
            if let Some(res) = a.take() {
                finished.push(i);
                match res {
                    Err(e) => self.error = Some(e),
                    Ok(v) => {
                        if let Some(d) = failure(&v) {
                            self.error = Some(d)
                        }
                    }
                }
            }
        }
        for i in finished.into_iter().rev() {
            self.actions.remove(i);
        }
        // Per-endpoint model lists land lazily (fetched on first expand).
        let ids: Vec<String> = self.ep_jobs.keys().cloned().collect();
        for id in ids {
            let done = self.ep_jobs.get_mut(&id).and_then(|j| j.take());
            if let Some(res) = done {
                self.ep_jobs.remove(&id);
                let list = res.map(|v| arr(&v, "models")).unwrap_or_default();
                let mut hid = HashSet::new();
                let mut models = vec![];
                for m in &list {
                    let mid = if s(m, "id").is_empty() { s(m, "display") } else { s(m, "id") };
                    if mid.is_empty() {
                        continue;
                    }
                    if b(m, "is_hidden", false) {
                        hid.insert(mid.to_string());
                    }
                    let disp = if s(m, "display").is_empty() { mid } else { s(m, "display") };
                    models.push((mid.to_string(), disp.to_string()));
                }
                self.hidden.insert(id.clone(), hid);
                self.ep_models.insert(id, models);
            }
        }
        if self.user_backdrops.is_empty() && !self.backdrops_scanned {
            self.backdrops_scanned = true;
            self.user_backdrops = backdrop::user_backdrops();
        }
        if let Some(j) = &mut self.sheet_job {
            if let Some(res) = j.take() {
                self.sheet_job = None;
                match res {
                    Err(e) => self.sheet_err = Some(e),
                    Ok(v) => match failure(&v) {
                        Some(d) => self.sheet_err = Some(d),
                        None => {
                            let was = self.sheet;
                            self.sheet = Sheet::None;
                            self.sheet_err = None;
                            self.em_pw.clear();
                            self.cd_pw.clear();
                            self.ep_key.clear();
                            match was {
                                Sheet::Endpoint => {
                                    self.ep_load = Some(sh.spawn(|a| a.model_endpoints()));
                                    sh.reload_models = true;
                                }
                                _ => {
                                    self.email_status = Some(sh.spawn(|a| a.email_status()));
                                    self.cal_status = Some(sh.spawn(|a| a.calendar_status()));
                                    sh.unread_email = 0;
                                }
                            }
                        }
                    },
                }
            }
        }
    }

    #[cfg(feature = "snapshot")]
    pub fn demo_tab(&mut self, n: u8) {
        self.tab = Tab::ALL[n as usize % 8];
    }
    #[cfg(feature = "snapshot")]
    pub fn demo_email_sheet(&mut self) {
        self.sheet = Sheet::Email;
        self.imap_mode = true;
    }

    pub fn ui(&mut self, ctx: &Context, sh: &mut Shared, open: &mut bool, bd: &mut Backdrop) {
        self.poll(sh);
        let p = sh.palette;
        let screen = ctx.screen_rect();
        // 780 x 600 like the Mac window, shrunk to fit small screens.
        let h = (screen.height() - 60.0).clamp(240.0, 600.0);
        let w = (screen.width() - 32.0).clamp(420.0, 780.0);
        let side_w = 196.0_f32.min(w * 0.4);
        egui::Window::new(RichText::new(self.tab.title()).font(theme::jb(13.0, theme::W::Semibold)))
            .id(egui::Id::new("settings_window"))
            .open(open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .fixed_size([w, h])
            .frame(widgets::floating_frame(&p, theme::R_PANEL).inner_margin(egui::Margin::ZERO))
            .show(ctx, |ui| {
                let full = ui.available_rect_before_wrap();
                let side = egui::Rect::from_min_size(full.min, egui::vec2(side_w, full.height()));
                let main = egui::Rect::from_min_max(egui::pos2(side.right() + 1.0, full.top()), full.max);
                ui.painter().rect_filled(side, egui::Rounding { sw: theme::R_PANEL, ..Default::default() }, p.surface);
                ui.painter().rect_filled(main, egui::Rounding { se: theme::R_PANEL, ..Default::default() }, p.ground);
                ui.painter().vline(side.right() + 0.5, full.y_range(), Stroke::new(1.0, p.line));
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(side.shrink2(egui::vec2(10.0, 0.0)).translate(egui::vec2(0.0, 14.0))), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for t in Tab::ALL {
                        let on = self.tab == t;
                        let resp = widgets::hover_row(ui, &p, on, theme::R_CTRL, |ui| {
                            ui.set_height(30.0);
                            ui.set_width(ui.available_width());
                            ui.horizontal_centered(|ui| {
                                ui.add_space(10.0);
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                                crate::icons::paint_symbol(ui.painter(), r.center(), t.icon(), 13.0, if on { p.ink } else { p.ink3 });
                                let w = if on { theme::W::Medium } else { theme::W::Regular };
                                ui.label(theme::ty::sized(t.title(), 13.0, w).color(if on { p.ink } else { p.ink2 }));
                            });
                        });
                        if resp.clicked() {
                            self.tab = t;
                        }
                    }
                });
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(main), |ui| {
                    ui.set_clip_rect(main.intersect(ui.clip_rect()));
                    if let Some(e) = self.error.clone() {
                        ui.horizontal(|ui| {
                            ui.add_space(20.0);
                            ui.add(egui::Label::new(theme::ty::caption(&e).color(p.danger)).wrap());
                            if btn(ui, sh.palette, "Dismiss") {
                                self.error = None;
                            }
                        });
                    }
                    egui::ScrollArea::vertical().id_salt("settings_scroll").auto_shrink([false, false]).show(ui, |ui| {
                        egui::Frame::none().inner_margin(egui::Margin::symmetric(20.0, 18.0)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            match self.tab {
                                Tab::General => self.general(ui, sh),
                                Tab::Appearance => self.appearance(ui, sh, bd),
                                Tab::Models => self.models(ui, sh),
                                Tab::Agents => self.agents(ui, sh),
                                Tab::Mobile => self.mobile.ui(ui, sh),
                                Tab::Integrations => self.integrations(ui, sh),
                                Tab::Bot => self.bot.ui(ui, sh),
                                Tab::Advanced => self.advanced(ui, sh),
                            }
                        });
                    });
                });
            });
        if self.sheet != Sheet::None {
            self.sheet_ui(ctx, sh);
        }
        if !*open {
            self.sheet = Sheet::None;
        }
    }
}

// ---- General / Agents / Integrations / Advanced ---------------------------------

impl SettingsWindow {
    fn general(&mut self, ui: &mut Ui, sh: &mut Shared) {
        section(ui, sh.palette, Some("Privacy"), |ui| {
            if toggle(ui, sh.palette, "Cloak mode", &mut self.cloak) {
                let v = self.cloak;
                self.pref(sh, "cloak_mode", json!(v));
            }
            help(ui, sh.palette, "Scrubs secrets and personal data before anything reaches a model or search engine.");
        });
        // Sparkle is macOS-only: here only the version row remains.
        section(ui, sh.palette, Some("Updates"), |ui| {
            row(ui, sh.palette, "Version", |ui| {
                let flavour = if config::PERSONAL { " (personal)" } else { "" };
                ui.label(theme::ty::sized(format!("{}{flavour}", config::VERSION), 11.0, theme::W::Regular).color(sh.palette.ink3));
            });
        });
    }

    fn agents(&mut self, ui: &mut Ui, sh: &mut Shared) {
        section(ui, sh.palette, Some("Documents"), |ui| {
            if toggle(ui, sh.palette, "Require approval for AI edits", &mut self.doc_approval) {
                let v = self.doc_approval;
                sh.require_doc_approval = v;
                self.pref(sh, "require_doc_approval", json!(v));
            }
        });
        section(ui, sh.palette, Some("Tools"), |ui| {
            let mut unlimited = self.max_tools == 0;
            if toggle(ui, sh.palette, "Unlimited tool use", &mut unlimited) {
                self.max_tools = if unlimited { 0 } else { 10 };
                let v = self.max_tools;
                self.pref(sh, "max_tool_calls", json!(v));
            }
            if self.max_tools > 0 {
                row(ui, sh.palette, "Tool-use limit per turn", |ui| {
                    let mut n = self.max_tools;
                    if ui.add(egui::DragValue::new(&mut n).range(1..=9999).speed(0.2)).changed() {
                        self.max_tools = n.max(1);
                        let v = self.max_tools;
                        self.pref(sh, "max_tool_calls", json!(v));
                    }
                });
            }
            if toggle(ui, sh.palette, "Ask before running commands", &mut self.ask_cmds) {
                let v = self.ask_cmds;
                self.pref(sh, "ask_before_commands", json!(v));
            }
        });
    }

    fn integrations(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let p = sh.palette;
        section(ui, p, Some("Email"), |ui| {
            let sources = self.email_info.get("sources").and_then(Value::as_array).cloned().unwrap_or_default();
            if sources.is_empty() {
                row(ui, sh.palette, "Status", |ui| {
                    ui.label(theme::ty::body("Not connected").color(p.ink2));
                });
            }
            for src in &sources {
                let id = s(src, "id").to_string();
                let name = if s(src, "display").is_empty() { "Mailbox" } else { s(src, "display") };
                item(ui, sh.palette, |ui| {
                    ui.horizontal(|ui| {
                        crate::icons::symbol(ui, "envelope", 13.0, p.ink2);
                        ui.label(theme::ty::body(name).color(p.ink));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if danger_btn(ui, sh.palette, "Remove") {
                                self.integ_job = Some(sh.spawn(move |a| a.remove_email_source(&id)));
                            }
                        });
                    });
                });
            }
            item(ui, sh.palette, |ui| {
                if btn(ui, sh.palette, "Add email account\u{2026}") {
                    self.sheet = Sheet::Email;
                    self.sheet_err = None;
                }
            });
            row(ui, sh.palette, "Emails to load", |ui| {
                let label = if self.email_load == 0 { "All".to_string() } else { self.email_load.to_string() };
                egui::ComboBox::from_id_salt("email_load").selected_text(label).show_ui(ui, |ui| {
                    for (n, name) in [(0, "All"), (50, "50"), (100, "100"), (200, "200"), (500, "500")] {
                        if ui.selectable_label(self.email_load == n, name).clicked() && self.email_load != n {
                            self.email_load = n;
                            self.pref(sh, "email_load_count", json!(n));
                        }
                    }
                });
            });
        });

        section(ui, p, Some("Calendar"), |ui| {
            let configured = self.cal_info.get("configured").and_then(Value::as_bool).unwrap_or(false);
            let status = if !configured {
                "Not connected".to_string()
            } else {
                match s(&self.cal_info, "type") {
                    "caldav" if !s(&self.cal_info, "display").is_empty() => format!("CalDAV: {}", s(&self.cal_info, "display")),
                    "caldav" => "CalDAV".into(),
                    _ if !s(&self.cal_info, "display").is_empty() => s(&self.cal_info, "display").to_string(),
                    _ => "Connected".into(),
                }
            };
            row(ui, sh.palette, "Status", |ui| {
                ui.label(theme::ty::body(status).color(p.ink2));
            });
            item(ui, sh.palette, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if btn(ui, sh.palette, "Connect CalDAV\u{2026}") {
                        self.sheet = Sheet::CalDav;
                        self.sheet_err = None;
                    }
                    if configured && danger_btn(ui, sh.palette, "Disconnect") {
                        self.integ_job = Some(sh.spawn(|a| a.disconnect_calendar()));
                    }
                });
            });
        });
        if let Some(e) = self.integration_error.clone() {
            section(ui, p, None, |ui| note(ui, sh.palette, &e, p.danger));
        }
    }

    fn advanced(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let p = sh.palette;
        section(ui, p, Some("Server"), |ui| {
            item(ui, sh.palette, |ui| {
                widgets::field(ui, &p, &mut self.conn_url, "Server URL");
            });
            item(ui, sh.palette, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.conn_token)
                        .password(true)
                        .font(theme::mono(12.0, theme::W::Regular))
                        .hint_text(hint("Bearer token (remote backend only)"))
                        .desired_width(f32::INFINITY),
                );
                let _ = r;
            });
            let url = config::normalize_url(&self.conn_url);
            let changed = url != config::server_url() || Some(self.conn_token.trim()) != config::load_token().as_deref().or(Some(""));
            item(ui, sh.palette, |ui| {
                ui.horizontal(|ui| {
                    let can_test = !url.is_empty() && self.conn_test.is_none();
                    if widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, can_test, "Test connection").clicked() {
                        let t = self.conn_token.clone();
                        let u = url.clone();
                        self.conn_verdict = None;
                        self.conn_test = Some(sh.spawn(move |_| crate::api::Api::test_connection(&u, &t)));
                    }
                    if widgets::button_ex(ui, &p, widgets::BtnKind::Primary, true, !url.is_empty() && changed, "Save & reconnect").clicked() {
                        sh.pending_connection = Some((url.clone(), self.conn_token.trim().to_string()));
                        self.loaded = false;
                        self.load = None;
                        self.conn_verdict = None;
                    }
                    if self.conn_test.is_some() {
                        crate::ascii::spinner(ui, 12.0, p.ink2);
                    }
                });
            });
            match self.conn_verdict.clone() {
                Some(Ok(m)) => note(ui, sh.palette, &m, p.live),
                Some(Err(m)) => note(ui, sh.palette, &m, p.danger),
                None => {}
            }
            if !config::PERSONAL {
                item(ui, sh.palette, |ui| self.bundled.ui(ui, sh));
            }
        });
    }
}

// ---- Models -------------------------------------------------------------------

impl SettingsWindow {
    fn models(&mut self, ui: &mut Ui, sh: &mut Shared) {
        section(ui, sh.palette, Some("Defaults"), |ui| {
            row(ui, sh.palette, "Default model", |ui| {
                if widgets::icon_button(ui, theme::icon::REFRESH).on_hover_text("Refresh models").clicked() {
                    sh.reload_models = true;
                }
                let (dm, de) = (s(&self.prefs, "default_model").to_string(), s(&self.prefs, "default_endpoint_id").to_string());
                if let Some(pick) = model_menu(ui, sh, "default_model", "None", &dm, &de) {
                    let (m, e) = pick.map(|m| (m.model_id, m.endpoint_id)).unwrap_or_default();
                    self.pref(sh, "default_model", json!(m));
                    self.pref(sh, "default_endpoint_id", json!(e));
                    self.prefs["default_model"] = json!(m);
                    self.prefs["default_endpoint_id"] = json!(e);
                }
            });
            help(ui, sh.palette, "Used for compaction, calendar quick-add and other background AI jobs.");
        });

        section(ui, sh.palette, Some("Subagents"), |ui| {
            row(ui, sh.palette, "Subagent model", |ui| {
                ui.add_enabled_ui(!self.sub_auto, |ui| {
                    let (sm, se) = (s(&self.prefs, "subagent_model").to_string(), s(&self.prefs, "subagent_endpoint_id").to_string());
                    if let Some(pick) = model_menu(ui, sh, "subagent_model", "Same as default", &sm, &se) {
                        let (m, e) = pick.map(|m| (m.model_id, m.endpoint_id)).unwrap_or_default();
                        self.pref(sh, "subagent_model", json!(m));
                        self.pref(sh, "subagent_endpoint_id", json!(e));
                        self.prefs["subagent_model"] = json!(m);
                        self.prefs["subagent_endpoint_id"] = json!(e);
                    }
                });
            });
            if toggle(ui, sh.palette, "Let the main agent choose the model per task", &mut self.sub_auto) {
                let v = if self.sub_auto { "true" } else { "false" };
                self.pref(sh, "subagent_model_auto", json!(v));
            }
            help(ui, sh.palette, "Subagents are helpers the main agent spawns for side tasks. When the switch is on, the agent picks a model for each one and the fixed model above is not used.");
        });

        // Mac's "Apple Intelligence on-device" row is macOS-only and is omitted.
        section(ui, sh.palette, Some("Endpoints & API Keys"), |ui| {
            let eps = self.endpoints.clone();
            let mut delete: Option<String> = None;
            for ep in &eps {
                let id = s(ep, "id").to_string();
                let name = if s(ep, "name").is_empty() { id.clone() } else { s(ep, "name").to_string() };
                let cat = s(ep, "category").to_string();
                let cid = ui.make_persistent_id(("settings_ep", &id));
                let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), cid, false);
                if state.is_open() && !self.ep_models.contains_key(&id) && !self.ep_jobs.contains_key(&id) {
                    let ep_id = id.clone();
                    self.ep_jobs.insert(id.clone(), sh.spawn(move |a| a.endpoint_models(&ep_id)));
                }
                state
                    .show_header(ui, |ui| {
                        ui.label(RichText::new(&name).size(13.0).strong());
                        if !cat.is_empty() {
                            capsule(ui, sh.palette, &cat);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::icon_button(ui, "\u{f1f8}").on_hover_text("Delete endpoint").clicked() {
                                delete = Some(id.clone());
                            }
                        });
                    })
                    .body(|ui| match self.ep_models.get(&id).cloned() {
                        None => {
                            ui.add(egui::Spinner::new().size(14.0));
                        }
                        Some(models) if models.is_empty() => {
                            ui.label(RichText::new("No models discovered").size(11.5).color(sh.palette.ink_dim));
                        }
                        Some(models) => {
                            let mut hset = self.hidden.get(&id).cloned().unwrap_or_default();
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                if ui.link("Select all").clicked() {
                                    hset.clear();
                                    changed = true;
                                }
                                if ui.link("Deselect all").clicked() {
                                    hset = models.iter().map(|m| m.0.clone()).collect();
                                    changed = true;
                                }
                            });
                            for (mid, disp) in &models {
                                let mut visible = !hset.contains(mid);
                                if ui.checkbox(&mut visible, RichText::new(disp).size(12.0)).changed() {
                                    if visible {
                                        hset.remove(mid);
                                    } else {
                                        hset.insert(mid.clone());
                                    }
                                    changed = true;
                                }
                            }
                            if changed {
                                self.hidden.insert(id.clone(), hset.clone());
                                let (ep_id, hidden) = (id.clone(), hset.into_iter().collect::<Vec<String>>());
                                self.actions.push(sh.spawn(move |a| a.set_hidden_models(&ep_id, &hidden)));
                                sh.reload_models = true;
                            }
                        }
                    });
            }
            if let Some(id) = delete {
                self.actions.push(sh.spawn(move |a| a.delete_endpoint(&id)));
                self.ep_load = Some(sh.spawn(|a| a.model_endpoints()));
                sh.reload_models = true;
            }
            if btn(ui, sh.palette, "Add Endpoint\u{2026}") {
                self.sheet = Sheet::Endpoint;
                self.sheet_err = None;
            }
        });
    }
}

// ---- Appearance ---------------------------------------------------------------

impl SettingsWindow {
    fn appearance(&mut self, ui: &mut Ui, sh: &mut Shared, bd: &mut Backdrop) {
        let p = sh.palette;
        let mut restyle = false;
        section(ui, p, Some("Palette"), |ui| {
            item(ui, p, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for pal in theme::JbPalette::ALL {
                        let sel = theme::palette() == pal;
                        if widgets::palette_swatch(ui, &p, pal, 18.0, sel).on_hover_text(pal.label()).clicked() {
                            theme::set_palette(pal);
                            restyle = true;
                        }
                    }
                });
            });
        });
        section(ui, p, Some("Panels"), |ui| {
            item(ui, p, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::ty::body("Opacity").color(p.ink));
                    let mut sol = theme::solidity();
                    ui.spacing_mut().slider_width = (ui.available_width() - 56.0).max(60.0);
                    let r = ui.add(egui::Slider::new(&mut sol, 0.35..=1.0).show_value(false));
                    if r.changed() {
                        theme::set_solidity(sol);
                        restyle = true;
                    }
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        theme::save_solidity(sol);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(theme::ty::sized(format!("{}%", (theme::solidity() * 100.0).round() as i32), 11.0, theme::W::Regular).color(p.ink3));
                    });
                });
            });
        });
        if restyle {
            sh.palette = theme::Palette::for_state(sh.aero, sh.dark);
            theme::apply(&sh.ctx, sh.palette);
        }

        let p = sh.palette;
        section(ui, p, Some("Backdrop"), |ui| {
            let kind = backdrop::kind();
            let mut sel = ["palette", "photo", "scene"].iter().position(|k| *k == kind).unwrap_or(0);
            item(ui, p, |ui| {
                if widgets::segmented_n(ui, sh, "backdrop_kind", &["Palette", "Photo", "Animated"], &mut sel) {
                    config::update(|c| c.backdrop = ["palette", "photo", "scene"][sel].to_string());
                }
            });
            if kind == "photo" {
                let has = config::get().wallpaper.is_some();
                if let (true, Some((tex, size))) = (has, bd.wallpaper()) {
                    item(ui, p, |ui| {
                        let target = egui::vec2(ui.available_width(), 110.0);
                        ui.add(
                            egui::Image::from_texture(egui::load::SizedTexture::new(tex.id(), size))
                                .uv(backdrop::fill_uv(size, target))
                                .rounding(theme::R_CARD)
                                .maintain_aspect_ratio(false)
                                .fit_to_exact_size(target),
                        );
                    });
                }
                item(ui, p, |ui| {
                    ui.horizontal(|ui| {
                        let label = if has { "Change image\u{2026}" } else { "Choose image\u{2026}" };
                        if widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, true, label).clicked() {
                            sh.picker.open("bg_photo", "Choose a picture", crate::pickers::PickMode::File(backdrop::IMAGE_EXT), dirs::picture_dir());
                        }
                        if has && danger_btn(ui, p, "Remove") {
                            backdrop::clear_wallpaper();
                        }
                    });
                });
                if let Some(e) = &bd.error {
                    note(ui, p, e, p.danger);
                }
            }
            if kind == "scene" {
                self.user_backdrops = backdrop::user_backdrops();
                let cur = config::get().scene;
                let name_of = |id: &str| backdrop::display_name(id);
                row(ui, p, "Scene", |ui| {
                    egui::ComboBox::from_id_salt("backdrop_scene")
                        .selected_text(if cur.is_empty() { "None".to_string() } else { name_of(&cur) })
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(cur.is_empty(), "None").clicked() {
                                config::update(|c| c.scene.clear());
                            }
                            if !self.user_backdrops.is_empty() {
                                ui.label(theme::ty::mono("YOURS", 10.5, theme::W::Regular).color(p.ink3));
                                for u in &self.user_backdrops {
                                    let id = u.to_string_lossy().into_owned();
                                    if ui.selectable_label(cur == id, name_of(&id)).clicked() {
                                        backdrop::use_scene(&id);
                                    }
                                }
                            }
                            ui.label(theme::ty::mono("ASCII SCENES", 10.5, theme::W::Regular).color(p.ink3));
                            for n in backdrop::BUNDLED_SCENES {
                                if ui.selectable_label(cur == *n, name_of(n)).clicked() {
                                    backdrop::use_scene(n);
                                }
                            }
                        });
                });
                item(ui, p, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, true, "Add video\u{2026}").clicked() {
                            sh.picker.open("bg_video", "Choose a video", crate::pickers::PickMode::File(backdrop::VIDEO_EXT), dirs::video_dir());
                        }
                        if btn(ui, p, "Open backdrops folder") {
                            let d = backdrop::folder();
                            if let Err(e) = crate::platform::open_path(&d) {
                                sh.err(format!("Couldn't open the backdrops folder: {e}"));
                            }
                        }
                        if btn(ui, p, "Rescan") {
                            self.user_backdrops = backdrop::user_backdrops();
                        }
                    });
                });
                item(ui, p, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if btn(ui, p, "Install backdrop skill for agents") {
                            self.backdrop_note = Some(match backdrop::install_skill() {
                                Ok(d) => format!("Installed to {}", d.join(", ")),
                                Err(e) => e,
                            });
                        }
                        if let Some(n) = &self.backdrop_note {
                            ui.label(theme::ty::sized(n, 11.0, theme::W::Regular).color(p.ink3));
                        }
                    });
                });
            }
        });
    }
}

// ---- Sheets (Connect Email / CalDAV / Add Endpoint) ----------------------------

impl SettingsWindow {
    fn sheet_ui(&mut self, ctx: &Context, sh: &mut Shared) {
        let (title, action) = match self.sheet {
            Sheet::Email => ("Connect Email", "Connect"),
            Sheet::CalDav => ("Connect CalDAV", "Connect"),
            _ => ("Add Endpoint", "Add"),
        };
        let can = match self.sheet {
            Sheet::Email if self.imap_mode => {
                !self.em_email.trim().is_empty()
                    && !self.em_pw.is_empty()
                    && !self.imap_host.trim().is_empty()
                    && self.imap_port.trim().parse::<u16>().is_ok()
                    && !self.smtp_host.trim().is_empty()
                    && self.smtp_port.trim().parse::<u16>().is_ok()
            }
            Sheet::Email => !self.em_email.trim().is_empty() && !self.em_pw.is_empty(),
            Sheet::CalDav => !self.cd_url.trim().is_empty() && !self.cd_principal.trim().is_empty(),
            _ => !self.ep_url.trim().is_empty(),
        };
        let busy = self.sheet_job.is_some();
        let mut close = false;
        let mut go = false;
        let screen = ctx.screen_rect();
        egui::Window::new(RichText::new(title).font(egui::FontId::new(14.0, theme::display_font())))
            .id(egui::Id::new("settings_sheet"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width((screen.width() - 40.0).clamp(280.0, 440.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, true]).max_height((screen.height() - 140.0).max(120.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match self.sheet {
                        Sheet::Email => {
                            ui.horizontal(|ui| {
                                if widgets::chip(ui, "Email + Password", !self.imap_mode, sh).clicked() {
                                    self.imap_mode = false;
                                }
                                if widgets::chip(ui, "Manual IMAP", self.imap_mode, sh).clicked() {
                                    self.imap_mode = true;
                                    if self.imap_port.is_empty() {
                                        self.imap_port = "993".into();
                                        self.smtp_port = "465".into();
                                    }
                                }
                            });
                            ui.add_space(6.0);
                            ui.add(egui::TextEdit::singleline(&mut self.em_email).hint_text(hint("Email address")).desired_width(f32::INFINITY));
                            ui.add(egui::TextEdit::singleline(&mut self.em_pw).password(true).hint_text(hint("Bridge password")).desired_width(f32::INFINITY));
                            help(ui, sh.palette, "This is the bridge password printed by `hydroxide auth <your email>` in Terminal \u{2014} NOT your ProtonMail password.");
                            if self.imap_mode {
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut self.imap_host).font(egui::TextStyle::Monospace).hint_text(hint("IMAP host")).desired_width(250.0));
                                    ui.add(egui::TextEdit::singleline(&mut self.imap_port).hint_text(hint("Port")).desired_width(60.0));
                                });
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut self.smtp_host).font(egui::TextStyle::Monospace).hint_text(hint("SMTP host")).desired_width(250.0));
                                    ui.add(egui::TextEdit::singleline(&mut self.smtp_port).hint_text(hint("Port")).desired_width(60.0));
                                });
                                help(ui, sh.palette, "Manual IMAP uses SSL by default. For Proton, use Email + Password and JoeBro will use the local bridge under the hood.");
                            }
                        }
                        Sheet::CalDav => {
                            ui.add(egui::TextEdit::singleline(&mut self.cd_url).font(egui::TextStyle::Monospace).hint_text(hint("CalDAV URL")).desired_width(f32::INFINITY));
                            ui.add(egui::TextEdit::singleline(&mut self.cd_principal).hint_text(hint("Principal")).desired_width(f32::INFINITY));
                            ui.add(egui::TextEdit::singleline(&mut self.cd_pw).password(true).hint_text(hint("Password")).desired_width(f32::INFINITY));
                            help(ui, sh.palette, "Calendar data stays on your JoeBro server; this stores the CalDAV source for it.");
                        }
                        _ => {
                            ui.add(egui::TextEdit::singleline(&mut self.ep_name).hint_text(hint("Name (e.g. Groq, OpenRouter)")).desired_width(f32::INFINITY));
                            ui.add(egui::TextEdit::singleline(&mut self.ep_url).font(egui::TextStyle::Monospace).hint_text(hint("Base URL (https://api.../v1)")).desired_width(f32::INFINITY));
                            ui.add(egui::TextEdit::singleline(&mut self.ep_key).password(true).hint_text(hint("API key (optional for local)")).desired_width(f32::INFINITY));
                        }
                    }
                    if let Some(e) = self.sheet_err.clone() {
                        ui.add_space(6.0);
                        ui.add(egui::Label::new(RichText::new(&e).size(11.5).color(sh.palette.warning)).wrap());
                        if self.sheet == Sheet::Email
                            && e.to_lowercase().contains("hydroxide is not installed")
                            && btn(ui, sh.palette, "Install Hydroxide")
                        {
                            if let Err(e) = crate::platform::open_url("https://github.com/emersion/hydroxide#installation") {
                                sh.err(format!("Couldn't open the browser: {e}"));
                            }
                        }
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::primary(ui, sh, action, can && !busy).clicked() {
                        go = true;
                    }
                    if btn(ui, sh.palette, "Cancel") {
                        close = true;
                    }
                    if busy {
                        ui.spinner();
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if go {
            self.sheet_err = None;
            self.sheet_job = Some(match self.sheet {
                Sheet::Email if self.imap_mode => {
                    let (e, pw) = (self.em_email.trim().to_string(), self.em_pw.clone());
                    let (ih, ip, sh_, sp) = (
                        self.imap_host.trim().to_string(),
                        self.imap_port.trim().to_string(),
                        self.smtp_host.trim().to_string(),
                        self.smtp_port.trim().to_string(),
                    );
                    sh.spawn(move |a| {
                        a.connect_imap(&[
                            ("email", &e),
                            ("imap_host", &ih),
                            ("imap_port", &ip),
                            ("imap_user", &e),
                            ("imap_password", &pw),
                            ("smtp_host", &sh_),
                            ("smtp_port", &sp),
                            ("smtp_user", &e),
                            ("smtp_password", &pw),
                        ])
                    })
                }
                Sheet::Email => {
                    let (e, pw) = (self.em_email.trim().to_string(), self.em_pw.clone());
                    sh.spawn(move |a| a.connect_email(&e, &pw))
                }
                Sheet::CalDav => {
                    let (u, pr, pw) = (self.cd_url.trim().to_string(), self.cd_principal.trim().to_string(), self.cd_pw.clone());
                    sh.spawn(move |a| a.connect_caldav(&u, &pr, &pw))
                }
                _ => {
                    let (n, u, k) = (self.ep_name.trim().to_string(), self.ep_url.trim().to_string(), self.ep_key.clone());
                    sh.spawn(move |a| a.add_endpoint(&n, &u, &k))
                }
            });
        }
        if close && !busy {
            self.sheet = Sheet::None;
            self.sheet_err = None;
        }
    }
}
