//! Top-level eframe app: owns Shared + every panel, lays out the window
//! (sidebar | main | optional editor), routes the active tab, drains startup
//! loads, and paints the error toast.
use crate::api::Api;
use crate::panels::{
    agents::AgentsPanel, aicheck::AICheckPanel, calendar::CalendarPanel, chat::ChatPanel,
    coding_agents::{CodingAgentConversation, NewCodingAgentSheet}, editor::EditorPanel, email::EmailPanel,
    inbox::InboxPanel, night_shift::NightShiftPanel, notes::NotesPanel,
    palette::{Action, CommandPalette}, plugins::PluginPanelsView, research::ResearchPanel,
    settings::SettingsWindow, shared_memory::SharedMemoryPanel, sidebar::Sidebar, skills::SkillsPanel,
    tasks::TasksPanel, tools::ToolsPanel,
};
use crate::state::{Job, Model, Shared, Tab};
use crate::theme;
use crate::widgets;
use eframe::egui;
use serde_json::Value;
use std::sync::Arc;

pub struct JoeBro {
    pub(crate) sh: Shared,
    pub(crate) tab: Tab,
    sidebar: Sidebar,
    pub(crate) chat: ChatPanel,
    email: EmailPanel,
    pub(crate) calendar: CalendarPanel,
    memory: SharedMemoryPanel,
    night_shift: NightShiftPanel,
    inbox: InboxPanel,
    plugins: PluginPanelsView,
    coding_chat: CodingAgentConversation,
    new_coding_agent: NewCodingAgentSheet,
    command_palette: CommandPalette,
    intro: crate::intro::Intro,
    notes: NotesPanel,
    tasks: TasksPanel,
    pub(crate) agents: AgentsPanel,
    skills: SkillsPanel,
    tools: ToolsPanel,
    pub research: ResearchPanel,
    aicheck: AICheckPanel,
    editor: EditorPanel,
    pub(crate) settings: SettingsWindow,
    pub(crate) show_settings: bool,

    inbox_poll: Option<Job<crate::api::R<Vec<crate::models::InboxItem>>>>,
    inbox_timer: f64,
    plugins_load: Option<Job<crate::api::R<Vec<crate::models::Plugin>>>>,
    boot_models: Option<Job<crate::api::R<Value>>>,
    boot_prefs: Option<Job<crate::api::R<Value>>>,
    unread_poll: Option<Job<crate::api::R<Value>>>,
    unread_timer: f64,
    composer_pull: Option<Job<crate::api::R<Value>>>,
    composer_push: Option<Job<crate::api::R<Value>>>,
    composer_timer: f64,
    composer_push_delay: f64,
    /// The last values known to be on the backend — pushes send only the
    /// fields that differ, so two devices editing different switches merge.
    composer_base: crate::state::ComposerSettings,
    last_tab: Tab,
    last_sid: Option<String>,
    theme_poll: f32,
    close_pending: bool,
    allow_close: bool,
    fade_tab: Tab,
    fade_t0: f64,
    /// First-run connection screen (Production, until a backend is set).
    pub setup: Option<crate::setup::Setup>,
    /// The bundled backend being started at launch (mode `bundled`).
    bundled_boot: Option<Job<Result<u16, String>>>,
    pub backdrop: crate::backdrop::Backdrop,
}

