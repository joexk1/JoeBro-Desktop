//! Email panel — folder switcher, All/Unread/Flagged filter chips, client-side
//! search, two-pane list + reading pane, mark read/unread + archive + delete
//! (single, bulk and context-menu), and a compose / reply / forward sheet.
//!
//! HTML bodies can be opened in a browser with remote resources blocked.
//! Attachments download through the backend's read-only attachment endpoint.
use crate::api::R;
use crate::state::{Job, Shared};
use crate::theme;
use crate::widgets;
use eframe::egui::{self, Align, FontId, Layout, Ui};
use serde_json::{json, Value};
use std::collections::HashSet;

pub struct EmailPanel {
    // data
    folders: Vec<String>,
    folders_loaded: bool,
    folder: String,
    filter: &'static str, // "all" | "unread" | "flagged"
    search: String,
    emails: Vec<Value>,
    loaded: bool,
    load_err: Option<String>,
    configured: Option<bool>,

    // selection / reading pane
    selected_uid: Option<String>,
    detail: Option<Value>,
    detail_err: Option<String>,
    checked: HashSet<String>,

    // compose sheet
    compose: Option<Compose>,

    // in-flight jobs
    folders_job: Option<Job<R<Value>>>,
    status_job: Option<Job<R<Value>>>,
    list_job: Option<Job<R<Value>>>,
    read_job: Option<Job<R<Value>>>,
    send_job: Option<Job<R<Value>>>,
    attachment_jobs: Vec<(String, Job<R<Value>>)>,
    attachment_download: Option<Job<Result<String, String>>>,
    attachment_error: Option<String>,
    html_job: Option<Job<Result<(), String>>>,
    pending_download: Option<(String, String, i64)>,
    unread_job: Option<Job<R<Value>>>,
    actions: Vec<Job<R<Value>>>,
    pending_reload: bool,
    summary: Option<String>,
    summary_uid: Option<String>,
    summary_job: Option<Job<R<Value>>>,
    draft_job: Option<Job<R<Value>>>,
    ai_error: Option<String>,
}

impl Default for EmailPanel {
    fn default() -> Self {
        EmailPanel {
            folders: vec!["INBOX".to_string()],
            folders_loaded: false,
            folder: "INBOX".to_string(),
            filter: "all",
            search: String::new(),
            emails: vec![],
            loaded: false,
            load_err: None,
            configured: None,
            selected_uid: None,
            detail: None,
            detail_err: None,
            checked: HashSet::new(),
            compose: None,
            folders_job: None,
            status_job: None,
            list_job: None,
            read_job: None,
            send_job: None,
            attachment_jobs: vec![],
            attachment_download: None,
            attachment_error: None,
            html_job: None,
            pending_download: None,
            unread_job: None,
            actions: vec![],
            pending_reload: false,
            summary: None,
            summary_uid: None,
            summary_job: None,
            draft_job: None,
            ai_error: None,
        }
    }
}

#[derive(Default)]
struct Compose {
    title: String,
    to: String,
    cc: String,
    subject: String,
    body: String,
    in_reply_to: String,
    references: String,
    sending: bool,
    error: Option<String>,
    attachments: Vec<UploadedAttachment>,
}

#[derive(Clone)]
struct UploadedAttachment {
    token: String,
    filename: String,
}

