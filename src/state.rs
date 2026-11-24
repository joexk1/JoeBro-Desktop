//! Shared app state handed to every panel, plus the small async helper the
//! whole app uses: spawn a blocking API call on a worker thread, get the
//! result back over an mpsc channel, repaint when it lands.
use crate::api::Api;
use crate::theme::Palette;
use eframe::egui;
use serde_json::Value;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// A model choice as the backend returns it (api/models rows).
#[derive(Clone, Debug)]
pub struct Model {
    pub id: String,
    pub model_id: String,
    pub display: String,
    pub tag: String,
    pub endpoint_name: String,
    pub endpoint_id: String,
    pub endpoint_url: String,
    pub category: String,
}

impl Model {
    pub fn from_value(v: &Value) -> Option<Model> {
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let id = g("id");
        if id.is_empty() {
            return None;
        }
        Some(Model {
            model_id: {
                let m = g("model_id");
                if m.is_empty() {
                    id.clone()
                } else {
                    m
                }
            },
            display: {
                let d = g("display");
                if d.is_empty() {
                    id.clone()
                } else {
                    d
                }
            },
            tag: g("tag"),
            endpoint_name: {
                let n = g("endpoint_name");
                if n.is_empty() {
                    "Models".into()
                } else {
                    n
                }
            },
            endpoint_id: g("endpoint_id"),
            endpoint_url: g("endpoint_url"),
            category: g("category"),
            id,
        })
    }
    pub fn display_with_tag(&self) -> String {
        if self.tag.is_empty() {
            self.display.clone()
        } else {
            format!("{} ({})", self.display, self.tag)
        }
    }