impl JoeBro {
    pub fn new(api: Arc<Api>, ctx: egui::Context) -> Self {
        let auto_bundled = crate::setup::first_run_auto();
        theme::load_prefs();
        theme::set_reduce_glass_quiet(crate::config::get().reduce_glass);
        let (aero, dark) = theme::desktop_state(ctx_sys_dark(&ctx));
        theme::apply(&ctx, theme::Palette::for_state(aero, dark));
        let sh = Shared::new(api, ctx, aero, dark);
        let boot_models = Some(sh.spawn(|a| a.models()));
        let boot_prefs = Some(sh.spawn(|a| a.prefs()));

        let mut app = JoeBro {
            tab: Tab::Chats,
            sidebar: Sidebar::default(),
            chat: ChatPanel::default(),
            email: EmailPanel::default(),
            calendar: CalendarPanel::default(),
            memory: SharedMemoryPanel::default(),
            night_shift: NightShiftPanel::default(),
            inbox: InboxPanel::default(),
            plugins: PluginPanelsView::default(),
            coding_chat: CodingAgentConversation::default(),
            new_coding_agent: NewCodingAgentSheet::default(),
            command_palette: CommandPalette::default(),
            intro: {
                let mut i = crate::intro::Intro::default();
                // No launch animation in tests / snapshots / when asked.
                i.done = cfg!(feature = "snapshot") || cfg!(test) || std::env::var("JOEBRO_NO_INTRO").is_ok();
                i
            },
            notes: NotesPanel::default(),
            tasks: TasksPanel::default(),
            agents: AgentsPanel::default(),
            skills: SkillsPanel::default(),
            tools: ToolsPanel::default(),
            research: ResearchPanel::default(),
            aicheck: AICheckPanel::default(),
            editor: EditorPanel::default(),
            settings: SettingsWindow::default(),
            show_settings: false,
            inbox_poll: None,
            inbox_timer: 0.0,
            plugins_load: None,
            boot_models,
            boot_prefs,
            unread_poll: None,
            unread_timer: 0.0,
            composer_pull: None,
            composer_push: None,
            composer_timer: 0.0,
            composer_push_delay: 0.0,
            composer_base: sh.composer.clone(),
            last_tab: Tab::Chats,
            last_sid: None,
            theme_poll: 0.0,
            close_pending: false,
            allow_close: false,
            fade_tab: Tab::Chats,
            fade_t0: -10.0,
            setup: if auto_bundled || crate::config::configured() || crate::config::get().mode == "bundled" {
                None
            } else if !crate::config::PERSONAL && crate::config::get().mode.is_empty() {
                Some(crate::setup::Setup::local_backend(None)) // Python missing
            } else {
                Some(Default::default())
            },
            bundled_boot: None,
            backdrop: Default::default(),
            sh,
        };
        app.sh.api.detect_in_background(crate::config::load_token());
        // A backend this app owns is started with it (and stopped on exit).
        let cfg = crate::config::get();
        if !crate::config::PERSONAL && cfg.mode == "bundled" && cfg.bundled_autostart {
            app.bundled_boot = Some(app.sh.spawn(|_| crate::backend::start()));
        }
        app
    }

    /// Switch to another backend: persist URL + token (0600), rebuild the API
    /// client and reload everything that was fetched from the old one.
    pub fn apply_connection(&mut self, url: &str, token: &str) {
        let url = crate::config::normalize_url(url);
        crate::config::update(|c| c.server_url = url.clone());
        if let Err(e) = crate::config::save_token(token) {
            self.sh.err(format!("Couldn't store the token: {e}"));
        }
        self.sh.api = Arc::new(Api::with_token(&url, Some(token)));
        self.sh.api.detect_in_background(Some(token.to_string()));
        self.sh.models.clear();
        self.sh.selected_model = None;
        self.sh.prefs = Value::Null;
        self.sh.current_sid = None;
        self.sh.reload_sessions = true;
        self.sh.reload_workdir = true;
        self.sh.composer_rev += 1;
        self.boot_models = Some(self.sh.spawn(|a| a.models()));
        self.boot_prefs = Some(self.sh.spawn(|a| a.prefs()));
        self.settings = SettingsWindow::default();
    }