impl EmailPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        self.drain(sh);
        self.poll_ai();
        while let Some(path) = sh.take_picked("email_attach") {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
            let n2 = name.clone();
            self.attachment_jobs.push((
                name,
                sh.spawn(move |api| {
                    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                    api.email_compose_upload(&n2, bytes)
                }),
            ));
        }
        if let Some(path) = sh.take_picked("email_att_save") {
            if let Some((uid, folder, index)) = self.pending_download.take() {
                self.attachment_error = None;
                let path = path.to_string_lossy().into_owned();
                self.attachment_download = Some(sh.spawn(move |api| {
                    let bytes = retry_read(|| api.email_attachment_bytes(&uid, &folder, index))?;
                    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                    open_attachment(&path).map_err(|e| format!("Downloaded, but couldn't open attachment: {e}"))?;
                    Ok(path)
                }));
            }
        }
        if !self.loaded && self.list_job.is_none() && self.load_err.is_none() {
            self.reload(sh);
        }
        if !self.folders_loaded && self.folders_job.is_none() {
            self.folders_job = Some(sh.spawn(|a| retry_read(|| a.email_folders())));
        }
        if self.configured.is_none() && self.status_job.is_none() {
            self.status_job = Some(sh.spawn(|a| retry_read(|| a.email_status())));
        }
        if self.summary_uid != self.selected_uid {
            self.summary_uid = self.selected_uid.clone();
            self.summary = None;
            self.summary_job = None;
            self.draft_job = None;
            self.ai_error = None;
        }
        let p = sh.palette;

        // ---- header actions (Mac: All | Unread, folder menu, refresh, compose) -----
        let mut picked: Option<String> = None;
        let mut filt = self.filter;
        let (mut refresh, mut compose) = (false, false);
        let folders = self.folders.clone();
        let folder = self.folder.clone();
        widgets::panel_chrome(
            ui,
            &p,
            "Email",
            "envelope",
            |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                compose = widgets::icon_btn(ui, &p, "square.and.pencil", 26.0, false).on_hover_text("Compose").clicked();
                refresh = widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).on_hover_text("Refresh").clicked();
                let font = theme::jb(11.5, theme::W::Regular);
                let galley = ui.painter().layout_no_wrap(folder.clone(), font, p.ink);
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 32.0, 24.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, egui::Rounding::same(theme::R_CTRL), p.hover);
                }
                crate::icons::paint_symbol(ui.painter(), egui::pos2(rect.left() + 12.0, rect.center().y), "folder", 12.0, p.ink2);
                ui.painter().galley(egui::pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0), galley, p.ink);
                widgets::menu_popup(ui, &p, &resp, |ui| {
                    for f in &folders {
                        if widgets::menu_item(ui, &p, None, f, *f == folder, true).clicked() {
                            picked = Some(f.clone());
                            ui.memory_mut(|m| m.close_popup());
                        }
                    }
                });
                for (i, (key, label)) in [("unread", "Unread"), ("all", "All")].into_iter().enumerate() {
                    if i == 1 {
                        ui.spacing_mut().item_spacing.x = 2.0;
                    }
                    let kind = if filt == key { widgets::BtnKind::Secondary } else { widgets::BtnKind::Ghost };
                    if widgets::button_ex(ui, &p, kind, true, true, label).clicked() {
                        filt = key;
                    }
                }
            },
            |ui| self.content(ui, sh, &p),
        );
        if filt != self.filter {
            self.filter = filt;
            self.checked.clear();
            self.reload(sh);
        }
        if refresh {
            self.reload(sh);
        }
        if compose {
            self.compose = Some(Compose { title: "New email".into(), ..Default::default() });
        }
        if let Some(f) = picked {
            if f != self.folder {
                self.folder = f;
                self.checked.clear();
                self.selected_uid = None;
                self.detail = None;
                self.detail_err = None;
                self.reload(sh);
            }
        }
        self.compose_window(ui, sh);
    }

    fn column(ui: &mut Ui, p: &crate::theme::Palette, rect: egui::Rect) -> Ui {
        ui.allocate_rect(rect, egui::Sense::hover());
        let r = egui::Rounding::same(theme::R_PANEL);
        ui.painter().rect_filled(rect, r, p.panel_fill);
        ui.painter().rect_stroke(rect.shrink(0.5), r, egui::Stroke::new(1.0, p.line));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        child
    }

    fn content(&mut self, ui: &mut Ui, sh: &mut Shared, p: &crate::theme::Palette) {
        let rect = ui.max_rect();
        let needs_connection = self.loaded && self.emails.is_empty() && self.load_err.is_none() && self.configured != Some(true);
        if needs_connection {
            let mut c = Self::column(ui, p, rect);
            crate::ascii::empty_state(&mut c, p, crate::ascii::Art::Plug, "Connect email in Settings");
            return;
        }
        let left = egui::Rect::from_min_size(rect.min, egui::vec2(340.0f32.min(rect.width()), rect.height()));
        let right = egui::Rect::from_min_max(egui::pos2(left.right() + 12.0, rect.top()), rect.max);
        let mut lc = Self::column(ui, p, left);
        self.list_pane(&mut lc, sh);
        if right.width() > 20.0 {
            let mut rc = Self::column(ui, p, right);
            self.detail_pane(&mut rc, sh);
        }
    }

    fn poll_ai(&mut self) {
        let fail = |v: &Value, fallback: &str| -> String {
            v.get("error").and_then(Value::as_str).unwrap_or(fallback).to_owned()
        };
        if let Some(job) = &mut self.summary_job {
            if let Some(res) = job.take() {
                self.summary_job = None;
                match res {
                    Ok(v) if v.get("success").and_then(Value::as_bool) == Some(true) => {
                        self.summary = v.get("summary").and_then(Value::as_str).map(str::to_owned);
                    }
                    Ok(v) => self.ai_error = Some(fail(&v, "Summary failed")),
                    Err(e) => self.ai_error = Some(e),
                }
            }
        }
        if let Some(job) = &mut self.draft_job {
            if let Some(res) = job.take() {
                self.draft_job = None;
                match res {
                    Ok(v) if v.get("success").and_then(Value::as_bool) == Some(true) => {
                        if let Some(d) = self.detail.clone() {
                            let mut c = compose_reply(&d);
                            c.body = format!("{}{}", v.get("reply").and_then(Value::as_str).unwrap_or(""), c.body);
                            self.compose = Some(c);
                        }
                    }
                    Ok(v) => self.ai_error = Some(fail(&v, "Draft failed")),
                    Err(e) => self.ai_error = Some(e),
                }
            }
        }
    }

    // ---- job draining -------------------------------------------------------
    fn drain(&mut self, sh: &mut Shared) {
        if let Some(job) = &mut self.folders_job {
            if let Some(res) = job.take() {
                self.folders_job = None;
                self.folders_loaded = true;
                if let Ok(v) = res {
                    let mut list: Vec<String> = v
                        .get("folders")
                        .and_then(|x| x.as_array())
                        .or_else(|| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    if list.is_empty() {
                        list.push("INBOX".to_string());
                    }
                    if !list.contains(&self.folder) {
                        list.insert(0, self.folder.clone());
                    }
                    self.folders = list;
                }
            }
        }
        if let Some(job) = &mut self.status_job {
            if let Some(res) = job.take() {
                self.status_job = None;
                self.configured = Some(match res {
                    Ok(v) => v
                        .get("configured")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false),
                    Err(_) => false,
                });
            }
        }
        if let Some(job) = &mut self.list_job {
            if let Some(res) = job.take() {
                self.list_job = None;
                match res {
                    Ok(v) => {
                        if let Some(err) = v
                            .get("error")
                            .and_then(|x| x.as_str())
                            .filter(|e| !e.is_empty())
                        {
                            if !self.loaded {
                                self.load_err = Some(err.to_string());
                            }
                        } else {
                            self.emails = v
                                .get("emails")
                                .and_then(|x| x.as_array())
                                .or_else(|| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            self.loaded = true;
                            self.load_err = None;
                            self.sync_badge(sh);
                            self.unread_job =
                                Some(sh.spawn(|a| retry_read(|| a.email_unread_total())));
                        }
                    }
                    Err(e) => {
                        if !self.loaded {
                            self.load_err = Some(e);
                        } else {
                            sh.err(format!("Email refresh: {e}"));
                        }
                    }
                }
            }
        }
        if let Some(job) = &mut self.read_job {
            if let Some(res) = job.take() {
                self.read_job = None;
                match res {
                    Ok(v) => {
                        if let Some(err) = v
                            .get("error")
                            .and_then(|x| x.as_str())
                            .filter(|e| !e.is_empty())
                        {
                            self.detail_err = Some(err.to_string());
                        } else {
                            self.detail = Some(v);
                        }
                    }
                    Err(e) => self.detail_err = Some(e),
                }
            }
        }
        if let Some(job) = &mut self.send_job {
            if let Some(res) = job.take() {
                self.send_job = None;
                let err = match res {
                    Ok(v) => api_error(&v),
                    Err(e) => Some(e),
                };
                match err {
                    Some(e) => {
                        if let Some(c) = &mut self.compose {
                            c.sending = false;
                            c.error = Some(e);
                        }
                    }
                    None => {
                        self.compose = None;
                        self.reload(sh);
                    }
                }
            }
        }
        self.attachment_jobs.retain_mut(|(name, job)| {
            let Some(result) = job.take() else {
                return !job.done;
            };
            match result {
                Ok(v) => {
                    let token = s(&v, "token");
                    if token.is_empty() {
                        if let Some(c) = &mut self.compose {
                            c.error =
                                Some(format!("Couldn't upload {name}: server returned no token"));
                        }
                    } else if let Some(c) = &mut self.compose {
                        c.attachments.push(UploadedAttachment {
                            token: token.to_string(),
                            filename: {
                                let response_name = s(&v, "filename");
                                if response_name.is_empty() {
                                    name.clone()
                                } else {
                                    response_name.to_string()
                                }
                            },
                        });
                    }
                }
                Err(e) => {
                    if let Some(c) = &mut self.compose {
                        c.error = Some(format!("Couldn't upload {name}: {e}"));
                    }
                }
            }
            false
        });
        if let Some(job) = &mut self.html_job {
            if let Some(result) = job.take() {
                self.html_job = None;
                if let Err(e) = result {
                    self.attachment_error = Some(e);
                }
            }
        }
        if let Some(job) = &mut self.attachment_download {
            if let Some(result) = job.take() {
                self.attachment_download = None;
                self.attachment_error = result.err();
            }
        }
        if let Some(job) = &mut self.unread_job {
            if let Some(Ok(v)) = job.take() {
                let n = v
                    .as_i64()
                    .or_else(|| v.get("total").and_then(|x| x.as_i64()))
                    .or_else(|| v.get("unread").and_then(|x| x.as_i64()))
                    .or_else(|| v.get("count").and_then(|x| x.as_i64()));
                if let Some(n) = n {
                    sh.unread_email = n;
                }
                self.unread_job = None;
            } else if self.unread_job.as_ref().map(|j| j.done).unwrap_or(false) {
                self.unread_job = None;
            }
        }
        // Per-message actions: drop finished; when the batch drains, refresh.
        let mut action_err: Option<String> = None;
        self.actions.retain_mut(|j| match j.take() {
            Some(Err(e)) => {
                action_err = Some(e);
                false
            }
            Some(Ok(v)) => {
                if let Some(e) = api_error(&v) {
                    action_err = Some(e);
                }
                false
            }
            None => !j.done,
        });
        if let Some(e) = action_err {
            sh.err(format!("Email action: {e}"));
            self.pending_reload = true;
        }
        if self.actions.is_empty() && self.pending_reload {
            self.pending_reload = false;
            self.reload(sh);
        }
    }

    fn reload(&mut self, sh: &Shared) {
        self.load_err = None;
        let folder = self.folder.clone();
        let filter = self.filter.to_string();
        self.list_job = Some(sh.spawn(move |a| retry_read(|| a.email_list(&folder, &filter, 50))));
        self.unread_job = Some(sh.spawn(|a| retry_read(|| a.email_unread_total())));
    }

    /// Push the inbox's unread count to the sidebar badge; only the full inbox
    /// view is authoritative (mirrors the Swift syncUnreadBadge).
    fn sync_badge(&self, sh: &mut Shared) {
        if self.folder == "INBOX" && self.filter == "all" {
            sh.unread_email = self
                .emails
                .iter()
                .filter(|m| !m.get("is_read").and_then(|x| x.as_bool()).unwrap_or(false))
                .count() as i64;
        }
    }

    // ---- filtering ----------------------------------------------------------
    /// Client-side view of the loaded page: re-applies the Unread/Flagged
    /// filter (the IMAP server's SEARCH can be unreliable) plus the search box.
    fn visible(&self) -> Vec<Value> {
        let q = self.search.trim().to_lowercase();
        self.emails
            .iter()
            .filter(|m| match self.filter {
                "unread" => !m.get("is_read").and_then(|x| x.as_bool()).unwrap_or(false),
                "flagged" => m
                    .get("is_flagged")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
                _ => true,
            })
            .filter(|m| {
                if q.is_empty() {
                    return true;
                }
                for k in ["subject", "from_name", "from_address", "cached_summary"] {
                    if s(m, k).to_lowercase().contains(&q) {
                        return true;
                    }
                }
                false
            })
            .cloned()
            .collect()
    }

    // ---- list pane ----------------------------------------------------------
    fn list_pane(&mut self, ui: &mut Ui, sh: &mut Shared) {
        use crate::theme::{ty, W};
        let p = sh.palette;
        // Search row: magnifier, field, clear; hairline below.
        let area = ui.max_rect();
        let row = egui::Rect::from_min_size(area.min, egui::vec2(area.width(), 36.0));
        crate::icons::paint_symbol(ui.painter(), egui::pos2(row.left() + 19.0, row.center().y), "magnifyingglass", 11.0, p.ink3);
        let clear_w = if self.search.is_empty() { 0.0 } else { 22.0 };
        let field = egui::Rect::from_min_max(egui::pos2(row.left() + 32.0, row.top() + 6.0), egui::pos2(row.right() - 14.0 - clear_w, row.bottom() - 6.0));
        ui.put(
            field,
            egui::TextEdit::singleline(&mut self.search)
                .frame(false)
                .hint_text(ty::sized("Search", 12.5, W::Regular).color(p.ink3))
                .font(theme::jb(12.5, W::Regular))
                .text_color(p.ink)
                .desired_width(field.width()),
        );
        if !self.search.is_empty() {
            let cr = egui::Rect::from_center_size(egui::pos2(row.right() - 19.0, row.center().y), egui::vec2(18.0, 18.0));
            let resp = ui.interact(cr, ui.id().with("search-clear"), egui::Sense::click());
            crate::icons::paint_symbol(ui.painter(), cr.center(), "xmark.circle.fill", 11.0, p.ink3);
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.search.clear();
            }
        }
        ui.painter().hline(row.x_range(), row.bottom() - 0.5, egui::Stroke::new(1.0, p.line));
        let rest = egui::Rect::from_min_max(egui::pos2(area.left(), row.bottom()), area.max);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rest), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let visible = self.visible();
            if !self.checked.is_empty() {
                self.bulk_bar(ui, sh, &visible);
            }
            if self.list_job.is_some() && !self.loaded {
                widgets::panel_loading(ui, &p);
            } else if let Some(e) = self.load_err.clone() {
                widgets::panel_failure(ui, &p, "Couldn't load", Some(&e));
            } else if visible.is_empty() {
                if self.configured == Some(true) {
                    let unread = self.filter == "unread";
                    crate::ascii::empty_state(
                        ui,
                        &p,
                        if unread { crate::ascii::Art::Done } else { crate::ascii::Art::Inbox },
                        if unread { "All read" } else { "No emails" },
                    );
                } else {
                    crate::ascii::empty_state(ui, &p, crate::ascii::Art::Plug, "Connect email in Settings");
                }
            } else {
                // Up/Down move the open message.
                let step = ui.input(|i| i.key_pressed(egui::Key::ArrowDown) as i32 - i.key_pressed(egui::Key::ArrowUp) as i32);
                if step != 0 && !ui.ctx().wants_keyboard_input() {
                    let cur = visible.iter().position(|m| Some(s(m, "uid")) == self.selected_uid.as_deref());
                    let next = match cur {
                        Some(c) => (c as i32 + step).clamp(0, visible.len() as i32 - 1) as usize,
                        None if step > 0 => 0,
                        None => visible.len() - 1,
                    };
                    let uid = s(&visible[next], "uid").to_string();
                    self.open(sh, &uid);
                }
                egui::ScrollArea::vertical().id_salt("email_list").auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add_space(6.0);
                    for m in &visible {
                        self.email_row(ui, sh, m);
                    }
                    ui.add_space(6.0);
                });
            }
        });
    }

    fn bulk_bar(&mut self, ui: &mut Ui, sh: &mut Shared, visible: &[Value]) {
        use crate::theme::{ty, W};
        let p = sh.palette;
        let all_selected = !visible.is_empty() && visible.iter().all(|m| self.checked.contains(s(m, "uid")));
        let slot = ui.painter().add(egui::Shape::Noop);
        let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(12.0, 8.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(ty::sized(format!("{} selected", self.checked.len()), 11.0, W::Medium).color(p.ink));
                if widgets::button_ex(ui, &p, widgets::BtnKind::Ghost, true, true, if all_selected { "None" } else { "All" }).clicked() {
                    if all_selected {
                        self.checked.clear();
                    } else {
                        self.checked = visible.iter().map(|m| s(m, "uid").to_string()).collect();
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if widgets::icon_btn(ui, &p, "xmark", 26.0, false).clicked() {
                        self.checked.clear();
                    }
                    if widgets::icon_btn(ui, &p, "trash", 26.0, false).on_hover_text("Delete").clicked() {
                        self.bulk_remove(sh, true);
                    }
                    if widgets::icon_btn(ui, &p, "archivebox", 26.0, false).on_hover_text("Archive").clicked() {
                        self.bulk_remove(sh, false);
                    }
                    if widgets::icon_btn(ui, &p, "envelope.badge", 26.0, false).on_hover_text("Mark unread").clicked() {
                        self.bulk_mark(sh, false);
                    }
                    if widgets::icon_btn(ui, &p, "envelope.open", 26.0, false).on_hover_text("Mark read").clicked() {
                        self.bulk_mark(sh, true);
                    }
                });
            });
        });
        ui.painter().set(slot, egui::Shape::rect_filled(inner.response.rect, 0.0, p.accent_soft));
        let (l, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().rect_filled(l, 0.0, p.line);
    }

    fn email_row(&mut self, ui: &mut Ui, sh: &mut Shared, m: &Value) {
        use crate::theme::W;
        let uid = s(m, "uid").to_string();
        if uid.is_empty() {
            return;
        }
        let p = sh.palette;
        let flag = |k: &str| m.get(k).and_then(Value::as_bool).unwrap_or(false);
        let unread = !flag("is_read");
        let (flagged, has_att) = (flag("is_flagged"), flag("has_attachments"));
        let from = {
            let n = s(m, "from_name");
            let a = s(m, "from_address");
            (if !n.is_empty() { n } else if !a.is_empty() { a } else { "Unknown" }).to_string()
        };
        let subject = if s(m, "subject").is_empty() { "(no subject)".to_string() } else { s(m, "subject").to_string() };
        let summary = s(m, "cached_summary").to_string();
        let date = short_date(s(m, "date"));
        let checked = self.checked.contains(&uid);
        let selecting = !self.checked.is_empty();
        let selected = self.selected_uid.as_deref() == Some(&uid);
        let h = 14.0 + 16.0 + 1.0 + 16.0 + if summary.is_empty() { 0.0 } else { 15.0 };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width() - 12.0, h), egui::Sense::hover());
        let rect = rect.translate(egui::vec2(6.0, 0.0));
        let check_rect = egui::Rect::from_min_size(egui::pos2(rect.left() + 8.0, rect.top() + 7.0), egui::vec2(16.0, 18.0));
        let resp = ui.interact(rect, ui.id().with(("erow", &uid)), egui::Sense::click());
        let hovering = resp.hovered();
        let show_check = selecting || hovering || checked;
        let check_resp = show_check.then(|| ui.interact(check_rect.expand(3.0), ui.id().with(("echeck", &uid)), egui::Sense::click()));
        let hov = ui.ctx().animate_bool_with_time(resp.id.with("hov"), hovering && !selected, theme::FADE);
        let painter = ui.painter().clone();
        if checked {
            painter.rect_filled(rect, egui::Rounding::same(theme::R_CTRL), p.accent_soft);
        }
        painter.extend(widgets::hover_row_shapes(&p, rect, theme::R_CTRL, selected, hov));
        if show_check {
            let (name, col) = if checked { ("checkmark.circle.fill", p.accent) } else { ("circle", p.ink3) };
            crate::icons::paint_symbol(&painter, check_rect.center(), name, 14.0, col);
        } else if unread {
            painter.circle_filled(check_rect.center(), 3.0, p.accent);
        }
        let x0 = rect.left() + 8.0 + 16.0 + 8.0;
        let mut right = rect.right() - 8.0;
        let top = rect.top() + 7.0;
        let dg = painter.layout_no_wrap(date, theme::jb(10.5, W::Regular), p.ink3);
        painter.galley(egui::pos2(right - dg.size().x, top + 1.0), dg.clone(), p.ink3);
        right -= dg.size().x + 6.0;
        if has_att {
            crate::icons::paint_symbol(&painter, egui::pos2(right - 4.0, top + 8.0), "paperclip", 9.0, p.ink3);
            right -= 15.0;
        }
        if flagged {
            crate::icons::paint_symbol(&painter, egui::pos2(right - 4.0, top + 8.0), "star.fill", 9.0, p.warn);
            right -= 15.0;
        }
        let f1 = theme::jb(13.0, if unread { W::Semibold } else { W::Regular });
        let c1 = if unread { p.ink } else { p.ink2 };
        painter.text(egui::pos2(x0, top), egui::Align2::LEFT_TOP, fit_text(ui, &from, &f1, (right - x0 - 4.0).max(10.0)), f1, c1);
        let f2 = theme::jb(12.5, if unread { W::Medium } else { W::Regular });
        painter.text(egui::pos2(x0, top + 17.0), egui::Align2::LEFT_TOP, fit_text(ui, &subject, &f2, rect.right() - 8.0 - x0), f2, c1);
        if !summary.is_empty() {
            let f3 = theme::jb(11.5, W::Regular);
            painter.text(egui::pos2(x0, top + 33.0), egui::Align2::LEFT_TOP, fit_text(ui, &summary, &f3, rect.right() - 8.0 - x0), f3, p.ink3);
        }
        if check_resp.map(|c| c.clicked()).unwrap_or(false) {
            self.toggle_check(&uid);
        } else if resp.clicked() {
            if self.checked.is_empty() {
                self.open(sh, &uid);
            } else {
                self.toggle_check(&uid);
            }
        }
        #[derive(Clone, Copy)]
        enum A {
            Select,
            Read,
            Archive,
            Delete,
        }
        let mut act = None;
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).context_menu(|ui| {
            widgets::floating_frame(&p, theme::R_CARD).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
                ui.set_min_width(150.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                for (a, label) in [
                    (A::Select, if checked { "Deselect" } else { "Select" }),
                    (A::Read, if unread { "Mark Read" } else { "Mark Unread" }),
                    (A::Archive, "Archive"),
                    (A::Delete, "Delete"),
                ] {
                    if widgets::menu_item(ui, &p, None, label, false, true).clicked() {
                        act = Some(a);
                        ui.close_menu();
                    }
                }
            });
        });
        match act {
            Some(A::Select) => self.toggle_check(&uid),
            Some(A::Read) => self.set_read(sh, &uid, unread),
            Some(a @ (A::Archive | A::Delete)) => {
                let del = matches!(a, A::Delete);
                self.remove_locally(&uid, sh);
                let (u, f) = (uid.clone(), self.folder.clone());
                self.actions.push(sh.spawn(move |api| if del { api.email_delete(&u, &f) } else { api.email_archive(&u, &f) }));
            }
            None => {}
        }
    }

    fn toggle_check(&mut self, uid: &str) {
        if !self.checked.remove(uid) {
            self.checked.insert(uid.to_string());
        }
    }

    // ---- actions ------------------------------------------------------------
    /// Optimistic read-state flip: instant UI, server call in background.
    fn set_read(&mut self, sh: &mut Shared, uid: &str, read: bool) {
        if let Some(m) = self.emails.iter_mut().find(|m| s(m, "uid") == uid) {
            m["is_read"] = json!(read);
        }
        self.sync_badge(sh);
        let u = uid.to_string();
        let f = self.folder.clone();
        self.actions
            .push(sh.spawn(move |a| a.email_mark_read(&u, &f, read)));
    }

    fn remove_locally(&mut self, uid: &str, sh: &mut Shared) {
        self.emails.retain(|m| s(m, "uid") != uid);
        self.checked.remove(uid);
        if self.selected_uid.as_deref() == Some(uid) {
            self.selected_uid = None;
            self.detail = None;
            self.detail_err = None;
        }
        self.sync_badge(sh);
    }

    fn bulk_mark(&mut self, sh: &mut Shared, read: bool) {
        let uids: Vec<String> = self.checked.drain().collect();
        for uid in &uids {
            if let Some(m) = self.emails.iter_mut().find(|m| s(m, "uid") == uid.as_str()) {
                m["is_read"] = json!(read);
            }
        }
        self.sync_badge(sh);
        let f = self.folder.clone();
        for uid in uids {
            let f = f.clone();
            self.actions
                .push(sh.spawn(move |a| a.email_mark_read(&uid, &f, read)));
        }
        self.pending_reload = true;
    }

    fn bulk_remove(&mut self, sh: &mut Shared, delete: bool) {
        let uids: Vec<String> = self.checked.drain().collect();
        for uid in &uids {
            self.remove_locally(uid, sh);
        }
        let f = self.folder.clone();
        for uid in uids {
            let f = f.clone();
            self.actions.push(sh.spawn(move |a| {
                if delete {
                    a.email_delete(&uid, &f)
                } else {
                    a.email_archive(&uid, &f)
                }
            }));
        }
        self.pending_reload = true;
    }

    fn open(&mut self, sh: &mut Shared, uid: &str) {
        self.selected_uid = Some(uid.to_string());
        self.detail = None;
        self.detail_err = None;
        let u = uid.to_string();
        let f = self.folder.clone();
        self.read_job = Some(sh.spawn(move |a| retry_read(|| a.email_read(&u, &f))));
        // Opening marks it read (optimistic, like the Mac app).
        let was_unread = self
            .emails
            .iter()
            .find(|m| s(m, "uid") == uid)
            .map(|m| !m.get("is_read").and_then(|x| x.as_bool()).unwrap_or(false))
            .unwrap_or(false);
        if was_unread {
            self.set_read(sh, uid, true);
        }
    }

    // ---- reading pane -------------------------------------------------------
    fn detail_pane(&mut self, ui: &mut Ui, sh: &mut Shared) {
        use crate::theme::{ty, W};
        let p = sh.palette;
        let Some(uid) = self.selected_uid.clone() else {
            crate::ascii::empty_state(ui, &p, crate::ascii::Art::Inbox, "Select an email");
            return;
        };
        if let Some(e) = self.detail_err.clone() {
            widgets::panel_failure(ui, &p, "Couldn't open", Some(&e));
            return;
        }
        let Some(d) = self.detail.clone() else {
            widgets::panel_loading(ui, &p);
            return;
        };
        let subject = if s(&d, "subject").is_empty() { "(no subject)".to_string() } else { s(&d, "subject").to_string() };
        let from_name = s(&d, "from_name").to_string();
        let from_addr = s(&d, "from_address").to_string();
        let mut act: Option<u8> = None; // 1 reply, 2 forward, 3 archive, 4 delete, 5 summarize, 6 draft

        egui::Frame::none().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let btns_w = 26.0 * 6.0 + 4.0 * 6.0 + 2.0;
                let tw = (ui.available_width() - btns_w - 8.0).max(80.0);
                ui.allocate_ui_with_layout(egui::vec2(tw, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(tw);
                    ui.add(egui::Label::new(ty::sized(subject.as_str(), 20.0, W::Semibold).extra_letter_spacing(-0.3).color(p.ink)).wrap());
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let more = widgets::icon_btn(ui, &p, "ellipsis", 26.0, false);
                    widgets::menu_popup(ui, &p, &more, |ui| {
                        if widgets::menu_item(ui, &p, None, "Archive", false, true).clicked() {
                            act = Some(3);
                            ui.memory_mut(|m| m.close_popup());
                        }
                        if widgets::menu_item(ui, &p, None, "Delete", false, true).clicked() {
                            act = Some(4);
                            ui.memory_mut(|m| m.close_popup());
                        }
                    });
                    if widgets::icon_btn(ui, &p, "arrowshape.turn.up.right", 26.0, false).on_hover_text("Forward").clicked() {
                        act = Some(2);
                    }
                    if widgets::icon_btn(ui, &p, "arrowshape.turn.up.left", 26.0, false).on_hover_text("Reply").clicked() {
                        act = Some(1);
                    }
                    let (dv, _) = ui.allocate_exact_size(egui::vec2(1.0, 14.0), egui::Sense::hover());
                    ui.painter().rect_filled(dv.translate(egui::vec2(0.0, 6.0)), 0.0, p.line);
                    let drafting = self.draft_job.is_some();
                    let r = widgets::icon_btn(ui, &p, if drafting { "" } else { "arrowshape.turn.up.left.2" }, 26.0, false).on_hover_text("Draft a reply");
                    if drafting {
                        paint_spinner(ui, &p, r.rect);
                    } else if r.clicked() {
                        act = Some(6);
                    }
                    let summarizing = self.summary_job.is_some();
                    let r = widgets::icon_btn(ui, &p, "", 26.0, false).on_hover_text("Summarize");
                    if summarizing {
                        paint_spinner(ui, &p, r.rect);
                    } else {
                        crate::icons::paint_glyph(ui.painter(), r.rect.center(), crate::icons::JbGlyph::Spark, 15.0, if r.hovered() { p.ink } else { p.ink2 });
                        if r.clicked() {
                            act = Some(5);
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let name = if !from_name.is_empty() { from_name.as_str() } else { from_addr.as_str() };
                ui.label(ty::sized(name, 13.0, W::Medium).color(p.ink));
                if !from_name.is_empty() && !from_addr.is_empty() {
                    ui.add(egui::Label::new(ty::sized(from_addr.as_str(), 11.0, W::Regular).color(p.ink3)).truncate());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(ty::sized(short_date(s(&d, "date")), 11.0, W::Regular).color(p.ink3));
                });
            });
            let to = s(&d, "to");
            if !to.is_empty() {
                ui.add(egui::Label::new(ty::sized(format!("to {to}"), 11.0, W::Regular).color(p.ink3)).truncate());
            }
            let atts = d.get("attachments").and_then(|x| x.as_array()).cloned().unwrap_or_default();
            let html = s(&d, "body_html").to_string();
            let mut open_html = false;
            if !atts.is_empty() || !html.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for a in &atts {
                        let name = if s(a, "filename").is_empty() { "file" } else { s(a, "filename") };
                        let size = a.get("size").and_then(|x| x.as_i64()).unwrap_or(0);
                        let index = a.get("index").and_then(|x| x.as_i64()).unwrap_or(-1);
                        let resp = attachment_chip(ui, &p, name);
                        let resp = if size > 0 { resp.on_hover_text(format!("{name} ({})", human_size(size))) } else { resp };
                        // Downloading is an explicit user action; the worker fetches the read-only BODY.PEEK endpoint.
                        if resp.clicked() && index >= 0 && self.attachment_download.is_none() {
                            self.pending_download = Some((uid.clone(), self.folder.clone(), index));
                            sh.picker.open_save("email_att_save", "Save attachment", &crate::panels::attachments::safe_name(name), None);
                        }
                    }
                    if !html.is_empty() && widgets::button_ex(ui, &p, widgets::BtnKind::Ghost, true, true, "Open rich view").clicked() {
                        open_html = true;
                    }
                });
            }
            if open_html {
                let safe = email_html_document(&s(&d, "body_html").to_string());
                self.html_job = Some(sh.spawn(move |_| crate::panels::editor::open_html_browser(&safe)));
            }
            if let Some(e) = self.attachment_error.as_ref().or(self.ai_error.as_ref()) {
                ui.add(egui::Label::new(ty::caption(e.as_str()).color(p.danger)).truncate());
            }
            if self.summary_job.is_some() || self.summary.is_some() {
                let slot = ui.painter().add(egui::Shape::Noop);
                let inner = egui::Frame::none().inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(ty::eyebrow("Summary").color(p.ink2));
                    match &self.summary {
                        Some(t) => {
                            ui.add(egui::Label::new(ty::sized(t.replace("**", ""), 12.5, W::Regular).color(p.ink)).wrap());
                        }
                        None => {
                            crate::ascii::spinner(ui, 12.0, p.ink2);
                        }
                    }
                });
                ui.painter().set(slot, egui::Shape::rect_filled(inner.response.rect, egui::Rounding::same(theme::R_CARD), p.accent_soft));
            }
        });
        let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().rect_filled(line, 0.0, p.line);

        // Body: readable text fallback (the rich HTML view opens in the system browser).
        let body = body_text(&d);
        egui::ScrollArea::vertical().id_salt("email_body").auto_shrink([false, false]).show(ui, |ui| {
            let w = (ui.available_width() - 40.0).clamp(100.0, 680.0);
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - w) / 2.0).max(20.0));
                ui.vertical(|ui| {
                    ui.set_width(w);
                    if body.is_empty() {
                        ui.label(ty::sized("(empty message)", 14.0, W::Regular).color(p.ink3));
                    } else {
                        ui.add(egui::Label::new(ty::sized(body, 14.0, W::Regular).color(p.ink).line_height(Some(21.0))).wrap());
                    }
                });
            });
            ui.add_space(20.0);
        });

        match act {
            Some(1) => self.compose = Some(compose_reply(&d)),
            Some(2) => self.compose = Some(compose_forward(&d)),
            Some(a @ (3 | 4)) => {
                let (u, f) = (uid.clone(), self.folder.clone());
                self.actions.push(sh.spawn(move |api| if a == 4 { api.email_delete(&u, &f) } else { api.email_archive(&u, &f) }));
                self.remove_locally(&uid, sh);
            }
            Some(a @ (5 | 6)) if self.summary_job.is_none() && self.draft_job.is_none() => {
                self.ai_error = None;
                let (u, f, sj, fr, bd) = (uid.clone(), self.folder.clone(), subject.clone(), format!("{from_name} <{from_addr}>"), body_text(&d));
                let job = sh.spawn(move |api| if a == 5 { api.email_summarize(&u, &f, &sj, &fr, &bd) } else { api.email_ai_reply(&u, &f, &sj, &fr, &bd) });
                if a == 5 { self.summary_job = Some(job) } else { self.draft_job = Some(job) }
            }
            _ => {}
        }
    }

    // ---- compose (`EmailComposeSheet`) ----------------------------------------------
    fn compose_window(&mut self, ui: &mut Ui, sh: &mut Shared) {
        use crate::theme::{ty, W};
        let p = sh.palette;
        let ctx = ui.ctx().clone();
        let (mut send, mut close) = (false, false);
        let uploading = !self.attachment_jobs.is_empty();
        if let Some(c) = &mut self.compose {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) && !c.sending && !uploading && !c.to.trim().is_empty() {
                send = true;
            }
            let screen = ctx.screen_rect();
            egui::Area::new(egui::Id::new("email-scrim")).order(egui::Order::Middle).fixed_pos(screen.min).show(&ctx, |ui| {
                ui.painter().rect_filled(screen, 0.0, egui::Color32::from_black_alpha(90));
                ui.allocate_rect(screen, egui::Sense::click());
            });
            egui::Window::new("email_compose")
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .fixed_size(egui::vec2(620.0, 540.0))
                .frame(widgets::floating_frame(&p, theme::R_PANEL).inner_margin(egui::Margin::ZERO))
                .show(&ctx, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
                    egui::Frame::none().inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(ty::sized(c.title.as_str(), 14.0, W::Semibold).color(p.ink));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let can = !c.sending && !uploading && !c.to.trim().is_empty();
                                let r = widgets::button_ex(ui, &p, widgets::BtnKind::Primary, false, can, if c.sending { "" } else { "Send" });
                                if c.sending {
                                    paint_spinner_tint(ui, r.rect, p.on_accent);
                                }
                                if r.clicked() {
                                    send = true;
                                }
                                if widgets::button_ex(ui, &p, widgets::BtnKind::Ghost, false, true, "Cancel").clicked() && self.attachment_jobs.is_empty() {
                                    close = true;
                                }
                            });
                        });
                    });
                    let (l, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
                    ui.painter().rect_filled(l, 0.0, p.line);
                    egui::Frame::none().inner_margin(egui::Margin { left: 14.0, right: 14.0, top: 10.0, bottom: 0.0 }).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        widgets::field(ui, &p, &mut c.to, "To");
                        widgets::field(ui, &p, &mut c.cc, "Cc");
                        widgets::field(ui, &p, &mut c.subject, "Subject");
                        if !c.attachments.is_empty() || uploading {
                            ui.add_space(2.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                let mut remove = None;
                                for a in &c.attachments {
                                    let resp = attachment_chip(ui, &p, &format!("{}  \u{2715}", a.filename));
                                    if resp.on_hover_text("Remove").clicked() {
                                        remove = Some(a.token.clone());
                                    }
                                }
                                if let Some(t) = remove {
                                    c.attachments.retain(|a| a.token != t);
                                }
                                if uploading {
                                    ui.label(ty::caption(format!("Uploading {}\u{2026}", self.attachment_jobs.len())).color(p.ink3));
                                }
                            });
                        }
                    });
                    // Body editor in a raised card; the footer keeps ~46 pt.
                    let h = (ui.available_height() - 46.0 - if c.error.is_some() { 22.0 } else { 0.0 } - 28.0).max(80.0);
                    egui::Frame::none().inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
                        let r = egui::Rounding::same(theme::R_CARD);
                        ui.painter().rect_filled(rect, r, p.raised);
                        ui.painter().rect_stroke(rect.shrink(0.5), r, egui::Stroke::new(1.0, p.line));
                        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(10.0)));
                        egui::ScrollArea::vertical().id_salt("email_compose_body").auto_shrink([false, false]).show(&mut child, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut c.body)
                                    .frame(false)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(8)
                                    .font(theme::jb(13.0, W::Regular))
                                    .text_color(p.ink),
                            );
                        });
                    });
                    if let Some(e) = &c.error {
                        ui.horizontal(|ui| {
                            ui.add_space(14.0);
                            ui.add(egui::Label::new(ty::caption(e.as_str()).color(p.danger)).truncate());
                        });
                        ui.add_space(8.0);
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        let r = widgets::button_ex(ui, &p, widgets::BtnKind::Ghost, true, true, "      Attach");
                        crate::icons::paint_symbol(ui.painter(), egui::pos2(r.rect.left() + 17.0, r.rect.center().y), "paperclip", 12.0, p.ink);
                        if r.clicked() {
                            sh.picker.open("email_attach", "Attach files", crate::pickers::PickMode::Files(&[]), None);
                        }
                    });
                    ui.add_space(12.0);
                });
        }
        if close {
            self.compose = None;
        }
        if send {
            if let Some(c) = &mut self.compose {
                c.sending = true;
                c.error = None;
                let mut payload = json!({"to": c.to.trim(), "subject": c.subject.clone(), "body": c.body.clone()});
                if !c.cc.trim().is_empty() {
                    payload["cc"] = json!(c.cc.trim());
                }
                if !c.in_reply_to.is_empty() {
                    payload["in_reply_to"] = json!(c.in_reply_to);
                }
                if !c.references.is_empty() {
                    payload["references"] = json!(c.references);
                }
                if !c.attachments.is_empty() {
                    payload["attachment_tokens"] = json!(c.attachments.iter().map(|a| a.token.as_str()).collect::<Vec<_>>());
                }
                self.send_job = Some(sh.spawn(move |a| a.email_send(payload)));
            }
        }
    }
}

