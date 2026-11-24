//! Settings > Message Bot (embedded content, no panel chrome).
//! Mirrors: MessageBotTab.swift. Config is stored as backend prefs, read once and
//! written per field so the backend's poll loop picks changes up live.
use super::settings::{hint, model_menu, section, toggle, row};
use crate::api::R;
use crate::state::{Job, Shared};
use crate::theme;
use eframe::egui::{self, Ui};
use serde_json::{json, Value};

#[derive(Default)]
pub struct MessageBotTab {
    load: Option<Job<R<Value>>>,
    started: bool,
    loaded: bool,
    enabled: bool,
    token: String,
    ids: String,
    model: String,
    endpoint_id: String,
    prompt: String,
    ask: bool,
    skills: bool,
    memories: bool,
    tools: bool,
    plugins: bool,
    writes: Vec<Job<R<Value>>>,
}

fn sv(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}
fn bv(v: &Value, k: &str, d: bool) -> bool {
    match v.get(k) {
        Some(Value::Bool(x)) => *x,
        Some(Value::String(x)) => x.eq_ignore_ascii_case("true"),
        _ => d,
    }
}

impl MessageBotTab {
    fn save(&mut self, sh: &mut Shared, key: &str, val: Value) {
        if !self.loaded {
            return;
        }
        let k = key.to_string();
        self.writes.push(sh.spawn(move |a| a.set_pref(&k, val)));
    }

    fn poll(&mut self, sh: &mut Shared) {
        if !self.started {
            self.started = true;
            self.load = Some(sh.spawn(|a| a.prefs()));
            if sh.models.is_empty() {
                sh.reload_models = true;
            }
        }
        if let Some(j) = &mut self.load {
            if let Some(res) = j.take() {
                self.load = None;
                if let Ok(v) = res {
                    self.enabled = bv(&v, "bot_enabled", false);
                    self.token = sv(&v, "bot_token");
                    self.ids = sv(&v, "bot_allowed_ids");
                    self.model = sv(&v, "bot_model");
                    self.endpoint_id = sv(&v, "bot_endpoint_id");
                    self.prompt = sv(&v, "bot_system_prompt");
                    self.ask = bv(&v, "bot_ask_permissions", false);
                    self.skills = bv(&v, "bot_show_skills", false);
                    self.memories = bv(&v, "bot_show_memories", false);
                    self.tools = bv(&v, "bot_show_tools", true);
                    self.plugins = bv(&v, "bot_show_plugins", false);
                }
                self.loaded = true;
            }
        }
        self.writes.retain_mut(|w| w.take().is_none());
    }

    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        self.poll(sh);
        let p = sh.palette;
        section(ui, p, Some("Telegram"), |ui| {
            let mut e = self.enabled;
            if toggle(ui, p, "Enable Message Bot", &mut e) {
                self.enabled = e;
                self.save(sh, "bot_enabled", json!(e));
            }
            super::settings::item(ui, p, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.token)
                        .password(true)
                        .font(theme::mono(12.0, theme::W::Regular))
                        .hint_text(hint("Bot token"))
                        .desired_width(f32::INFINITY),
                );
                if r.lost_focus() {
                    let v = self.token.trim().to_string();
                    self.save(sh, "bot_token", json!(v));
                }
            });
            super::settings::item(ui, p, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.ids).hint_text(hint("Allowed Telegram IDs (comma-separated)")).desired_width(f32::INFINITY));
                if r.lost_focus() {
                    let v = self.ids.clone();
                    self.save(sh, "bot_allowed_ids", json!(v));
                }
            });
            super::settings::help(ui, p, "Token from @BotFather. Leave IDs blank to allow anyone.");
        });
        section(ui, p, Some("Model"), |ui| {
            row(ui, p, "Model", |ui| {
                let (m0, e0) = (self.model.clone(), self.endpoint_id.clone());
                if let Some(pick) = model_menu(ui, sh, "bot_model", "Default model", &m0, &e0) {
                    let (m, e) = pick.map(|m| (m.model_id, m.endpoint_id)).unwrap_or_default();
                    self.model = m.clone();
                    self.endpoint_id = e.clone();
                    self.save(sh, "bot_model", json!(m));
                    self.save(sh, "bot_endpoint_id", json!(e));
                }
            });
            let mut a = self.ask;
            if toggle(ui, p, "Ask before commands & edits", &mut a) {
                self.ask = a;
                self.save(sh, "bot_ask_permissions", json!(a));
            }
        });
        section(ui, p, Some("System prompt"), |ui| {
            super::settings::item(ui, p, |ui| {
                let r = ui.add(
                    egui::TextEdit::multiline(&mut self.prompt)
                        .font(theme::mono(12.0, theme::W::Regular))
                        .desired_rows(6)
                        .desired_width(f32::INFINITY),
                );
                if r.lost_focus() {
                    let v = self.prompt.clone();
                    self.save(sh, "bot_system_prompt", json!(v));
                }
            });
        });
        section(ui, p, Some("Show in replies"), |ui| {
            let rows = [
                ("Skills used", "bot_show_skills"),
                ("Memories used", "bot_show_memories"),
                ("Tool calls", "bot_show_tools"),
                ("Plugins used", "bot_show_plugins"),
            ];
            for (i, (label, key)) in rows.iter().enumerate() {
                let flag = match i {
                    0 => &mut self.skills,
                    1 => &mut self.memories,
                    2 => &mut self.tools,
                    _ => &mut self.plugins,
                };
                let mut v = *flag;
                if toggle(ui, p, label, &mut v) {
                    *flag = v;
                    self.save(sh, key, json!(v));
                }
            }
        });
    }
}