    /// Close the workspace editor when the user switches away from Chats or to
    /// a different chat — a bound-folder doc belongs to its chat.
    fn sync_workspace(&mut self) {
        if self.tab != self.last_tab {
            self.last_tab = self.tab;
            if self.tab != Tab::Chats {
                self.sh.editor_visible = false;
            }
        }
        if self.sh.current_sid != self.last_sid {
            if self.editor.has_unsaved(&self.sh) {
                self.sh.current_sid = self.last_sid.clone();
                self.sh
                    .err("Finish saving your open documents before switching chats.");
                return;
            }
            self.last_sid = self.sh.current_sid.clone();
            self.sh.open_docs.clear();
            self.sh.active_doc = None;
            self.sh.editor_visible = false;
        }
    }

    fn drain_boot(&mut self) {
        if let Some(job) = &mut self.boot_models {
            if let Some(res) = job.take() {
                if let Ok(v) = res {
                    self.sh.models = Model::flatten_response(&v);
                    if self.sh.selected_model.is_none() {
                        let saved = crate::state::saved_model_id();
                        self.sh.selected_model = self
                            .sh
                            .models
                            .iter()
                            .find(|m| m.id == saved)
                            .cloned()
                            .or_else(|| {
                                self.sh
                                    .models
                                    .iter()
                                    .find(|m| m.category == "local")
                                    .cloned()
                            })
                            .or_else(|| self.sh.models.first().cloned());
                    }
                }
                self.boot_models = None;
            }
        }
        if let Some(job) = &mut self.boot_prefs {
            if let Some(Ok(v)) = job.take() {
                self.sh.prefs = v.clone();
                if let Some(b) = v.get("require_doc_approval").and_then(|x| x.as_bool()) {
                    self.sh.require_doc_approval = b;
                }
                self.boot_prefs = None;
            } else if self.boot_prefs.as_ref().map(|j| j.done).unwrap_or(false) {
                self.boot_prefs = None;
            }
        }
    }

    fn poll_unread(&mut self, dt: f64) {
        self.unread_timer -= dt;
        if let Some(job) = &mut self.unread_poll {
            if let Some(result) = job.take() {
                if let Ok(v) = result {
                    self.sh.unread_email = v.get("unread").and_then(|x| x.as_i64()).unwrap_or(0);
                }
                self.unread_poll = None;
            }
        }
        if self.unread_timer <= 0.0 && self.unread_poll.is_none() {
            self.unread_timer = 60.0;
            self.unread_poll = Some(self.sh.spawn(|a| a.email_unread_total()));
        }
    }

    /// Keep the composer settings in step with the backend: pull on start and
    /// every 30 s (another device may have changed them), push local changes
    /// shortly after they happen. The local cache covers offline starts.
    fn sync_composer(&mut self, dt: f64) {
        use crate::state::ComposerSettings;
        if let Some(job) = &mut self.composer_push {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) => {
                        if let Some(remote) = ComposerSettings::from_value(&v) {
                            self.composer_base = remote.clone();
                            if !self.sh.composer_dirty {
                                self.sh.composer.updated_at = remote.updated_at;
                                self.sh.composer.save_cache();
                            }
                        }
                    }
                    // Offline: the cache already holds the change; try again later.
                    Err(_) => {
                        self.sh.composer_dirty = true;
                        self.composer_push_delay = 30.0;
                    }
                }
                self.composer_push = None;
            }
        }
        if self.sh.composer_dirty {
            if self.composer_push_delay <= 0.0 {
                self.composer_push_delay = 0.4; // debounce rapid toggling
                self.sh.composer.save_cache();
            }
            self.composer_push_delay -= dt;
            if self.composer_push_delay <= 0.0 && self.composer_push.is_none() {
                self.sh.composer_dirty = false;
                let full = self.sh.composer.to_value();
                let mut changed = serde_json::Map::new();
                let base = self.composer_base.to_value();
                for (k, v) in full.as_object().into_iter().flatten() {
                    if k != "updated_at" && (k == "client" || base.get(k) != Some(v)) {
                        changed.insert(k.clone(), v.clone());
                    }
                }
                if changed.len() > 1 {
                    let changed = Value::Object(changed);
                    self.composer_push =
                        Some(self.sh.spawn(move |a| a.put_client_settings(changed, full)));
                }
            }
            self.sh.ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(job) = &mut self.composer_pull {
            if let Some(result) = job.take() {
                if let Ok(v) = result {
                    if let Some(remote) = ComposerSettings::from_value(&v) {
                        // A pending local edit wins over what we just read.
                        if !self.sh.composer_dirty && self.composer_push.is_none() {
                            self.composer_base = remote.clone();
                            if !remote.same_values(&self.sh.composer) {
                                self.sh.composer = remote;
                                self.sh.composer_rev += 1;
                            } else {
                                self.sh.composer.updated_at = remote.updated_at;
                            }
                            self.sh.composer.save_cache();
                        }
                    }
                }
                self.composer_pull = None;
            }
        }
        self.composer_timer -= dt;
        if self.composer_timer <= 0.0 && self.composer_pull.is_none() {
            self.composer_timer = 30.0;
            self.composer_pull = Some(self.sh.spawn(|a| a.client_settings()));
        }
    }
}

