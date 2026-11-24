//! Chat panel: transcript + streaming composer. Mirrors the macOS chat view.
use crate::api::R;
use crate::state::{EditorDoc, Job, Shared};
use crate::panels::attachments::{Att, Attachments};
use crate::widgets;
use eframe::egui::{self, Align, Key, Layout, RichText, ScrollArea, Ui};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use serde_json::Value;
use std::sync::mpsc::{channel, Receiver};

#[path = "chat_ui.rs"]
pub(crate) mod cui;

/// One ordered piece of an assistant turn. Text the model streams and the tool
/// calls it makes are kept in the order they arrived so they render inline —
/// prose, then the tool it triggered, then the prose after it — instead of all
/// the tool cards bunching at the end.
#[derive(Clone)]
enum Seg {
    Text(String),
    Tool(String, String, i64, String), // tool, command, exit_code
}

/// What informed a reply, by category (the backend's `*_used` events and the
/// same keys in persisted message metadata).
#[derive(Default, Clone)]
struct Used {
    memories: Vec<String>,
    skills: Vec<String>,
    plugins: Vec<String>,
}

impl Used {
    fn bucket(&mut self, key: &str) -> Option<&mut Vec<String>> {
        match key {
            "memories_used" => Some(&mut self.memories),
            "skills_used" => Some(&mut self.skills),
            "plugins_used" => Some(&mut self.plugins),
            _ => None,
        }
    }
    fn push(&mut self, key: &str, item: &str) {
        if let Some(b) = self.bucket(key) {
            if !item.trim().is_empty() && !b.iter().any(|x| x == item) {
                b.push(item.to_string());
            }
        }
    }
}

#[derive(Default, Clone)]
struct Msg {
    db_id: Option<String>,
    role: String,
    content: String, // full text, concatenated — used for copy + persistence
    thinking: String,
    segs: Vec<Seg>,    // text + tool calls, in stream order
    used: Used, // memories/skills/plugins that informed this reply
    metrics: String,
    ctx: f64,
    model: String,
    atts: Vec<Att>,
}

impl Msg {
    /// Append streamed text to both the flat `content` and the trailing text
    /// segment (opening a new one if a tool card was the last thing emitted).
    fn push_text(&mut self, s: &str) {
        self.content.push_str(s);
        match self.segs.last_mut() {
            Some(Seg::Text(t)) => t.push_str(s),
            _ => self.segs.push(Seg::Text(s.to_string())),
        }
    }
}

#[derive(Default)]
pub struct ChatPanel {
    sid: Option<String>,
    /// Sent from the no-chat composer: the session is being created, send once it opens (Mac `ensureSession`).
    deferred_send: bool,
    msgs: Vec<Msg>,
    load: Option<Job<R<Value>>>,
    input: String,
    agent: bool,
    think: bool,
    web: bool,
    term: bool,
    perm_idx: usize, // 0 sandbox, 1 readonly, 2 full
    streaming: bool,
    stream_rx: Option<Receiver<Value>>,
    perm_prompt: Option<(String, String, String)>, // rid, tool, command
    md: CommonMarkCache,
    renaming: bool,
    rename_text: String,
    rename_focus: bool, // one-shot: grab the caret only on the first frame
    action: Option<Job<R<Value>>>,
    background_actions: Vec<Job<R<Value>>>,
    uploads: Vec<Job<R<Uploaded>>>,
    attachments: Vec<Att>,
    files: Attachments,
    /// Transcript paging: render only the newest N messages (Mac: 60, +100).
    visible: usize,
    /// Jump the transcript to the bottom on the next frame.
    to_bottom: bool,
    near_bottom: bool,
    load_error: Option<String>,
    doc_open: Option<Job<R<Value>>>,
    recording: Option<crate::voice::Recording>,
    transcription: Option<Job<Result<String, String>>>,
    tip: Option<Job<R<Value>>>,
    tip_timer: f32,
    last_message_id: i64,
    streamed_doc: Option<String>,
    open_picker: bool,
    export: Option<Job<Result<String, String>>>,
    /// Last `Shared::composer_rev` adopted into agent/think/web/term/perm.
    composer_rev: u64,
    /// The next history load is a fresh chat (not a background refresh), so
    /// it may adopt the chat's mode / model / access level.
    adopt_session: bool,
    slash: cui::SlashSource,
    slash_idx: usize,
    slash_dismissed: bool,
    slash_scroll: bool,
}

/// Upload reply + the bytes we sent (seeded into the attachment cache).
type Uploaded = (Value, Vec<u8>);

const PERMS: [(&str, &str); 3] = [
    ("sandbox", "Bound folder"),
    ("readonly", "Read-only"),
    ("full", "Full access"),
];

impl ChatPanel {
    /// Snapshot harness: a turn in flight (user asked, model still thinking).
    #[cfg(feature = "snapshot")]
    pub fn demo_streaming(&mut self) {
        self.msgs.push(Msg {
            role: "user".into(),
            content: "Now draft the conclusion in my voice.".into(),
            segs: vec![Seg::Text("Now draft the conclusion in my voice.".into())],
            ..Default::default()
        });
        self.msgs.push(Msg {
            role: "assistant".into(),
            model: "deepseek-chat".into(),
            ..Default::default()
        });
        self.streaming = true;
    }

    /// Snapshot harness: type "/" so the slash menu is showing.
    #[cfg(feature = "snapshot")]
    pub fn demo_slash(&mut self) {
        self.input = "/".into();
    }

    /// Snapshot harness: open the composer's model popover.
    #[cfg(feature = "snapshot")]
    pub fn demo_open_picker(&mut self, _ctx: &egui::Context) {
        self.open_picker = true;
    }

    /// Snapshot harness: open the first reply's "memories used" popover.
    #[cfg(feature = "snapshot")]
    pub fn demo_open_used(&mut self, ctx: &egui::Context) {
        let id = egui::Id::new(("used_chip", "12", "memories")).with("used_popover");
        ctx.memory_mut(|m| m.open_popup(id));
    }

