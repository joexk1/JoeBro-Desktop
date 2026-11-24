//! Sidebar: new-chat + search, chats grouped by project folder (collapsible),
//! per-chat context menu (pin/rename/folder/delete), the bound-folder file
//! tree for the open chat, the workspace tab switcher, and the footer.
use crate::api::R;
use crate::state::{Job, SessionRef, Shared, Tab};
use crate::theme::{self, ty, W};
use crate::widgets;
use crate::{ascii, icons};
use eframe::egui::{self, Align, Layout, RichText, Ui};
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Default)]
pub struct Sidebar {
    sessions: Vec<Value>,
    load: Option<Job<R<Value>>>,
    loaded: bool,
    load_error: Option<String>,
    search: String,
    collapsed_folders: std::collections::HashSet<String>,
    pinned_folders: HashSet<String>,
    prefs_initialized: bool,
    // collapsible sections (Mac `agentsExpanded` / `chatsExpanded` / `workspaceExpanded` / `filesExpanded`)
    agents_closed: bool,
    chats_closed: bool,
    workspace_closed: bool,
    files_closed: bool,

    tree_sid: Option<String>,
    tree_sub: String,
    tree: Vec<Value>,
    tree_job: Option<Job<R<Value>>>,
    tree_error: Option<String>,

    rename_target: Option<String>,
    rename_text: String,
    new_folder_for: Option<String>,
    new_folder_text: String,
    rename_folder_target: Option<String>,
    rename_folder_text: String,

    actions: Vec<Job<R<Value>>>,
    pending_new: Option<Job<R<Value>>>,

    // folder-bind picker (This PC vs Nextcloud/Pi)
    show_bind: bool,
    pi_path: String,
    pi_entries: Vec<Value>,
    pi_browse: Option<Job<R<Value>>>,
    bind_host: usize,
    bind_local: Option<String>,
    bind_err: Option<String>,
    bind_job: Option<Job<R<Value>>>,
    bind_after_new: Option<(String, Option<String>)>,
}

#[derive(Clone)]
enum SidebarDrag {
    Session(String),
}

impl Sidebar {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared, tab: &mut Tab, show_settings: &mut bool) {
        widgets::aero_backdrop(ui, sh, true);
        self.drain(sh);
        self.load_folder_prefs(sh);
        if !self.loaded && self.load.is_none() {
            if self.load_error.is_none() {
                self.reload(sh);
            }
        }
        if sh.reload_sessions {
            sh.reload_sessions = false;
            self.reload(sh);
        }
        if sh.current_sid != self.tree_sid || sh.reload_workdir {
            sh.reload_workdir = false;
            if sh.current_sid != self.tree_sid {
                self.tree_sub.clear();
            }
            self.tree_sid = sh.current_sid.clone();
            self.load_tree(sh);
        }
        if std::mem::take(&mut sh.new_chat_requested) {
            self.new_chat(sh);
            *tab = Tab::Chats;
        }
        // Publish the chat list for the command palette.
        sh.sessions = self
            .sessions
            .iter()
            .filter(|s| !s.get("archived").and_then(|x| x.as_bool()).unwrap_or(false))
            .map(|s| SessionRef {
                id: s.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                name: s.get("name").and_then(Value::as_str).unwrap_or("Untitled").to_string(),
            })
            .collect();
        let p = sh.palette;
        let width = ui.available_width();