impl JoeBro {
    /// Small centred card while the bundled backend boots.
    fn paint_starting(&self, ctx: &egui::Context) {
        let pal = self.sh.palette;
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |_| {});
        egui::Area::new(egui::Id::new("starting_backend"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let slot = ui.painter().add(egui::Shape::Noop);
                let inner = egui::Frame::none().inner_margin(egui::Margin::same(22.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new("Starting your local backend\u{2026}").color(pal.ink));
                    });
                });
                ui.painter().set(
                    slot,
                    egui::Shape::Vec(crate::widgets::glass_surface(&pal, inner.response.rect, theme::R_WIN, pal.paper, true)),
                );
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

impl eframe::App for JoeBro {
    fn on_exit(&mut self) {
        crate::backend::stop();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame(ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if crate::theme::opaque_window() {
            let mut c = self.clear_rgba();
            c[3] = 1.0;
            return c;
        }
        self.clear_rgba()
    }
}

impl JoeBro {
    /// One UI frame. Split from `eframe::App::update` so the offscreen
    /// snapshot harness can drive the exact same layout without a window.
    pub fn frame(&mut self, ctx: &egui::Context) {
        // Keep the Wayland event loop, background jobs, streaming indicators,
        // and theme watcher responsive even when the user is not generating
        // pointer/keyboard events. Four idle frames/sec is cheap and avoids the
        // compositor treating a quiet window as stalled.
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        self.theme_poll -= ctx.input(|i| i.stable_dt);
        if self.theme_poll <= 0.0 {
            self.theme_poll = 0.5;
            let (aero, dark) = theme::desktop_state(ctx_sys_dark(ctx));
            if aero != self.sh.aero || dark != self.sh.dark {
                self.sh.aero = aero;
                self.sh.dark = dark;
                self.sh.palette = theme::Palette::for_state(aero, dark);
                theme::apply(ctx, self.sh.palette);
            }
        }
        // Backdrop first, so every panel's glass lands on top of it.
        self.backdrop.paint(ctx, self.sh.dark);
        if let Some(job) = &mut self.bundled_boot {
            if let Some(res) = job.take() {
                self.bundled_boot = None;
                match res {
                    Ok(port) => {
                        let url = crate::backend::url_for(port);
                        let token = crate::backend::read_token().unwrap_or_default();
                        self.apply_connection(&url, &token);
                    }
                    Err(m) => self.setup = Some(crate::setup::Setup::local_backend(Some(m))),
                }
            } else {
                self.paint_starting(ctx);
                return;
            }
        }
        if let Some(setup) = &mut self.setup {
            if let Some(d) = setup.ui(ctx, &mut self.sh) {
                crate::config::update(|c| {
                    c.mode = d.mode.to_string();
                    if d.mode == "bundled" {
                        c.bundled_autostart = true;
                    }
                });
                self.apply_connection(&d.url, &d.token);
                self.setup = None;
            }
            return;
        }
        self.editor.tick(&mut self.sh);
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.editor.has_unsaved(&self.sh)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_pending = true;
        }
        if std::mem::take(&mut self.sh.reload_models) && self.boot_models.is_none() {
            self.boot_models = Some(self.sh.spawn(|a| a.models()));
        }
        self.drain_boot();
        self.poll_unread(ctx.input(|i| i.stable_dt) as f64);
        self.sync_composer(ctx.input(|i| i.stable_dt) as f64);
        if let Some(t) = self.sh.goto_tab.take() {
            self.tab = t;
        }
        self.shell_keys(ctx);
        self.sync_workspace();
        self.poll_agent_nav(ctx.input(|i| i.stable_dt) as f64);