    /// Snapshot harness: open the preview sheet for attachment `id`.
    #[cfg(feature = "snapshot")]
    pub fn demo_preview(&mut self, id: &str, ctx: &egui::Context) {
        if let Some(att) = self.msgs.iter().flat_map(|m| m.atts.iter()).find(|a| a.id == id).cloned() {
            self.files.demo_open(ctx, &att);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        while let Some(path) = sh.take_picked("chat_attach") {
            self.queue_upload(path, sh);
        }
        if let Some(path) = sh.take_picked("chat_pdf") {
            let title = sh.current_title.clone().unwrap_or_else(|| "New Chat".into());
            let body = self.chat_text();
            self.export = Some(sh.spawn(move |a| {
                let bytes = a.chat_pdf(&title, &body)?;
                std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(format!("Exported {}", path.display()))
            }));
        }
        #[cfg(feature = "snapshot")]
        if let Ok(t) = std::env::var("JOEBRO_SNAP_INPUT") {
            if self.input.is_empty() {
                self.input = t;
            }
        }
        // Adopt synced composer settings (backend / cache / another device).
        if self.composer_rev != sh.composer_rev {
            self.composer_rev = sh.composer_rev;
            let c = &sh.composer;
            self.agent = c.agent;
            self.think = c.think;
            self.web = c.web;
            self.term = c.term;
            self.perm_idx = PERMS.iter().position(|(id, _)| *id == c.perm).unwrap_or(0);
        }
        // Adopt the open chat + load its history when it changes.
        if sh.current_sid != self.sid {
            self.sid = sh.current_sid.clone();
            self.msgs.clear();
            let kept_attachments = if self.deferred_send { std::mem::take(&mut self.attachments) } else { Vec::new() };
            self.attachments.clear();
            self.visible = PAGE;
            self.to_bottom = true;
            self.load_error = None;
            // Mac: the composer is first responder when a chat opens — but
            // never steal the caret from another text field (sidebar rename).
            if !ui.ctx().wants_keyboard_input() {
                ui.memory_mut(|m| m.request_focus(egui::Id::new("composer_input")));
            }
            self.uploads.clear();
            self.perm_prompt = None;
            self.tip = None;
            self.last_message_id = 0;
            self.tip_timer = 0.0;
            self.streaming = false;
            self.stream_rx = None;
            self.adopt_session = true;
            // Start from the synced settings; a chat with history then
            // restores its own mode / access when it loads.
            self.composer_rev = 0;
            if self.deferred_send && self.sid.is_some() {
                self.deferred_send = false;
                self.attachments = kept_attachments;
                self.load = None;
                self.send(sh);
            } else if let Some(sid) = self.sid.clone() {
                self.load = Some(sh.spawn(move |a| a.history(&sid)));
            }
        }
        if let Some(job) = &mut self.load {
            if let Some(res) = job.take() {
                match res {
                    Ok(v) => {
                        self.load_error = None;
                        self.msgs = parse_history(&v);
                        self.last_message_id = self
                            .msgs
                            .iter()
                            .filter_map(|m| m.db_id.as_ref()?.parse::<i64>().ok())
                            .max()
                            .unwrap_or(0);
                        // Only when the chat is first opened: the 3 s tip poll and
                        // post-stream refreshes reload history too, and must not
                        // undo a mode / model / access pick made since (that's
                        // what made the composer's picks "snap back").
                        // A brand-new (empty) chat keeps the synced composer
                        // settings instead of the backend's per-chat defaults.
                        let adopt = std::mem::take(&mut self.adopt_session) && !self.msgs.is_empty();
                        if adopt {
                            self.agent = v.get("mode").and_then(Value::as_str) == Some("agent");
                        }
                        if let Some(permission) = v.get("permission_mode").and_then(Value::as_str).filter(|_| adopt) {
                            self.perm_idx = PERMS
                                .iter()
                                .position(|(id, _)| *id == permission)
                                .unwrap_or(0);
                        }
                        if let Some(model) = v.get("model").and_then(Value::as_str).filter(|_| adopt) {
                            let ep = v.get("endpoint_id").and_then(Value::as_str).unwrap_or("");
                            if let Some(m) = sh.models.iter().find(|m| {
                                m.model_id == model && (ep.is_empty() || m.endpoint_id == ep)
                            }) {
                                sh.selected_model = Some(m.clone());
                            }
                        }
                    }
                    Err(e) => {
                        if self.msgs.is_empty() {
                            self.load_error = Some(e);
                        } else {
                            sh.err(format!("Load conversation: {e}"));
                        }
                    }
                }
                self.load = None;
            }
        }
        // A header action (compact) finished — reload the (now shorter) history.
        if let Some(job) = &mut self.action {
            if let Some(result) = job.take() {
                if let Err(e) = result {
                    sh.err(e);
                }
                self.action = None;
                if let Some(sid) = self.sid.clone() {
                    self.load = Some(sh.spawn(move |a| a.history(&sid)));
                }
            }
        }
        self.pump_jobs(sh);
        if let Some(job) = &mut self.doc_open {
            if let Some(res) = job.take() {
                match res {
                    Ok(d) => {
                        let ev = serde_json::json!({
                            "type": "doc_update",
                            "doc_id": d.get("id").cloned().unwrap_or(Value::Null),
                            "title": d.get("title").cloned().unwrap_or(Value::Null),
                            "content": d.get("content").cloned().unwrap_or(Value::Null),
                            "language": d.get("language").cloned().unwrap_or(Value::Null),
                        });
                        let mut streamed = None;
                        apply_doc_event(&ev, sh, &mut streamed);
                        sh.editor_visible = true;
                    }
                    Err(e) => sh.err(format!("Open document: {e}")),
                }
                self.doc_open = None;
            }
        }
        self.tip_timer -= ui.input(|i| i.stable_dt);
        if !self.streaming && self.load.is_none() && self.tip.is_none() && self.tip_timer <= 0.0 {
            if let Some(sid) = self.sid.clone() {
                self.tip = Some(sh.spawn(move |a| a.session_tip(&sid)));
            }
            self.tip_timer = 3.0;
        }
        if let Some(job) = &mut self.tip {
            if let Some(result) = job.take() {
                if let Ok(value) = result {
                    let last = value.get("last_id").and_then(Value::as_i64).unwrap_or(0);
                    if last != self.last_message_id && !self.streaming {
                        if let Some(sid) = self.sid.clone() {
                            self.load = Some(sh.spawn(move |a| a.history(&sid)));
                        }
                    }
                }
                self.tip = None;
            }
        }
        self.pump_stream(sh);
        self.files.preview_ui(ui.ctx(), sh);

        if self.sid.is_none() {
            // Mac: header + banner + composer even with no chat; the first send creates the session.
            egui::TopBottomPanel::top("chat_header")
                .show_separator_line(false)
                .frame(egui::Frame::none().inner_margin(egui::Margin { left: 0.0, right: 0.0, top: 0.0, bottom: 12.0 }))
                .show_inside(ui, |ui| self.header(ui, sh));
            let h = ui.available_height();
            ui.add_space(((h - 250.0) / 2.0).max(0.0));
            empty_state(ui, sh, "What should we work on today?");
            ui.add_space(28.0);
            self.composer_column(ui, sh);
            self.drop_overlay(ui, sh, ui.max_rect());
            return;
        }

        // Header card pinned to the top (12 pt gap below, like the Mac VStack).
        egui::TopBottomPanel::top("chat_header")
            .show_separator_line(false)
            .frame(egui::Frame::none().inner_margin(egui::Margin { left: 0.0, right: 0.0, top: 0.0, bottom: 12.0 }))
            .show_inside(ui, |ui| self.header(ui, sh));

        let blank = self.msgs.is_empty() && self.load.is_none() && self.load_error.is_none();

        if let Some((rid, tool, cmd)) = self.perm_prompt.clone() {
            self.permission_prompt(ui, sh, rid, tool, cmd);
        }

        if blank {
            // Empty chat: wordmark + composer, centred together.
            let h = ui.available_height();
            ui.add_space(((h - 250.0) / 2.0).max(0.0));
            empty_state(ui, sh, "What should we work on today?");
            ui.add_space(28.0);
            self.composer_column(ui, sh);
            self.drop_overlay(ui, sh, ui.max_rect());
            return;
        }

        // Composer pinned to the bottom; transcript fills the rest.
        egui::TopBottomPanel::bottom("composer")
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::none().inner_margin(egui::Margin { left: 0.0, right: 0.0, top: 12.0, bottom: 0.0 }))
            .show_inside(ui, |ui| self.composer_column(ui, sh));

        if self.msgs.is_empty() {
            if let Some(e) = self.load_error.clone() {
                crate::pdfview::state_message(ui, sh, "\u{f071}", "Couldn't load this chat", &e);
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    if widgets::button_ex(ui, &sh.palette, widgets::BtnKind::Secondary, true, true, "Try again").clicked() {
                        self.load_error = None;
                        if let Some(sid) = self.sid.clone() {
                            self.load = Some(sh.spawn(move |a| a.history(&sid)));
                        }
                    }
                });
            } else {
                widgets::panel_loading(ui, &sh.palette);
            }
            self.drop_overlay(ui, sh, ui.max_rect());
            return;
        }

        let whole = ui.max_rect();
        let msgs = std::mem::take(&mut self.msgs);
        let n = msgs.len();
        let first = n.saturating_sub(self.visible.max(PAGE));
        let streaming = self.streaming;
        let jump = std::mem::take(&mut self.to_bottom);
        let mut actions: Vec<MessageAction> = vec![];
        let mut more = false;
        let files = &mut self.files;
        let md = &mut self.md;
        let out = ScrollArea::vertical()
            .id_salt(("transcript", self.sid.clone()))
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                // A centred reading column (Mac: max 860 pt).
                let full = ui.available_width();
                let col = full.min(MAX_COLUMN) - 36.0;
                ui.horizontal(|ui| {
                    ui.add_space(((full - col) / 2.0).floor().max(0.0));
                    ui.vertical(|ui| {
                        ui.set_width(col);
                        ui.add_space(8.0);
                        if first > 0 {
                            ui.vertical_centered(|ui| {
                                let r = ui
                                    .horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;
                                        crate::icons::symbol(ui, "arrow.up", 11.0, sh.palette.ink2);
                                        ui.label(RichText::new(format!("{first} earlier")).font(crate::theme::jb(11.5, crate::theme::W::Regular)).color(sh.palette.ink2));
                                    })
                                    .response
                                    .interact(egui::Sense::click())
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if r.clicked() {
                                    more = true;
                                }
                            });
                            ui.add_space(6.0);
                        }
                        for (i, m) in msgs.iter().enumerate().skip(first) {
                            if m.role == "compacted" {
                                compacted_pill(ui, sh);
                                continue;
                            }
                            let live = streaming && i + 1 == n && m.role == "assistant";
                            if let Some(a) = message_view(ui, sh, m, md, files, i, live) {
                                actions.push(a);
                            }
                        }
                        if jump {
                            ui.scroll_to_cursor(Some(Align::BOTTOM));
                        }
                    });
                });
            });
        self.msgs = msgs;
        if more {
            self.visible = self.visible.max(PAGE) + 100;
        }
        for a in actions {
            match a {
                MessageAction::Edit(text) => {
                    self.input = text;
                    ui.memory_mut(|m| m.request_focus(egui::Id::new("composer_input")));
                }
                MessageAction::Resend(text) => {
                    self.input = text;
                    ui.memory_mut(|m| m.request_focus(egui::Id::new("composer_input")));
                }
                MessageAction::Unsend(id) => {
                    if let Some(sid) = self.sid.clone() {
                        self.action = Some(sh.spawn(move |a| a.delete_messages(&sid, &[id])));
                    }
                }
                MessageAction::Open(target) => self.open_workspace(sh, &target),
            }
        }
        // "Jump to latest" — appears once the user has scrolled away.
        let view = out.inner_rect;
        self.near_bottom = out.state.offset.y + view.height() >= out.content_size.y - 80.0;
        let show = ui.ctx().animate_bool_with_time(egui::Id::new("jump_latest"), !self.near_bottom, 0.18);
        if show > 0.01 {
            let p = sh.palette;
            let c = egui::pos2(view.center().x, view.bottom() - 27.0 + 4.0 * (1.0 - show));
            let rect = egui::Rect::from_center_size(c, egui::vec2(30.0, 30.0));
            ui.painter().add(egui::Shape::rect_filled(rect.expand(1.0), egui::Rounding::same(crate::theme::R_CTRL + 2.0), p.line_strong.gamma_multiply(show)));
            ui.painter().add(egui::Shape::rect_filled(rect, egui::Rounding::same(crate::theme::R_CTRL + 2.0), p.surface.gamma_multiply(show)));
            let resp = ui.put(rect, |ui: &mut Ui| {
                ui.set_opacity(show);
                widgets::icon_btn(ui, &p, "arrow.down", 30.0, false).on_hover_text("Jump to latest")
            });
            if resp.clicked() {
                self.to_bottom = true;
            }
        }
        self.drop_overlay(ui, sh, whole);
    }

    /// Workspace links in a reply (`[label](research-…)` etc.) — the Mac
    /// WorkspaceActionButtons.
    fn open_workspace(&mut self, sh: &mut Shared, target: &str) {
        use crate::state::Tab;
        let id = |p: &str| target.strip_prefix(p).map(str::to_string);
        if let Some(sid) = id("session-") {
            sh.current_sid = Some(sid);
        } else if let Some(did) = id("document-") {
            self.doc_open = Some(sh.spawn(move |a| a.get(&format!("api/document/{did}"))));
        } else if target.starts_with("research-") {
            sh.goto_tab = Some(Tab::Research);
        } else if target.starts_with("event-") {
            sh.goto_tab = Some(Tab::Calendar);
        } else if target.starts_with("task-") {
            sh.goto_tab = Some(Tab::Tasks);
        } else if target.starts_with("note-") {
            sh.goto_tab = Some(Tab::Notes);
        }
    }

    /// "Drop to attach" veil while files are dragged over the window.
    fn drop_overlay(&self, ui: &Ui, sh: &Shared, rect: egui::Rect) {
        use crate::theme::{self, W};
        if ui.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let p = sh.palette;
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop_veil")));
        painter.rect_filled(rect, egui::Rounding::same(theme::R_PANEL), p.surface.gamma_multiply(0.94));
        let inset = rect.shrink(6.0);
        let stroke = egui::Stroke::new(1.5, p.accent_a(0.7));
        let pts = [inset.left_top(), inset.right_top(), inset.right_bottom(), inset.left_bottom(), inset.left_top()];
        painter.extend(egui::Shape::dashed_line(&pts, stroke, 6.0, 5.0));
        crate::icons::paint_symbol(&painter, rect.center() - egui::vec2(0.0, 22.0), "paperclip", 28.0, p.accent);
        painter.text(rect.center() + egui::vec2(0.0, 14.0), egui::Align2::CENTER_CENTER, "Drop to attach", theme::jb(12.0, W::Regular), p.ink2);
        ui.ctx().request_repaint();
    }

    fn composer_column(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let full = ui.available_width();
        let col = full.min(MAX_COLUMN);
        ui.horizontal(|ui| {
            ui.add_space(((full - col) / 2.0).floor().max(0.0));
            ui.vertical(|ui| {
                ui.set_width(col);
                self.composer(ui, sh);
            });
        });
    }

    /// `CommandPermissionPrompt`: approval card for a command the agent wants to run.
    fn permission_prompt(&mut self, ui: &mut Ui, sh: &Shared, rid: String, tool: String, cmd: String) {
        use crate::theme::{self, W};
        let p = sh.palette;
        let rect = ui.max_rect();
        let mut decision: Option<&str> = None;
        let doc = tool.contains("document");
        egui::Area::new(egui::Id::new("chat_perm_prompt"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(rect.center().x, rect.bottom() - 90.0))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .show(ui.ctx(), |ui| {
                widgets::floating_frame(&p, theme::R_CARD).stroke(egui::Stroke::new(1.0, p.warn.gamma_multiply(0.45))).inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
                    ui.set_width(352.0);
                    ui.spacing_mut().item_spacing.y = 10.0;
                    ui.horizontal(|ui| {
                        if doc {
                            crate::icons::symbol(ui, "pencil.line", 12.0, p.warn);
                        } else {
                            crate::icons::glyph(ui, crate::icons::JbGlyph::Coding, 14.0, p.warn);
                        }
                        let t = if doc { "Let the AI edit this file?".to_string() } else { format!("Run {tool} command?") };
                        ui.label(RichText::new(t).font(theme::jb(13.0, W::Medium)).color(p.ink));
                    });
                    widgets::surface(ui, &p, widgets::Level::Raised, Some(theme::R_CTRL), |ui| {
                        egui::Frame::none().inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(egui::Label::new(RichText::new(&cmd).font(theme::jb(12.0, W::Regular)).color(p.ink2)).selectable(true));
                        });
                    });
                    ui.horizontal(|ui| {
                        if widgets::button_ex(ui, &p, widgets::BtnKind::Ghost, true, true, "Deny").clicked() {
                            decision = Some("deny");
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::button_ex(ui, &p, widgets::BtnKind::Primary, true, true, "Allow").clicked() {
                                decision = Some("allow");
                            }
                            if widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, true, "Always allow this session").clicked() {
                                decision = Some("always");
                            }
                        });
                    });
                });
            });
        if let Some(d) = decision {
            let d = d.to_string();
            self.background_actions.push(sh.spawn(move |a| a.respond_permission(&rid, &d)));
            self.perm_prompt = None;
        }
    }

    /// Header card (Mac ChatView.header): sidebar toggle, title menu, message count, spinner,
    /// "show editor" affordance when docs are open.
    fn header(&mut self, ui: &mut Ui, sh: &mut Shared) {
        use crate::theme::{self, W};
        let p = sh.palette;
        if let Some(job) = &mut self.export {
            if let Some(res) = job.take() {
                match res {
                    Ok(msg) => sh.err(msg),
                    Err(e) => sh.err(e),
                }
                self.export = None;
            }
        }
        let slot = ui.painter().add(egui::Shape::Noop);
        let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(16.0, 10.0)).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if widgets::icon_btn(ui, &p, "sidebar.left", 26.0, false).on_hover_text("Toggle sidebar").clicked() {
                    sh.sidebar_hidden = !sh.sidebar_hidden;
                }
                self.title_menu(ui, sh);
                if !self.msgs.is_empty() {
                    ui.label(RichText::new(self.msgs.len().to_string()).font(theme::jb(11.0, W::Regular)).color(p.ink3))
                        .on_hover_text("Messages in this chat");
                }
                if self.action.is_some() || self.export.is_some() {
                    crate::ascii::spinner(ui, 12.0, p.ink2);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !sh.open_docs.is_empty() && !sh.editor_visible {
                        let r = ui
                            .horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                crate::icons::symbol(ui, "doc.text", 11.5, p.ink2);
                                ui.label(RichText::new(sh.open_docs.len().to_string()).font(theme::jb(11.5, W::Regular)).color(p.ink2));
                            })
                            .response
                            .interact(egui::Sense::click())
                            .on_hover_text("Show editor")
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if r.clicked() {
                            sh.editor_visible = true;
                        }
                    }
                });
            });
        });
        let r = inner.response.rect;
        let rr = egui::Rounding::same(theme::R_PANEL);
        ui.painter().set(
            slot,
            egui::Shape::Vec(vec![
                egui::Shape::rect_filled(r, rr, p.panel_fill),
                egui::Shape::rect_stroke(r.shrink(0.5), rr, egui::Stroke::new(1.0, p.line)),
            ]),
        );
    }

    fn chat_text(&self) -> String {
        self.msgs
            .iter()
            .filter(|m| m.role == "user" || m.role == "assistant")
            .map(|m| format!("**{}:** {}", if m.role == "user" { "You" } else { "JoeBro" }, m.content))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn do_compact(&mut self, sh: &Shared) {
        if let Some(sid) = self.sid.clone() {
            self.action = Some(sh.spawn(move |a| a.compact_session(&sid)));
        }
    }

    fn do_save(&mut self, sh: &Shared) {
        if self.export.is_none() && !self.msgs.is_empty() {
            let t = sh.current_title.clone().unwrap_or_else(|| "New Chat".into());
            let body = self.chat_text();
            self.export = Some(sh.spawn(move |a| {
                a.create_document(&t, &body, "markdown").map(|_| format!("Saved “{t}” to Documents."))
            }));
        }
    }

    fn do_pdf(&mut self, sh: &mut Shared) {
        if self.export.is_none() && !self.msgs.is_empty() {
            let title = sh.current_title.clone().unwrap_or_else(|| "New Chat".into());
            sh.picker.open_save("chat_pdf", "Export PDF", &format!("{}.pdf", crate::panels::attachments::safe_name(&title)), None);
        }
    }

    /// Title dropdown: Rename / Compact Chat / Copy Chat / PDF / Save to Documents.
    fn title_menu(&mut self, ui: &mut Ui, sh: &mut Shared) {
        use crate::theme::{self, W};
        let p = sh.palette;
        let title = sh.current_title.clone().unwrap_or_else(|| "New Chat".into());
        if self.renaming {
            let resp = ui.add(egui::TextEdit::singleline(&mut self.rename_text).desired_width(240.0).hint_text("Chat name"));
            if self.rename_focus {
                resp.request_focus();
                self.rename_focus = false;
            }
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            let esc = ui.input(|i| i.key_pressed(Key::Escape));
            let ok = widgets::button_ex(ui, &p, widgets::BtnKind::Primary, true, !self.rename_text.trim().is_empty(), "Rename").clicked();
            let cancel = widgets::button_ex(ui, &p, widgets::BtnKind::Ghost, true, true, "Cancel").clicked();
            let commit = (enter || ok) && !self.rename_text.trim().is_empty();
            if commit {
                if let Some(sid) = self.sid.clone() {
                    let txt = self.rename_text.clone();
                    self.action = Some(sh.spawn(move |a| a.rename_session(&sid, &txt)));
                    sh.reload_sessions = true;
                }
            }
            if commit || cancel || esc || (resp.lost_focus() && !ok && !cancel) {
                resp.surrender_focus();
                self.renaming = false;
            }
            return;
        }
        let g = ui.painter().layout_no_wrap(title.clone(), theme::jb(14.0, W::Medium), p.ink);
        let w = g.size().x + 5.0 + 8.0;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 26.0), egui::Sense::click());
        let gw = g.size().x;
        ui.painter().galley(egui::pos2(rect.left(), rect.center().y - g.size().y / 2.0), g, p.ink);
        crate::icons::paint_symbol(ui.painter(), egui::pos2(rect.left() + gw + 5.0 + 4.0, rect.center().y), "chevron.down", 8.0, p.ink3);
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let has = !self.msgs.is_empty();
        let (mut rename, mut compact, mut copy, mut pdf, mut save) = (false, false, false, false, false);
        let can_compact = self.msgs.len() >= 6;
        widgets::menu_popup(ui, &p, &resp, |ui| {
            if widgets::menu_item(ui, &p, Some("pencil"), "Rename", false, true).clicked() {
                rename = true;
                ui.memory_mut(|m| m.close_popup());
            }
            if widgets::menu_item(ui, &p, Some("arrow.down.right.and.arrow.up.left"), "Compact Chat", false, can_compact).clicked() {
                compact = true;
                ui.memory_mut(|m| m.close_popup());
            }
            if widgets::menu_item(ui, &p, Some("doc.on.doc"), "Copy Chat", false, has).clicked() {
                copy = true;
                ui.memory_mut(|m| m.close_popup());
            }
            if widgets::menu_item(ui, &p, Some("doc.richtext"), "PDF", false, has).clicked() {
                pdf = true;
                ui.memory_mut(|m| m.close_popup());
            }
            if widgets::menu_item(ui, &p, Some("doc.badge.plus"), "Save to Documents", false, has).clicked() {
                save = true;
                ui.memory_mut(|m| m.close_popup());
            }
        });
        if rename {
            self.rename_text = title;
            self.renaming = true;
            self.rename_focus = true;
        }
        if compact {
            self.do_compact(sh);
        }
        if copy {
            widgets::set_clipboard(ui.ctx(), &self.chat_text());
        }
        if save {
            self.do_save(sh);
        }
        if pdf {
            self.do_pdf(sh);
        }
    }

    fn set_input(&mut self, ctx: &egui::Context, text: String) {
        self.input = text;
        let id = egui::Id::new("composer_input");
        if let Some(mut st) = egui::TextEdit::load_state(ctx, id) {
            let n = self.input.chars().count();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(n))));
            st.store(ctx, id);
        }
    }

    fn slash_items(&self, sh: &Shared) -> Vec<cui::SlashItem> {
        if self.slash_dismissed {
            return vec![];
        }
        let t = &self.input;
        let perm = PERMS[self.perm_idx].0;
        if !t.starts_with('/') {
            if let Some(i) = t.rfind('/') {
                let before = &t[..i];
                let q = &t[i + 1..];
                if before.chars().last().map_or(false, char::is_whitespace) && !q.chars().any(char::is_whitespace) {
                    return self.slash.matches(q, sh, perm).into_iter().filter(|x| x.kind == cui::SlashKind::Skill).collect();
                }
            }
            return vec![];
        }
        if t.contains('\n') || t.matches(' ').count() > 1 {
            return vec![];
        }
        if t.contains(' ') {
            let menu = t[1..].split(' ').next().unwrap_or("").to_lowercase();
            if cui::SlashSource::options(&menu, sh, perm).is_empty() {
                return vec![];
            }
        }
        self.slash.matches(&t[1..], sh, perm)
    }

    /// Runs a built-in command / picks an option. Returns the composer text to leave behind.
    fn run_slash(&mut self, item: &cui::SlashItem, sh: &mut Shared) -> String {
        use cui::SlashKind::*;
        match item.kind {
            Skill => return format!("Use the {} skill: ", item.name),
            Menu => return format!("/{} ", item.name),
            Option => {
                match item.parent.as_str() {
                    "background" => {
                        if item.value.is_empty() {
                            crate::backdrop::use_palette();
                        } else if item.value == "photo" {
                            sh.picker.open("bg_photo", "Choose a picture", crate::pickers::PickMode::File(crate::backdrop::IMAGE_EXT), dirs::picture_dir());
                        } else {
                            crate::backdrop::use_scene(&item.value);
                        }
                    }
                    "palette" => {
                        crate::theme::set_palette(crate::theme::JbPalette::parse(&item.value));
                        sh.palette = crate::theme::Palette::for_state(sh.aero, sh.dark);
                        crate::theme::apply(&sh.ctx, sh.palette);
                    }
                    "panels" => {
                        let v: f32 = item.value.parse().unwrap_or(100.0);
                        crate::theme::save_solidity((v / 100.0).max(0.35));
                        sh.palette = crate::theme::Palette::for_state(sh.aero, sh.dark);
                        crate::theme::apply(&sh.ctx, sh.palette);
                    }
                    "open" => {
                        if let Some((_, _, t)) = cui::OPEN_TABS.iter().find(|(n, _, _)| *n == item.value) {
                            sh.goto_tab = Some(*t);
                        }
                    }
                    "permissions" => {
                        if let Some(i) = PERMS.iter().position(|(id, _)| *id == item.value) {
                            self.perm_idx = i;
                            self.sync_composer(sh);
                        }
                    }
                    _ => {}
                }
                return String::new();
            }
            Action => {}
        }
        match item.name.as_str() {
            "model" => self.open_picker = true,
            "settings" => sh.settings_requested = true,
            "new" => sh.new_chat_requested = true,
            "compact" => self.do_compact(sh),
            "copy" => widgets::set_clipboard(&sh.ctx, &self.chat_text()),
            "save" => self.do_save(sh),
            "think" => self.think = true,
            "fast" => self.think = false,
            "agent" => self.agent = true,
            "chat" => self.agent = false,
            _ => {}
        }
        self.sync_composer(sh);
        String::new()
    }

    fn sync_composer(&mut self, sh: &mut Shared) {
        sh.composer.agent = self.agent;
        sh.composer.think = self.think;
        sh.composer.web = self.web;
        sh.composer.term = self.term;
        sh.composer.perm = PERMS[self.perm_idx].0.to_string();
        sh.composer_dirty = true;
    }

    fn stop(&mut self, sh: &Shared) {
        if let Some(sid) = self.sid.clone() {
            self.background_actions.push(sh.spawn(move |a| a.stop_stream(&sid)));
        }
        self.streaming = false;
        self.stream_rx = None;
    }

    /// The composer card (Mac ComposerView): text on top, one control row below.
    fn composer(&mut self, ui: &mut Ui, sh: &mut Shared) {
        use crate::theme::{self, W};
        let p = sh.palette;
        let ctx = ui.ctx().clone();
        let input_id = egui::Id::new("composer_input");
        let focused = ui.memory(|m| m.has_focus(input_id));
        let before = (self.agent, self.think, self.web, self.term, self.perm_idx);

        // Slash suggestions: keys first, so Enter / arrows never reach the text field.
        let h = {
            use std::hash::{Hash, Hasher};
            let mut s = std::collections::hash_map::DefaultHasher::new();
            self.input.hash(&mut s);
            s.finish()
        };
        let hid = egui::Id::new("composer_hash");
        if ui.data(|d| d.get_temp::<u64>(hid)) != Some(h) {
            ui.data_mut(|d| d.insert_temp(hid, h));
            self.slash_idx = 0;
            if !self.input.contains('/') {
                self.slash_dismissed = false;
            }
            if self.input.starts_with('/') || self.input.contains(" /") || self.input.contains("\n/") {
                self.slash.refresh(sh);
            }
        }
        let items = self.slash_items(sh);
        let mut accept: Option<usize> = None;
        let mut taken_enter = false;
        if focused && !items.is_empty() {
            let n = items.len();
            let i = self.slash_idx.min(n - 1);
            let k = |ui: &mut Ui, key| ui.input_mut(|s| s.consume_key(egui::Modifiers::NONE, key));
            if k(ui, Key::ArrowUp) {
                self.slash_idx = (i + n - 1) % n;
                self.slash_scroll = true;
            } else if k(ui, Key::ArrowDown) {
                self.slash_idx = (i + 1) % n;
                self.slash_scroll = true;
            } else if k(ui, Key::Enter) || k(ui, Key::Tab) {
                accept = Some(i);
                taken_enter = true;
            } else if k(ui, Key::Escape) {
                self.slash_dismissed = true;
            }
        } else if focused {
            if self.streaming && ui.input_mut(|s| s.consume_key(egui::Modifiers::NONE, Key::Escape)) {
                self.stop(sh);
            } else if self.input.is_empty() && !self.streaming && ui.input_mut(|s| s.consume_key(egui::Modifiers::NONE, Key::ArrowUp)) {
                if let Some(last) = self.msgs.iter().rev().find(|m| m.role == "user") {
                    let t = last.content.clone();
                    self.set_input(&ctx, t);
                }
            }
        }

        let slot = ui.painter().add(egui::Shape::Noop);
        let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(14.0, 11.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            if self.recording.is_some() {
                ui.horizontal(|ui| {
                    let t = ui.input(|i| i.time) as f32;
                    let a = 0.55 + 0.45 * (t * 4.0).sin().abs();
                    ui.label(RichText::new("●").color(p.danger.gamma_multiply(a)));
                    ui.label(RichText::new("Listening… tap the mic again to transcribe").font(theme::jb(11.0, W::Regular)).color(p.ink2));
                });
                ui.ctx().request_repaint();
            }
            if !self.uploads.is_empty() || self.transcription.is_some() {
                ui.horizontal(|ui| {
                    crate::ascii::spinner(ui, 12.0, p.ink2);
                    ui.label(RichText::new("Preparing input…").font(theme::jb(11.0, W::Regular)).color(p.ink2));
                });
            }
            if !self.attachments.is_empty() {
                let mut remove = None;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for (index, att) in self.attachments.iter().enumerate() {
                        let slot = ui.painter().add(egui::Shape::Noop);
                        let r = egui::Frame::none().inner_margin(egui::Margin::symmetric(8.0, 4.0)).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.horizontal(|ui| {
                                crate::icons::symbol(ui, "paperclip", 10.0, p.ink3);
                                ui.label(RichText::new(&att.name).font(theme::jb(11.0, W::Regular)).color(p.ink2));
                                let x = crate::icons::symbol(ui, "xmark", 8.0, p.ink3).interact(egui::Sense::click());
                                if x.on_hover_text("Remove attachment").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    remove = Some(index);
                                }
                            });
                        });
                        let rr = egui::Rounding::same(5.0);
                        ui.painter().set(
                            slot,
                            egui::Shape::Vec(vec![
                                egui::Shape::rect_filled(r.response.rect, rr, p.hover),
                                egui::Shape::rect_stroke(r.response.rect.shrink(0.5), rr, egui::Stroke::new(1.0, p.line)),
                            ]),
                        );
                    }
                });
                if let Some(i) = remove {
                    self.attachments.remove(i);
                }
            }
            // Enter sends, Shift+Enter is a newline (Ctrl+Enter also sends, Mac ⌘↩).
            let enter = !taken_enter
                && focused
                && ui.input_mut(|i| {
                    let n = i.events.len();
                    i.events.retain(|e| {
                        !matches!(e, egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. }
                            if !modifiers.shift || modifiers.command)
                    });
                    n != i.events.len()
                });
            ui.visuals_mut().text_cursor.stroke.color = p.accent;
            ui.visuals_mut().selection.bg_fill = p.accent_soft;
            egui::ScrollArea::vertical().id_salt("composer_scroll").max_height(190.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.input)
                        .id(input_id)
                        .frame(false)
                        .desired_rows(1)
                        .desired_width(f32::INFINITY)
                        .font(theme::jb(14.0, W::Regular))
                        .text_color(p.ink)
                        .margin(egui::Margin::symmetric(7.0, 4.0))
                        .hint_text(RichText::new("Message").font(theme::jb(14.0, W::Regular)).color(p.ink3)),
                );
            });
            if enter && self.can_send() {
                self.send(sh);
            }
            self.control_row(ui, sh);
        });
        let r = inner.response.rect;
        let rr = egui::Rounding::same(theme::R_PANEL);
        let ring = if focused { p.accent_a(0.7) } else { p.line };
        ui.painter().set(
            slot,
            egui::Shape::Vec(vec![
                egui::Shape::rect_filled(r, rr, p.panel_fill),
                egui::Shape::rect_stroke(r.shrink(0.5), rr, egui::Stroke::new(1.0, ring)),
            ]),
        );

        if !items.is_empty() {
            let sel = self.slash_idx.min(items.len() - 1);
            let scroll = std::mem::take(&mut self.slash_scroll);
            let (hov, pick) = cui::slash_popover(&ctx, &p, &items, sel, r.left_top(), scroll);
            if let Some(h) = hov {
                self.slash_idx = h;
            }
            if pick.is_some() {
                accept = pick;
            }
        }
        if let Some(i) = accept {
            let item = items[i].clone();
            let mid = !self.input.starts_with('/');
            let text = if mid && item.kind == cui::SlashKind::Skill {
                let at = self.input.rfind('/').unwrap_or(0);
                format!("{}using the {} skill ", &self.input[..at], item.name)
            } else {
                self.run_slash(&item, sh)
            };
            self.set_input(&ctx, text);
            self.slash_idx = 0;
            self.slash_dismissed = false;
            ctx.memory_mut(|m| m.request_focus(input_id));
        }
        if before != (self.agent, self.think, self.web, self.term, self.perm_idx) {
            self.sync_composer(sh);
        }

        let drops = ui.input(|i| i.raw.dropped_files.clone());
        for file in drops {
            if let Some(path) = file.path {
                self.queue_upload(path, sh);
            } else if let Some(bytes) = file.bytes {
                let name = if file.name.is_empty() { "image.png".to_string() } else { file.name };
                let data = bytes.to_vec();
                self.uploads.push(sh.spawn(move |a| a.upload(&name, data.clone()).map(|v| (v, data))));
            }
        }
    }

    fn control_row(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let p = sh.palette;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let agent = self.agent;
            if cui::sym_button(ui, &p, "magnifyingglass", self.web && agent, agent).on_hover_text("Web search (agent mode)").clicked() {
                self.web = !self.web;
            }
            if cui::lit_button(ui, &p, 26.0, self.term && agent, agent, |pt, c, col| {
                crate::icons::paint_glyph(pt, c, crate::icons::JbGlyph::Coding, 14.0, col)
            })
            .on_hover_text("Allow terminal commands (agent mode)")
            .clicked()
            {
                self.term = !self.term;
            }
            // File access mode.
            let mode = PERMS[self.perm_idx].0;
            let col = match mode {
                "sandbox" => p.ink2,
                "readonly" => p.warn,
                _ => p.danger,
            };
            let lock = cui::lit_button(ui, &p, 26.0, false, true, |pt, c, _| crate::icons::paint_symbol(pt, c, "lock", 13.0, col))
                .on_hover_text("Agent file access mode");
            let ask = sh.prefs.get("ask_before_commands").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut set_ask = None;
            widgets::menu_popup(ui, &p, &lock, |ui| {
                for (i, (title, icon, c)) in [("Bound folder", "folder", p.ink2), ("Read-only", "eye", p.warn), ("Full access", "exclamationmark.triangle", p.danger)].iter().enumerate() {
                    if cui::perm_row(ui, &p, title, icon, *c, self.perm_idx == i).clicked() {
                        self.perm_idx = i;
                        ui.memory_mut(|m| m.close_popup());
                    }
                }
                if self.perm_idx == 2 {
                    widgets::menu_separator(ui, &p);
                    if widgets::menu_item(ui, &p, None, "Ask before running commands", ask, true).clicked() {
                        set_ask = Some(!ask);
                    }
                }
            });
            if let Some(v) = set_ask {
                sh.prefs["ask_before_commands"] = serde_json::json!(v);
                self.background_actions.push(sh.spawn(move |a| a.set_pref("ask_before_commands", serde_json::json!(v))));
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                // Send / stop.
                if self.streaming {
                    if cui::lit_button(ui, &p, 28.0, false, true, |pt, c, _| crate::icons::paint_symbol(pt, c, "stop.fill", 13.0, p.danger))
                        .on_hover_text("Stop (esc)")
                        .clicked()
                    {
                        self.stop(sh);
                    }
                } else {
                    let ready = self.can_send();
                    if cui::lit_button(ui, &p, 28.0, ready, ready, |pt, c, col| crate::icons::paint_symbol(pt, c, "arrow.up", 14.0, col))
                        .on_hover_text("Send (⏎)")
                        .clicked()
                    {
                        self.send(sh);
                    }
                }
                if cui::sym_button(ui, &p, "paperclip", false, true).on_hover_text("Attach files").clicked() {
                    self.attach(sh);
                }
                // Mic.
                let rec = self.recording.is_some();
                if self.transcription.is_some() {
                    ui.allocate_ui(egui::vec2(26.0, 26.0), |ui| ui.centered_and_justified(|ui| crate::ascii::spinner(ui, 12.0, p.ink2)));
                } else if cui::lit_button(ui, &p, 26.0, false, true, |pt, c, col| {
                    crate::icons::paint_symbol(pt, c, if rec { "mic.fill" } else { "mic" }, 14.0, if rec { p.danger } else { col });
                    let _ = c;
                })
                .on_hover_text(if rec { "Stop recording" } else { "Voice input" })
                .clicked()
                {
                    if let Some(recording) = self.recording.take() {
                        self.transcription = Some(sh.spawn(move |_| recording.transcribe()));
                    } else {
                        match crate::voice::Recording::start() {
                            Ok(r) => self.recording = Some(r),
                            Err(e) => sh.err(e),
                        }
                    }
                }
                if rec {
                    ui.ctx().request_repaint();
                }
                // Model picker.
                let pid = egui::Id::new("chat_model_picker_popup");
                if std::mem::take(&mut self.open_picker) {
                    ui.memory_mut(|m| m.open_popup(pid));
                }
                if let Some(model) = cui::model_picker(ui, sh, pid) {
                    crate::state::remember_model(&model);
                    if let Some(sid) = self.sid.clone() {
                        let m = model.clone();
                        self.background_actions.push(sh.spawn(move |a| a.update_session_model(&sid, &m.model_id, &m.endpoint_url, &m.endpoint_id)));
                    }
                    sh.selected_model = Some(model);
                }
                let mut is_agent = self.agent;
                if cui::slide_switch(ui, &p, "agent", "Agent", "Chat", &mut is_agent) {
                    self.agent = is_agent;
                    if !is_agent {
                        self.term = false;
                    }
                }
                let mut is_think = self.think;
                // Narrow column (editor open): drop Think/Fast rather than overlap the left toggles.
                if ui.available_width() > 100.0 && cui::slide_switch(ui, &p, "think", "Think", "Fast", &mut is_think) {
                    self.think = is_think;
                }
            });
        });
    }

    fn attach(&mut self, sh: &mut Shared) {
        sh.picker.open("chat_attach", "Attach files", crate::pickers::PickMode::Files(&[]), None);
    }

    fn queue_upload(&mut self, path: std::path::PathBuf, sh: &Shared) {
        self.uploads.push(sh.spawn(move |a| {
            let bytes = std::fs::read(&path).map_err(|e| format!("Read attachment: {e}"))?;
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
            a.upload(name, bytes.clone()).map(|v| (v, bytes))
        }));
    }

    fn pump_jobs(&mut self, sh: &mut Shared) {
        self.background_actions.retain_mut(|job| match job.take() {
            Some(Err(e)) => {
                sh.err(e);
                false
            }
            Some(Ok(_)) => false,
            None => !job.done,
        });
        self.uploads.retain_mut(|job| match job.take() {
            Some(Ok((v, bytes))) => {
                if let Some(files) = v.get("files").and_then(Value::as_array) {
                    let single = files.len() == 1;
                    for file in files {
                        if let Some(att) = Att::from_json(file) {
                            if single {
                                self.files.seed(&att.id, bytes.clone());
                            }
                            self.attachments.push(att);
                        }
                    }
                } else {
                    sh.err("Upload returned no attachment IDs.");
                }
                false
            }
            Some(Err(e)) => {
                sh.err(format!("Attachment: {e}"));
                false
            }
            None => !job.done,
        });
        if self
            .recording
            .as_mut()
            .map(|r| r.finished())
            .unwrap_or(false)
        {
            if let Some(recording) = self.recording.take() {
                self.transcription = Some(sh.spawn(move |_| recording.transcribe()));
            }
        }
        if let Some(job) = &mut self.transcription {
            if let Some(result) = job.take() {
                match result {
                    Ok(text) => {
                        if !self.input.is_empty() {
                            self.input.push(' ');
                        }
                        self.input.push_str(&text);
                    }
                    Err(e) => sh.err(e),
                }
                self.transcription = None;
            }
        }
    }

    /// Text or at least one attachment, nothing uploading, not mid-reply.
    fn can_send(&self) -> bool {
        !self.streaming
            && self.uploads.is_empty()
            && (!self.input.trim().is_empty() || !self.attachments.is_empty())
    }

    fn send(&mut self, sh: &mut Shared) {
        let sid = match self.sid.clone() {
            Some(s) => s,
            None => {
                // No chat open: create one (named after the message like the Mac), then send.
                if !self.deferred_send && !self.input.trim().is_empty() {
                    self.deferred_send = true;
                    sh.new_chat_title = Some(self.input.trim().chars().take(48).collect());
                    sh.new_chat_requested = true;
                }
                return;
            }
        };
        let text = std::mem::take(&mut self.input).trim().to_string();
        self.msgs.push(Msg {
            role: "user".into(),
            content: text.clone(),
            segs: vec![Seg::Text(text.clone())],
            atts: self.attachments.clone(),
            ..Default::default()
        });
        self.to_bottom = true;
        self.msgs.push(Msg {
            role: "assistant".into(),
            model: sh.selected_model.as_ref().map(|m| m.display.clone()).unwrap_or_default(),
            ..Default::default()
        });

        let (m, eid, eurl) = sh
            .selected_model
            .as_ref()
            .map(|m| {
                (
                    m.model_id.clone(),
                    m.endpoint_id.clone(),
                    m.endpoint_url.clone(),
                )
            })
            .unwrap_or_default();
        let ask = sh
            .prefs
            .get("ask_before_commands")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let open_docs: Vec<String> = sh
            .open_docs
            .iter()
            .flat_map(|d| {
                [
                    d.remote_sub
                        .as_ref()
                        .map(|sub| format!("local-{sub}"))
                        .unwrap_or_else(|| d.id.clone()),
                    d.title.clone(),
                ]
            })
            .collect();
        let active = sh
            .active_doc
            .as_ref()
            .and_then(|id| sh.open_docs.iter().find(|d| &d.id == id))
            .map(|d| {
                d.remote_sub
                    .as_ref()
                    .map(|sub| format!("local-{sub}"))
                    .unwrap_or_else(|| d.id.clone())
            })
            .unwrap_or_default();

        let mut fields: Vec<(String, String)> = vec![
            ("message".into(), text),
            ("session".into(), sid.clone()),
            (
                "mode".into(),
                if self.agent {
                    "agent".into()
                } else {
                    "chat".into()
                },
            ),
            ("allow_web_search".into(), self.web.to_string()),
            ("allow_bash".into(), self.term.to_string()),
            ("permission_mode".into(), PERMS[self.perm_idx].0.into()),
            ("thinking".into(), self.think.to_string()),
            ("model".into(), m),
            ("endpoint_id".into(), eid),
            ("endpoint_url".into(), eurl),
            ("ask_permission".into(), ask.to_string()),
            ("active_doc_id".into(), active),
        ];
        fields.push(("device".into(), crate::config::device_id()));
        // Settings → Documents → tool-use limit (0 / unset = unlimited), agent turns only.
        let max_tools = sh.prefs.get("max_tool_calls").and_then(|v| v.as_i64()).unwrap_or(0);
        if self.agent && max_tools > 0 {
            fields.push(("max_tool_calls".into(), max_tools.to_string()));
        }
        fields.push(("ask_doc_edit".into(), sh.require_doc_approval.to_string()));
        if !self.attachments.is_empty() {
            let ids: Vec<&str> = self.attachments.iter().map(|a| a.id.as_str()).collect();
            fields.push((
                "attachments".into(),
                serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into()),
            ));
            self.attachments.clear();
        }
        if !open_docs.is_empty() {
            fields.push(("open_docs".into(), open_docs.join("\n")));
        }

        let (tx, rx) = channel();
        self.stream_rx = Some(rx);
        self.streaming = true;
        let api = sh.api.clone();
        let ctx = sh.ctx.clone();
        std::thread::spawn(move || {
            api.chat_stream(fields, tx);
            ctx.request_repaint();
        });
    }

    fn pump_stream(&mut self, sh: &mut Shared) {
        let mut events = vec![];
        let mut ended = false;
        if let Some(rx) = &self.stream_rx {
            for ev in rx.try_iter() {
                if ev.is_null() {
                    ended = true;
                    break;
                }
                events.push(ev);
            }
        }
        for ev in events {
            self.apply_event(&ev, sh);
        }
        if ended {
            self.perm_prompt = None;
            for doc in &mut sh.open_docs {
                doc.ai_writing = false;
            }
            self.streaming = false;
            self.stream_rx = None;
        }
        if self.streaming {
            sh.ctx
                .request_repaint_after(std::time::Duration::from_millis(40));
        }
    }

    fn apply_event(&mut self, ev: &Value, sh: &mut Shared) {
        if let Some(err) = ev.get("error").and_then(|v| v.as_str()) {
            if let Some(m) = self.msgs.last_mut() {
                m.push_text(&format!("\n[error] {err}"));
            }
            return;
        }
        if let Some(delta) = ev.get("delta").and_then(|v| v.as_str()) {
            let thinking = ev
                .get("thinking")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if let Some(m) = self.msgs.last_mut() {
                if thinking {
                    m.thinking.push_str(delta);
                } else {
                    m.push_text(delta);
                }
            }
            return;
        }
        match ev.get("type").and_then(|v| v.as_str()) {
            Some("tool_start") | Some("tool_output") => {
                let tool = ev
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let cmd = ev
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let code = ev.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(
                    if ev["type"] == "tool_start" {
                        i64::MIN
                    } else {
                        0
                    },
                );
                let output = ev
                    .get("output")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Some(m) = self.msgs.last_mut() {
                    // A tool_output updates the matching tool card in place; a new
                    // tool_start appends a card at the current position (after the
                    // prose emitted so far), giving the inline ordering.
                    let existing = m.segs.iter_mut().rev().find_map(|s| match s {
                        Seg::Tool(t, c, code, out)
                            if *t == tool && (*c == cmd || cmd.is_empty()) =>
                        {
                            Some((code, out))
                        }
                        _ => None,
                    });
                    if let Some((slot, out)) = existing {
                        *slot = code;
                        *out = output;
                    } else {
                        m.segs.push(Seg::Tool(tool, cmd, code, output));
                    }
                }
            }
            Some("permission_request") => {
                self.perm_prompt = Some((
                    ev.get("request_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .into(),
                    ev.get("tool").and_then(|v| v.as_str()).unwrap_or("").into(),
                    ev.get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .into(),
                ));
            }
            Some(key @ ("memories_used" | "skills_used" | "plugins_used")) => {
                if let Some(arr) = ev.get("data").and_then(|v| v.as_array()) {
                    if let Some(m) = self.msgs.last_mut() {
                        for x in arr {
                            if let Some(s) = x.as_str() {
                                m.used.push(key, s);
                            }
                        }
                    }
                }
            }
            Some("metrics") => {
                if let Some(d) = ev.get("data") {
                    let tps = d
                        .get("tokens_per_second")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let ctxp = d
                        .get("context_percent")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    if let Some(m) = self.msgs.last_mut() {
                        m.metrics = metrics_line(tps, 0.0);
                        m.ctx = ctxp;
                    }
                }
            }
            Some("doc_stream_open") | Some("doc_stream_delta") | Some("doc_update") => {
                apply_doc_event(ev, sh, &mut self.streamed_doc);
            }
            _ => {}
        }
    }
}