/// Secondary-button look with a paperclip: attachment chips.
fn attachment_chip(ui: &mut Ui, p: &crate::theme::Palette, text: &str) -> egui::Response {
    let font = theme::jb(11.0, theme::W::Regular);
    let max = (ui.available_width() - 40.0).clamp(80.0, 260.0);
    let label = fit_text(ui, text, &font, max);
    let g = ui.painter().layout_no_wrap(label, font, p.ink);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(g.size().x + 18.0 + 16.0, 24.0), egui::Sense::click());
    let fill = if resp.hovered() { p.hover } else { p.raised };
    ui.painter().rect_filled(rect, egui::Rounding::same(theme::R_CTRL), fill);
    ui.painter().rect_stroke(rect.shrink(0.5), egui::Rounding::same(theme::R_CTRL), egui::Stroke::new(1.0, p.line));
    crate::icons::paint_symbol(ui.painter(), egui::pos2(rect.left() + 14.0, rect.center().y), "paperclip", 11.0, p.ink);
    ui.painter().galley(egui::pos2(rect.left() + 25.0, rect.center().y - g.size().y / 2.0), g, p.ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn paint_spinner(ui: &mut Ui, p: &crate::theme::Palette, rect: egui::Rect) {
    paint_spinner_tint(ui, rect, p.ink2);
}

fn paint_spinner_tint(ui: &mut Ui, rect: egui::Rect, tint: egui::Color32) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(12.0))));
    crate::ascii::spinner(&mut child, 12.0, tint);
}