        let pal = self.sh.palette;
        // Sidebar (left): a 260 pt flat panel, 10 pt gutters (MainSplitView). Hidden with
        // Ctrl+\ / the chat header toggle / the palette, on every tab.
        let side_open = !self.sh.sidebar_hidden;
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(SIDEBAR_W)
            .show_separator_line(false)
            .frame(egui::Frame::none().outer_margin(egui::Margin { left: GAP, right: GAP / 2.0, top: GAP, bottom: GAP }))
            .show_animated(ctx, side_open, |ui| {
                let rect = ui.max_rect();
                widgets::panel_glass(ctx, &pal, rect, pal.panel_fill);
                let mut sidebar_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                sidebar_ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                self.sidebar.ui(&mut sidebar_ui, &mut self.sh, &mut self.tab, &mut self.show_settings);
                ui.allocate_space(rect.size());
            });

        // Keep enough room for the conversation beside the editor. At narrow
        // window sizes, detach the editor instead of squeezing chat to a sliver.
        let editor_requested = self.sh.editor_visible || self.sh.open_request.is_some();
        let inline_editor_width =
            (ctx.screen_rect().width() - SIDEBAR_W - 380.0 - 3.0 * GAP).clamp(240.0, 560.0);
        let editor_detached =
            self.sh.editor_popped_out || (editor_requested && inline_editor_width <= 240.0);

        // Editor pane (right): visible when a doc is open OR a file was just
        // requested (so clicking a file can actually open the pane).
        if editor_requested && !editor_detached {
            egui::SidePanel::right("editor")
                .resizable(true)
                .default_width(inline_editor_width)
                .min_width(240.0)
                .max_width(inline_editor_width)
                .show_separator_line(false)
                .frame(
                    egui::Frame::none()
                        .outer_margin(egui::Margin { left: GAP / 2.0, right: GAP, top: GAP, bottom: GAP })
                        .inner_margin(egui::Margin::ZERO),
                )
                .show(ctx, |ui| {
                    let r = ui.max_rect();
                    widgets::panel_glass(ctx, &pal, r, pal.panel_fill);
                    self.editor.ui(ui, &mut self.sh);
                });
        }