fn apply_doc_event(ev: &Value, sh: &mut Shared, streamed: &mut Option<String>) {
    let kind = ev.get("type").and_then(Value::as_str).unwrap_or("");
    if kind == "doc_stream_delta" {
        if let Some(doc) = sh
            .open_docs
            .iter_mut()
            .find(|d| Some(&d.id) == streamed.as_ref())
        {
            if let Some(content) = ev.get("content").and_then(Value::as_str) {
                doc.content = content.into();
            } else if let Some(delta) = ev.get("delta").and_then(Value::as_str) {
                doc.content.push_str(delta);
            }
            doc.ai_writing = true;
        }
        return;
    }
    let title = ev
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Untitled")
        .to_string();
    let id = ev
        .get("doc_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let sub = id.strip_prefix("local-").map(str::to_string);
    let rich = title.to_lowercase().ends_with(".docx")
        || title.to_lowercase().ends_with(".doc")
        || title.to_lowercase().ends_with(".xlsx");
    if rich {
        if kind == "doc_update" {
            if let (Some(sid), Some(sub)) = (sh.current_sid.clone(), sub) {
                let key = format!("local-{sid}:{sub}");
                if sh.open_docs.iter().any(|d| d.id == key && d.dirty) {
                    sh.err("This document changed on the backend. Save your local edits before reopening it.");
                } else {
                    sh.open_docs
                        .retain(|d| d.id != key && Some(&d.id) != streamed.as_ref());
                    sh.open_request = Some((sid, sub));
                    sh.editor_visible = true;
                }
            }
            *streamed = None;
        }
        return;
    }
    let key = if id.is_empty() {
        format!("stream-{}:{title}", sh.current_sid.as_deref().unwrap_or(""))
    } else if let (Some(sub), Some(sid)) = (sub.as_ref(), sh.current_sid.as_ref()) {
        format!("local-{sid}:{sub}")
    } else {
        id.clone()
    };
    let content = ev
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let is_new =
        kind == "doc_stream_open" || ev.get("is_new").and_then(Value::as_bool).unwrap_or(false);
    let index = sh.open_docs.iter().position(|d| {
        d.id == key
            || (!id.is_empty() && d.id == id)
            || (kind == "doc_update" && (Some(&d.id) == streamed.as_ref() || d.title == title))
    });
    if let Some(index) = index {
        let d = &mut sh.open_docs[index];
        if kind == "doc_update"
            && !is_new
            && (sh.require_doc_approval || d.dirty)
            && d.content != content
        {
            d.pending_ai = Some(content);
        } else {
            d.content = content;
        }
        d.ai_writing = kind == "doc_stream_open";
        if !id.is_empty() && d.id.starts_with("stream-") {
            d.id = key.clone();
        }
        if let Some(sub) = &sub {
            d.remote_sub = Some(sub.clone());
            d.remote_session = sh.current_sid.clone();
        }
        sh.active_doc = Some(d.id.clone());
    } else {
        let lower = title.to_lowercase();
        sh.open_docs.push(EditorDoc {
            id: key.clone(),
            title,
            content,
            language: ev
                .get("language")
                .and_then(Value::as_str)
                .unwrap_or("markdown")
                .into(),
            spreadsheet: lower.ends_with(".csv") || lower.ends_with(".xlsx"),
            flashcards: lower.ends_with(".apkg"),
            ai_writing: kind == "doc_stream_open",
            remote_sub: sub,
            remote_session: sh.current_sid.clone(),
            ..Default::default()
        });
        sh.active_doc = Some(key);
    }
    if kind == "doc_stream_open" {
        *streamed = sh.active_doc.clone();
    } else {
        *streamed = None;
    }
    sh.editor_visible = true;
}

enum MessageAction {
    Resend(String),
    Edit(String),
    Unsend(String),
    Open(String),
}

/// Messages rendered per page of the transcript (Mac: 60, +100 per click).
const PAGE: usize = 60;
/// Reading-column cap (Mac ChatView: 860 pt).
const MAX_COLUMN: f32 = 860.0;

/// "41 tok/s · 18% ctx", leaving out whichever figure the backend didn't send.
fn metrics_line(tps: f64, ctx: f64) -> String {
    let mut parts = vec![];
    if tps > 0.0 {
        parts.push(format!("{tps:.0} tok/s"));
    }
    if ctx > 0.0 {
        parts.push(format!("{ctx:.0}% ctx"));
    }
    parts.join(" · ")
}

/// "Compacted chat" — older turns were collapsed server-side.
fn compacted_pill(ui: &mut Ui, sh: &Shared) {
    use crate::theme::{self, W};
    let p = sh.palette;
    ui.add_space(4.0);
    let w = ui.available_width();
    let g = ui.painter().layout_no_wrap("compacted".into(), theme::jb(11.0, W::Regular), p.ink3);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, g.size().y.max(1.0)), egui::Sense::hover());
    let c = rect.center();
    let half = g.size().x / 2.0 + 10.0;
    ui.painter().line_segment([egui::pos2(rect.left(), c.y), egui::pos2(c.x - half, c.y)], egui::Stroke::new(1.0, p.line));
    ui.painter().line_segment([egui::pos2(c.x + half, c.y), egui::pos2(rect.right(), c.y)], egui::Stroke::new(1.0, p.line));
    ui.painter().galley(c - g.size() / 2.0, g, p.ink3);
    ui.add_space(21.0);
}

