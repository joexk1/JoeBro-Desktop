//! Tools (Mac ToolsPanel): API Tools + MCP Servers side by side, Plugins
//! below in one wide glass box. Rows are switch / name + badges / mono detail /
//! pencil + trash. Sheets: API tool, MCP server, Add/Edit plugin, Computer Use
//! and Advisor settings (prefs-backed).
use crate::api::R;
use crate::state::{Job, Model, Shared};
use crate::panels::workkit as wk;
use crate::icons;
use crate::theme::{ty, W};
use crate::widgets;
use eframe::egui::{self, Align, Layout, Rect, RichText, Ui, Vec2};
use serde_json::{json, Value};

const ADVISOR_DEFAULT_DESC: &str = "Ask the user's Advisor — a more powerful model they chose — for a second opinion. Call it when you're genuinely stuck, the problem needs deeper reasoning than you can do (tricky bugs, maths, architecture, high-stakes decisions), your attempts keep failing, or the user asks for a second opinion. Send ONE self-contained question containing ALL the context needed (relevant code, errors, constraints) — the advisor cannot see this chat and has no tools. Weigh its advice against what you know, then act; you own the final answer.";

const ACCESS: [(&str, &str); 3] = [("sandbox", "Bound folder"), ("readonly", "Read-only"), ("full", "Full access")];

#[derive(Default)]
pub struct ToolsPanel {
    api_tools: Vec<Value>,
    mcp: Vec<Value>,
    plugins: Vec<Value>,
    load: Option<Job<(R<Value>, R<Value>, R<Value>)>>,
    loaded: bool,
    action: Option<Job<R<Value>>>,

    // API tool sheet
    show_tool: bool,
    edit_tool: Option<String>,
    tool_has_key: bool,
    t_name: String,
    t_url: String,
    t_key: String,
    t_desc: String,
    t_post: bool,
    // MCP sheet
    show_mcp: bool,
    edit_mcp: Option<String>,
    m_name: String,
    m_cmd: String,
    m_args: String,
    m_perm: String,
    // Add plugin (folder + type) / edit plugin sheets
    show_add_plugin: bool,
    add_folder: Option<String>,
    add_kind: usize,
    show_plugin: bool,
    edit_plugin: Option<String>,
    p_bundled: bool,
    p_name: String,
    p_repo: String,
    p_kind: String,
    p_permission: String,
    // Computer Use / Advisor settings sheets
    show_cu: bool,
    cu_blocked: String,
    cu_job: Option<Job<R<Value>>>,
    show_adv: bool,
    adv_model: Option<Model>,
    adv_desc: String,
    adv_job: Option<Job<R<Value>>>,
    error: Option<String>,
}