        if editor_detached {
            let mut open = true;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("personal-editor"),
                egui::ViewportBuilder::default()
                    .with_title(format!("{} · Documents", crate::config::APP_TITLE))
                    .with_app_id("JoeBro")
                    .with_inner_size([850.0, 750.0]),
                |ctx, class| {
                    if ctx.input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    if class == egui::ViewportClass::Embedded {
                        egui::Window::new("Documents")
                            .open(&mut open)
                            .show(ctx, |ui| self.editor.ui(ui, &mut self.sh));
                    } else {
                        let pal = self.sh.palette;
                        egui::CentralPanel::default()
                            .frame(
                                egui::Frame::none()
                                    .fill(pal.ground)
                                    .outer_margin(egui::Margin::same(0.0))
                                    .inner_margin(egui::Margin::same(GAP)),
                            )
                            .show(ctx, |ui| {
                                let r = ui.max_rect();
                                widgets::panel_glass(ctx, &pal, r, pal.panel_fill);
                                self.editor.ui(ui, &mut self.sh);
                            });
                    }
                },
            );
            if !open {
                if self.sh.editor_popped_out {
                    self.sh.editor_popped_out = false;
                    self.sh.editor_visible = true;
                } else {
                    self.sh.editor_visible = false;
                }
            }
        }
        if self.close_pending {
            egui::Window::new("Unsaved documents").collapsible(false).show(ctx, |ui| {
                ui.label("Documents are still saving or need attention. Closing now would lose unsaved edits.");
                if ui.button("Keep working").clicked() { self.close_pending = false; }
                if ui.button("Discard unsaved edits and close").clicked() {
                    self.allow_close = true; ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }

        // Detail (centre). Tab switches fade + rise in (`.transition(.opacity.combined(with: .offset(y: 6)))`).
        if self.tab != self.fade_tab {
            self.fade_tab = self.tab;
            self.fade_t0 = ctx.input(|i| i.time);
        }
        let since = (ctx.input(|i| i.time) - self.fade_t0) as f32;
        let fade = if theme::reduce_motion() { 1.0 } else { theme::ease_out((since / theme::SMOOTH).clamp(0.0, 1.0)) };
        if fade < 1.0 {
            ctx.request_repaint();
        }
        let left_gap = if side_open { GAP / 2.0 } else { GAP };
        let right_gap = if editor_requested && !editor_detached { GAP / 2.0 } else { GAP };
        egui::CentralPanel::default()
            .frame(egui::Frame::none().outer_margin(egui::Margin { left: left_gap, right: right_gap, top: GAP, bottom: GAP }))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let own_chrome = crate::panels::SELF_CHROME.contains(&self.tab);
                // Panels that have not moved to `panel_chrome` get the flat surface from the shell.
                if !own_chrome {
                    widgets::panel_glass(ctx, &pal, rect, pal.panel_fill);
                }
                ui.multiply_opacity(0.0f32.max(fade));
                let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect.translate(egui::vec2(0.0, (1.0 - fade) * 6.0))));
                inner.set_clip_rect(rect.expand(2.0));
                let pad = if own_chrome { 0.0 } else { 14.0 };
                let ui = &mut inner;
                egui::Frame::none().inner_margin(egui::Margin::same(pad)).show(ui, |ui| {
                    ui.set_min_size(rect.size() - egui::vec2(pad * 2.0, pad * 2.0));
                    match self.tab {
                        Tab::Chats => match self.sh.agent_nav.open_id.clone() {
                            Some(id) => self.coding_chat.ui(ui, &mut self.sh, &id),
                            None => self.chat.ui(ui, &mut self.sh),
                        },
                        Tab::Email => self.email.ui(ui, &mut self.sh),
                        Tab::Calendar => self.calendar.ui(ui, &mut self.sh),
                        Tab::Brain | Tab::Memory => self.memory.ui(ui, &mut self.sh),
                        Tab::NightShift => self.night_shift.ui(ui, &mut self.sh),
                        Tab::Notes => self.notes.ui(ui, &mut self.sh),
                        Tab::Tasks => self.tasks.ui(ui, &mut self.sh),
                        Tab::Agents => self.agents.ui(ui, &mut self.sh),
                        Tab::Skills => self.skills.ui(ui, &mut self.sh),
                        Tab::Tools => self.tools.ui(ui, &mut self.sh),
                        Tab::Plugins => self.plugins.ui(ui, &mut self.sh),
                        Tab::Research => self.research.ui(ui, &mut self.sh),
                        Tab::AICheck => self.aicheck.ui(ui, &mut self.sh),
                        Tab::Inbox => self.inbox.ui(ui, &mut self.sh),
                    }
                });
            });

        // Sheets that live above the panels.
        self.new_coding_agent.ui(ctx, &mut self.sh);

        // Settings window (modal-ish).
        if self.show_settings {
            self.settings.ui(ctx, &mut self.sh, &mut self.show_settings, &mut self.backdrop);
        }
        for done in self.sh.picker.ui(ctx, &self.sh.palette.clone()) {
            match done.0.as_str() {
                "bg_photo" => {
                    if let Err(e) = crate::backdrop::set_wallpaper(&done.1) {
                        self.sh.err(e);
                    }
                }
                "bg_video" => match crate::backdrop::import_video(&done.1) {
                    Ok(id) => crate::backdrop::use_scene(&id),
                    Err(e) => self.sh.err(e),
                },
                _ => self.sh.picked.push(done),
            }
        }
        if let Some((url, token)) = self.sh.pending_connection.take() {
            self.apply_connection(&url, &token);
        }

        // Command palette (Ctrl+K).
        if let Some(action) = self.command_palette.ui(ctx, &mut self.sh) {
            self.run_action(action);
        }

        // Error toast: a floating surface, bottom centre.
        if let Some(msg) = self.sh.toast.clone() {
            let screen = ctx.screen_rect();
            egui::Area::new(egui::Id::new("toast"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -GAP - 18.0))
                .show(ctx, |ui| {
                    ui.set_max_width((screen.width() - 120.0).min(640.0));
                    widgets::floating_frame(&pal, theme::R_CARD).inner_margin(egui::Margin::symmetric(14.0, 9.0)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            crate::icons::symbol(ui, "exclamationmark.triangle", 13.0, pal.warn);
                            ui.add(egui::Label::new(theme::ty::body(&msg).color(pal.ink)).wrap());
                            if widgets::icon_btn(ui, &pal, "xmark", 22.0, false).clicked() {
                                self.sh.toast = None;
                            }
                        });
                    });
                });
        }

        // Launch intro, over everything.
        self.intro.ui(ctx, &pal);
    }

    /// Global shortcuts (the Mac's menu commands): Ctrl+N new chat, Ctrl+K palette, Ctrl+, settings,
    /// Ctrl+\ sidebar, Ctrl+1..9 panels, Esc closes the palette / settings.
    fn shell_keys(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let cmd = Modifiers::COMMAND;
        if ctx.input_mut(|i| i.consume_key(cmd, Key::K)) {
            self.sh.palette_open = !self.sh.palette_open;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::N)) {
            self.run_action(Action::NewChat);
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Comma)) {
            self.show_settings = !self.show_settings;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Backslash)) {
            self.sh.sidebar_hidden = !self.sh.sidebar_hidden;
        }
        for (i, key) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9].into_iter().enumerate() {
            if ctx.input_mut(|inp| inp.consume_key(cmd, key)) {
                self.run_action(Action::Panel(Tab::JUMP[i]));
            }
        }
        if std::mem::take(&mut self.sh.settings_requested) {
            self.show_settings = true;
        }
        if !self.sh.palette_open && !self.sh.picker.is_open() && self.show_settings && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.show_settings = false;
        }
    }

    /// Run a palette / shortcut action.
    fn run_action(&mut self, action: Action) {
        match action {
            Action::Chat(id) => {
                self.sh.select_chat(&id); // picking a chat leaves the coding agent
                self.tab = Tab::Chats;
            }
            Action::Panel(t) => self.tab = t,
            Action::NewChat => {
                self.sh.agent_nav.close();
                self.sh.new_chat_requested = true;
                self.tab = Tab::Chats;
            }
            Action::Settings => self.show_settings = true,
            Action::ToggleSidebar => self.sh.sidebar_hidden = !self.sh.sidebar_hidden,
        }
    }

    /// Poll the coding agents shown in the sidebar (`AgentNav.refresh`): every 6 s while any is live, else 20 s.
    fn poll_agent_nav(&mut self, dt: f64) {
        if let Some(job) = &mut self.sh.agent_nav.load {
            if let Some(res) = job.take() {
                self.sh.agent_nav.load = None;
                if let Ok(env) = res {
                    self.sh.agent_nav.runs = env
                        .agents
                        .into_iter()
                        .filter(|a| a.kind == "coding" && a.parent_id.is_empty() && (a.open || a.live || a.status == "waiting"))
                        .collect();
                }
            } else if job.done {
                self.sh.agent_nav.load = None;
            }
        }
        // Notification count (sidebar dot) every 60 s; plugins once per connection.
        if let Some(job) = &mut self.inbox_poll {
            if let Some(res) = job.take() {
                self.inbox_poll = None;
                if let Ok(items) = res {
                    self.sh.inbox_unread = items.iter().filter(|i| !i.read).count() as i64;
                }
            } else if job.done {
                self.inbox_poll = None;
            }
        }
        self.inbox_timer -= dt;
        if self.inbox_timer <= 0.0 && self.inbox_poll.is_none() {
            self.inbox_timer = 60.0;
            self.inbox_poll = Some(self.sh.spawn(|a| a.inbox()));
        }
        if self.plugins_load.is_none() && self.sh.plugins.is_empty() && self.inbox_timer > 59.0 {
            self.plugins_load = Some(self.sh.spawn(|a| a.plugins_typed()));
        }
        if let Some(job) = &mut self.plugins_load {
            if let Some(res) = job.take() {
                if let Ok(p) = res {
                    self.sh.plugins = p;
                }
            }
        }
        self.sh.agent_nav.timer -= dt;
        if self.sh.agent_nav.timer <= 0.0 && self.sh.agent_nav.load.is_none() {
            let live = self.sh.agent_nav.runs.iter().any(|r| r.live);
            self.sh.agent_nav.timer = if live { 6.0 } else { 20.0 };
            self.sh.agent_nav.load = Some(self.sh.spawn(|a| a.agents_typed("coding", "")));
        }
    }

    /// Window clear colour (premultiplied, gamma) — the thin tint between the
    /// floating panels. Hyprland blurs whatever is behind it.
    pub fn clear_rgba(&self) -> [f32; 4] {
        self.sh.palette.clear_rgba()
    }

    /// Snapshot harness: open a sample document in the editor pane.
    #[cfg(feature = "snapshot")]
    pub fn demo_editor_pdf(&mut self) {
        let bytes = std::env::var("SNAP_PDF").ok().and_then(|p| std::fs::read(p).ok());
        self.sh.open_docs.push(crate::state::EditorDoc {
            id: "demo-pdf".into(),
            title: "Lease agreement 2026.pdf".into(),
            is_binary: true,
            raw_bytes: bytes,
            ..Default::default()
        });
        self.sh.active_doc = Some("demo-pdf".into());
        self.sh.editor_visible = true;
    }

    #[cfg(feature = "snapshot")]
    pub fn demo_editor(&mut self) {
        self.sh.open_docs.push(crate::state::EditorDoc {
            id: "demo-outline".into(),
            title: "outline.md".into(),
            language: "markdown".into(),
            content: "# Hope as atmosphere\n\nHope is usually described as a forecast: a reasoned bet that tomorrow will be kinder than today.\n\n## 1. Hope as atmosphere\n- The room image\n- Bloch: hope as a plan\n\n## 2. Solnit\nHope lives *because* the future is unknown.\n".into(),
            ..Default::default()
        });
        self.sh.active_doc = Some("demo-outline".into());
        self.sh.editor_visible = true;
    }
}

const GAP: f32 = 12.0;
const SIDEBAR_W: f32 = 260.0;


/// The OS light/dark preference as reported by the windowing system.
fn ctx_sys_dark(ctx: &egui::Context) -> Option<bool> {
    ctx.system_theme().map(|t| t == egui::Theme::Dark)
}