    /// Flatten the backend's `/api/models` response — `{"items":[{endpoint...,
    /// "models":[name,...]}]}` — into one Model per (endpoint, model). Mirrors
    /// the Swift `models()` flattening. Falls back to a flat array/`models` key.
    pub fn flatten_response(v: &Value) -> Vec<Model> {
        let mut out = vec![];
        if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
            for item in items {
                let ep_name = item
                    .get("endpoint_name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("Endpoint");
                let url = item.get("url").and_then(|x| x.as_str()).unwrap_or("");
                let ep_id = item
                    .get("endpoint_id")
                    .and_then(|x| x.as_str())
                    .unwrap_or(url);
                let category = item
                    .get("category")
                    .and_then(|x| x.as_str())
                    .unwrap_or("cloud");
                let tag = match category {
                    "local" => item
                        .get("endpoint_name")
                        .and_then(|x| x.as_str())
                        .and_then(|n| n.split('—').nth(1))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                if let Some(models) = item.get("models").and_then(|x| x.as_array()) {
                    for m in models.iter().filter_map(|x| x.as_str()) {
                        out.push(Model {
                            id: format!("{ep_id}:{m}"),
                            model_id: m.to_string(),
                            display: m.to_string(),
                            tag: tag.clone(),
                            endpoint_name: ep_name.to_string(),
                            endpoint_id: ep_id.to_string(),
                            endpoint_url: url.to_string(),
                            category: category.to_string(),
                        });
                    }
                }
            }
            return out;
        }
        let arr = v
            .as_array()
            .cloned()
            .or_else(|| v.get("models").and_then(|x| x.as_array()).cloned())
            .unwrap_or_default();
        arr.iter().filter_map(Model::from_value).collect()
    }
}

/// A document open in the editor pane (mirrors Swift EditorDoc, trimmed).
#[derive(Clone, Default)]
pub struct EditorDoc {
    pub id: String,
    pub title: String,
    #[allow(dead_code)]
    pub language: String,
    pub content: String,
    pub dirty: bool,
    pub ai_writing: bool,
    pub pending_ai: Option<String>,
    pub spreadsheet: bool,
    pub flashcards: bool,
    pub remote_sub: Option<String>,
    pub remote_session: Option<String>,
    pub local_path: Option<String>,
    pub is_binary: bool, // pdf/image — download+view only
    pub raw_bytes: Option<Vec<u8>>,
}

/// A background job: run `f(api)` on a thread, deliver `T` back on a channel.
/// Panels keep the `Job` and poll `take()` each frame.
pub struct Job<T> {
    rx: Receiver<T>,
    pub done: bool,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn(
        api: Arc<Api>,
        ctx: egui::Context,
        f: impl FnOnce(&Api) -> T + Send + 'static,
    ) -> Job<T> {
        let (tx, rx): (Sender<T>, Receiver<T>) = channel();
        std::thread::spawn(move || {
            let out = f(&api);
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        Job { rx, done: false }
    }
    /// Non-blocking: Some(result) once, then None.
    pub fn take(&mut self) -> Option<T> {
        if self.done {
            return None;
        }
        match self.rx.try_recv() {
            Ok(v) => {
                self.done = true;
                Some(v)
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.done = true;
                None
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
        }
    }
}

/// State every panel can read/mutate. Cross-panel concerns live here; panel-
/// local state lives in the panel struct.
pub struct Shared {
    pub api: Arc<Api>,
    pub ctx: egui::Context,
    pub palette: Palette,
    pub aero: bool,
    pub dark: bool,

    pub models: Vec<Model>,
    pub prefs: Value,

    /// The chat currently open (drives the editor's workdir + doc routing).
    pub current_sid: Option<String>,
    /// Name of the open chat (kept in sync by the sidebar, shown in the chat header).
    pub current_title: Option<String>,
    /// The selected model for new/typed messages.
    pub selected_model: Option<Model>,

    /// Editor pane.
    pub open_docs: Vec<EditorDoc>,
    pub active_doc: Option<String>,
    pub editor_visible: bool,
    pub editor_popped_out: bool,
    pub require_doc_approval: bool,
    /// Settings asked to switch backend: (url, token). Applied by the app.
    pub pending_connection: Option<(String, String)>,

    /// Transient user-facing error banner.
    pub toast: Option<String>,
    pub unread_email: i64,
    pub research_running: bool,

    /// Signal to sibling panels that data changed (e.g. a new chat was made).
    pub reload_sessions: bool,
    /// Re-fetch the model list (after endpoint/visibility edits).
    pub reload_models: bool,
    pub reload_workdir: bool,
    /// In-app file/folder chooser (native dialogs can fail silently on Wayland).
    pub picker: crate::pickers::LocalPicker,
    pub picked: Vec<(String, std::path::PathBuf)>,
    /// A file the sidebar asked the editor to open: (sid, sub).
    pub open_request: Option<(String, String)>,
    /// Chat-header sidebar toggle (Mac parity).
    pub sidebar_hidden: bool,
    /// A panel asking the app to switch tabs (e.g. Agents → "Open chat").
    pub goto_tab: Option<Tab>,
    /// Any agent run is queued/running (sidebar spinner on the Agents tab).
    pub agents_running: bool,
    /// Composer settings synced through the backend (PC / Mac / iOS).
    pub composer: ComposerSettings,
    /// Bumped whenever `composer` changes from the backend/cache, so the chat
    /// panel knows to re-read it.
    pub composer_rev: u64,
    /// The user changed `composer` locally; the app pushes it to the backend.
    pub composer_dirty: bool,

    // ---- shell (Mac AppStore / MainSplitView state) ----
    /// Command palette open (Ctrl+K).
    pub palette_open: bool,
    /// Ctrl+N / palette "New chat": the sidebar creates a chat and the app switches to Chats.
    pub new_chat_requested: bool,
    /// Name for the next chat the sidebar creates (first message, like the Mac).
    pub new_chat_title: Option<String>,
    /// Settings window open request (Ctrl+, / palette): the app opens it.
    pub settings_requested: bool,
    /// Chats known to the sidebar (for the palette), newest first as the backend orders them.
    pub sessions: Vec<SessionRef>,
    /// Unread notifications (sidebar dot on Notifications).
    pub inbox_unread: i64,
    /// Plugins installed on the backend (Tools / Plugins panels).
    pub plugins: Vec<crate::models::Plugin>,
    pub agent_nav: AgentNav,
    /// "New coding agent" sheet requested (sidebar / Agents panel); `panels::coding_agents` shows it.
    pub new_coding_agent_requested: bool,
}

/// Agent vs chat, think vs fast, which tools are on. One record on the
/// backend (`/api/client-settings`), shared by every JoeBro client, with a
/// local JSON cache so the PC opens with the last-known values when offline.
#[derive(Clone, Debug, PartialEq)]
pub struct ComposerSettings {
    pub agent: bool,
    pub think: bool,
    pub web: bool,
    pub term: bool,
    pub perm: String,
    pub updated_at: String,
}

impl Default for ComposerSettings {
    fn default() -> Self {
        // Same defaults as the backend and the Mac app.
        ComposerSettings {
            agent: false,
            think: true,
            web: true,
            term: false,
            perm: "sandbox".into(),
            updated_at: String::new(),
        }
    }
}

impl ComposerSettings {
    pub fn from_value(v: &Value) -> Option<ComposerSettings> {
        let o = v.as_object()?;
        let d = ComposerSettings::default();
        let b = |k: &str, def: bool| o.get(k).and_then(Value::as_bool).unwrap_or(def);
        let perm = o
            .get("permission_mode")
            .and_then(Value::as_str)
            .filter(|p| ["sandbox", "readonly", "full"].contains(p))
            .unwrap_or("sandbox")
            .to_string();
        Some(ComposerSettings {
            agent: o.get("mode").and_then(Value::as_str) == Some("agent"),
            think: b("thinking", d.think),
            web: b("web_search", d.web),
            term: b("terminal", d.term),
            perm,
            updated_at: o
                .get("updated_at")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
    }
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "mode": if self.agent { "agent" } else { "chat" },
            "thinking": self.think,
            "web_search": self.web,
            "terminal": self.term,
            "permission_mode": self.perm,
            "updated_at": self.updated_at,
            "client": "pc",
        })
    }
    /// Same values, ignoring the timestamp.
    pub fn same_values(&self, o: &ComposerSettings) -> bool {
        self.agent == o.agent
            && self.think == o.think
            && self.web == o.web
            && self.term == o.term
            && self.perm == o.perm
    }
    fn cache_path() -> std::path::PathBuf {
        crate::config::dir().join("composer-settings.json")
    }
    pub fn load_cache() -> Option<ComposerSettings> {
        let text = std::fs::read_to_string(Self::cache_path()).ok()?;
        Self::from_value(&serde_json::from_str(&text).ok()?)
    }
    pub fn save_cache(&self) {
        let path = Self::cache_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, self.to_value().to_string());
    }
}

impl Shared {
    pub fn new(api: Arc<Api>, ctx: egui::Context, aero: bool, dark: bool) -> Shared {
        Shared {
            api,
            ctx,
            palette: Palette::for_state(aero, dark),
            aero,
            dark,
            models: vec![],
            prefs: Value::Null,
            current_sid: None,
            current_title: None,
            selected_model: None,
            open_docs: vec![],
            active_doc: None,
            editor_visible: false,
            editor_popped_out: false,
            require_doc_approval: true,
            pending_connection: None,
            toast: None,
            unread_email: 0,
            research_running: false,
            reload_sessions: false,
            reload_models: false,
            reload_workdir: false,
            picker: Default::default(),
            picked: vec![],
            open_request: None,
            sidebar_hidden: false,
            goto_tab: None,
            agents_running: false,
            composer: ComposerSettings::load_cache().unwrap_or_default(),
            composer_rev: 1,
            composer_dirty: false,
            palette_open: false,
            new_chat_requested: false,
            new_chat_title: None,
            settings_requested: false,
            sessions: vec![],
            inbox_unread: 0,
            plugins: vec![],
            agent_nav: AgentNav::default(),
            new_coding_agent_requested: false,
        }
    }

