//! First-run onboarding (Production build). Like the Mac app, a first launch
//! with Python available needs no screen at all: the bundled backend starts and
//! the app opens (`first_run_auto`). This screen appears when Python is missing
//! or the backend fails to start, and from Settings → Server. Three ways in:
//!
//! * **Connect to your Mac app** - enter or discover the Mac's address, test.
//! * **Start from scratch** - check Python, launch the bundled local backend,
//!   then (optionally) connect calendar, email and a model API key.
//! * **Connect to an existing server** - URL + token.
//!
//! The screen is also reachable from Settings → Server ("Run setup again").
use crate::api::{Api, R};
use crate::backend::{self, PythonSearch};
use crate::panels::settings::hint;
use crate::state::{Job, Shared};
use crate::{theme, widgets};
use eframe::egui::{self, Context, RichText};
use serde_json::Value;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

/// What the user chose; the app persists it and connects.
pub struct Done {
    pub url: String,
    pub token: String,
    /// `mac`, `bundled` or `server` (stored as `Config::mode`).
    pub mode: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Choose,
    Mac,
    Scratch,
    Services,
    Server,
}

/// A calendar / email / model-key connection attempt on the new backend.
type Svc = (&'static str, Job<R<Value>>);

pub struct Setup {
    stage: Stage,
    pub url: String,
    pub token: String,
    test: Option<Job<R<String>>>,
    pub verdict: Option<R<String>>,
    // Mac discovery
    disc: Option<Receiver<Vec<(String, String)>>>,
    found: Vec<(String, String)>,
    disc_done: bool,
    // Start from scratch
    py: Option<Job<PythonSearch>>,
    py_state: Option<PythonSearch>,
    start: Option<Job<Result<u16, String>>>,
    start_err: Option<String>,
    started: Option<(String, String)>,
    // Optional services (forms are free-text; nothing is stored until Connect)
    cal_url: String,
    cal_user: String,
    cal_pw: String,
    em_addr: String,
    em_pw: String,
    imap_host: String,
    smtp_host: String,
    prov: usize,
    key_url: String,
    key: String,
    svc: Vec<Svc>,
    svc_msg: Vec<(&'static str, R<String>)>,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            stage: Stage::Choose,
            url: String::new(),
            token: String::new(),
            test: None,
            verdict: None,
            disc: None,
            found: vec![],
            disc_done: false,
            py: None,
            py_state: None,
            start: None,
            start_err: None,
            started: None,
            cal_url: String::new(),
            cal_user: String::new(),
            cal_pw: String::new(),
            em_addr: String::new(),
            em_pw: String::new(),
            imap_host: String::new(),
            smtp_host: String::new(),
            prov: 0,
            key_url: String::new(),
            key: String::new(),
            svc: vec![],
            svc_msg: vec![],
        }
    }
}

/// Mac-style first launch: nothing configured yet and Python is installed, so
/// choose the bundled backend straight away (persisted; the app starts it).
pub fn first_run_auto() -> bool {
    if crate::config::PERSONAL || crate::config::configured() || !crate::config::get().mode.is_empty() {
        return false;
    }
    if matches!(backend::find_python_ex(), PythonSearch::Found(_)) {
        crate::config::update(|c| {
            c.mode = "bundled".into();
            c.bundled_autostart = true;
        });
        return true;
    }
    false
}

const PROVIDERS: &[(&str, &str)] = &[
    ("OpenAI", "https://api.openai.com/v1"),
    ("Anthropic", "https://api.anthropic.com/v1"),
    ("OpenRouter", "https://openrouter.ai/api/v1"),
    ("Google Gemini", "https://generativelanguage.googleapis.com/v1beta/openai"),
    ("Other (OpenAI-compatible)", ""),
];

impl Setup {
    /// The "run on this computer" page, opened when Python is missing (re-checks
    /// it) or the backend could not start (`err` is shown).
    pub fn local_backend(err: Option<String>) -> Self {
        let mut s = Setup { stage: Stage::Scratch, start_err: err, ..Default::default() };
        s.py_state = None;
        s
    }

    /// Kick off a connection test (off the UI thread).
    pub fn start_test(&mut self, sh: &Shared) {
        let (u, t) = (crate::config::normalize_url(&self.url), self.token.clone());
        self.verdict = None;
        self.test = Some(sh.spawn(move |_| Api::test_connection(&u, &t)));
    }