/// Truncate with an ellipsis so the text fits `max_w`.
fn fit_text(ui: &Ui, text: &str, font: &FontId, max_w: f32) -> String {
    let w = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE).size().x;
    if w(text) <= max_w {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let cand: String = chars[..mid].iter().collect::<String>().trim_end().to_owned() + "\u{2026}";
        if w(&cand) <= max_w { lo = mid } else { hi = mid - 1 }
    }
    chars[..lo].iter().collect::<String>().trim_end().to_owned() + "\u{2026}"
}

// ---- compose prefill helpers -----------------------------------------------

fn compose_reply(d: &Value) -> Compose {
    let subj = s(d, "subject");
    let subject = if subj.starts_with("Re:") {
        subj.to_string()
    } else {
        format!("Re: {subj}")
    };
    let msg_id = s(d, "message_id").to_string();
    let mut refs: Vec<&str> = vec![];
    let existing = s(d, "references");
    if !existing.is_empty() {
        refs.push(existing);
    }
    if !msg_id.is_empty() {
        refs.push(&msg_id);
    }
    Compose {
        title: "Reply".into(),
        to: s(d, "from_address").to_string(),
        subject,
        body: format!("\n\n---\n{}", quoted(d)),
        in_reply_to: msg_id.clone(),
        references: refs.join(" "),
        ..Default::default()
    }
}