    pub fn spawn<T: Send + 'static>(&self, f: impl FnOnce(&Api) -> T + Send + 'static) -> Job<T> {
        Job::spawn(self.api.clone(), self.ctx.clone(), f)
    }

    /// Open a normal chat: leaves any coding agent (Mac `selectedSessionID` clears the agent).
    pub fn select_chat(&mut self, id: &str) {
        self.agent_nav.close();
        self.current_sid = Some(id.to_string());
    }

    /// Collect the result of a picker opened with `tag` (once).
    pub fn take_picked(&mut self, tag: &str) -> Option<std::path::PathBuf> {
        let i = self.picked.iter().position(|(t, _)| t == tag)?;
        Some(self.picked.remove(i).1)
    }

    pub fn err(&mut self, msg: impl Into<String>) {
        self.toast = Some(msg.into());
    }

    /// Endpoint-grouped models, local machines first — the picker ordering
    /// shared by every model dropdown (mirrors ModelMenuItems.grouped).
    pub fn grouped_models(&self) -> Vec<(String, Vec<Model>)> {
        let mut order: Vec<String> = vec![];
        let mut buckets: std::collections::HashMap<String, Vec<Model>> = Default::default();
        for m in &self.models {
            if !buckets.contains_key(&m.endpoint_name) {
                order.push(m.endpoint_name.clone());
            }
            buckets
                .entry(m.endpoint_name.clone())
                .or_default()
                .push(m.clone());
        }
        order.sort_by(|a, b| {
            let la = buckets[a]
                .first()
                .map(|m| m.category == "local")
                .unwrap_or(false);
            let lb = buckets[b]
                .first()
                .map(|m| m.category == "local")
                .unwrap_or(false);
            match (la, lb) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.cmp(b),
            }
        });
        order
            .into_iter()
            .map(|k| (k.clone(), buckets.remove(&k).unwrap()))
            .collect()
    }
}