        // Header: wordmark + search (palette) + new chat.
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(ty::sized("JoeBro", 15.0, W::Semibold).extra_letter_spacing(-0.2).color(p.ink));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(14.0);
                ui.spacing_mut().item_spacing.x = 8.0;
                if self.pending_new.is_none()
                    && widgets::icon_btn(ui, &p, "plus", 26.0, false)
                        .on_hover_text(format!("New chat ({})", widgets::shortcut_label("N")))
                        .clicked()
                {
                    self.new_chat(sh);
                    *tab = Tab::Chats;
                }
                if widgets::icon_btn(ui, &p, "magnifyingglass", 26.0, false)
                    .on_hover_text(format!("Command palette ({})", widgets::shortcut_label("K")))
                    .clicked()
                {
                    sh.palette_open = true;
                }
            });
        });
        ui.add_space(10.0);
        // Filter field.
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.allocate_ui(egui::vec2(width - 24.0, 0.0), |ui| {
                widgets::field_frame(&p, ui.memory(|m| m.has_focus(egui::Id::new("sidebar_filter")))).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        icons::symbol(ui, "magnifyingglass", 11.0, p.ink3);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search)
                                .id(egui::Id::new("sidebar_filter"))
                                .frame(false)
                                .desired_width(f32::INFINITY)
                                .font(theme::jb(13.0, W::Regular))
                                .hint_text(RichText::new("Filter chats").color(p.ink3)),
                        );
                    });
                });
            });
        });
        ui.add_space(6.0);

        // One scroll: coding agents, chats, files, workspace.
        let footer_h = 43.0;
        let scroll_h = (ui.available_height() - footer_h).max(60.0);
        ui.allocate_ui_with_layout(egui::vec2(width, scroll_h), Layout::top_down(Align::Min), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("sidebar_scroll")
                .max_height(scroll_h)
                .max_width(width)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::none().inner_margin(egui::Margin::symmetric(8.0, 0.0)).show(ui, |ui| {
                        ui.set_width(width - 16.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        // Coding agents, as conversations (only when there are any).
                        if !sh.agent_nav.runs.is_empty() {
                            let mut open = !self.agents_closed;
                            section_header(ui, &p, "Coding agents", &mut open);
                            self.agents_closed = !open;
                            if open {
                                self.coding_agents(ui, sh, tab);
                            }
                        }
                        // Chats.
                        let mut open = !self.chats_closed;
                        section_header(ui, &p, "Chats", &mut open);
                        self.chats_closed = !open;
                        if open {
                            self.chats_list(ui, sh, tab);
                        }
                        // Bound project folder, only on the chat tab (a coding agent has no JoeBro chat files).
                        if *tab == Tab::Chats && sh.agent_nav.open_id.is_none() {
                            self.files_section(ui, sh);
                        }
                        // Workspace.
                        let mut open = !self.workspace_closed;
                        section_header(ui, &p, "Workspace", &mut open);
                        self.workspace_closed = !open;
                        if open {
                            for t in Tab::WORKSPACE {
                                self.nav_row(ui, sh, tab, t);
                            }
                        }
                        ui.add_space(8.0);
                    });
                });
        });

        // Footer: lock, appearance, settings.
        let (line_y, _) = (ui.cursor().top(), ());
        ui.painter().hline(ui.max_rect().x_range(), line_y, egui::Stroke::new(1.0, p.line));
        ui.add_space(1.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.add_space(0.0);
            let (r, resp) = ui.allocate_exact_size(egui::vec2(26.0, 42.0), egui::Sense::hover());
            icons::paint_symbol(ui.painter(), r.center(), "lock", 12.0, p.ink3);
            resp.on_hover_text(if crate::config::PERSONAL { "Your backend. Nothing leaves it." } else { "Local only. Everything stays on this device." });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(12.0);
                ui.spacing_mut().item_spacing.x = 4.0;
                if widgets::icon_btn(ui, &p, "gearshape", 26.0, false)
                    .on_hover_text(format!("Settings ({})", widgets::shortcut_label(",")))
                    .clicked()
                {
                    *show_settings = true;
                }
                let r = widgets::icon_btn(ui, &p, "circle.lefthalf.filled", 26.0, false).on_hover_text("Appearance");
                let id = egui::Id::new("appearance-popover");
                if r.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(id));
                }
                egui::popup::popup_above_or_below_widget(ui, id, &r, egui::AboveOrBelow::Above, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                    ui.set_min_width(8.0 * 24.0 + 7.0 * 8.0 + 28.0);
                    widgets::floating_frame(&p, theme::R_CARD).inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
                        widgets::appearance_tuner(ui, sh);
                    });
                });
            });
        });
        self.folder_dialogs(ui, sh);
    }

    fn coding_agents(&mut self, ui: &mut Ui, sh: &mut Shared, tab: &mut Tab) {
        let p = sh.palette;
        let runs = sh.agent_nav.runs.clone();
        for r in runs {
            let sel = *tab == Tab::Chats && sh.agent_nav.open_id.as_deref() == Some(r.id.as_str());
            let resp = widgets::row(ui, sh, sel, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 9.0;
                    let col = if r.status == "waiting" { p.warn } else if r.live { p.live } else { p.ink3 };
                    ui.allocate_ui(egui::vec2(14.0, 16.0), |ui| {
                        ui.centered_and_justified(|ui| widgets::status_dot(ui, col, r.live, 6.0));
                    });
                    let title = if r.title.is_empty() { "Coding agent".to_string() } else { r.title.clone() };
                    let reserve = if r.status == "waiting" { 62.0 } else { 0.0 };
                    ui.allocate_ui_with_layout(egui::vec2((ui.available_width() - reserve).max(20.0), 16.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.add(egui::Label::new(ty::body(title).color(p.ink)).truncate());
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if r.status == "waiting" {
                            ui.label(ty::sized("Needs you", 11.0, W::Medium).color(p.warn));
                        }
                    });
                });
            });
            if resp.clicked() {
                sh.agent_nav.open(&r.id);
                *tab = Tab::Chats;
            }
        }
    }

    fn nav_row(&mut self, ui: &mut Ui, sh: &mut Shared, tab: &mut Tab, t: Tab) {
        let p = sh.palette;
        let selected = *tab == t || (t == Tab::Memory && *tab == Tab::Brain);
        let dot = (t == Tab::Email && sh.unread_email > 0) || (t == Tab::Inbox && sh.inbox_unread > 0);
        let working = (t == Tab::Research && sh.research_running) || (t == Tab::Agents && sh.agents_running);
        let ink = if selected { p.ink } else { p.ink2 };
        let r = widgets::row(ui, sh, selected, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 9.0;
                let (gr, _) = ui.allocate_exact_size(egui::vec2(20.0, 16.0), egui::Sense::hover());
                icons::paint_symbol(ui.painter(), gr.center(), t.icon(), 14.0, ink);
                ui.label(ty::sized(t.label(), 13.0, if selected { W::Medium } else { W::Regular }).color(ink));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if working {
                        ascii::spinner(ui, 12.0, p.ink2);
                    } else if dot {
                        let (dr, _) = ui.allocate_exact_size(egui::vec2(6.0, 14.0), egui::Sense::hover());
                        ui.painter().circle_filled(dr.center(), 3.0, p.accent);
                    }
                });
            });
        });
        if r.clicked() {
            *tab = t;
        }
    }

    /// The "Files" section (`FileTreeSection`): bound-folder tree, or a "Bind to a folder" row.
    fn files_section(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let p = sh.palette;
        let mut open = !self.files_closed;
        let folder = sh
            .current_sid
            .as_ref()
            .and_then(|sid| self.sessions.iter().find(|s| s.get("id").and_then(Value::as_str) == Some(sid.as_str())))
            .and_then(|s| s.get("workdir").and_then(Value::as_str))
            .map(|w| w.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string())
            .filter(|n| !n.is_empty());
        let bound = folder.is_some() || !self.tree.is_empty() || self.tree_error.is_some() || !self.tree_sub.is_empty();
        // Mac: "Files" + chevron, then the bound folder's name and a small refresh arrow.
        let name_w = match (&folder, bound) {
            (Some(n), _) => ui.painter().layout_no_wrap(n.clone(), theme::jb(10.5, W::Regular), p.ink3).size().x.min(90.0) + 4.0 + 16.0 + 6.0,
            (None, true) => 16.0 + 6.0,
            _ => 0.0,
        };
        let (_, hr) = section_header_inset(ui, &p, "Files", &mut open, name_w);
        self.files_closed = !open;
        if bound {
            let refresh = egui::Rect::from_center_size(egui::pos2(hr.right() - 8.0, hr.center().y), egui::vec2(16.0, 16.0));
            let rr = ui.interact(refresh, ui.id().with("files_refresh"), egui::Sense::click()).on_hover_text("Refresh folder");
            icons::paint_symbol(ui.painter(), refresh.center(), "arrow.clockwise", 9.0, if rr.hovered() { p.ink } else { p.ink3 });
            if rr.clicked() {
                self.load_tree(sh);
            }
            if let Some(n) = &folder {
                let galley = ui.painter().layout_no_wrap(n.clone(), theme::jb(10.5, W::Regular), p.ink3);
                let w = galley.size().x.min(90.0);
                let nr = egui::Rect::from_min_size(egui::pos2(refresh.left() - 4.0 - w, hr.center().y - 8.0), egui::vec2(w, 16.0));
                let nresp = ui.interact(nr, ui.id().with("files_folder"), egui::Sense::click()).on_hover_text("Change folder\u{2026}");
                ui.painter().with_clip_rect(nr).galley(egui::pos2(nr.left(), hr.center().y - galley.size().y / 2.0), galley, p.ink3);
                if nresp.clicked() {
                    self.open_bind(sh);
                }
            }
        }
        self.bind_window(ui.ctx(), sh);
        if !open {
            return;
        }
        if !bound {
            let r = widgets::row(ui, sh, false, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 7.0;
                    let (gr, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    icons::paint_symbol(ui.painter(), gr.center(), "folder.badge.plus", 12.0, p.ink2);
                    ui.label(ty::sized("Bind to a folder", 12.0, W::Regular).color(p.ink2));
                });
            });
            if r.clicked() {
                self.open_bind(sh);
            }
            return;
        }
        self.tree_rows(ui, sh);
    }

    fn drain(&mut self, sh: &mut Shared) {
        if let Some(job) = &mut self.load {
            if let Some(res) = job.take() {
                match res {
                    Ok(v) => {
                        if let Some(error) = response_error(&v) {
                            self.load_error = Some(error);
                        } else {
                            self.sessions = v
                                .as_array()
                                .cloned()
                                .or_else(|| v.get("sessions").and_then(|x| x.as_array()).cloned())
                                .unwrap_or_default();
                            self.loaded = true;
                            self.load_error = None;
                        }
                    }
                    Err(e) => self.load_error = Some(e),
                }
                self.load = None;
            } else if job.done {
                self.load_error = Some("The chat list request ended without a response".into());
                self.load = None;
            }
        }
        if let Some(job) = &mut self.tree_job {
            if let Some(res) = job.take() {
                match res {
                    Ok(v) if response_error(&v).is_none() => {
                        self.tree = v
                            .get("entries")
                            .and_then(|x| x.as_array())
                            .cloned()
                            .unwrap_or_default();
                        self.tree_error = None;
                    }
                    Ok(v) => self.tree_error = response_error(&v),
                    Err(e) => self.tree_error = Some(e),
                }
                self.tree_job = None;
            } else if job.done {
                self.tree_error = Some("The folder request ended without a response".into());
                self.tree_job = None;
            }
        }
        let mut action_errors = Vec::new();
        let mut action_finished = false;
        self.actions.retain_mut(|job| {
            if let Some(res) = job.take() {
                action_finished = true;
                match res {
                    Ok(v) => {
                        if let Some(error) = response_error(&v) {
                            action_errors.push(error);
                        }
                    }
                    Err(error) => action_errors.push(error),
                }
                false
            } else if job.done {
                action_finished = true;
                action_errors.push("The sidebar action ended without a response".into());
                false
            } else {
                true
            }
        });
        for error in action_errors {
            sh.err(format!("Sidebar: {error}"));
        }
        if action_finished && self.actions.is_empty() {
            self.load_tree(sh);
            self.reload(sh);
        }
        if let Some(job) = &mut self.pending_new {
            if let Some(res) = job.take() {
                match res {
                    Ok(v) => {
                        if let Some(error) = response_error(&v) {
                            sh.err(format!("New chat: {error}"));
                        } else if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                            if sh.agent_nav.open_id.is_some() {
                                sh.agent_nav.close();
                            }
                            sh.current_sid = Some(id.to_string());
                            if let Some((path, dev)) = self.bind_after_new.take() {
                                let sid = id.to_string();
                                self.bind_job = Some(sh.spawn(move |a| a.bind_workdir(&sid, &path, dev.as_deref())));
                            }
                        } else {
                            sh.err("New chat: server returned no chat id");
                        }
                    }
                    Err(error) => sh.err(format!("New chat: {error}")),
                }
                self.pending_new = None;
                self.reload(sh);
            } else if job.done {
                self.pending_new = None;
                sh.err("New chat request ended without a response");
            }
        }
    }

    fn reload(&mut self, sh: &Shared) {
        self.load_error = None;
        self.loaded = false;
        self.load = Some(sh.spawn(|a| a.sessions()));
    }

    fn load_folder_prefs(&mut self, sh: &Shared) {
        if self.prefs_initialized || !sh.prefs.is_object() {
            return;
        }
        self.prefs_initialized = true;
        self.collapsed_folders = pref_string_set(&sh.prefs, "collapsed_chat_folders");
        self.pinned_folders = pref_string_set(&sh.prefs, "pinned_chat_folders");
        if let Some(root) = sh
            .prefs
            .get("nextcloud_root")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
        {
            self.pi_path = root.to_string();
        }
    }

    fn save_folder_set(&mut self, sh: &mut Shared, key: &str, values: &HashSet<String>) {
        let mut values: Vec<String> = values.iter().cloned().collect();
        values.sort();
        let value = json!(values);
        if !sh.prefs.is_object() {
            sh.prefs = json!({});
        }
        sh.prefs[key] = value.clone();
        let key = key.to_string();
        self.actions
            .push(sh.spawn(move |a| a.set_pref(&key, value)));
    }

    fn toggle_folder(&mut self, folder: &str, sh: &mut Shared) {
        if !self.collapsed_folders.remove(folder) {
            self.collapsed_folders.insert(folder.to_string());
        }
        let values = self.collapsed_folders.clone();
        self.save_folder_set(sh, "collapsed_chat_folders", &values);
    }

    fn toggle_pin_folder(&mut self, folder: &str, sh: &mut Shared) {
        if !self.pinned_folders.remove(folder) {
            self.pinned_folders.insert(folder.to_string());
        }
        let values = self.pinned_folders.clone();
        self.save_folder_set(sh, "pinned_chat_folders", &values);
    }

    fn rename_folder(&mut self, old: &str, new: &str, sh: &mut Shared) {
        let new = new.trim();
        if new.is_empty() || new == old {
            return;
        }
        if self.collapsed_folders.remove(old) {
            self.collapsed_folders.insert(new.to_string());
            let values = self.collapsed_folders.clone();
            self.save_folder_set(sh, "collapsed_chat_folders", &values);
        }
        if self.pinned_folders.remove(old) {
            self.pinned_folders.insert(new.to_string());
            let values = self.pinned_folders.clone();
            self.save_folder_set(sh, "pinned_chat_folders", &values);
        }
        for session in &mut self.sessions {
            if session.get("folder").and_then(Value::as_str) == Some(old) {
                session["folder"] = json!(new);
            }
        }
        let old = old.to_string();
        let new = new.to_string();
        self.actions
            .push(sh.spawn(move |a| a.rename_folder(&old, &new)));
    }

    fn move_chat(&mut self, sh: &mut Shared, sid: &str, folder: &str, order: f64) {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|s| s.get("id").and_then(Value::as_str) == Some(sid))
        {
            session["folder"] = json!(folder);
            session["sort_order"] = json!(order);
        }
        let sid = sid.to_string();
        let folder = folder.to_string();
        self.actions
            .push(sh.spawn(move |a| a.move_session(&sid, &folder, order)));
    }

    fn order_at(&self, target: &str, folder: &str, below: bool, dragged: &str) -> f64 {
        let mut group: Vec<(String, f64)> = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.get("folder").and_then(Value::as_str).unwrap_or("") == folder)
            .filter(|(_, s)| s.get("id").and_then(Value::as_str) != Some(dragged))
            .map(|(i, s)| {
                (
                    s.get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    s.get("sort_order")
                        .and_then(Value::as_f64)
                        .unwrap_or(i as f64),
                )
            })
            .collect();
        group.sort_by(|a, b| a.1.total_cmp(&b.1));
        let Some(index) = group.iter().position(|(id, _)| id == target) else {
            return if below {
                group.last().map(|x| x.1 + 1.0).unwrap_or(0.0)
            } else {
                group.first().map(|x| x.1 - 1.0).unwrap_or(0.0)
            };
        };
        let target_order = group[index].1;
        if below {
            let next = group
                .get(index + 1)
                .map(|x| x.1)
                .unwrap_or(target_order + 2.0);
            (target_order + next) / 2.0
        } else {
            let prev = index
                .checked_sub(1)
                .and_then(|i| group.get(i))
                .map(|x| x.1)
                .unwrap_or(target_order - 2.0);
            (prev + target_order) / 2.0
        }
    }

    fn folder_edge_order(&self, folder: &str, top: bool) -> f64 {
        let orders: Vec<f64> = self
            .sessions
            .iter()
            .filter(|s| s.get("folder").and_then(Value::as_str).unwrap_or("") == folder)
            .filter_map(|s| s.get("sort_order").and_then(Value::as_f64))
            .collect();
        if top {
            orders
                .into_iter()
                .min_by(f64::total_cmp)
                .map(|x| x - 1.0)
                .unwrap_or(0.0)
        } else {
            orders
                .into_iter()
                .max_by(f64::total_cmp)
                .map(|x| x + 1.0)
                .unwrap_or(0.0)
        }
    }

    fn folder_dialogs(&mut self, ui: &Ui, sh: &mut Shared) {
        if let Some(old) = self.rename_folder_target.clone() {
            let mut open = true;
            let mut commit = false;
            egui::Window::new("Rename chat folder")
                .collapsible(false)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Rename {old}"));
                    let resp = ui.text_edit_singleline(&mut self.rename_folder_text);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        commit = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Rename").clicked() {
                            commit = true;
                        }
                        if ui.button("Cancel").clicked() {
                            self.rename_folder_target = None;
                        }
                    });
                });
            if commit {
                let new = self.rename_folder_text.trim().to_string();
                self.rename_folder_target = None;
                self.rename_folder(&old, &new, sh);
            } else if !open {
                self.rename_folder_target = None;
            }
        }
        if let Some(sid) = self.new_folder_for.clone() {
            let mut open = true;
            let mut commit = false;
            egui::Window::new("Create chat folder")
                .collapsible(false)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.label("Move this chat to a new folder");
                    let resp = ui.text_edit_singleline(&mut self.new_folder_text);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        commit = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            commit = true;
                        }
                        if ui.button("Cancel").clicked() {
                            self.new_folder_for = None;
                        }
                    });
                });
            if commit {
                let folder = self.new_folder_text.trim().to_string();
                self.new_folder_for = None;
                self.new_folder_text.clear();
                if !folder.is_empty() {
                    let order = self.folder_edge_order(&folder, true);
                    self.move_chat(sh, &sid, &folder, order);
                }
            } else if !open {
                self.new_folder_for = None;
            }
        }
    }

    fn load_tree(&mut self, sh: &Shared) {
        self.tree.clear();
        self.tree_error = None;
        if let Some(sid) = sh.current_sid.clone() {
            let sub = self.tree_sub.clone();
            self.tree_job = Some(sh.spawn(move |a| a.workdir_list(&sid, &sub)));
        }
    }

    fn new_chat(&mut self, sh: &mut Shared) {
        if self.pending_new.is_some() {
            return;
        }
        let (model, ep_url, ep_id) = sh
            .selected_model
            .as_ref()
            .map(|m| {
                (
                    m.model_id.clone(),
                    m.endpoint_url.clone(),
                    m.endpoint_id.clone(),
                )
            })
            .unwrap_or_default();
        let name = sh.new_chat_title.take().unwrap_or_else(|| "New Chat".to_string());
        self.pending_new =
            Some(sh.spawn(move |a| a.create_session(&name, &model, &ep_url, &ep_id)));
    }

    fn chats_list(&mut self, ui: &mut Ui, sh: &mut Shared, tab: &mut Tab) {
        // Keep the chat header's title in sync with the open session.
        sh.current_title = sh.current_sid.clone().and_then(|sid| {
            self.sessions
                .iter()
                .find(|s| s.get("id").and_then(|x| x.as_str()) == Some(sid.as_str()))
                .and_then(|s| s.get("name").and_then(|x| x.as_str()).map(String::from))
        });
        let q = self.search.to_lowercase();
        let mut visible: Vec<Value> = self
            .sessions
            .iter()
            .filter(|s| !s.get("archived").and_then(|x| x.as_bool()).unwrap_or(false))
            .filter(|s| {
                q.is_empty()
                    || s.get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
            })
            .cloned()
            .collect();
        // Pinned chats float to the top (stable within each group).
        let pin = |s: &Value| {
            s.get("is_important")
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
        };
        visible.sort_by(|a, b| pin(b).cmp(&pin(a)));

        let mut folders: Vec<String> = vec![];
        for s in &visible {
            let f = s
                .get("folder")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if !f.is_empty() && !folders.contains(&f) {
                folders.push(f);
            }
        }
        folders.sort_by(|a, b| {
            let pinned_a = self.pinned_folders.contains(a);
            let pinned_b = self.pinned_folders.contains(b);
            pinned_b
                .cmp(&pinned_a)
                .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
        });
        for folder in &folders {
            let collapsed = self.collapsed_folders.contains(folder);
            let count = visible
                .iter()
                .filter(|s| s.get("folder").and_then(|x| x.as_str()).unwrap_or("") == folder)
                .count();
            // Flat disclosure row: chevron, folder icon, name, count (`folderHeader`).
            let p = sh.palette;
            let hdr = widgets::row(ui, sh, false, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (cr, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                    let k = theme::anim(ui.ctx(), egui::Id::new(("folder-chev", folder)), !collapsed, theme::SNAP);
                    let ang = k * std::f32::consts::FRAC_PI_2;
                    let c = cr.center();
                    let rot = |x: f32, y: f32| egui::pos2(c.x + x * ang.cos() - y * ang.sin(), c.y + x * ang.sin() + y * ang.cos());
                    let st = egui::Stroke::new(1.3, p.ink3);
                    ui.painter().line_segment([rot(-1.5, -3.2), rot(1.5, 0.0)], st);
                    ui.painter().line_segment([rot(1.5, 0.0), rot(-1.5, 3.2)], st);
                    icons::symbol(ui, "folder", 11.0, p.ink2);
                    ui.label(ty::sized(folder.as_str(), 12.0, W::Medium).color(p.ink));
                    ui.label(ty::sized(count.to_string(), 10.5, W::Regular).color(p.ink3));
                    if self.pinned_folders.contains(folder) {
                        icons::symbol(ui, "pin.fill", 8.0, p.ink3);
                    }
                });
            });
            let dropped = hdr.dnd_release_payload::<SidebarDrag>();
            if let Some(SidebarDrag::Session(dragged)) = dropped.as_deref() {
                let order = self.folder_edge_order(folder, true);
                self.move_chat(sh, dragged, folder, order);
            }
            hdr.context_menu(|ui| {
                if ui.button("Rename folder…").clicked() {
                    self.rename_folder_target = Some(folder.clone());
                    self.rename_folder_text = folder.clone();
                    ui.close_menu();
                }
                let pinned = self.pinned_folders.contains(folder);
                if ui
                    .button(if pinned { "Unpin folder" } else { "Pin folder" })
                    .clicked()
                {
                    self.toggle_pin_folder(folder, sh);
                    ui.close_menu();
                }
            });
            if hdr.clicked() {
                self.toggle_folder(folder, sh);
            }
            if !collapsed {
                for s in visible
                    .iter()
                    .filter(|s| s.get("folder").and_then(|x| x.as_str()).unwrap_or("") == folder)
                {
                    self.session_row(ui, sh, s, tab, true);
                }
                let dropped = sidebar_drop_target(ui, "Drop at end of folder");
                if let Some(SidebarDrag::Session(dragged)) = dropped.as_deref() {
                    let order = self.folder_edge_order(folder, false);
                    self.move_chat(sh, dragged, folder, order);
                }
            }
            ui.add_space(2.0);
        }
        let ungrouped: Vec<Value> = visible
            .iter()
            .filter(|s| {
                s.get("folder")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .is_empty()
            })
            .cloned()
            .collect();
        for session in &ungrouped {
            self.session_row(ui, sh, session, tab, false);
        }
        let dropped = sidebar_drop_target(
            ui,
            if ungrouped.is_empty() || ui.ctx().dragged_id().is_some() {
                "Drop here to remove from a folder"
            } else {
                ""
            },
        );
        if let Some(SidebarDrag::Session(dragged)) = dropped.as_deref() {
            let order = self.folder_edge_order("", false);
            self.move_chat(sh, dragged, "", order);
        }
        if visible.is_empty() {
            ui.add_space(6.0);
            if let Some(error) = self.load_error.clone() {
                ui.label(
                    RichText::new(format!("Couldn't load chats: {error}"))
                        .small()
                        .weak(),
                );
                if ui.small_button("Retry").clicked() {
                    self.reload(sh);
                }
            } else {
                ui.label(
                    RichText::new(if self.loaded {
                        "No chats — + New to start"
                    } else {
                        "Loading…"
                    })
                    .weak(),
                );
            }
        }
    }

    fn session_row(
        &mut self,
        ui: &mut Ui,
        sh: &mut Shared,
        s: &Value,
        tab: &mut Tab,
        indented: bool,
    ) {
        let id = s
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let name = s
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("Untitled")
            .to_string();
        let pinned = s
            .get("is_important")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let selected = *tab == Tab::Chats && sh.agent_nav.open_id.is_none() && sh.current_sid.as_deref() == Some(&id);

        // Inline rename mode.
        if self.rename_target.as_deref() == Some(&id) {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.rename_text).desired_width(f32::INFINITY),
            );
            if resp.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !self.rename_text.trim().is_empty()
                {
                    let (sid, txt) = (id.clone(), self.rename_text.clone());
                    self.actions
                        .push(sh.spawn(move |a| a.rename_session(&sid, &txt)));
                }
                self.rename_target = None;
            }
            return;
        }

        let label = name.clone();
        let p = sh.palette;
        let resp = ui
            .push_id(("sidebar-chat", &id), |ui| {
                widgets::draggable_row(ui, sh, selected, |ui| {
                    ui.horizontal(|ui| {
                        if indented {
                            ui.add_space(12.0);
                        }
                        let ink = if selected { p.ink } else { p.ink2 };
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let tail = time_label(s);
                        let is_agent = s.get("mode").and_then(Value::as_str) == Some("agent");
                        let reserve = 8.0 + 12.0 * (pinned as i32 as f32) + 19.0 * (is_agent as i32 as f32) + (tail.chars().count() as f32 * 6.2) + 8.0;
                        ui.allocate_ui_with_layout(
                            egui::vec2((ui.available_width() - reserve).max(30.0), 16.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(ty::body(&label).color(ink)).truncate()).on_hover_text(label.clone());
                            },
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add(egui::Label::new(ty::sized(tail, 10.5, W::Regular).color(p.ink3)).wrap_mode(egui::TextWrapMode::Extend));
                            if is_agent {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(11.0, 11.0), egui::Sense::hover());
                                icons::paint_glyph(ui.painter(), r.center(), icons::JbGlyph::Tools, 11.0, p.ink3);
                            }
                            if pinned {
                                icons::symbol(ui, "pin.fill", 9.0, p.accent);
                            }
                        });
                    });
                })
            })
            .inner;
        // One response owns clicks and drags. A drag-only overlay swallows
        // clicks even when the user never starts dragging.
        resp.dnd_set_drag_payload(SidebarDrag::Session(id.clone()));
        let dropped = resp.dnd_release_payload::<SidebarDrag>();
        if let Some(SidebarDrag::Session(dragged)) = dropped.as_deref() {
            let folder = s
                .get("folder")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let below = ui
                .ctx()
                .pointer_latest_pos()
                .map(|p| p.y >= resp.rect.center().y)
                .unwrap_or(false);
            let sort = self.order_at(&id, &folder, below, dragged);
            self.move_chat(sh, dragged, &folder, sort);
        }
        if resp.clicked() {
            sh.select_chat(&id);
            *tab = Tab::Chats;
        }
        resp.context_menu(|ui| {
            if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                let sid = id.clone();
                self.actions
                    .push(sh.spawn(move |a| a.set_session_important(&sid, !pinned)));
                ui.close_menu();
            }
            if ui.button("Rename…").clicked() {
                self.rename_target = Some(id.clone());
                self.rename_text = name.clone();
                ui.close_menu();
            }
            ui.menu_button("Move to folder", |ui| {
                let mut fs: Vec<String> = self
                    .sessions
                    .iter()
                    .filter_map(|s| s.get("folder").and_then(|x| x.as_str()))
                    .filter(|f| !f.is_empty())
                    .map(String::from)
                    .collect();
                fs.sort();
                fs.dedup();
                for f in fs {
                    if ui.button(&f).clicked() {
                        let (sid, fc) = (id.clone(), f.clone());
                        self.actions
                            .push(sh.spawn(move |a| a.set_session_folder(&sid, &fc)));
                        ui.close_menu();
                    }
                }
                ui.separator();
                if ui.button("New folder…").clicked() {
                    self.new_folder_for = Some(id.clone());
                    self.new_folder_text.clear();
                    ui.close_menu();
                }
            });
            if ui.button("Remove from folder").clicked() {
                let sid = id.clone();
                self.actions
                    .push(sh.spawn(move |a| a.set_session_folder(&sid, "")));
                ui.close_menu();
            }
            if ui.button("Delete").clicked() {
                let sid = id.clone();
                if sh.current_sid.as_deref() == Some(&id) {
                    sh.current_sid = None;
                }
                self.actions.push(sh.spawn(move |a| a.delete_session(&sid)));
                ui.close_menu();
            }
        });
    }

    fn tree_rows(&mut self, ui: &mut Ui, sh: &mut Shared) {
        if !self.tree_sub.is_empty() {
            ui.horizontal(|ui| {
                if ui
                    .small_button("↑")
                    .on_hover_text("Parent folder")
                    .clicked()
                {
                    self.tree_sub = self
                        .tree_sub
                        .rsplit_once('/')
                        .map(|(parent, _)| parent.to_string())
                        .unwrap_or_default();
                    self.load_tree(sh);
                }
                ui.add(egui::Label::new(RichText::new(&self.tree_sub).small()).truncate());
            });
        }
        {
            {
                if let Some(error) = self.tree_error.clone() {
                    ui.label(
                        RichText::new(format!("Folder unavailable: {error}"))
                            .small()
                            .weak(),
                    );
                    if ui.small_button("Retry folder").clicked() {
                        self.load_tree(sh);
                    }
                } else if self.tree_job.is_some() {
                    ui.label(RichText::new("Loading folder…").small().weak());
                } else if self.tree.is_empty() {
                }
                for entry in self.tree.clone() {
                    let name = entry
                        .get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let is_dir = entry.get("type").and_then(|x| x.as_str()) == Some("dir")
                        || entry.get("is_dir").and_then(|x| x.as_bool()).unwrap_or(false);
                    let glyph = if is_dir {
                        theme::icon::FOLDER
                    } else {
                        theme::icon::FILE
                    };
                    let sub = if self.tree_sub.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{name}", self.tree_sub)
                    };
                    let response = ui
                        .horizontal(|ui| {
                            ui.add_space(4.0);
                            ui.add_sized(
                                [14.0, 16.0],
                                egui::Label::new(widgets::ico(glyph, 10.5, sh).color(if is_dir {
                                    sh.palette.accent_hi
                                } else {
                                    sh.palette.ink_faint
                                })),
                            );
                            ui.add(
                                egui::Button::new(
                                    RichText::new(&name).size(12.5).family(egui::FontFamily::Proportional).color(sh.palette.ink_dim),
                                )
                                .frame(false),
                            )
                        })
                        .inner;
                    if response.clicked() {
                        if is_dir {
                            self.tree_sub = sub.clone();
                            self.load_tree(sh);
                        } else if let Some(sid) = sh.current_sid.clone() {
                            sh.open_request = Some((sid, sub.clone()));
                            sh.editor_visible = true;
                        }
                    }
                    if !is_dir {
                        response.context_menu(|ui| {
                            if ui.button("Delete file").clicked() {
                                if let Some(sid) = sh.current_sid.clone() {
                                    let path = sub.clone();
                                    self.actions
                                        .push(sh.spawn(move |a| a.delete_file(&sid, &path)));
                                }
                                ui.close_menu();
                            }
                        });
                    }
                }
            }
        }
    }

    /// Open the bind sheet (Mac `showBind`): works with or without an open chat.
    fn open_bind(&mut self, sh: &Shared) {
        self.show_bind = true;
        self.bind_err = None;
        self.bind_local = None;
        if self.pi_path.is_empty() || self.pi_entries.is_empty() {
            let root = sh
                .prefs
                .get("nextcloud_root")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(option_env!("JOEBRO_PI_ROOT").unwrap_or("/"))
                .to_string();
            self.pi_path = root.clone();
            self.pi_browse = Some(sh.spawn(move |a| a.fs_browse(&root, None)));
        }
    }

    /// Start the bind: creates a chat first when none is open (Mac `bindWorkdir`).
    fn start_bind(&mut self, sh: &mut Shared, path: String, device: Option<String>) {
        self.bind_err = None;
        match sh.current_sid.clone() {
            Some(sid) => {
                self.bind_job = Some(sh.spawn(move |a| a.bind_workdir(&sid, &path, device.as_deref())));
            }
            None => {
                self.bind_after_new = Some((path, device));
                self.new_chat(sh);
            }
        }
    }

    /// Bind picker: Pi · Nextcloud (browse the Pi's fs) or This PC (in-app browser).
    fn bind_window(&mut self, ctx: &egui::Context, sh: &mut Shared) {
        if let Some(job) = &mut self.pi_browse {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) if response_error(&v).is_none() => {
                        if let Some(p) = v.get("path").and_then(|x| x.as_str()) {
                            self.pi_path = p.to_string();
                        }
                        let mut e = v.get("entries").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                        e.retain(|e| {
                            e.get("is_dir").and_then(|x| x.as_bool()).unwrap_or(false)
                                || e.get("type").and_then(|x| x.as_str()) == Some("dir")
                        });
                        e.sort_by_key(|e| e.get("name").and_then(Value::as_str).unwrap_or("").to_lowercase());
                        self.pi_entries = e;
                        self.bind_err = None;
                    }
                    Ok(v) => self.bind_err = response_error(&v),
                    Err(e) => self.bind_err = Some(e),
                }
                self.pi_browse = None;
            } else if job.done {
                self.pi_browse = None;
            }
        }
        if let Some(job) = &mut self.bind_job {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) if response_error(&v).is_none() => {
                        self.show_bind = false;
                        sh.reload_workdir = true;
                    }
                    Ok(v) => self.bind_err = response_error(&v),
                    Err(e) => self.bind_err = Some(e),
                }
                self.bind_job = None;
            } else if job.done {
                self.bind_job = None;
                self.bind_err = Some("The bind request ended without a response".into());
            }
        }
        if let Some(dir) = sh.take_picked("bind_local") {
            self.bind_local = Some(dir.to_string_lossy().into_owned());
        }
        if !self.show_bind {
            return;
        }
        let p = sh.palette;
        let cancel = false;
        let bind: Option<(String, Option<String>)> = None;
        let mut nav: Option<String> = None;
        let mut choose_local = false;
        let busy = self.bind_job.is_some() || self.bind_after_new.is_some();
        let target = if self.bind_host == 1 {
            (self.bind_local.clone().unwrap_or_default(), Some(crate::config::device_id()))
        } else {
            (self.pi_path.clone(), None)
        };
        let mut bind2: Option<(String, Option<String>)> = None;
        let mut cancel2 = false;
        let mut host = self.bind_host;
        let can_bind = !busy && if self.bind_host == 1 { self.bind_local.is_some() } else { !self.pi_path.is_empty() };
        let esc = crate::panels::workkit::sheet(
            ctx,
            &p,
            "bind_folder",
            "Bind Project Folder",
            520.0,
            |ui| {
                if widgets::button_ex(ui, &p, widgets::BtnKind::Primary, true, can_bind, "Bind Folder").clicked() {
                    bind2 = Some(target.clone());
                }
                if widgets::button_ex(ui, &p, widgets::BtnKind::Ghost, true, true, "Cancel").clicked() {
                    cancel2 = true;
                }
                if busy {
                    ascii::spinner(ui, 12.0, p.ink3);
                }
            },
            |ui| {
                widgets::segmented_n(ui, sh, "bind_host", &["Pi \u{b7} Nextcloud", "This PC"], &mut host);
                if self.bind_host == 1 {
                    ui.vertical_centered(|ui| {
                        ui.add_space(12.0);
                        icons::symbol(ui, "laptopcomputer", 40.0, p.accent);
                        ui.add_space(8.0);
                        let (txt, col) = match &self.bind_local {
                            Some(t) => (t.clone(), p.ink),
                            None => ("Choose a folder on this computer".to_string(), p.ink2),
                        };
                        ui.label(ty::sized(txt, 12.0, W::Regular).color(col));
                        ui.add_space(8.0);
                        if widgets::button(ui, &p, widgets::BtnKind::Primary, "Choose Folder\u{2026}").clicked() {
                            choose_local = true;
                        }
                        ui.add_space(8.0);
                        ui.label(ty::caption("The agent works on this computer through its bridge. The computer must be on Tailscale and running its bridge agent.").color(p.ink2));
                    });
                } else {
                    ui.horizontal(|ui| {
                        let at_root = self.pi_path.is_empty() || self.pi_path == "/";
                        if widgets::icon_btn(ui, &p, "chevron.up", 26.0, false).on_hover_text("Parent folder").clicked() && !at_root {
                            nav = Some(
                                std::path::Path::new(&self.pi_path)
                                    .parent()
                                    .and_then(|p| p.to_str())
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or("/")
                                    .to_string(),
                            );
                        }
                        ui.label(ty::sized(if self.pi_path.is_empty() { "~" } else { &self.pi_path }, 11.0, W::Regular).color(p.ink2));
                        if self.pi_browse.is_some() {
                            ascii::spinner(ui, 12.0, p.ink3);
                        }
                    });
                    egui::ScrollArea::vertical().id_salt("pi_browse").max_height(240.0).auto_shrink([false, false]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for e in &self.pi_entries {
                            let name = e.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                            let r = widgets::hover_row(ui, &p, false, theme::R_CTRL, |ui| {
                                ui.horizontal(|ui| {
                                    icons::symbol(ui, "folder", 13.0, p.accent);
                                    ui.label(ty::sized(&name, 12.5, W::Regular).color(p.ink));
                                });
                            });
                            if r.clicked() {
                                nav = Some(format!("{}/{}", self.pi_path.trim_end_matches('/'), name));
                            }
                        }
                        if self.pi_entries.is_empty() && self.pi_browse.is_none() {
                            ui.label(ty::caption("No sub-folders").color(p.ink3));
                        }
                    });
                    ui.label(ty::caption("Binds this folder on the Pi (where Nextcloud is mounted). The agent works on the Pi.").color(p.ink2));
                }
                if let Some(e) = &self.bind_err {
                    ui.label(ty::caption(e).color(p.danger));
                }
            },
        );
        self.bind_host = host;
        let bind = bind.or(bind2);
        let cancel = cancel || cancel2;
        if choose_local {
            let start = self.bind_local.as_ref().map(std::path::PathBuf::from);
            sh.picker.open("bind_local", "Choose Folder", crate::pickers::PickMode::Folder, start);
        }
        if let Some(path) = nav {
            if self.pi_browse.is_none() {
                self.pi_path = path.clone();
                self.pi_browse = Some(sh.spawn(move |a| a.fs_browse(&path, None)));
            }
        }
        if let Some((path, dev)) = bind {
            self.start_bind(sh, path, dev);
        }
        if (cancel || esc) && !sh.picker.is_open() {
            self.show_bind = false;
        }
    }
}
fn sidebar_drop_target(ui: &mut Ui, label: &str) -> Option<std::sync::Arc<SidebarDrag>> {
    let dragging = egui::DragAndDrop::has_payload_of_type::<SidebarDrag>(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), if dragging { 16.0 } else { 0.0 }),
        egui::Sense::hover(),
    );
    if dragging {
        let stroke = ui.visuals().widgets.active.bg_stroke;
        ui.painter()
            .rect_filled(rect, 2.0, ui.visuals().widgets.hovered.bg_fill);
        ui.painter().rect_stroke(rect, 2.0, stroke);
        if !label.is_empty() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::TextStyle::Small.resolve(ui.style()),
                ui.visuals().text_color(),
            );
        }
    }
    response.dnd_release_payload::<SidebarDrag>()
}

