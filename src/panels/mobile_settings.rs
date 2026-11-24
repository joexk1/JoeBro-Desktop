//! Settings > Mobile (embedded content, no panel chrome).
//! Mirrors: MobileSettingsTab.swift (the Personal flavour: the phone talks to the always-on
//! backend). State lives in the backend; the token is read fresh each load.
use super::settings::{btn, danger_btn, help, item, note, row, section, toggle};
use crate::api::R;
use crate::state::{Job, Shared};
use crate::{theme, widgets};
use eframe::egui::{self, Align, Layout, Ui};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Default)]
pub struct MobileSettingsTab {
    started: bool,
    loaded: bool,
    load: Option<Job<R<Value>>>,
    writes: Vec<Job<R<Value>>>,
    reload: bool,
    urls: Vec<(String, String)>,
    token: String,
    pair_uri: String,
    show_token: bool,
    confirm_rotate: bool,
    devices: Vec<crate::models::MobileDevice>,
    test: Option<Job<R<String>>>,
    test_result: Option<String>,
    message: Option<String>,
    qr: Option<(String, egui::TextureHandle)>,

    push_configured: bool,
    push_blocker: String,
    team_id: String,
    key_id: String,
    bundle_id: String,
    key_file: Option<PathBuf>,
    pick_key: Option<Job<Option<PathBuf>>>,
    editing_push: bool,

    agent_name: String,
    allow_full: bool,
    show_session: bool,
    location: bool,
    muse_learn: bool,
    heartbeat_on: bool,
    heartbeat_interval: i64,
    pulse_on: bool,
    pulse_interval: i64,
}

fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
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
fn iv(v: &Value, k: &str, d: i64) -> i64 {
    v.get(k).and_then(Value::as_i64).unwrap_or(d)
}

impl MobileSettingsTab {
    fn fetch(&mut self, sh: &mut Shared) {
        self.load = Some(sh.spawn(|a| {
            let mob = a.mobile_status().unwrap_or(Value::Null);
            let devs = a.mobile_devices().unwrap_or_default();
            let devs: Vec<Value> = devs
                .iter()
                .map(|d| json!({"install_id": d.install_id, "name": d.name, "enabled": d.enabled, "push": d.push, "last_seen": d.last_seen}))
                .collect();
            Ok(json!({
                "mobile": mob,
                "devices": devs,
                "push": a.push_status().unwrap_or(Value::Null),
                "prefs": a.prefs().unwrap_or(Value::Null),
                "heartbeat": a.heartbeat().unwrap_or(Value::Null),
                "pulse": a.pulse().unwrap_or(Value::Null),
            }))
        }));
    }

    fn write(&mut self, sh: &mut Shared, f: impl FnOnce(&crate::api::Api) -> R<Value> + Send + 'static) {
        if self.loaded {
            self.writes.push(sh.spawn(f));
        }
    }
    fn pref(&mut self, sh: &mut Shared, key: &str, v: Value) {
        let k = key.to_string();
        self.write(sh, move |a| a.set_pref(&k, v));
    }

