//! Settings → Server: status and controls for the bundled local backend.
use crate::backend::{self, PythonSearch, State};
use crate::state::{Job, Shared};
use eframe::egui::{self, RichText, Ui};
use serde_json::Value;

#[derive(Default)]
pub struct BundledPanel {
    health: Option<Job<Result<String, String>>>,
    health_msg: Option<Result<String, String>>,
    health_age: f32,
    py: Option<Job<PythonSearch>>,
    py_line: Option<Result<String, String>>,
    action: Option<Job<Result<u16, String>>>,
    stopping: Option<Job<()>>,
    err: Option<String>,
}

fn health_probe(port: u16) -> Result<String, String> {
    let c = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;
    let r = c.get(format!("{}/api/health", backend::url_for(port))).send().map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status().as_u16()));
    }
    let v: Value = r.json().map_err(|e| e.to_string())?;
    Ok(format!("healthy, v{}", v.get("version").and_then(Value::as_str).unwrap_or("?")))
}

impl BundledPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let pal = sh.palette;
        let dt = ui.input(|i| i.stable_dt);
        if let Some(j) = &mut self.py {
            if let Some(r) = j.take() {
                self.py_line = Some(match r {
                    PythonSearch::Found(p) => Ok(p.display()),
                    other => Err(backend::python_help(&other)),
                });
                self.py = None;
            }
        }
        if self.py.is_none() && self.py_line.is_none() {
            self.py = Some(sh.spawn(|_| backend::find_python_ex()));
        }
        if let Some(j) = &mut self.health {
            if let Some(r) = j.take() {
                self.health_msg = Some(r);
                self.health = None;
            }
        }
        if let Some(j) = &mut self.stopping {
            if j.take().is_some() {
                self.stopping = None;
            }
        }
        if let Some(j) = &mut self.action {
            if let Some(r) = j.take() {
                self.action = None;
                match r {
                    Ok(port) => {
                        self.err = None;
                        let url = backend::url_for(port);
                        let token = backend::read_token().unwrap_or_default();
                        crate::config::update(|c| c.mode = "bundled".into());
                        sh.pending_connection = Some((url, token));
                    }
                    Err(m) => self.err = Some(m),
                }
            }
        }
        let state = backend::snapshot();
        self.health_age += dt;
        if let State::Running { port, .. } = state {
            if self.health.is_none() && (self.health_age > 5.0 || self.health_msg.is_none()) {
                self.health_age = 0.0;
                self.health = Some(sh.spawn(move |_| health_probe(port)));
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        } else {
            self.health_msg = None;
        }
        let busy = self.action.is_some() || self.stopping.is_some();
        if busy {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
        }

        ui.label(RichText::new("Bundled local backend").size(12.0).color(pal.ink_dim));
        ui.horizontal_wrapped(|ui| match (&state, busy) {
            (_, true) => {
                ui.spinner();
                ui.label(if self.stopping.is_some() { "Stopping\u{2026}" } else { "Starting\u{2026}" });
            }
            (State::Running { port, pid }, _) => {
                let h = match &self.health_msg {
                    Some(Ok(m)) => RichText::new(m.clone()).color(pal.accent),
                    Some(Err(m)) => RichText::new(format!("not responding ({m})")).color(pal.warning),
                    None => RichText::new("checking\u{2026}").color(pal.ink_faint),
                };
                ui.label(format!("Running on 127.0.0.1:{port} (pid {pid}) \u{2014}"));
                ui.label(h);
            }
            (State::Starting, _) => {
                ui.spinner();
                ui.label("Starting\u{2026}");
            }
            (State::Failed(m), _) => {
                ui.add(egui::Label::new(RichText::new(m.clone()).color(pal.warning)).wrap());
            }
            (State::Stopped, _) => {
                ui.label(RichText::new("Not running").color(pal.ink_dim));
            }
        });
        if let Some(e) = &self.err {
            ui.add(egui::Label::new(RichText::new(e).color(pal.warning)).wrap());
        }
        match &self.py_line {
            Some(Ok(p)) => {
                ui.label(RichText::new(format!("Python: {p}")).small().color(pal.ink_faint));
            }
            Some(Err(m)) => {
                ui.add(egui::Label::new(RichText::new(m).small().color(pal.warning)).wrap());
                if ui.small_button("Check again").clicked() {
                    self.py_line = None;
                }
            }
            None => {}
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let running = matches!(state, State::Running { .. });
            if ui.add_enabled(!busy && !running, egui::Button::new("Start")).clicked() {
                self.err = None;
                self.action = Some(sh.spawn(|_| backend::start()));
            }
            if ui.add_enabled(!busy && backend::started_by_app(), egui::Button::new("Stop")).clicked() {
                self.stopping = Some(sh.spawn(|_| backend::stop()));
            }
            if ui.add_enabled(!busy && backend::started_by_app(), egui::Button::new("Restart")).clicked() {
                self.err = None;
                self.action = Some(sh.spawn(|_| {
                    backend::stop();
                    backend::start()
                }));
            }
            if ui.button("Open log").clicked() {
                if let Err(e) = crate::platform::open_path(backend::log_path()) {
                    self.err = Some(format!("Couldn't open the log: {e}"));
                }
            }
            if ui.button("Open data folder").clicked() {
                let _ = std::fs::create_dir_all(backend::data_dir());
                if let Err(e) = crate::platform::open_path(backend::data_dir()) {
                    self.err = Some(format!("Couldn't open the data folder: {e}"));
                }
            }
        });
        let mut auto = crate::config::get().bundled_autostart;
        if ui.checkbox(&mut auto, "Start it automatically when JoeBro opens").changed() {
            crate::config::update(|c| c.bundled_autostart = auto);
        }
        ui.label(
            RichText::new(format!("Data: {}", backend::data_dir().display())).small().color(pal.ink_faint),
        );
    }
}