    pub fn testing(&self) -> bool {
        self.test.is_some()
    }

    /// Snapshot harness: jump to a stage with plausible content.
    #[cfg(feature = "snapshot")]
    pub fn demo_stage(&mut self, n: u8) {
        match n {
            1 => {
                self.stage = Stage::Mac;
                self.disc_done = true;
                self.found = vec![("http://100.101.102.103:8765".into(), "MacBook (Tailscale)".into())];
                self.url = "http://100.101.102.103:8765".into();
                self.verdict = Some(Ok("Connected to a JoeBro Mac app backend v1.0.0.".into()));
            }
            2 => {
                self.stage = Stage::Scratch;
                self.py_state = Some(PythonSearch::Found(backend::PythonInfo {
                    cmd: "python3".into(),
                    pre_args: vec![],
                    version: "3.12.4".into(),
                }));
            }
            3 => {
                self.stage = Stage::Scratch;
                self.py_state = Some(PythonSearch::Missing);
            }
            4 => {
                self.stage = Stage::Services;
                self.started = Some(("http://127.0.0.1:8765".into(), "x".into()));
                self.svc_msg.push(("model", Ok("Connected.".into())));
            }
            5 => self.stage = Stage::Server,
            _ => {}
        }
    }

    /// Land finished background work.
    pub fn poll(&mut self) {
        if let Some(job) = &mut self.test {
            if let Some(res) = job.take() {
                self.verdict = Some(res);
                self.test = None;
            }
        }
        if let Some(rx) = &self.disc {
            if let Ok(list) = rx.try_recv() {
                self.found = list;
                self.disc_done = true;
                self.disc = None;
            }
        }
        if let Some(job) = &mut self.py {
            if let Some(res) = job.take() {
                self.py_state = Some(res);
                self.py = None;
            }
        }
        if let Some(job) = &mut self.start {
            if let Some(res) = job.take() {
                self.start = None;
                match res {
                    Ok(port) => match backend::read_token() {
                        Some(tok) => {
                            self.started = Some((backend::url_for(port), tok));
                            self.stage = Stage::Services;
                        }
                        None => self.start_err = Some("The backend started but did not create its token.".into()),
                    },
                    Err(m) => self.start_err = Some(m),
                }
            }
        }
        let mut i = 0;
        while i < self.svc.len() {
            if let Some(res) = self.svc[i].1.take() {
                let (name, _) = self.svc.remove(i);
                self.svc_msg.retain(|(n, _)| *n != name);
                self.svc_msg.push((name, res.map(|_| "Connected.".to_string())));
            } else {
                i += 1;
            }
        }
    }

    fn busy(&self, name: &str) -> bool {
        self.svc.iter().any(|(n, _)| *n == name)
    }

    fn start_discovery(&mut self) {
        let (tx, rx) = channel();
        self.disc = Some(rx);
        self.disc_done = false;
        self.found.clear();
        std::thread::spawn(move || {
            let _ = tx.send(discover());
        });
    }