fn response_error(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(Value::as_str)
        .filter(|error| !error.is_empty())
        .map(str::to_string)
        .or_else(|| {
            (value.get("ok").and_then(Value::as_bool) == Some(false)
                || value.get("success").and_then(Value::as_bool) == Some(false))
            .then(|| "The backend did not complete the request".to_string())
        })
}

fn pref_string_set(prefs: &Value, key: &str) -> HashSet<String> {
    prefs
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Collapsible section header (`sectionHeader`): eyebrow + chevron that rotates open. Returns true if toggled.
fn section_header(ui: &mut Ui, p: &theme::Palette, text: &str, open: &mut bool) -> bool {
    section_header_inset(ui, p, text, open, 0.0).0
}

/// `section_header` with the chevron pulled `inset` pt in from the right (room for trailing items); also returns the row rect.
fn section_header_inset(ui: &mut Ui, p: &theme::Palette, text: &str, open: &mut bool, inset: f32) -> (bool, egui::Rect) {
    ui.add_space(14.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::click());
    let full = rect.shrink2(egui::vec2(10.0, 0.0));
    let rect = egui::Rect::from_min_max(full.min, egui::pos2(full.right() - inset, full.bottom()));
    ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, text, theme::jb(12.0, W::Semibold), p.ink2);
    // chevron.right, 8 pt semibold: rotated 90deg when open.
    let k = theme::anim(ui.ctx(), resp.id.with("chev"), *open, theme::SNAP);
    let c = egui::pos2(rect.right() - 3.0, rect.center().y);
    let ang = k * std::f32::consts::FRAC_PI_2;
    let rot = |x: f32, y: f32| egui::pos2(c.x + x * ang.cos() - y * ang.sin(), c.y + x * ang.sin() + y * ang.cos());
    let st = egui::Stroke::new(1.3, p.ink3);
    ui.painter().line_segment([rot(-1.5, -3.2), rot(1.5, 0.0)], st);
    ui.painter().line_segment([rot(1.5, 0.0), rot(-1.5, 3.2)], st);
    ui.add_space(4.0);
    if resp.clicked() {
        *open = !*open;
        return (true, full);
    }
    (false, full)
}