impl ToolsPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        self.pump(sh);
        let p = sh.palette;
        let mut refresh = false;
        let err = self.error.clone();
        let busy = self.action.is_some();
        widgets::panel_chrome(
            ui,
            &p,
            "Tools",
            "wrench.and.screwdriver",
            |ui| {
                refresh = widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).on_hover_text("Refresh").clicked();
                if busy {
                    crate::ascii::spinner(ui, 12.0, p.ink2);
                }
            },
            |ui| {
                if let Some(e) = &err {
                    wk::danger_text(ui, &p, e);
                }
                let gap = 14.0;
                let full = ui.available_rect_before_wrap().shrink(2.0);
                let row_h = ((full.height() - gap) / 2.0).max(210.0);
                let half = ((full.width() - gap) / 2.0).floor();
                let left = Rect::from_min_size(full.min, Vec2::new(half, row_h));
                let right = Rect::from_min_size(egui::pos2(full.left() + half + gap, full.top()), Vec2::new(half, row_h));
                let bottom = Rect::from_min_size(egui::pos2(full.left(), full.top() + row_h + gap), Vec2::new(full.width(), row_h));
                wk::box_in_rect(ui, &p, left, 14.0, |ui| self.api_section(ui, sh));
                wk::box_in_rect(ui, &p, right, 14.0, |ui| self.mcp_section(ui, sh));
                wk::box_in_rect(ui, &p, bottom, 14.0, |ui| self.plugin_section(ui, sh));
            },
        );
        if refresh {
            self.error = None;
            self.loaded = false;
        }

        self.tool_sheet(ui, sh);
        self.mcp_sheet(ui, sh);
        self.add_plugin_sheet(ui, sh);
        self.plugin_sheet(ui, sh);
        self.cu_sheet(ui, sh);
        self.adv_sheet(ui, sh);
    }

    fn pump(&mut self, sh: &mut Shared) {
        if !self.loaded && self.load.is_none() {
            self.load = Some(sh.spawn(|a| (a.api_tools(), a.mcp_servers(), a.plugins())));
        }
        if let Some(job) = &mut self.load {
            if let Some((t, m, p)) = job.take() {
                let mut failures = Vec::new();
                match t {
                    Ok(v) => self.api_tools = arr(&v, "tools"),
                    Err(e) => failures.push(format!("API tools: {e}")),
                }
                match m {
                    Ok(v) => self.mcp = arr(&v, "servers"),
                    Err(e) => failures.push(format!("MCP servers: {e}")),
                }
                match p {
                    Ok(v) => self.plugins = arr(&v, "plugins"),
                    Err(e) => failures.push(format!("Plugins: {e}")),
                }
                self.error = if failures.is_empty() { None } else { Some(failures.join(" · ")) };
                self.loaded = true;
                self.load = None;
            }
        }
        if let Some(job) = &mut self.action {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) if v.get("ok").and_then(Value::as_bool) == Some(false) => {
                        self.error = Some(v.get("detail").and_then(Value::as_str).unwrap_or("The action failed.").to_owned())
                    }
                    Err(e) => self.error = Some(e),
                    _ => self.error = None,
                }
                self.loaded = false;
                self.action = None;
            }
        }
        if let Some(job) = &mut self.cu_job {
            if let Some(Ok(v)) = job.take() {
                let lines: Vec<String> = v
                    .get("macos_blocked_apps")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                self.cu_blocked = lines.join("\n");
            }
            if job.done {
                self.cu_job = None;
            }
        }
        if let Some(job) = &mut self.adv_job {
            if let Some(Ok(v)) = job.take() {
                let m = v.get("advisor_model").and_then(Value::as_str).unwrap_or("");
                let e = v.get("advisor_endpoint_id").and_then(Value::as_str).unwrap_or("");
                if !m.is_empty() && !e.is_empty() {
                    self.adv_model = sh.models.iter().find(|x| x.model_id == m && x.endpoint_id == e).cloned();
                }
                let d = v.get("advisor_description").and_then(Value::as_str).unwrap_or("");
                self.adv_desc = if d.is_empty() { ADVISOR_DEFAULT_DESC.to_string() } else { d.to_string() };
            }
            if job.done {
                self.adv_job = None;
            }
        }
    }

    /// Section chrome: "glyph Title" + borderless add button, caption, then
    /// either the centred empty text or a scrolling list.
    #[allow(clippy::too_many_arguments)]
    fn section(
        &mut self,
        ui: &mut Ui,
        sh: &mut Shared,
        title: &str,
        count: usize,
        empty: &str,
        extra_err: Option<&str>,
        body: impl FnOnce(&mut Self, &mut Ui, &mut Shared),
    ) -> bool {
        let p = sh.palette;
        let mut add = false;
        ui.spacing_mut().item_spacing.y = 10.0;
        widgets::section_header(ui, &p, title, Some(count), |ui| {
            add = wk::add_button(ui, &p, "Add").clicked();
        });
        if let Some(e) = extra_err.filter(|e| !e.is_empty()) {
            wk::danger_text(ui, &p, e);
        }
        if count == 0 {
            crate::ascii::empty_state(ui, &p, crate::ascii::Art::Plug, empty);
        } else {
            egui::ScrollArea::vertical().id_salt(("tools_sec", title)).auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.add_space(2.0);
                body(self, ui, sh);
            });
        }
        add
    }

    fn api_section(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let rows = self.api_tools.clone();
        let add = self.section(ui, sh, "API tools", rows.len(), "No API tools yet", None,
            |this, ui, sh| {
                for t in &rows {
                    this.api_row(ui, sh, t);
                }
            },
        );
        if add {
            self.open_tool_form(None);
        }
    }

    fn api_row(&mut self, ui: &mut Ui, sh: &mut Shared, t: &Value) {
        let p = sh.palette;
        let id = sid(t);
        let mut on = enabled(t);
        wk::raised_row(ui, &p, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if widgets::jb_switch(ui, &p, &mut on).changed() {
                    let (i, v) = (id.clone(), on.to_string());
                    self.action = Some(sh.spawn(move |a| a.update_api_tool(&i, &[("is_enabled", &v)])));
                }
                ui.vertical(|ui| {
                    ui.set_max_width((ui.available_width() - 64.0).max(40.0));
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(ty::sized(strv(t, "name", "(unnamed)"), 13.0, W::Medium).color(p.ink));
                        widgets::chip_tag(ui, &p, &strv(t, "method", "GET").to_uppercase(), None);
                        if t.get("has_key").and_then(Value::as_bool) == Some(true) {
                            icons::symbol(ui, "key", 11.0, p.ink3);
                        }
                    });
                    let url = strv(t, "base_url", "");
                    if !url.is_empty() {
                        ui.add(egui::Label::new(ty::sized(url, 11.0, W::Regular).color(p.ink2)).truncate());
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if widgets::icon_btn(ui, &p, "trash", 26.0, false).on_hover_text("Delete").clicked() {
                        let i = id.clone();
                        self.action = Some(sh.spawn(move |a| a.delete_api_tool(&i)));
                    }
                    if widgets::icon_btn(ui, &p, "pencil", 26.0, false).on_hover_text("Edit").clicked() {
                        self.open_tool_form(Some(t));
                    }
                });
            });
        });
    }

    fn mcp_section(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let rows = self.mcp.clone();
        let add = self.section(ui, sh, "MCP servers", rows.len(), "No MCP servers yet", None,
            |this, ui, sh| {
                for m in &rows {
                    this.mcp_row(ui, sh, m);
                }
            },
        );
        if add {
            self.open_mcp_form(None);
        }
    }

    fn mcp_row(&mut self, ui: &mut Ui, sh: &mut Shared, m: &Value) {
        let p = sh.palette;
        let id = sid(m);
        let mut on = enabled(m);
        wk::raised_row(ui, &p, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if widgets::jb_switch(ui, &p, &mut on).changed() {
                    let (i, v) = (id.clone(), on.to_string());
                    self.action = Some(sh.spawn(move |a| a.update_mcp(&i, &[("is_enabled", &v)])));
                }
                ui.vertical(|ui| {
                    ui.set_max_width((ui.available_width() - 64.0).max(40.0));
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(ty::sized(strv(m, "name", "(unnamed)"), 13.0, W::Medium).color(p.ink));
                    let cmd = format!("{} {}", strv(m, "command", ""), strv(m, "args", ""));
                    ui.add(egui::Label::new(ty::sized(cmd.trim(), 11.0, W::Regular).color(p.ink2)).truncate());
                    let err = strv(m, "error", "");
                    let tools: Vec<String> = m
                        .get("tools")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from).or_else(|| x.get("name").and_then(Value::as_str).map(String::from))).collect())
                        .unwrap_or_default();
                    if !err.is_empty() {
                        ui.add(egui::Label::new(ty::sized(err, 11.0, W::Regular).color(p.danger)).wrap());
                    } else if tools.is_empty() {
                        ui.label(ty::sized("No tools", 11.0, W::Regular).color(p.ink3));
                    } else {
                        ui.add(egui::Label::new(ty::sized(tools.join(", "), 11.0, W::Regular).color(p.ink2)).wrap());
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if widgets::icon_btn(ui, &p, "trash", 26.0, false).on_hover_text("Delete").clicked() {
                        let i = id.clone();
                        self.action = Some(sh.spawn(move |a| a.delete_mcp(&i)));
                    }
                    if widgets::icon_btn(ui, &p, "pencil", 26.0, false).on_hover_text("Edit").clicked() {
                        self.open_mcp_form(Some(m));
                    }
                });
            });
        });
    }

    fn plugin_section(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let rows = self.plugins.clone();
        let add = self.section(ui, sh, "Plugins", rows.len(), "No plugins yet", None,
            |this, ui, sh| {
                for pl in &rows {
                    this.plugin_row(ui, sh, pl);
                }
            },
        );
        if add {
            self.add_folder = None;
            self.add_kind = 0;
            self.show_add_plugin = true;
        }
    }

    fn plugin_row(&mut self, ui: &mut Ui, sh: &mut Shared, pl: &Value) {
        let p = sh.palette;
        let id = sid(pl);
        let mut on = enabled(pl);
        let bundled = pl.get("source").and_then(Value::as_str) == Some("bundled");
        wk::raised_row(ui, &p, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if widgets::jb_switch(ui, &p, &mut on).changed() {
                    let (i, v) = (id.clone(), on.to_string());
                    self.action = Some(sh.spawn(move |a| a.update_plugin(&i, &[("is_enabled", &v)])));
                }
                ui.vertical(|ui| {
                    ui.set_max_width((ui.available_width() - 90.0).max(40.0));
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(ty::sized(strv(pl, "name", "(unnamed)"), 13.0, W::Medium).color(p.ink));
                        widgets::chip_tag(ui, &p, if strv(pl, "kind", "foreground") == "background" { "background" } else { "foreground" }, None);
                        if bundled {
                            widgets::chip_tag(ui, &p, "bundled", Some(p.accent));
                        }
                    });
                    let d = strv(pl, "description", "");
                    if !d.is_empty() {
                        ui.add(egui::Label::new(ty::sized(d, 11.0, W::Regular).color(p.ink2)).wrap());
                    }
                    let mut bits: Vec<String> = Vec::new();
                    let ver = strv(pl, "version", "");
                    if !ver.is_empty() {
                        bits.push(format!("v{ver}"));
                    }
                    let c = pl.get("contributes");
                    let nt = c.and_then(|c| c.get("tools")).and_then(Value::as_array).map_or(0, |a| a.len());
                    if nt > 0 {
                        bits.push(if nt == 1 { "1 tool".into() } else { format!("{nt} tools") });
                    }
                    let np = c.and_then(|c| c.get("panels")).and_then(Value::as_array).map_or(0, |a| a.len());
                    if np > 0 {
                        bits.push(if np == 1 { "1 panel".into() } else { format!("{np} panels") });
                    }
                    if c.and_then(|c| c.get("guardrail")).and_then(Value::as_bool) == Some(true) {
                        bits.push("guardrail".into());
                    }
                    let summary = bits.join(" \u{b7} ");
                    if !summary.is_empty() {
                        ui.label(ty::sized(summary, 10.5, W::Regular).color(p.ink3));
                    }
                    let e = strv(pl, "error", "");
                    if !e.is_empty() {
                        ui.add(egui::Label::new(ty::sized(e, 10.0, W::Regular).color(p.danger)).wrap());
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if id == "plugin_macos_use" {
                        if widgets::icon_btn(ui, &p, "gearshape", 26.0, false).on_hover_text("Settings").clicked() {
                            self.show_cu = true;
                            self.cu_job = Some(sh.spawn(|a| a.prefs()));
                        }
                    } else if id == "plugin_advisor" {
                        if widgets::icon_btn(ui, &p, "gearshape", 26.0, false).on_hover_text("Settings").clicked() {
                            self.show_adv = true;
                            self.adv_model = None;
                            self.adv_job = Some(sh.spawn(|a| a.prefs()));
                        }
                    } else if !bundled && widgets::icon_btn(ui, &p, "trash", 26.0, false).on_hover_text("Delete").clicked() {
                        let i = id.clone();
                        self.action = Some(sh.spawn(move |a| a.delete_plugin(&i)));
                    }
                    if widgets::icon_btn(ui, &p, "pencil", 26.0, false).on_hover_text("Edit").clicked() {
                        self.open_plugin_form(pl);
                    }
                });
            });
        });
    }

    // ---- sheets --------------------------------------------------------------

    fn open_tool_form(&mut self, existing: Option<&Value>) {
        match existing {
            Some(t) => {
                self.edit_tool = Some(sid(t));
                self.t_name = strv(t, "name", "");
                self.t_url = strv(t, "base_url", "");
                self.t_desc = strv(t, "description", "");
                self.t_post = strv(t, "method", "GET").eq_ignore_ascii_case("POST");
                self.tool_has_key = t.get("has_key").and_then(Value::as_bool) == Some(true);
            }
            None => {
                self.edit_tool = None;
                self.t_name.clear();
                self.t_url.clear();
                self.t_desc.clear();
                self.t_post = false;
                self.tool_has_key = false;
            }
        }
        self.t_key.clear();
        self.show_tool = true;
    }

    fn tool_sheet(&mut self, ui: &Ui, sh: &mut Shared) {
        if !self.show_tool {
            return;
        }
        let editing = self.edit_tool.is_some();
        let busy = self.action.is_some();
        let can = !busy && !self.t_name.trim().is_empty() && !self.t_url.trim().is_empty();
        let (mut save, mut cancel) = (false, false);
        let esc = widgets::dialog(
            ui.ctx(),
            "api_tool",
            if editing { "Edit API Tool" } else { "Add API Tool" },
            None,
            460.0,
            |ui| {
                if widgets::primary(ui, sh, if editing { "Save" } else { "Add" }, can).clicked() {
                    save = true;
                }
                if widgets::btn(ui, sh, "Cancel").clicked() {
                    cancel = true;
                }
            },
            |ui| {
                widgets::field_label(ui, "Name (e.g. LinkedIn Search)");
                ui.add(egui::TextEdit::singleline(&mut self.t_name).desired_width(f32::INFINITY));
                widgets::field_label(ui, "Base URL (https://api.example.com/v2/search)");
                ui.add(egui::TextEdit::singleline(&mut self.t_url).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                ui.label(
                    RichText::new("Put {query} anywhere in the URL to place the model's input exactly (e.g. …/search?name={query}). Otherwise it's appended as ?query=…")
                        .size(10.5)
                        .color(sh.palette.ink_dim),
                );
                widgets::field_label(ui, if self.tool_has_key { "API key (leave blank to keep)" } else { "API key (optional)" });
                ui.add(egui::TextEdit::singleline(&mut self.t_key).password(true).desired_width(f32::INFINITY));
                let mut idx = self.t_post as usize;
                ui.horizontal(|ui| {
                    ui.label("Method");
                    if widgets::segmented_n(ui, sh, "tool_method", &["GET", "POST"], &mut idx) {
                        self.t_post = idx == 1;
                    }
                });
                widgets::field_label(ui, "Description");
                ui.add(egui::TextEdit::multiline(&mut self.t_desc).desired_rows(4).desired_width(f32::INFINITY));
                ui.label(RichText::new("The model reads this to know when and how to call the tool.").size(10.5).color(sh.palette.ink_dim));
            },
        );
        if esc || cancel {
            self.show_tool = false;
        }
        if save && can {
            let method = if self.t_post { "POST" } else { "GET" };
            let mut owned = vec![
                ("name".to_string(), self.t_name.clone()),
                ("base_url".to_string(), self.t_url.clone()),
                ("method".to_string(), method.to_string()),
                ("description".to_string(), self.t_desc.clone()),
            ];
            // Only send a key when one was typed so editing never wipes it.
            if !self.t_key.is_empty() {
                owned.push(("api_key".to_string(), self.t_key.clone()));
            }
            let edit = self.edit_tool.clone();
            self.action = Some(sh.spawn(move |a| {
                let refs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                match edit {
                    Some(id) => a.update_api_tool(&id, &refs),
                    None => a.create_api_tool(&refs),
                }
            }));
            self.show_tool = false;
        }
    }

    fn open_mcp_form(&mut self, existing: Option<&Value>) {
        match existing {
            Some(m) => {
                self.edit_mcp = Some(sid(m));
                self.m_name = strv(m, "name", "");
                self.m_cmd = strv(m, "command", "");
                self.m_args = strv(m, "args", "");
                self.m_perm = strv(m, "permission_mode", "sandbox");
            }
            None => {
                self.edit_mcp = None;
                self.m_name.clear();
                self.m_cmd.clear();
                self.m_args.clear();
                self.m_perm = "sandbox".into();
            }
        }
        self.show_mcp = true;
    }

    fn mcp_sheet(&mut self, ui: &Ui, sh: &mut Shared) {
        if !self.show_mcp {
            return;
        }
        let editing = self.edit_mcp.is_some();
        let busy = self.action.is_some();
        let can = !busy && !self.m_name.trim().is_empty() && !self.m_cmd.trim().is_empty();
        let (mut save, mut cancel) = (false, false);
        let esc = widgets::dialog(
            ui.ctx(),
            "mcp_server",
            if editing { "Edit MCP Server" } else { "Add MCP Server" },
            None,
            460.0,
            |ui| {
                if widgets::primary(ui, sh, if editing { "Save" } else { "Add" }, can).clicked() {
                    save = true;
                }
                if widgets::btn(ui, sh, "Cancel").clicked() {
                    cancel = true;
                }
            },
            |ui| {
                let dim = sh.palette.ink_dim;
                widgets::field_label(ui, "Name (e.g. Filesystem)");
                ui.add(egui::TextEdit::singleline(&mut self.m_name).desired_width(f32::INFINITY));
                widgets::field_label(ui, "Command (e.g. npx)");
                ui.add(egui::TextEdit::singleline(&mut self.m_cmd).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                widgets::field_label(ui, "Arguments");
                ui.add(egui::TextEdit::multiline(&mut self.m_args).font(egui::TextStyle::Monospace).desired_rows(3).desired_width(f32::INFINITY));
                ui.label(RichText::new("e.g. -y @modelcontextprotocol/server-filesystem /Users/me/Documents").size(10.5).color(dim));
                access_combo(ui, "mcp_perm", "Agent access", &mut self.m_perm);
                ui.label(RichText::new("The permission level the agent must be in to use this server's tools (e.g. phone or computer control should require Full access).").size(10.5).color(dim));
                ui.label(RichText::new("JoeBro launches the server, discovers its tools, and offers them to the model in Agent mode. The first connection may take a moment.").size(10.5).color(dim));
            },
        );
        if esc || cancel {
            self.show_mcp = false;
        }
        if save && can {
            let owned = vec![
                ("name".to_string(), self.m_name.trim().to_string()),
                ("command".to_string(), self.m_cmd.trim().to_string()),
                ("args".to_string(), self.m_args.trim().to_string()),
                ("permission_mode".to_string(), self.m_perm.clone()),
            ];
            let edit = self.edit_mcp.clone();
            self.action = Some(sh.spawn(move |a| {
                let refs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                match edit {
                    Some(id) => a.update_mcp(&id, &refs),
                    None => a.create_mcp(&refs),
                }
            }));
            self.show_mcp = false;
        }
    }

    /// Mac `addPlugin()`: choose the plugin's repo folder + its type.
    fn add_plugin_sheet(&mut self, ui: &Ui, sh: &mut Shared) {
        if !self.show_add_plugin {
            return;
        }
        let (mut add, mut cancel, mut pick) = (false, false, false);
        let can = self.add_folder.is_some() && self.action.is_none();
        let esc = widgets::dialog(
            ui.ctx(),
            "add_plugin",
            "Add Plugin",
            None,
            470.0,
            |ui| {
                if widgets::primary(ui, sh, "Add Plugin", can).clicked() {
                    add = true;
                }
                if widgets::btn(ui, sh, "Cancel").clicked() {
                    cancel = true;
                }
            },
            |ui| {
                ui.label(RichText::new("Choose the folder containing the plugin's repo").size(12.0).color(sh.palette.ink_dim));
                ui.horizontal(|ui| {
                    let t = self.add_folder.clone().unwrap_or_else(|| "No folder selected".into());
                    ui.add(egui::Label::new(RichText::new(t).color(if self.add_folder.is_some() { sh.palette.ink } else { sh.palette.ink_dim })).truncate());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::btn(ui, sh, "Choose folder…").clicked() {
                            pick = true;
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Plugin type:");
                    widgets::segmented_n(ui, sh, "plugin_type", &["Foreground (active tool)", "Background (guardrail)"], &mut self.add_kind);
                });
            },
        );
        if pick {
            sh.picker.open("plugin_repo", "Choose the folder containing the plugin's repo", crate::pickers::PickMode::Folder, None);
        }
        if let Some(dir) = sh.take_picked("plugin_repo") {
            self.add_folder = Some(dir.display().to_string());
        }
        if esc || cancel {
            self.show_add_plugin = false;
        }
        if add && can {
            if let Some(path) = self.add_folder.clone() {
                let name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("Plugin").to_string();
                let kind = if self.add_kind == 1 { "background" } else { "foreground" }.to_string();
                self.action = Some(sh.spawn(move |a| {
                    a.create_plugin(&[("name", &name), ("kind", &kind), ("repo_path", &path), ("description", "")])
                }));
            }
            self.show_add_plugin = false;
        }
    }

    fn open_plugin_form(&mut self, pl: &Value) {
        self.edit_plugin = Some(sid(pl));
        self.p_bundled = pl.get("source").and_then(Value::as_str) == Some("bundled");
        self.p_name = strv(pl, "name", "");
        self.p_repo = strv(pl, "repo_path", "");
        self.p_kind = strv(pl, "kind", "foreground");
        self.p_permission = strv(pl, "permission_mode", "sandbox");
        self.show_plugin = true;
    }

    fn plugin_sheet(&mut self, ui: &Ui, sh: &mut Shared) {
        if !self.show_plugin {
            return;
        }
        let (mut save, mut cancel) = (false, false);
        let esc = widgets::dialog(
            ui.ctx(),
            "edit_plugin",
            "Edit Plugin",
            None,
            460.0,
            |ui| {
                if widgets::primary(ui, sh, "Save", true).clicked() {
                    save = true;
                }
                if widgets::btn(ui, sh, "Cancel").clicked() {
                    cancel = true;
                }
            },
            |ui| {
                widgets::field_label(ui, "Name");
                ui.add_enabled(!self.p_bundled, egui::TextEdit::singleline(&mut self.p_name).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    ui.label("Type");
                    egui::ComboBox::from_id_salt("plugin_kind")
                        .selected_text(if self.p_kind == "background" { "Background (guardrail)" } else { "Foreground (active tool)" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.p_kind, "foreground".into(), "Foreground (active tool)");
                            ui.selectable_value(&mut self.p_kind, "background".into(), "Background (guardrail)");
                        });
                });
                access_combo(ui, "plugin_permission", "Agent access", &mut self.p_permission);
                ui.label(RichText::new("The permission level the agent must be in to use this plugin (computer-use plugins need Full access).").size(11.0).color(sh.palette.ink_dim));
                if !self.p_repo.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&self.p_repo).size(11.0).monospace().color(sh.palette.ink_dim)).truncate());
                }
            },
        );
        if esc || cancel {
            self.show_plugin = false;
        }
        if save {
            if let Some(id) = self.edit_plugin.clone() {
                let fields = vec![
                    ("name".to_string(), self.p_name.clone()),
                    ("kind".to_string(), self.p_kind.clone()),
                    ("permission_mode".to_string(), self.p_permission.clone()),
                ];
                self.action = Some(sh.spawn(move |a| {
                    let refs: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                    a.update_plugin(&id, &refs)
                }));
            }
            self.show_plugin = false;
        }
    }

    fn cu_sheet(&mut self, ui: &Ui, sh: &mut Shared) {
        if !self.show_cu {
            return;
        }
        let mut done = false;
        let esc = widgets::dialog(
            ui.ctx(),
            "computer_use",
            "Computer Use",
            Some("\u{f2db}"),
            440.0,
            |ui| {
                if widgets::primary(ui, sh, "Done", true).clicked() {
                    done = true;
                }
            },
            |ui| {
                let dim = sh.palette.ink_dim;
                ui.label(RichText::new("What it can do").size(12.0).family(crate::theme::medium_font()));
                ui.label(RichText::new("In Agent mode (Full access), JoeBro can drive a Mac: read the frontmost app's windows, menus and buttons as text, open apps, click elements, type, press shortcuts and use menus. It reads the screen as text first and only takes a picture when it must, so it stays fast and cheap.").size(12.0).color(dim));
                ui.label(RichText::new("The Mac it drives needs Accessibility access (System Settings > Privacy & Security > Accessibility). Turn the plugin off above to revoke all of this at once.").size(12.0).color(dim));
                ui.separator();
                ui.label(RichText::new("Blocked apps").size(12.0).family(crate::theme::medium_font()));
                ui.label(RichText::new("One app name per line. JoeBro will refuse to open, click, type into or read any app whose name matches — even if you ask it to. Good for banking and other sensitive apps.").size(12.0).color(dim));
                ui.add(egui::TextEdit::multiline(&mut self.cu_blocked).font(egui::TextStyle::Monospace).desired_rows(6).desired_width(f32::INFINITY));
            },
        );
        if done || esc {
            self.show_cu = false;
            if done {
                let list: Vec<String> = self
                    .cu_blocked
                    .split(|c| c == '\n' || c == ',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                self.action = Some(sh.spawn(move |a| a.set_pref("macos_blocked_apps", json!(list))));
            }
        }
    }

    fn adv_sheet(&mut self, ui: &Ui, sh: &mut Shared) {
        if !self.show_adv {
            return;
        }
        let mut done = false;
        let esc = widgets::dialog(
            ui.ctx(),
            "advisor",
            "Advisor",
            Some("\u{f19d}"),
            460.0,
            |ui| {
                if widgets::primary(ui, sh, "Done", true).clicked() {
                    done = true;
                }
            },
            |ui| {
                let dim = sh.palette.ink_dim;
                ui.label(RichText::new("Advisor model").size(12.0).family(crate::theme::medium_font()));
                ui.label(RichText::new("A more powerful model your everyday AI can call for a second opinion — at any agent access level. It sees only the question it's sent, never the whole chat.").size(12.0).color(dim));
                widgets::model_picker(ui, "advisor_model", sh, &mut self.adv_model);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("When should it be called?").size(12.0).family(crate::theme::medium_font()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("Reset to default").size(11.0).color(sh.palette.accent)).frame(false)).clicked() {
                            self.adv_desc = ADVISOR_DEFAULT_DESC.to_string();
                        }
                    });
                });
                ui.label(RichText::new("This is the advisor tool's description — it's what tells your everyday model when to ask for advice. Edit it to make calls more or less frequent, or to point them at specific topics.").size(12.0).color(dim));
                ui.add(egui::TextEdit::multiline(&mut self.adv_desc).desired_rows(7).desired_width(f32::INFINITY));
            },
        );
        if done || esc {
            self.show_adv = false;
            if done {
                let (m, e) = self.adv_model.as_ref().map(|m| (m.model_id.clone(), m.endpoint_id.clone())).unwrap_or_default();
                let d = self.adv_desc.trim().to_string();
                self.action = Some(sh.spawn(move |a| {
                    a.set_pref("advisor_model", json!(m))?;
                    a.set_pref("advisor_endpoint_id", json!(e))?;
                    // Empty = "use the built-in default" so the default can improve in updates.
                    a.set_pref("advisor_description", json!(if d == ADVISOR_DEFAULT_DESC { String::new() } else { d }))
                }));
            }
        }
    }
}

fn access_combo(ui: &mut Ui, id: &str, label: &str, cur: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id)
            .selected_text(ACCESS.iter().find(|(k, _)| *k == cur.as_str()).map(|a| a.1).unwrap_or("Bound folder"))
            .show_ui(ui, |ui| {
                for (k, l) in ACCESS {
                    ui.selectable_value(cur, k.to_string(), l);
                }
            });
    });
}

fn strv(v: &Value, k: &str, default: &str) -> String {
    match v.get(k).and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => default.to_string(),
    }
}

fn arr(v: &Value, key: &str) -> Vec<Value> {
    v.get(key).and_then(|x| x.as_array()).cloned().or_else(|| v.as_array().cloned()).unwrap_or_default()
}

fn sid(v: &Value) -> String {
    v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn enabled(v: &Value) -> bool {
    v.get("is_enabled").and_then(|x| x.as_bool()).unwrap_or(true)
}