/// `[label](target)` links that point into the workspace.
fn workspace_links(text: &str) -> Vec<(String, String)> {
    const PREFIXES: [&str; 6] = ["research-", "document-", "event-", "task-", "note-", "session-"];
    let mut out: Vec<(String, String)> = vec![];
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find("](") else { break };
        let label = &rest[..close];
        let after = &rest[close + 2..];
        let Some(end) = after.find(')') else { break };
        let mut target = after[..end].trim();
        target = target.strip_prefix("about:").unwrap_or(target);
        target = target.strip_prefix('#').unwrap_or(target);
        if !label.contains('[') && PREFIXES.iter().any(|p| target.starts_with(p)) && !out.iter().any(|(_, t)| t == target) {
            out.push((label.to_string(), target.to_string()));
        }
        rest = &after[end..];
    }
    out
}

fn workspace_icon(target: &str) -> &'static str {
    if target.starts_with("research-") {
        "\u{f0c3}"
    } else if target.starts_with("document-") {
        crate::theme::icon::FILE
    } else if target.starts_with("event-") {
        "\u{f073}"
    } else if target.starts_with("task-") {
        crate::theme::icon::CHECK_SQUARE
    } else if target.starts_with("note-") {
        "\u{f249}"
    } else {
        "\u{f086}"
    }
}
fn message_view(
    ui: &mut Ui,
    sh: &Shared,
    m: &Msg,
    md: &mut CommonMarkCache,
    files: &mut Attachments,
    idx: usize,
    live: bool,
) -> Option<MessageAction> {
    use crate::theme::{self, W};
    let mine = m.role == "user";
    let p = sh.palette;
    let full = ui.available_width();
    let mut action = None;
    let menu = |ui: &mut Ui, action: &mut Option<MessageAction>| {
        if ui.button("Copy").clicked() {
            widgets::set_clipboard(ui.ctx(), &m.content);
            ui.close_menu();
        }
        if mine {
            if ui.button("Resend").clicked() {
                *action = Some(MessageAction::Resend(m.content.clone()));
                ui.close_menu();
            }
            if let Some(id) = &m.db_id {
                if ui.button("Unsend").clicked() {
                    *action = Some(MessageAction::Unsend(id.clone()));
                    ui.close_menu();
                }
            }
        }
    };
    let hov_id = ui.id().with(("msg_hov", idx));
    let hovering = ui.data(|d| d.get_temp::<bool>(hov_id)).unwrap_or(false);
    let hov_a = ui.ctx().animate_bool_with_time(hov_id.with("a"), hovering, theme::FADE);
    let block_top = ui.cursor().min;

    if mine {
        if !m.atts.is_empty() {
            files.show(ui, sh, &m.atts, true);
            ui.add_space(4.0);
        }
        let text = m.content.trim().to_string();
        if !text.is_empty() {
            let max_w = (full - 80.0).max(120.0);
            let mut job = egui::text::LayoutJob::simple(
                text.clone(),
                theme::jb(14.0, W::Regular),
                p.ink,
                max_w - 28.0,
            );
            for sec in &mut job.sections {
                sec.format.line_height = Some(20.0);
            }
            let galley = ui.painter().layout_job(job);
            let size = galley.size() + egui::vec2(28.0, 20.0);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(full, size.y), egui::Sense::click());
            let r = egui::Rect::from_min_size(egui::pos2(rect.right() - size.x, rect.top()), size);
            let rr = egui::Rounding::same(theme::R_CARD);
            ui.painter().rect_filled(r, rr, p.panel_fill);
            ui.painter().rect_filled(r, rr, p.raised);
            ui.painter().rect_stroke(r.shrink(0.5), rr, egui::Stroke::new(1.0, p.line));
            ui.painter().galley(r.min + egui::vec2(14.0, 10.0), galley, p.ink);
            resp.context_menu(|ui| menu(ui, &mut action));
            ui.add_space(4.0);
            // Hover actions: copy, retry, edit.
            ui.allocate_ui_with_layout(egui::vec2(full, 24.0), Layout::right_to_left(Align::Center), |ui| {
                ui.set_opacity(hov_a);
                ui.spacing_mut().item_spacing.x = 0.0;
                if widgets::icon_btn(ui, &p, "pencil", 24.0, false).on_hover_text("Edit").clicked() && hov_a > 0.5 {
                    action = Some(MessageAction::Edit(m.content.clone()));
                }
                if widgets::icon_btn(ui, &p, "arrow.clockwise", 24.0, false).on_hover_text("Retry").clicked() && hov_a > 0.5 {
                    action = Some(MessageAction::Resend(m.content.clone()));
                }
                widgets::copy_check_button(ui, &p, &m.content, 24.0);
            });
        }
    } else {
        ui.spacing_mut().item_spacing.y = 8.0;
        let thinking_live = live && m.content.trim().is_empty();
        if !m.model.is_empty() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                crate::logos::logo(ui, &m.model, 11.0, p.ink3);
                ui.label(RichText::new(&m.model).font(theme::jb(11.0, W::Regular)).color(p.ink3));
            });
            ui.add_space(8.0);
        }
        let calls: Vec<cui::Call> = m
            .segs
            .iter()
            .filter_map(|s| match s {
                Seg::Tool(t, c, code, out) => Some(cui::Call { tool: t, cmd: c, code: *code, output: out }),
                _ => None,
            })
            .collect();
        let key = m.db_id.clone().unwrap_or_else(|| format!("idx{idx}"));
        if !calls.is_empty() {
            cui::tool_activity(ui, &p, ui.id().with(("steps", &key)), &calls, &pretty_tool_name);
            ui.add_space(8.0);
        }
        if !m.thinking.is_empty() {
            cui::thinking_section(ui, &p, ui.id().with(("think", &key)), &m.thinking, thinking_live);
            ui.add_space(8.0);
        }
        if live && m.content.trim().is_empty() && m.thinking.is_empty() && calls.is_empty() {
            crate::ascii::scramble_text(ui, &p, "thinking", true, theme::jb(11.5, W::Regular), p.ink3);
        }
        for seg in &m.segs {
            if let Seg::Text(t) = seg {
                if !t.trim().is_empty() {
                    render_md(ui, sh, md, t);
                }
            }
        }
        if live && !m.content.trim().is_empty() {
            crate::ascii::stream_cursor(ui, p.accent, 14.0);
        }
        let links = workspace_links(&m.content);
        if !links.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                for (label, target) in links {
                    if widgets::btn_icon(ui, sh, workspace_icon(&target), &label).clicked() {
                        action = Some(MessageAction::Open(target));
                    }
                }
            });
        }
        used_chips(ui, sh, m, idx);
        if !m.atts.is_empty() {
            files.show(ui, sh, &m.atts, false);
        }
        if !live && !m.content.is_empty() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.scope(|ui| {
                    ui.set_opacity(hov_a);
                    widgets::copy_check_button(ui, &p, &m.content, 24.0);
                });
                if !m.metrics.is_empty() {
                    ui.label(RichText::new(&m.metrics).font(theme::jb(10.5, W::Regular)).color(p.ink3));
                }
                if m.ctx > 0.0 {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    cui::context_ring(ui, &p, m.ctx);
                }
            });
        }
        let blk = egui::Rect::from_min_max(block_top, egui::pos2(block_top.x + full, ui.min_rect().bottom()));
        ui.interact(blk, hov_id.with("c"), egui::Sense::click()).context_menu(|ui| menu(ui, &mut action));
    }
    let blk = egui::Rect::from_min_max(block_top, egui::pos2(block_top.x + full, ui.min_rect().bottom()));
    let now_hov = ui.rect_contains_pointer(blk);
    if now_hov != hovering {
        ui.data_mut(|d| d.insert_temp(hov_id, now_hov));
        ui.ctx().request_repaint();
    }
    ui.add_space(17.0);
    action
}