fn compose_forward(d: &Value) -> Compose {
    let subj = s(d, "subject");
    let subject = if subj.starts_with("Fwd:") {
        subj.to_string()
    } else {
        format!("Fwd: {subj}")
    };
    Compose {
        title: "Forward".into(),
        subject,
        body: format!("\n\n--- Forwarded message ---\n{}", quoted(d)),
        ..Default::default()
    }
}

fn quoted(d: &Value) -> String {
    format!(
        "From: {} <{}>\nDate: {}\nSubject: {}\n\n{}",
        s(d, "from_name"),
        s(d, "from_address"),
        s(d, "date"),
        s(d, "subject"),
        body_text(d)
    )
}

// ---- value / text helpers ----------------------------------------------------

/// Field accessor: string value of `k`, or "".
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

/// Retry safe reads after short transport/server failures. Message sends and
/// mailbox mutations deliberately do not use this: replaying SMTP or IMAP
/// writes can send duplicates or repeat a destructive action.
fn retry_read<T>(mut call: impl FnMut() -> R<T>) -> R<T> {
    let mut last = None;
    for attempt in 0..3 {
        match call() {
            Ok(value) => return Ok(value),
            Err(error) => {
                last = Some(error);
                if attempt < 2 {
                    std::thread::sleep(std::time::Duration::from_millis(200 * (attempt + 1)));
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| "Email request failed".into()))
}

fn api_error(v: &Value) -> Option<String> {
    v.get("error")
        .and_then(Value::as_str)
        .filter(|error| !error.is_empty())
        .map(str::to_string)
        .or_else(|| {
            (v.get("success").and_then(Value::as_bool) == Some(false)
                || v.get("ok").and_then(Value::as_bool) == Some(false))
            .then(|| "The email server did not complete the action".to_string())
        })
}

/// Put email HTML in an isolated document. It can use inline formatting and
/// embedded images while scripts, frames, forms, and remote network requests
/// stay blocked by the browser's content policy.
fn email_html_document(html: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data: cid:; style-src 'unsafe-inline'; font-src data:; form-action 'none'; frame-src 'none'; connect-src 'none'\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Email</title></head><body>{html}</body></html>"
    )
}

fn open_attachment(path: &str) -> Result<(), String> {
    crate::platform::open_path(path)
}

/// Readable body: prefer the plain-text part; fall back to a tag-stripped
/// extraction of the HTML part.
fn body_text(d: &Value) -> String {
    let plain = s(d, "body").trim();
    if !plain.is_empty() {
        return plain.to_string();
    }
    let html = s(d, "body_html");
    if html.is_empty() {
        String::new()
    } else {
        html_to_text(html)
    }
}

/// Friendly list date: "09:12" today, "19 Oct" this year, "19 Oct 2025"
/// otherwise. Accepts ISO-8601 and RFC-2822; anything else passes through.
fn short_date(d: &str) -> String {
    const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let t = d.trim();
    let parsed: Option<(i64, u32, u32, String)> = if t.len() >= 10 && t.as_bytes()[4] == b'-' && t.is_char_boundary(10) && t.is_char_boundary(4) {
        let y = t.get(0..4).and_then(|x| x.parse().ok());
        let m = t.get(5..7).and_then(|x| x.parse().ok());
        let dd = t.get(8..10).and_then(|x| x.parse().ok());
        let hm = t.get(11..16).unwrap_or("").to_string();
        match (y, m, dd) {
            (Some(y), Some(m), Some(dd)) => Some((y, m, dd, hm)),
            _ => None,
        }
    } else {
        // "Mon, 19 Oct 2026 09:12:00 +0200" / "19 Oct 2026 09:12:00"
        let parts: Vec<&str> = t.trim_start_matches(|c: char| c.is_alphabetic() || c == ',' || c == ' ')
            .split_whitespace()
            .collect();
        let parts = if parts.is_empty() { t.split_whitespace().skip(1).collect() } else { parts };
        if parts.len() >= 3 {
            let dd = parts[0].parse().ok();
            let m = MON.iter().position(|x| parts[1].starts_with(x)).map(|i| i as u32 + 1);
            let y = parts[2].parse().ok();
            let hm = parts.get(3).map(|s| s.chars().take(5).collect()).unwrap_or_default();
            match (y, m, dd) {
                (Some(y), Some(m), Some(dd)) => Some((y, m, dd, hm)),
                _ => None,
            }
        } else {
            None
        }
    };
    if let Some((y, m, dd, hm)) = parsed {
        if (1..=12).contains(&m) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|x| x.as_secs() as i64)
                .unwrap_or(0);
            let (ty, tm, td) = civil(now.div_euclid(86_400));
            if (y, m, dd) == (ty, tm, td) && !hm.is_empty() {
                return hm;
            }
            if y == ty {
                return format!("{dd} {}", MON[(m - 1) as usize]);
            }
            return format!("{dd} {} {y}", MON[(m - 1) as usize]);
        }
    }
    legacy_short_date(d)
}