/// The workspace tabs (`WorkspaceTab` in Models.swift).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Chats,
    Email,
    Calendar,
    /// Legacy personal-memory list/graph (`panels/brain.rs`). The Mac routes `.brain`
    /// and `.memory` to the same Shared Memory panel; this stays until that panel
    /// absorbs it, and is not in the sidebar.
    Brain,
    Memory,
    NightShift,
    Notes,
    Agents,
    Tasks,
    Skills,
    Tools,
    Plugins,
    AICheck,
    Research,
    Inbox,
}

impl Tab {
    #[allow(dead_code)]
    pub const ALL: [Tab; 15] = [
        Tab::Chats, Tab::Email, Tab::Calendar, Tab::Brain, Tab::Memory, Tab::NightShift, Tab::Notes,
        Tab::Agents, Tab::Tasks, Tab::Skills, Tab::Tools, Tab::Plugins, Tab::AICheck, Tab::Research, Tab::Inbox,
    ];
    /// The panel order behind Ctrl+1..9 (`JBPanelOrder.jumpTabs`).
    pub const JUMP: [Tab; 9] = [
        Tab::Chats, Tab::Email, Tab::Calendar, Tab::Memory, Tab::Agents, Tab::NightShift, Tab::Tasks, Tab::Skills, Tab::Tools,
    ];
    /// Sidebar "Workspace" rows, in order (`SidebarView`).
    pub const WORKSPACE: [Tab; 11] = [
        Tab::Email, Tab::Calendar, Tab::Memory, Tab::Agents, Tab::NightShift, Tab::Tasks, Tab::Skills, Tab::Tools,
        Tab::Research, Tab::Inbox, Tab::AICheck,
    ];
    /// Palette order (`JBPanelOrder.paletteTabs`).
    pub fn palette_tabs() -> Vec<Tab> {
        let mut v = Tab::JUMP.to_vec();
        v.extend([Tab::Notes, Tab::Research, Tab::Inbox, Tab::AICheck]);
        v
    }
    /// "Ctrl+2" style shortcut for the jump tabs.
    pub fn shortcut(&self) -> Option<String> {
        Tab::JUMP.iter().position(|t| t == self).map(|i| crate::widgets::shortcut_label(&(i + 1).to_string()))
    }
    pub fn label(&self) -> &'static str {
        match self {
            Tab::Chats => "Chats",
            Tab::Email => "Email",
            Tab::Calendar => "Calendar",
            Tab::Brain => "Brain",
            Tab::Memory => "Memory",
            Tab::NightShift => "Night Shift",
            Tab::Notes => "Notes",
            Tab::Agents => "Agents",
            Tab::Tasks => "Tasks",
            Tab::Skills => "Skills",
            Tab::Tools => "Tools",
            Tab::Plugins => "Plugins",
            Tab::AICheck => "AI Check",
            Tab::Research => "Deep Research",
            Tab::Inbox => "Notifications",
        }
    }
    /// SF Symbol name (resolve with `icons::symbol` / `icons::sf_text`).
    pub fn icon(&self) -> &'static str {
        match self {
            Tab::Chats => "bubble.left.and.bubble.right",
            Tab::Email => "envelope",
            Tab::Calendar => "calendar",
            Tab::Brain | Tab::Memory => "brain.head.profile",
            Tab::NightShift => "moon.stars",
            Tab::Notes => "note.text",
            Tab::Agents => "person.3.sequence",
            Tab::Tasks => "clock.badge.checkmark",
            Tab::Skills => "graduationcap",
            Tab::Tools => "wrench.and.screwdriver",
            Tab::Plugins => "puzzlepiece.extension",
            Tab::AICheck => "checkmark.shield",
            Tab::Research => "antenna.radiowaves.left.and.right",
            Tab::Inbox => "tray",
        }
    }
}