/// (category key, icon, (singular, plural), items)
type UsedCat<'a> = (&'a str, &'a str, (&'a str, &'a str), &'a Vec<String>);

/// The Mac app's "N memories / skills / plugins used" disclosures, as one row
/// of compact glass chips (icon + count). Each opens a popover with the list.
/// Tools come from the reply's own tool calls (unique names, in order).
fn used_chips(ui: &mut Ui, sh: &Shared, m: &Msg, idx: usize) {
    let mut tools: Vec<String> = vec![];
    for seg in &m.segs {
        if let Seg::Tool(tool, ..) = seg {
            let name = pretty_tool_name(tool);
            if !name.is_empty() && !tools.contains(&name) {
                tools.push(name);
            }
        }
    }
    let cats: [UsedCat; 4] = [
        ("memories", crate::theme::icon::BRAIN, ("memory", "memories"), &m.used.memories),
        ("skills", "\u{f19d}", ("skill", "skills"), &m.used.skills),
        ("tools", "\u{f0ad}", ("tool", "tools"), &tools),
        ("plugins", "\u{f12e}", ("plugin", "plugins"), &m.used.plugins),
    ];
    if cats.iter().all(|c| c.3.is_empty()) {
        return;
    }
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        // Keyed by the stored message id when there is one, so an open
        // popover survives history reloads and never collides across chats.
        let key = m.db_id.clone().unwrap_or_else(|| format!("idx{idx}"));
        for (cat, glyph, noun, items) in cats {
            if !items.is_empty() {
                let id = egui::Id::new(("used_chip", key.as_str(), cat));
                widgets::used_chip(ui, sh, id, glyph, noun, items);
            }
        }
    });
}