    fn poll(&mut self, sh: &mut Shared) {
        if let Some(f) = sh.take_picked("apns_key") {
            self.key_file = Some(f);
        }
        if !self.started || self.reload {
            self.started = true;
            self.reload = false;
            self.loaded = false;
            self.fetch(sh);
        }
        if let Some(j) = &mut self.load {
            if let Some(res) = j.take() {
                self.load = None;
                if let Ok(v) = res {
                    let m = &v["mobile"];
                    self.token = sv(m, "token");
                    self.pair_uri = sv(m, "pair_uri");
                    self.urls = m
                        .get("urls")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|u| {
                                    let url = sv(u, "url");
                                    (!url.is_empty()).then(|| (if sv(u, "label").is_empty() { "URL".into() } else { sv(u, "label") }, url))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    if self.pair_uri.is_empty() && !self.token.is_empty() {
                        if let Some((_, u)) = self.urls.first() {
                            self.pair_uri = format!("joebro://pair?u={}&t={}", pct(u), pct(&self.token));
                        }
                    }
                    self.devices = v["devices"].as_array().map(|a| a.iter().map(crate::models::MobileDevice::from_value).collect()).unwrap_or_default();
                    let p = &v["push"];
                    self.push_configured = bv(p, "configured", false);
                    self.push_blocker = sv(p, "blocker");
                    if self.bundle_id.is_empty() {
                        self.bundle_id = sv(p, "bundle_id");
                    }
                    let pr = &v["prefs"];
                    self.agent_name = if pr.get("phone_agent_name").is_some() { sv(pr, "phone_agent_name") } else { "JoeBro".into() };
                    self.allow_full = bv(pr, "mobile_allow_full", false);
                    self.show_session = bv(pr, "show_phone_session", false);
                    self.location = bv(pr, "location_enabled", false);
                    self.muse_learn = bv(pr, "muse_auto_learn", true);
                    self.heartbeat_interval = iv(pr, "heartbeat_interval_minutes", 30);
                    self.heartbeat_on = bv(&v["heartbeat"], "enabled", false);
                    self.pulse_on = bv(&v["pulse"]["pulse"], "enabled", false);
                    self.pulse_interval = iv(&v["pulse"]["pulse"], "interval_minutes", 60);
                }
                self.loaded = true;
            }
        }
        let mut errs = vec![];
        let mut refresh = false;
        self.writes.retain_mut(|w| match w.take() {
            None => true,
            Some(r) => {
                match r {
                    Err(e) => errs.push(e),
                    Ok(v) => {
                        if v.get("ok").and_then(Value::as_bool) == Some(false) {
                            errs.push(sv(&v, "detail"));
                        }
                    }
                }
                refresh = true;
                false
            }
        });
        if let Some(e) = errs.pop() {
            self.message = Some(e);
        }
        if refresh && self.writes.is_empty() {
            self.reload = true;
        }
        if let Some(j) = &mut self.test {
            if let Some(r) = j.take() {
                self.test = None;
                self.test_result = Some(r.unwrap_or_else(|e| e));
            }
        }
        if let Some(j) = &mut self.pick_key {
            if let Some(f) = j.take() {
                self.pick_key = None;
                if f.is_some() {
                    self.key_file = f;
                }
            }
        }
    }

    fn qr_texture(&mut self, ctx: &egui::Context) -> Option<&egui::TextureHandle> {
        if self.pair_uri.is_empty() {
            return None;
        }
        if self.qr.as_ref().map(|(u, _)| u != &self.pair_uri).unwrap_or(true) {
            let code = qrcode::QrCode::with_error_correction_level(self.pair_uri.as_bytes(), qrcode::EcLevel::M).ok()?;
            let n = code.width();
            let q = 4; // quiet zone modules
            let scale = 8;
            let side = (n + 2 * q) * scale;
            let mut px = vec![egui::Color32::WHITE; side * side];
            let colors = code.to_colors();
            for y in 0..n {
                for x in 0..n {
                    if colors[y * n + x] == qrcode::Color::Dark {
                        for dy in 0..scale {
                            for dx in 0..scale {
                                px[((y + q) * scale + dy) * side + (x + q) * scale + dx] = egui::Color32::BLACK;
                            }
                        }
                    }
                }
            }
            let img = egui::ColorImage { size: [side, side], pixels: px };
            self.qr = Some((self.pair_uri.clone(), ctx.load_texture("pair-qr", img, egui::TextureOptions::NEAREST)));
        }
        self.qr.as_ref().map(|(_, t)| t)
    }

    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        self.poll(sh);
        let p = sh.palette;
        let mono = |s: &str| theme::ty::mono(s, 12.0, theme::W::Regular).color(p.ink);

        // Server (Personal: the phone talks to the Pi)
        section(ui, p, Some("Server"), |ui| {
            row(ui, p, "Phone connects to", |ui| {
                ui.label(theme::ty::body("JoeBro on your Pi").color(p.ink));
            });
            row(ui, p, "Address", |ui| {
                let a = self.urls.first().map(|u| u.1.clone()).unwrap_or_else(crate::config::server_url);
                ui.label(theme::ty::mono(a, 12.0, theme::W::Regular).color(p.ink2));
            });
            row(ui, p, "Status", |ui| {
                let (txt, col) = if !self.loaded {
                    ("Checking\u{2026}", p.ink3)
                } else if self.token.is_empty() {
                    ("Not reachable", p.ink3)
                } else {
                    ("Online", p.live)
                };
                ui.label(theme::ty::body(txt).color(p.ink2));
                widgets::status_dot(ui, col, false, 6.0);
            });
            if let Some(m) = self.message.clone() {
                note(ui, sh.palette, &m, p.danger);
            }
        });

        // Pair your iPhone
        section(ui, p, Some("Pair your iPhone"), |ui| {
            if let Some(tex) = self.qr_texture(ui.ctx()).map(|t| t.id()) {
                item(ui, p, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add(egui::Image::new(egui::load::SizedTexture::new(tex, egui::vec2(220.0, 220.0))));
                    });
                });
            }
            row(ui, p, "URL", |ui| {
                ui.label(mono(self.urls.first().map(|u| u.1.as_str()).unwrap_or("\u{2014}")));
            });
            for (label, url) in self.urls.iter().skip(1) {
                row(ui, p, label, |ui| {
                    ui.label(mono(url));
                });
            }
            row(ui, p, "Token", |ui| {
                if btn(ui, p, "Copy") {
                    ui.ctx().copy_text(self.token.clone());
                }
                if btn(ui, p, if self.show_token { "Hide" } else { "Show" }) {
                    self.show_token = !self.show_token;
                }
                let t = if self.show_token { self.token.clone() } else { "\u{2022}".repeat(16) };
                ui.label(mono(&t));
            });
            item(ui, p, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if btn(ui, p, "Copy link") {
                        ui.ctx().copy_text(self.pair_uri.clone());
                    }
                    if btn(ui, p, "Rotate token\u{2026}") {
                        self.confirm_rotate = true;
                    }
                    if btn(ui, p, "Test from this PC") {
                        if let Some((_, base)) = self.urls.first().cloned() {
                            let tok = self.token.clone();
                            self.test = Some(sh.spawn(move |_| {
                                let c = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(5)).build().map_err(|e| e.to_string())?;
                                Ok(match c.get(format!("{base}/api/ping")).bearer_auth(tok).send() {
                                    Ok(r) if r.status().as_u16() == 200 => "Reachable".to_string(),
                                    Ok(_) => "Rejected: the token did not match".to_string(),
                                    Err(_) => "Blocked: check your firewall".to_string(),
                                })
                            }));
                        }
                    }
                });
            });
            if self.confirm_rotate {
                item(ui, p, |ui| {
                    ui.label(theme::ty::body("Rotate the token?").color(p.ink));
                    ui.label(theme::ty::caption("Your phone will disconnect and need to scan again.").color(p.ink3));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if danger_btn(ui, p, "Rotate token") {
                            self.confirm_rotate = false;
                            self.write(sh, |a| a.rotate_mobile_token());
                        }
                        if btn(ui, p, "Cancel") {
                            self.confirm_rotate = false;
                        }
                    });
                });
            }
            if let Some(t) = self.test_result.clone() {
                help(ui, p, &t);
            }
        });

        // Connected phones
        section(ui, p, Some("Connected phones"), |ui| {
            if self.devices.is_empty() {
                help(ui, p, "No phones yet.");
            }
            let devs = self.devices.clone();
            for d in devs {
                item(ui, p, |ui| {
                    ui.horizontal(|ui| {
                        crate::icons::symbol(ui, "iphone", 13.0, p.ink2);
                        ui.vertical(|ui| {
                            ui.label(theme::ty::body(if d.name.is_empty() { "iPhone" } else { &d.name }).color(p.ink));
                            let seen = crate::models::relative_iso(&d.last_seen);
                            ui.label(theme::ty::caption(format!("{seen} \u{b7} push {}", if d.enabled { "on" } else { "off" })).color(p.ink3));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if danger_btn(ui, p, "Remove") {
                                let id = d.install_id.clone();
                                self.write(sh, move |a| a.remove_mobile_device(&id));
                            }
                        });
                    });
                });
            }
        });

        // Notifications
        section(ui, p, Some("Notifications"), |ui| {
            if self.push_configured && !self.editing_push {
                row(ui, p, "Push", |ui| {
                    ui.label(theme::ty::body("On").color(p.ink2));
                    widgets::status_dot(ui, p.live, false, 6.0);
                });
                item(ui, p, |ui| {
                    ui.horizontal(|ui| {
                        if btn(ui, p, "Send test notification") {
                            self.write(sh, |a| a.send_test_notification());
                            self.message = None;
                        }
                        if btn(ui, p, "Change key\u{2026}") {
                            self.editing_push = true;
                        }
                    });
                });
            } else {
                item(ui, p, |ui| {
                    ui.label(theme::ty::sized(if self.push_configured { "Push is set up." } else { "Push is not set up." }, 12.0, theme::W::Semibold).color(p.ink));
                });
                if !self.push_blocker.is_empty() {
                    note(ui, p, &self.push_blocker.clone(), p.warn);
                }
                help(ui, p, "1. In your Apple developer account, create an APNs Auth Key (.p8) and note its Key ID.\n2. Copy your Team ID from the Membership page.\n3. Enter the bundle ID of the iPhone app you built.\n4. Choose the .p8 file and save. It is stored only on the JoeBro server and used only to talk to Apple's push servers.");
                for (v, h) in [(&mut self.team_id, "Team ID"), (&mut self.key_id, "Key ID"), (&mut self.bundle_id, "Bundle ID")] {
                    item(ui, p, |ui| {
                        widgets::field(ui, &p, v, h);
                    });
                }
                item(ui, p, |ui| {
                    ui.horizontal(|ui| {
                        if widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, true, "Choose .p8 key\u{2026}").clicked() {
                            sh.picker.open("apns_key", "Choose .p8 key", crate::pickers::PickMode::File(&["p8"]), None);
                        }
                        let name = self.key_file.as_ref().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "No key chosen".into());
                        ui.label(theme::ty::caption(name).color(p.ink3));
                    });
                });
                item(ui, p, |ui| {
                    ui.horizontal(|ui| {
                        let can = self.key_file.is_some() && !self.team_id.is_empty() && !self.key_id.is_empty() && !self.bundle_id.is_empty();
                        if widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, can, "Save").clicked() {
                            if let Some(f) = self.key_file.take() {
                                let (t, k, b) = (self.team_id.clone(), self.key_id.clone(), self.bundle_id.clone());
                                self.editing_push = false;
                                self.write(sh, move |a| {
                                    let bytes = std::fs::read(&f).map_err(|e| e.to_string())?;
                                    let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "key.p8".into());
                                    a.save_apns(&name, bytes, &t, &k, &b)
                                });
                            }
                        }
                        if btn(ui, p, "Send test notification") {
                            self.write(sh, |a| a.send_test_notification());
                            self.message = None;
                        }
                        if danger_btn(ui, p, "Remove key") {
                            self.write(sh, |a| a.remove_apns_key());
                        }
                    });
                });
                help(ui, p, "Push is optional. Without it the phone still works, it just can't buzz you.");
            }
        });

        // Personal agent
        section(ui, p, Some("Personal agent (JoeBro on iPhone)"), |ui| {
            item(ui, p, |ui| {
                let r = widgets::field(ui, &p, &mut self.agent_name, "Agent name");
                if r.lost_focus() {
                    let v = self.agent_name.clone();
                    self.pref(sh, "phone_agent_name", json!(v));
                }
            });
            let mut v = self.allow_full;
            if toggle(ui, p, "Let the personal agent run commands", &mut v) {
                self.allow_full = v;
                self.pref(sh, "mobile_allow_full", json!(v));
            }
            let mut v = self.show_session;
            if toggle(ui, p, "Show the phone chat in my sidebar", &mut v) {
                self.show_session = v;
                self.pref(sh, "show_phone_session", json!(v));
            }
            let mut v = self.location;
            if toggle(ui, p, "Use my phone's location for weather", &mut v) {
                self.location = v;
                self.pref(sh, "location_enabled", json!(v));
            }
            help(ui, p, "Weather comes from open-meteo.com and place names from nominatim.openstreetmap.org, only while this is on.");
            let mut v = self.muse_learn;
            if toggle(ui, p, "Learn durable facts into phone memory", &mut v) {
                self.muse_learn = v;
                self.pref(sh, "muse_auto_learn", json!(v));
            }
            let mut v = self.heartbeat_on;
            if toggle(ui, p, "Background checks (heartbeat)", &mut v) {
                self.heartbeat_on = v;
                let act = if v { "resume" } else { "pause" };
                self.write(sh, move |a| a.set_heartbeat(act));
            }
            row(ui, p, &format!("Check every {} min", self.heartbeat_interval), |ui| {
                let mut n = self.heartbeat_interval;
                if ui.add(egui::DragValue::new(&mut n).range(5..=240).speed(0.5)).changed() {
                    self.heartbeat_interval = (n / 5 * 5).clamp(5, 240);
                    let v = self.heartbeat_interval;
                    self.pref(sh, "heartbeat_interval_minutes", json!(v));
                }
            });
            let mut v = self.pulse_on;
            if toggle(ui, p, "Pulse", &mut v) {
                self.pulse_on = v;
                self.write(sh, move |a| a.set_pulse(json!({"enabled": v})));
            }
            row(ui, p, &format!("Pulse every {} min", self.pulse_interval), |ui| {
                let mut n = self.pulse_interval;
                if ui.add(egui::DragValue::new(&mut n).range(15..=480).speed(0.5)).changed() {
                    self.pulse_interval = (n / 15 * 15).clamp(15, 480);
                    let v = self.pulse_interval;
                    self.write(sh, move |a| a.set_pulse(json!({"interval_minutes": v})));
                }
            });
            help(ui, p, "Anything the personal agent finds is posted into its chat. Reply there yes or no.");
        });
    }
}