/// A chat as the sidebar knows it, published for the palette (`Shared::sessions`).
#[derive(Clone, Debug, Default)]
pub struct SessionRef {
    pub id: String,
    pub name: String,
}

/// Coding agents shown in the sidebar and opened as conversations (`AgentNav` in CodingAgentConversation.swift).
#[derive(Default)]
pub struct AgentNav {
    /// The coding agent open in the chat area (replaces ChatView), if any.
    pub open_id: Option<String>,
    /// Top-level coding agents still open / live / waiting.
    pub runs: Vec<crate::models::AgentRun>,
    pub load: Option<Job<crate::api::R<crate::models::AgentsEnvelope>>>,
    pub timer: f64,
}

impl AgentNav {
    pub fn open(&mut self, id: &str) {
        self.open_id = Some(id.to_string());
    }
    pub fn close(&mut self) {
        self.open_id = None;
    }
}

/// Remember the user's model on this device, independent of backend defaults.
pub fn saved_model_id() -> String {
    let path = crate::config::dir().join("selected-model");
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}
pub fn remember_model(model: &Model) {
    let dir = crate::config::dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("selected-model"), &model.id);
    }
}

#[cfg(test)]
mod nav_tests {
    use super::*;

    #[test]
    fn chat_agent_chat_switches_back() {
        let api = Arc::new(crate::api::Api::new("http://127.0.0.1:9"));
        let mut sh = Shared::new(api, egui::Context::default(), true, true);
        sh.select_chat("s1");
        assert!(sh.agent_nav.open_id.is_none());
        sh.agent_nav.open("agt_1");
        assert_eq!(sh.agent_nav.open_id.as_deref(), Some("agt_1"));
        sh.select_chat("s2");
        assert!(sh.agent_nav.open_id.is_none());
        assert_eq!(sh.current_sid.as_deref(), Some("s2"));
    }
}