/// Raw tool identifiers read like plumbing — humane names, like the Mac's
/// `prettyToolName` (falls back to de-underscored title case).
fn pretty_tool_name(raw: &str) -> String {
    let known = [
        ("bash", "Terminal"),
        ("python", "Python"),
        ("read_file", "Read File"),
        ("write_file", "Write File"),
        ("web_search", "Web Search"),
        ("manage_memory", "Memory"),
        ("manage_notes", "Notes"),
        ("manage_skills", "Skills"),
        ("skill", "Skill"),
        ("create_document", "Create Document"),
        ("edit_document", "Edit Document"),
        ("update_document", "Update Document"),
        ("suggest_document", "Suggest Edits"),
    ];
    if let Some((_, v)) = known.iter().find(|(k, _)| *k == raw) {
        return v.to_string();
    }
    raw.split(['_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Mac `emptyState`: the dot-matrix wordmark and one line.
fn empty_state(ui: &mut Ui, sh: &Shared, title: &str) {
    use crate::theme::{self, W};
    let p = sh.palette;
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 22.0;
        crate::ascii::wordmark(ui, "JoeBro", 3.4, p.ink);
        ui.label(egui::RichText::new(title).font(theme::jb(15.0, W::Regular)).color(p.ink2));
    });
}

/// Render assistant markdown, but pull fenced code blocks out and draw them
/// ourselves with a working Copy button — egui_commonmark 0.18 renders no copy
/// affordance of its own. Prose runs go through the markdown viewer as before.
pub(crate) fn render_md(ui: &mut Ui, sh: &Shared, md: &mut CommonMarkCache, content: &str) {
    let pal = sh.palette;
    let src = widgets::preprocess_md(&crate::panels::mdview::soft_to_hard(content));
    let lines: Vec<&str> = src.lines().collect();
    let mut prose = String::new();
    let mut i = 0;
    let mut blocks = 0usize;
    let flush = |ui: &mut Ui, md: &mut CommonMarkCache, prose: &mut String| {
        if !prose.trim().is_empty() {
            ui.scope(|ui| {
                crate::panels::mdview::prose_style(ui, &pal, false);
                CommonMarkViewer::new().show(ui, md, prose);
            });
        }
        prose.clear();
    };
    while i < lines.len() {
        if lines[i].trim_start().starts_with("```") {
            flush(ui, md, &mut prose);
            let lang = lines[i]
                .trim_start()
                .trim_start_matches("```")
                .trim()
                .to_string();
            i += 1;
            let mut code = String::new();
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                code.push_str(lines[i]);
                code.push('\n');
                i += 1;
            }
            i += 1; // skip the closing fence
            blocks += 1;
            let salt = ui.id().with(("code", blocks));
            code_block(ui, sh, salt, &lang, code.trim_end_matches('\n'));
        } else {
            prose.push_str(lines[i]);
            prose.push('\n');
            i += 1;
        }
    }
    flush(ui, md, &mut prose);
}