    /// Draw the centred onboarding card. Returns the choice when the user
    /// confirms.
    pub fn ui(&mut self, ctx: &Context, sh: &mut Shared) -> Option<Done> {
        self.poll();
        let mut done = None;
        let pal = sh.palette;
        if self.disc.is_some() || self.py.is_some() || self.start.is_some() || self.testing() || !self.svc.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |_| {});
        egui::Area::new(egui::Id::new("first_run"))
            .order(egui::Order::Middle)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_width(500.0f32.min(ctx.screen_rect().width() - 32.0));
                let slot = ui.painter().add(egui::Shape::Noop);
                let inner = egui::Frame::none()
                    .inner_margin(egui::Margin::same(26.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Welcome to JoeBro")
                                .font(egui::FontId::new(26.0, theme::display_font())),
                        );
                        ui.add_space(4.0);
                        match self.stage {
                            Stage::Choose => self.choose(ui, sh),
                            Stage::Mac => done = self.mac(ui, sh),
                            Stage::Scratch => self.scratch(ui, sh),
                            Stage::Services => done = self.services(ui, sh),
                            Stage::Server => done = self.server(ui, sh),
                        }
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("You can change this later in Settings \u{2192} General \u{2192} Server.")
                                .small()
                                .color(pal.ink_faint),
                        );
                    });
                ui.painter().set(
                    slot,
                    egui::Shape::Vec(widgets::glass_surface(&pal, inner.response.rect, theme::R_WIN, pal.paper, true)),
                );
            });
        done
    }

    // ---- stages -------------------------------------------------------------

    fn back(&mut self, ui: &mut egui::Ui, sh: &Shared) {
        if ui.add(egui::Button::new(RichText::new("\u{2190} Back").color(sh.palette.ink_dim)).frame(false)).clicked() {
            self.stage = Stage::Choose;
            self.verdict = None;
            self.start_err = None;
        }
    }

    fn choose(&mut self, ui: &mut egui::Ui, sh: &Shared) {
        let pal = sh.palette;
        ui.label(RichText::new("How do you want to use it?").color(pal.ink_dim));
        ui.add_space(14.0);
        if choice(ui, sh, "Connect to your Mac app", "Use the JoeBro backend that runs on your Mac.") {
            self.stage = Stage::Mac;
            if self.found.is_empty() && !self.disc_done && self.disc.is_none() {
                self.start_discovery();
            }
        }
        ui.add_space(8.0);
        if choice(ui, sh, "Run on this computer", "Run a private backend on this computer. Needs Python.") {
            self.stage = Stage::Scratch;
            if self.py_state.is_none() && self.py.is_none() {
                self.py = Some(sh.spawn(|_| backend::find_python_ex()));
            }
        }
        ui.add_space(8.0);
        if choice(ui, sh, "Connect to an existing server", "Enter the address and token of a JoeBro server.") {
            self.stage = Stage::Server;
        }
    }

    fn url_token_fields(&mut self, ui: &mut egui::Ui, hint_url: &str) {
        widgets::header(ui, "Address");
        ui.add(
            egui::TextEdit::singleline(&mut self.url)
                .hint_text(hint(hint_url))
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        widgets::header(ui, "Access token");
        ui.add(
            egui::TextEdit::singleline(&mut self.token)
                .password(true)
                .hint_text(hint("Leave empty if the backend has none"))
                .desired_width(f32::INFINITY),
        );
    }

    fn test_row(&mut self, ui: &mut egui::Ui, sh: &mut Shared, mode: &'static str) -> Option<Done> {
        let pal = sh.palette;
        let mut done = None;
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let can = !self.url.trim().is_empty();
            if ui.add_enabled(can && !self.testing(), egui::Button::new("Test connection")).clicked() {
                self.start_test(sh);
            }
            if widgets::primary(ui, sh, "Continue", can).clicked() {
                done = Some(Done {
                    url: crate::config::normalize_url(&self.url),
                    token: self.token.trim().to_string(),
                    mode,
                });
            }
            if self.testing() {
                ui.spinner();
            }
        });
        match &self.verdict {
            Some(Ok(m)) => {
                ui.add_space(6.0);
                ui.colored_label(pal.accent, m);
            }
            Some(Err(m)) => {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(m).color(pal.warning)).wrap());
            }
            None => {}
        }
        done
    }

    fn mac(&mut self, ui: &mut egui::Ui, sh: &mut Shared) -> Option<Done> {
        let pal = sh.palette;
        self.back(ui, sh);
        ui.label(RichText::new("Connect to your Mac app").font(egui::FontId::new(18.0, theme::display_font())));
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "JoeBro for Mac runs its backend on the Mac itself. Pick it below, or type its address. \
                     By default it only listens on the Mac; to reach it from here, start it with \
                     --host <the Mac's LAN or Tailscale address> and a JOEBRO_TOKEN, then paste that token.",
                )
                .color(pal.ink_dim),
            )
            .wrap(),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            widgets::header(ui, "Found on your network");
            if self.disc.is_some() {
                ui.spinner();
            } else if ui.small_button("Scan again").clicked() {
                self.start_discovery();
            }
        });
        if self.found.is_empty() {
            let msg = if self.disc.is_some() { "Looking\u{2026}" } else { "Nothing found - type the address below." };
            ui.label(RichText::new(msg).small().color(pal.ink_faint));
        }
        let mut pick = None;
        for (u, label) in &self.found {
            if ui.selectable_label(self.url == *u, format!("{label}   {u}")).clicked() {
                pick = Some(u.clone());
            }
        }
        if let Some(u) = pick {
            self.url = u;
            self.verdict = None;
        }
        ui.add_space(8.0);
        self.url_token_fields(ui, "http://my-mac.local:8765");
        self.test_row(ui, sh, "mac")
    }

    fn server(&mut self, ui: &mut egui::Ui, sh: &mut Shared) -> Option<Done> {
        let pal = sh.palette;
        self.back(ui, sh);
        ui.label(RichText::new("Connect to an existing server").font(egui::FontId::new(18.0, theme::display_font())));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Chats, documents, email and everything else are served from it.").color(pal.ink_dim),
        );
        ui.add_space(10.0);
        self.url_token_fields(ui, "http://192.168.1.10:8771");
        self.test_row(ui, sh, "server")
    }

    fn scratch(&mut self, ui: &mut egui::Ui, sh: &mut Shared) {
        let pal = sh.palette;
        self.back(ui, sh);
        ui.label(RichText::new("Run on this computer").font(egui::FontId::new(18.0, theme::display_font())));
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "JoeBro will run its own private backend on this computer (localhost only, protected \
                     by a random token). Your data stays in your user folder.",
                )
                .color(pal.ink_dim),
            )
            .wrap(),
        );
        ui.add_space(12.0);
        widgets::header(ui, "1  Python");
        let mut ready = false;
        match (&self.py_state, self.py.is_some()) {
            (_, true) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Checking for Python\u{2026}");
                });
            }
            (Some(PythonSearch::Found(p)), _) => {
                ready = true;
                ui.colored_label(pal.accent, format!("Found Python {}", p.version));
            }
            (Some(other), _) => {
                ui.add(egui::Label::new(RichText::new(backend::python_help(other)).color(pal.warning)).wrap());
                if ui.button("Check again").clicked() {
                    self.py_state = None;
                    self.py = Some(sh.spawn(|_| backend::find_python_ex()));
                }
            }
            (None, false) => {
                self.py = Some(sh.spawn(|_| backend::find_python_ex()));
            }
        }
        ui.add_space(10.0);
        widgets::header(ui, "2  Start the backend");
        ui.horizontal(|ui| {
            let starting = self.start.is_some();
            if widgets::primary(ui, sh, "Start local backend", ready && !starting).clicked() {
                self.start_err = None;
                self.start = Some(sh.spawn(|_| backend::start()));
            }
            if starting {
                ui.spinner();
                ui.label(RichText::new("Starting\u{2026}").color(pal.ink_dim));
            }
        });
        if let Some(e) = &self.start_err {
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(e).color(pal.warning)).wrap());
        }
    }

    fn services(&mut self, ui: &mut egui::Ui, sh: &mut Shared) -> Option<Done> {
        let pal = sh.palette;
        let mut done = None;
        ui.label(RichText::new("Your backend is running").font(egui::FontId::new(18.0, theme::display_font())));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Connect what you want now - everything here is optional and can be done later in Settings.")
                .color(pal.ink_dim),
        );
        ui.add_space(10.0);
        let Some((url, tok)) = self.started.clone() else { return None };
        let api = Arc::new(Api::with_token(&url, Some(&tok)));

        // Model API key
        egui::CollapsingHeader::new(RichText::new("Model API key").strong()).default_open(true).show(ui, |ui| {
            egui::ComboBox::from_id_salt("prov")
                .selected_text(PROVIDERS[self.prov].0)
                .show_ui(ui, |ui| {
                    for (i, (n, _)) in PROVIDERS.iter().enumerate() {
                        ui.selectable_value(&mut self.prov, i, *n);
                    }
                });
            if PROVIDERS[self.prov].1.is_empty() {
                ui.add(
                    egui::TextEdit::singleline(&mut self.key_url)
                        .hint_text(hint("Base URL, e.g. http://localhost:11434/v1"))
                        .desired_width(f32::INFINITY),
                );
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.key)
                    .password(true)
                    .hint_text(hint("API key (leave empty for a local endpoint)"))
                    .desired_width(f32::INFINITY),
            );
            let base = if PROVIDERS[self.prov].1.is_empty() { self.key_url.trim().to_string() } else { PROVIDERS[self.prov].1.to_string() };
            let can = !base.is_empty() && !self.busy("model");
            if ui.add_enabled(can, egui::Button::new("Add")).clicked() {
                let (name, key) = (PROVIDERS[self.prov].0.to_string(), self.key.trim().to_string());
                self.svc.push(("model", Job::spawn(api.clone(), sh.ctx.clone(), move |a| a.add_endpoint(&name, &base, &key))));
            }
            self.svc_line(ui, "model");
        });

        // Calendar
        egui::CollapsingHeader::new(RichText::new("Calendar (CalDAV)").strong()).show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.cal_url)
                    .hint_text(hint("CalDAV URL, e.g. https://caldav.example.com/dav/"))
                    .desired_width(f32::INFINITY),
            );
            ui.add(egui::TextEdit::singleline(&mut self.cal_user).hint_text(hint("Username")).desired_width(f32::INFINITY));
            ui.add(
                egui::TextEdit::singleline(&mut self.cal_pw)
                    .password(true)
                    .hint_text(hint("Password or app password"))
                    .desired_width(f32::INFINITY),
            );
            let can = !self.cal_url.trim().is_empty() && !self.busy("calendar");
            if ui.add_enabled(can, egui::Button::new("Connect")).clicked() {
                let (u, p, w) = (self.cal_url.trim().to_string(), self.cal_user.trim().to_string(), self.cal_pw.clone());
                self.svc.push(("calendar", Job::spawn(api.clone(), sh.ctx.clone(), move |a| a.connect_caldav(&u, &p, &w))));
            }
            self.svc_line(ui, "calendar");
        });

        // Email
        egui::CollapsingHeader::new(RichText::new("Email (IMAP / SMTP)").strong()).show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.em_addr).hint_text(hint("Email address")).desired_width(f32::INFINITY));
            ui.add(
                egui::TextEdit::singleline(&mut self.em_pw)
                    .password(true)
                    .hint_text(hint("Password or app password"))
                    .desired_width(f32::INFINITY),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.imap_host)
                    .hint_text(hint("IMAP server (blank = guess from the address)"))
                    .desired_width(f32::INFINITY),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.smtp_host)
                    .hint_text(hint("SMTP server (blank = guess)"))
                    .desired_width(f32::INFINITY),
            );
            let can = self.em_addr.contains('@') && !self.em_pw.is_empty() && !self.busy("email");
            if ui.add_enabled(can, egui::Button::new("Connect")).clicked() {
                let e = self.em_addr.trim().to_string();
                let domain = e.rsplit('@').next().unwrap_or("").to_string();
                let ih = if self.imap_host.trim().is_empty() { format!("imap.{domain}") } else { self.imap_host.trim().to_string() };
                let sm = if self.smtp_host.trim().is_empty() { format!("smtp.{domain}") } else { self.smtp_host.trim().to_string() };
                let pw = self.em_pw.clone();
                self.svc.push((
                    "email",
                    Job::spawn(api.clone(), sh.ctx.clone(), move |a| {
                        a.connect_imap(&[
                            ("email", &e),
                            ("imap_host", &ih),
                            ("imap_port", "993"),
                            ("imap_user", &e),
                            ("imap_password", &pw),
                            ("smtp_host", &sm),
                            ("smtp_port", "465"),
                            ("smtp_user", &e),
                            ("smtp_password", &pw),
                        ])
                    }),
                ));
            }
            self.svc_line(ui, "email");
        });

        ui.add_space(12.0);
        if widgets::primary(ui, sh, "Finish", self.svc.is_empty()).clicked() {
            done = Some(Done { url, token: tok, mode: "bundled" });
        }
        done
    }

    fn svc_line(&self, ui: &mut egui::Ui, name: &str) {
        let pal_ok = ui.visuals().hyperlink_color;
        if self.busy(name) {
            ui.spinner();
        }
        if let Some((_, r)) = self.svc_msg.iter().find(|(n, _)| *n == name) {
            match r {
                Ok(m) => {
                    ui.colored_label(pal_ok, m);
                }
                Err(m) => {
                    ui.add(egui::Label::new(RichText::new(m).color(ui.visuals().warn_fg_color)).wrap());
                }
            }
        }
    }
}