/// Compact mono timestamp: now, 5m, 3h, 2d, then "14 Nov" (`SessionRow.timeLabel`).
fn time_label(s: &Value) -> String {
    let raw = ["created_at", "updated_at", "last_active", "created"]
        .iter()
        .find_map(|k| s.get(*k).filter(|v| !v.is_null()));
    let secs = match raw {
        Some(Value::Number(n)) => n.as_f64().map(|x| if x > 1e11 { x / 1000.0 } else { x }),
        Some(Value::String(t)) => widgets::iso_to_secs(t).or_else(|| t.parse::<f64>().ok().map(|x| if x > 1e11 { x / 1000.0 } else { x })),
        _ => None,
    };
    let Some(t) = secs else { return String::new() };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let d = now - t;
    if d < 60.0 {
        "now".into()
    } else if d < 3600.0 {
        format!("{}m", (d / 60.0) as i64)
    } else if d < 86400.0 {
        format!("{}h", (d / 3600.0) as i64)
    } else if d < 86400.0 * 7.0 {
        format!("{}d", (d / 86400.0) as i64)
    } else {
        widgets::fmt_date_abbr(t).split(',').next().unwrap_or("").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_row_selects_on_click_without_dragging() {
        let ctx = egui::Context::default();
        let mut shared = Shared::new(
            std::sync::Arc::new(crate::api::Api::new("http://127.0.0.1:1")),
            ctx.clone(),
            true,
            true,
        );
        let mut sidebar = Sidebar::default();
        let session = serde_json::json!({"id":"click-test","name":"Clickable chat"});
        let mut tab = Tab::Notes;
        let point = egui::pos2(60.0, 22.0);
        for pressed in [None, Some(true), Some(false)] {
            let mut raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(260.0, 200.0),
                )),
                ..Default::default()
            };
            raw.events.push(egui::Event::PointerMoved(point));
            if let Some(pressed) = pressed {
                raw.events.push(egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                });
            }
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    sidebar.session_row(ui, &mut shared, &session, &mut tab, false)
                });
            });
        }
        assert_eq!(shared.current_sid.as_deref(), Some("click-test"));
        assert!(tab == Tab::Chats);
    }
}