/// A fenced code block: language tag + Copy button, monospace body that scrolls
/// horizontally rather than wrapping.
fn code_block(ui: &mut Ui, sh: &Shared, salt: egui::Id, lang: &str, code: &str) {
    use crate::theme::{self, W};
    let p = sh.palette;
    let slot = ui.painter().add(egui::Shape::Noop);
    let w = ui.available_width();
    let out = egui::Frame::none().show(ui, |ui| {
        ui.set_width(w);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.set_min_height(28.0);
            let tag = if lang.is_empty() { "code" } else { lang };
            ui.label(RichText::new(tag).font(theme::jb(11.0, W::Medium)).color(p.ink3));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                widgets::copy_check_button(ui, &p, code, 22.0);
            });
        });
        let (lr, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
        ui.painter().rect_filled(lr, 0.0, p.line);
        ui.style_mut().spacing.scroll = egui::style::ScrollStyle::thin();
        egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
            egui::ScrollArea::horizontal().id_salt(salt).auto_shrink([false, true]).show(ui, |ui| {
                ui.add(egui::Label::new(highlight(code, lang, &p)).wrap_mode(egui::TextWrapMode::Extend).selectable(true));
            });
        });
    });
    let r = out.response.rect;
    let rr = egui::Rounding::same(theme::R_CARD);
    ui.painter().set(
        slot,
        egui::Shape::Vec(vec![egui::Shape::rect_filled(r, rr, p.raised), egui::Shape::rect_stroke(r.shrink(0.5), rr, egui::Stroke::new(1.0, p.line))]),
    );
    ui.add_space(10.0);
}