/// A full-width glass choice button: title + one-line description.
fn choice(ui: &mut egui::Ui, sh: &Shared, title: &str, sub: &str) -> bool {
    let pal = sh.palette;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 62.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if resp.is_pointer_button_down_on() {
        pal.pressed
    } else if resp.hovered() {
        pal.hover
    } else {
        pal.control
    };
    let r = egui::Rounding::same(theme::R_CARD);
    ui.painter().rect(rect, r, fill, egui::Stroke::new(1.0, pal.separator));
    ui.painter().text(
        rect.left_top() + egui::vec2(16.0, 12.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::new(15.5, theme::medium_font()),
        pal.ink,
    );
    ui.painter().text(
        rect.left_top() + egui::vec2(16.0, 35.0),
        egui::Align2::LEFT_TOP,
        sub,
        egui::FontId::proportional(12.5),
        pal.ink_dim,
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(16.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        "\u{203A}",
        egui::FontId::proportional(20.0),
        pal.ink_faint,
    );
    resp.clicked()
}

// ---- Mac discovery ------------------------------------------------------------

/// Look for JoeBro backends: this machine, Tailscale peers, and the local /24.
/// Every candidate must answer `GET /api/health` like a JoeBro backend.
fn discover() -> Vec<(String, String)> {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
    use std::time::Duration;
    const PORTS: [u16; 2] = [8765, 8771];
    let mut hosts: Vec<(String, String)> = vec![("127.0.0.1".into(), "This computer".into())];
    // Tailscale peers (read-only command; ignored when not installed).
    if let Some(ts) = crate::platform::which("tailscale") {
        if let Ok(out) = crate::platform::hide_console(&mut std::process::Command::new(ts)).args(["status", "--json"]).output() {
            if let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) {
                if let Some(peers) = v.get("Peer").and_then(Value::as_object) {
                    for p in peers.values() {
                        let ip = p.get("TailscaleIPs").and_then(|a| a.get(0)).and_then(Value::as_str);
                        let name = p.get("HostName").and_then(Value::as_str).unwrap_or("peer");
                        if let Some(ip) = ip {
                            hosts.push((ip.to_string(), format!("{name} (Tailscale)")));
                        }
                    }
                }
            }
        }
    }
    // Local /24 (UDP connect sends nothing; it only picks the outgoing interface).
    if let Ok(s) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if s.connect("192.0.2.1:9").is_ok() {
            if let Ok(SocketAddr::V4(a)) = s.local_addr() {
                let o = a.ip().octets();
                if !a.ip().is_loopback() && !a.ip().is_unspecified() {
                    for i in 1..255u8 {
                        let ip = Ipv4Addr::new(o[0], o[1], o[2], i);
                        if IpAddr::V4(ip) != IpAddr::V4(*a.ip()) {
                            hosts.push((ip.to_string(), "On your network".into()));
                        }
                    }
                }
            }
        }
    }
    let (tx, rx) = channel::<(String, String)>();
    let work = Arc::new(std::sync::Mutex::new(
        hosts.into_iter().flat_map(|(h, l)| PORTS.iter().map(move |p| (h.clone(), l.clone(), *p)).collect::<Vec<_>>()).collect::<Vec<_>>(),
    ));
    let mut handles = vec![];
    for _ in 0..48 {
        let (work, tx) = (work.clone(), tx.clone());
        handles.push(std::thread::spawn(move || {
            let http = reqwest::blocking::Client::builder().timeout(Duration::from_millis(1500)).build().ok();
            loop {
                let Some((h, label, port)) = work.lock().ok().and_then(|mut w| w.pop()) else { break };
                let Ok(addr) = format!("{h}:{port}").parse::<SocketAddr>() else { continue };
                if TcpStream::connect_timeout(&addr, Duration::from_millis(350)).is_err() {
                    continue;
                }
                let url = format!("http://{h}:{port}");
                let ok = http
                    .as_ref()
                    .and_then(|c| c.get(format!("{url}/api/health")).send().ok())
                    .and_then(|r| r.json::<Value>().ok())
                    .map(|v| v.get("ok").and_then(Value::as_bool) == Some(true))
                    .unwrap_or(false);
                if ok {
                    let _ = tx.send((url, label));
                }
            }
        }));
    }
    drop(tx);
    let mut found: Vec<(String, String)> = rx.iter().collect();
    for h in handles {
        let _ = h.join();
    }
    found.sort();
    found.dedup();
    found
}