/// Days since the Unix epoch → (year, month, day). Howard Hinnant's algorithm.
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn legacy_short_date(d: &str) -> String {
    let mut t = d.trim();
    // Drop a trailing numeric timezone (" +0000" / " -0500").
    if let Some(i) = t.rfind(" +").or_else(|| t.rfind(" -")) {
        if t.len() - i <= 7 {
            t = &t[..i];
        }
    }
    t.to_string()
}

fn human_size(n: i64) -> String {
    if n >= 1_048_576 {
        format!("{:.1} MB", n as f64 / 1_048_576.0)
    } else if n >= 1024 {
        format!("{} KB", n / 1024)
    } else {
        format!("{n} B")
    }
}

// ---- minimal HTML → text extraction ------------------------------------------

/// Strip tags to readable text: drop <script>/<style>/<head> blocks, turn
/// block-level tags into newlines and <li> into bullets, decode entities,
/// collapse blank runs. Good enough to read mail without a WebView.
fn html_to_text(html: &str) -> String {
    let mut src = html.to_string();
    for tag in ["script", "style", "head"] {
        src = strip_block(&src, tag);
    }
    let mut out = String::new();
    let mut rest = src.as_str();
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('>') {
            Some(j) => {
                let tag = after[..j].trim().to_ascii_lowercase();
                let closing = tag.starts_with('/');
                let name = tag
                    .trim_start_matches('/')
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("");
                match name {
                    "br" | "p" | "div" | "tr" | "table" | "blockquote" | "ul" | "ol" | "h1"
                    | "h2" | "h3" | "h4" | "h5" | "h6" | "hr" => out.push('\n'),
                    "li" => {
                        if !closing {
                            out.push_str("\n• ");
                        }
                    }
                    "td" | "th" => out.push(' '),
                    _ => {}
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    tidy(&decode_entities(&out))
}

/// Remove `<tag ...> ... </tag>` blocks, case-insensitive.
fn strip_block(src: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    let mut out = String::new();
    let mut pos = 0;
    while let Some(i) = find_ci(src, &open, pos) {
        out.push_str(&src[pos..i]);
        pos = match find_ci(src, &close, i) {
            Some(j) => src[j..].find('>').map(|k| j + k + 1).unwrap_or(src.len()),
            None => src.len(),
        };
    }
    out.push_str(&src[pos..]);
    out
}

/// ASCII case-insensitive substring search from `from`.
fn find_ci(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let hb = hay.as_bytes();
    let nb = needle.as_bytes();
    if nb.is_empty() || from + nb.len() > hb.len() {
        return None;
    }
    (from..=hb.len() - nb.len()).find(|&i| hb[i..i + nb.len()].eq_ignore_ascii_case(nb))
}

fn decode_entities(txt: &str) -> String {
    let chars: Vec<char> = txt.chars().collect();
    let mut out = String::with_capacity(txt.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            let limit = (i + 12).min(chars.len());
            if let Some(j) = (i + 1..limit).find(|&j| chars[j] == ';') {
                let ent: String = chars[i + 1..j].iter().collect();
                let rep = match ent.as_str() {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    "mdash" => Some('—'),
                    "ndash" => Some('–'),
                    "rsquo" => Some('’'),
                    "lsquo" => Some('‘'),
                    "ldquo" => Some('“'),
                    "rdquo" => Some('”'),
                    "hellip" => Some('…'),
                    "copy" => Some('©'),
                    _ => {
                        if let Some(hex) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X"))
                        {
                            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                        } else if let Some(dec) = ent.strip_prefix('#') {
                            dec.parse::<u32>().ok().and_then(char::from_u32)
                        } else {
                            None
                        }
                    }
                };
                if let Some(c) = rep {
                    out.push(c);
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Trim line ends, collapse runs of blank lines to one, trim edges.
fn tidy(txt: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in txt.lines() {
        let l = line.trim();
        if l.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
            out.push('\n');
        } else {
            blank = 0;
            // collapse intra-line whitespace runs left by table markup
            let mut prev_space = false;
            for c in l.chars() {
                if c.is_whitespace() {
                    if !prev_space {
                        out.push(' ');
                    }
                    prev_space = true;
                } else {
                    out.push(c);
                    prev_space = false;
                }
            }
            out.push('\n');
        }
    }
    out.trim().to_string()
}