/// Lightweight syntax colouring (the Mac MarkdownText highlighter): comments,
/// strings, numbers and common keywords. Huge blocks stay plain so a pasted
/// log can't stall a frame.
fn highlight(code: &str, lang: &str, p: &crate::theme::Palette) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let font = crate::theme::mono(12.5, crate::theme::W::Regular);
    let fmt = |c: egui::Color32| TextFormat { font_id: font.clone(), color: c, line_height: Some(18.5), ..Default::default() };
    let mut job = LayoutJob::default();
    let lang = lang.to_lowercase();
    const CODE: &[&str] = &[
        "py", "python", "js", "javascript", "jsx", "ts", "typescript", "tsx", "rs", "rust", "go", "c", "h", "cpp",
        "c++", "hpp", "cs", "csharp", "java", "kotlin", "kt", "swift", "sh", "bash", "zsh", "shell", "console",
        "sql", "json", "yaml", "yml", "toml", "ruby", "rb", "php", "lua", "r", "perl", "dockerfile", "make",
        "makefile", "ini", "conf", "haskell", "hs", "scala", "dart",
    ];
    if code.len() > 40_000 || !CODE.contains(&lang.as_str()) {
        job.append(code, 0.0, fmt(p.ink));
        return job;
    }
    let (kw_c, str_c, num_c, com_c) = if p.dark {
        (
            crate::theme::rgba(0xc792ea, 1.0),
            crate::theme::rgba(0xa5d6a7, 1.0),
            crate::theme::rgba(0xf78c6c, 1.0),
            crate::theme::rgba(0x8a93a6, 1.0),
        )
    } else {
        (
            crate::theme::rgba(0x7b3fb5, 1.0),
            crate::theme::rgba(0x2e7d32, 1.0),
            crate::theme::rgba(0xb5501f, 1.0),
            crate::theme::rgba(0x7a8190, 1.0),
        )
    };
    let hash_comments = matches!(
        lang.as_str(),
        "py" | "python" | "sh" | "bash" | "zsh" | "shell" | "console" | "yaml" | "yml" | "toml" | "rb" | "ruby" | "r" | "perl" | "conf" | "ini" | "dockerfile" | "make" | "makefile"
    );
    let dash_comments = matches!(lang.as_str(), "sql" | "lua" | "haskell" | "hs");
    const KW: &[&str] = &[
        "fn", "let", "mut", "pub", "use", "mod", "impl", "struct", "enum", "trait", "match", "if", "else", "for",
        "while", "loop", "return", "break", "continue", "in", "as", "where", "const", "static", "async", "await",
        "def", "class", "import", "from", "pass", "lambda", "with", "yield", "try", "except", "finally", "raise",
        "None", "True", "False", "and", "or", "not", "is", "elif", "function", "var", "new", "this", "self", "Self",
        "export", "default", "extends", "interface", "type", "null", "undefined", "true", "false", "catch", "throw",
        "switch", "case", "do", "func", "package", "go", "defer", "struct", "void", "int", "float", "double", "char",
        "bool", "string", "public", "private", "protected", "final", "override", "echo", "then", "fi", "esac", "done",
        "select", "insert", "update", "delete", "create", "table", "where", "join", "on", "group", "by", "order",
        "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "TABLE", "WHERE", "JOIN", "ON", "GROUP", "BY", "ORDER", "FROM",
    ];
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;
    let mut plain = String::new();
    let flush = |job: &mut LayoutJob, plain: &mut String| {
        if !plain.is_empty() {
            job.append(plain, 0.0, fmt(p.ink));
            plain.clear();
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        let line_comment = (c == '/' && next == '/' && !hash_comments)
            || (c == '#' && hash_comments)
            || (c == '-' && next == '-' && dash_comments);
        if line_comment {
            flush(&mut job, &mut plain);
            let start = i;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            job.append(&chars[start..i].iter().collect::<String>(), 0.0, fmt(com_c));
        } else if c == '/' && next == '*' {
            flush(&mut job, &mut plain);
            let start = i;
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(chars.len());
            job.append(&chars[start..i].iter().collect::<String>(), 0.0, fmt(com_c));
        } else if c == '"' || c == '\'' || c == '`' {
            // a lone apostrophe in Rust lifetimes / prose shouldn't eat the line
            let end = chars[i + 1..].iter().position(|&x| x == c || (x == '\n' && c != '`'));
            match end {
                Some(off) if chars[i + 1 + off] == c => {
                    flush(&mut job, &mut plain);
                    let mut j = i + 1;
                    while j < chars.len() && chars[j] != c {
                        if chars[j] == '\\' {
                            j += 1;
                        }
                        j += 1;
                    }
                    let j = (j + 1).min(chars.len());
                    job.append(&chars[i..j].iter().collect::<String>(), 0.0, fmt(str_c));
                    i = j;
                }
                _ => {
                    plain.push(c);
                    i += 1;
                }
            }
        } else if c.is_ascii_digit() && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_')) {
            flush(&mut job, &mut plain);
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            job.append(&chars[start..i].iter().collect::<String>(), 0.0, fmt(num_c));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if KW.contains(&word.as_str()) {
                flush(&mut job, &mut plain);
                job.append(&word, 0.0, fmt(kw_c));
            } else {
                plain.push_str(&word);
            }
        } else {
            plain.push(c);
            i += 1;
        }
    }
    flush(&mut job, &mut plain);
    job
}

/// A framed sub-block inside a message (tool call).
fn parse_history(v: &Value) -> Vec<Msg> {
    let session_model = v
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string();
    let arr = v
        .get("history")
        .or_else(|| v.get("messages"))
        .and_then(|x| x.as_array())
        .cloned()
        .or_else(|| v.as_array().cloned())
        .unwrap_or_default();
    arr.iter()
        .filter_map(|m| {
            let role = m.get("role").and_then(|x| x.as_str())?.to_string();
            let meta = m.get("metadata");
            if role == "system" {
                let compacted = meta.and_then(|md| md.get("compacted")).and_then(Value::as_bool) == Some(true)
                    || m.get("content").and_then(Value::as_str).unwrap_or("").starts_with("[Conversation summary");
                return compacted.then(|| Msg { role: "compacted".into(), ..Default::default() });
            }
            if role != "user" && role != "assistant" {
                return None;
            }
            let mut used = Used::default();
            if let Some(md) = meta {
                for k in ["memories_used", "skills_used", "plugins_used"] {
                    if let Some(a) = md.get(k).and_then(|x| x.as_array()) {
                        for x in a.iter().filter_map(Value::as_str) {
                            used.push(k, x);
                        }
                    }
                }
            }
            // Tool calls are persisted in metadata.tool_events — rehydrate them
            // so reopening a chat still shows the tool cards (not just live).
            let tools: Vec<(String, String, i64, String)> = meta
                .and_then(|md| md.get("tool_events"))
                .and_then(|x| x.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|t| {
                            (
                                t.get("tool")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                t.get("command")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                t.get("exit_code").and_then(|x| x.as_i64()).unwrap_or(0),
                                t.get("output")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let metrics = meta
                .map(|md| {
                    let tps = md
                        .get("tokens_per_second")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(0.0);
                    let ctxp = md
                        .get("context_percent")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(0.0);
                    let _ = ctxp;
                    metrics_line(tps, 0.0)
                })
                .unwrap_or_default();
            let ctx_pct = meta
                .and_then(|md| md.get("context_percent"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            let content = m
                .get("content")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            // Stream order isn't persisted, so a reloaded turn shows its tool
            // cards first, then the reply text — the same grouping as before.
            // Live turns interleave correctly via `push_text` / tool_start.
            let mut segs: Vec<Seg> = tools
                .into_iter()
                .map(|(t, c, x, out)| Seg::Tool(t, c, x, out))
                .collect();
            if !content.is_empty() {
                segs.push(Seg::Text(content.clone()));
            }
            Some(Msg {
                db_id: meta
                    .and_then(|md| md.get("_db_id"))
                    .and_then(Value::as_i64)
                    .map(|id| id.to_string()),
                role,
                content,
                thinking: meta
                    .and_then(|md| md.get("thinking"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                segs,
                used,
                metrics,
                ctx: ctx_pct,
                model: meta
                    .and_then(|md| md.get("model"))
                    .and_then(Value::as_str)
                    .map(|m| m.rsplit('/').next().unwrap_or(m).to_string())
                    .unwrap_or_else(|| session_model.clone()),
                atts: Att::parse_list(meta),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn shared() -> Shared {
        let mut sh = Shared::new(
            std::sync::Arc::new(crate::api::Api::new("http://127.0.0.1:1")),
            egui::Context::default(),
            true,
            true,
        );
        sh.current_sid = Some("test-session".into());
        sh
    }

    #[test]
    fn document_stream_keeps_one_tab_and_final_file_binding() {
        let mut sh = shared();
        let mut streamed = None;
        apply_doc_event(
            &json!({"type":"doc_stream_open","title":"report.md","is_new":true}),
            &mut sh,
            &mut streamed,
        );
        apply_doc_event(
            &json!({"type":"doc_stream_delta","content":"Hello"}),
            &mut sh,
            &mut streamed,
        );
        assert_eq!(sh.open_docs[0].content, "Hello");
        apply_doc_event(
            &json!({"type":"doc_update","doc_id":"local-folder/report.md","title":"report.md","content":"Hello world","is_new":true}),
            &mut sh,
            &mut streamed,
        );
        assert_eq!(sh.open_docs.len(), 1);
        assert_eq!(sh.open_docs[0].id, "local-test-session:folder/report.md");
        assert_eq!(
            sh.open_docs[0].remote_sub.as_deref(),
            Some("folder/report.md")
        );
        assert!(!sh.open_docs[0].ai_writing);
    }

    #[test]
    fn rich_document_events_open_original_binary() {
        let mut sh = shared();
        let mut streamed = None;
        apply_doc_event(
            &json!({"type":"doc_stream_open","title":"book.xlsx"}),
            &mut sh,
            &mut streamed,
        );
        apply_doc_event(
            &json!({"type":"doc_update","doc_id":"local-book.xlsx","title":"book.xlsx","content":"values"}),
            &mut sh,
            &mut streamed,
        );
        assert!(sh.open_docs.is_empty());
        assert_eq!(
            sh.open_request,
            Some(("test-session".into(), "book.xlsx".into()))
        );
    }
}
