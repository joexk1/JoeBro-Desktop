//! Chat building blocks mirroring the Mac views: ComposerView (SlideSwitch,
//! LitToggle, permission menu), ChatSlash (commands + popover), ModelPickerMenu,
//! ThinkingSection and the turn-level "steps" line from MessageBubbleView.
use crate::api::R;
use crate::icons;
use crate::state::{Job, Model, Shared};
use crate::theme::{self, Palette, W};
use crate::widgets;
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Response, Rounding, Sense, Stroke, Ui, Vec2};
use serde_json::Value;

// ---- SlideSwitch / LitToggle ---------------------------------------------------------------

/// `SlideSwitch`: two labels, an accent thumb glides to the active one. Returns true on change.
pub fn slide_switch(ui: &mut Ui, p: &Palette, id: &str, left: &str, right: &str, is_left: &mut bool) -> bool {
    let font = theme::jb(11.0, W::Medium);
    let lg = ui.painter().layout_no_wrap(left.to_string(), font.clone(), p.ink);
    let rg = ui.painter().layout_no_wrap(right.to_string(), font.clone(), p.ink);
    let (lw, rw) = (lg.size().x + 18.0, rg.size().x + 18.0);
    let h = lg.size().y + 7.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(lw + rw + 4.0, h + 4.0), Sense::hover());
    let lrect = Rect::from_min_size(rect.min + Vec2::splat(2.0), Vec2::new(lw, h));
    let rrect = Rect::from_min_size(lrect.right_top(), Vec2::new(rw, h));
    let lr = ui.interact(lrect, ui.id().with((id, "l")), Sense::click());
    let rr = ui.interact(rrect, ui.id().with((id, "r")), Sense::click());
    let mut changed = false;
    if lr.clicked() && !*is_left {
        *is_left = true;
        changed = true;
    }
    if rr.clicked() && *is_left {
        *is_left = false;
        changed = true;
    }
    let t = ui.ctx().animate_bool_with_time(ui.id().with((id, "thumb")), *is_left, if theme::reduce_motion() { 0.0 } else { theme::SNAP });
    let thumb = Rect::from_min_max(
        Pos2::new(egui::lerp(rrect.left()..=lrect.left(), t), lrect.top()),
        Pos2::new(egui::lerp(rrect.right()..=lrect.right(), t), lrect.bottom()),
    );
    // Widths differ per side: interpolate the whole rect.
    let thumb = Rect::from_min_max(thumb.min, Pos2::new(thumb.min.x + egui::lerp(rw..=lw, t), thumb.max.y));
    let painter = ui.painter();
    painter.rect_filled(rect, Rounding::same(theme::R_CTRL), p.hover.gamma_multiply(0.6));
    painter.rect_filled(thumb, Rounding::same(theme::R_CTRL - 2.0), p.accent);
    for (r, g, active) in [(lrect, lg, *is_left), (rrect, rg, !*is_left)] {
        let col = if active { p.on_accent } else { p.ink3 };
        painter.galley_with_override_text_color(r.center() - g.size() / 2.0, g, col);
    }
    if lr.hovered() || rr.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    changed
}

/// 26 pt icon button with an arbitrary painter (so JB glyphs work); accent when `on`.
pub fn lit_button(
    ui: &mut Ui,
    p: &Palette,
    size: f32,
    on: bool,
    enabled: bool,
    paint: impl FnOnce(&egui::Painter, Pos2, Color32),
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), if enabled { Sense::click() } else { Sense::hover() });
    let hov = enabled && resp.hovered();
    let down = enabled && resp.is_pointer_button_down_on();
    let r = if down { Rect::from_center_size(rect.center(), rect.size() * 0.92) } else { rect };
    let fill = if down {
        p.pressed
    } else if hov || on {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(r, Rounding::same(theme::R_CTRL), fill);
    let col = if on { p.accent } else if hov { p.ink } else { p.ink2 };
    paint(ui.painter(), r.center(), col.gamma_multiply(if enabled { 1.0 } else { 0.35 }));
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub fn sym_button(ui: &mut Ui, p: &Palette, name: &str, on: bool, enabled: bool) -> Response {
    lit_button(ui, p, 26.0, on, enabled, |pt, c, col| icons::paint_symbol(pt, c, name, 14.0, col))
}

// ---- Slash commands ------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlashKind {
    Action,
    Skill,
    Menu,
    Option,
}

#[derive(Clone, Debug)]
pub struct SlashItem {
    pub name: String,
    pub label: String,
    pub kind: SlashKind,
    pub parent: String,
    pub value: String,
    pub current: bool,
}

impl SlashItem {
    fn new(name: &str, label: &str, kind: SlashKind) -> Self {
        SlashItem { name: name.into(), label: label.into(), kind, parent: String::new(), value: String::new(), current: false }
    }
    fn option(parent: &str, name: &str, label: &str, value: &str, current: bool) -> Self {
        SlashItem {
            name: name.into(),
            label: label.into(),
            kind: SlashKind::Option,
            parent: parent.into(),
            value: value.into(),
            current,
        }
    }
}

pub fn builtins() -> Vec<SlashItem> {
    use SlashKind::*;
    vec![
        SlashItem::new("new", "new chat", Action),
        SlashItem::new("background", "scene, photo or plain", Menu),
        SlashItem::new("palette", "accent colour", Menu),
        SlashItem::new("panels", "panel opacity", Menu),
        SlashItem::new("permissions", "agent file access", Menu),
        SlashItem::new("model", "change model", Action),
        SlashItem::new("open", "go to a panel", Menu),
        SlashItem::new("settings", "", Action),
        SlashItem::new("compact", "shrink context", Action),
        SlashItem::new("copy", "chat to clipboard", Action),
        SlashItem::new("save", "to Documents", Action),
        SlashItem::new("think", "extended thinking", Action),
        SlashItem::new("fast", "", Action),
        SlashItem::new("agent", "", Action),
        SlashItem::new("chat", "", Action),
    ]
}

/// Built-ins + skills, fetched lazily and cached for a minute.
#[derive(Default)]
pub struct SlashSource {
    skills: Vec<SlashItem>,
    loaded: Option<std::time::Instant>,
    job: Option<Job<R<Value>>>,
}

pub const OPEN_TABS: [(&str, &str, crate::state::Tab); 11] = [
    ("chat", "Chat", crate::state::Tab::Chats),
    ("agents", "Agents", crate::state::Tab::Agents),
    ("tasks", "Tasks", crate::state::Tab::Tasks),
    ("night-shift", "Night Shift", crate::state::Tab::NightShift),
    ("memory", "Memory", crate::state::Tab::Memory),
    ("calendar", "Calendar", crate::state::Tab::Calendar),
    ("email", "Email", crate::state::Tab::Email),
    ("inbox", "Inbox", crate::state::Tab::Inbox),
    ("research", "Research", crate::state::Tab::Research),
    ("skills", "Skills", crate::state::Tab::Skills),
    ("tools", "Tools", crate::state::Tab::Tools),
];

impl SlashSource {
    pub fn refresh(&mut self, sh: &Shared) {
        if let Some(job) = &mut self.job {
            if let Some(res) = job.take() {
                self.job = None;
                if let Ok(v) = res {
                    let list = v.get("skills").and_then(Value::as_array).cloned().unwrap_or_default();
                    self.skills = list
                        .iter()
                        .filter_map(|s| {
                            let status = s.get("status").and_then(Value::as_str).unwrap_or("active");
                            if status == "disabled" || status == "archived" {
                                return None;
                            }
                            let n = s.get("name").or_else(|| s.get("id")).and_then(Value::as_str)?;
                            let d = s.get("description").and_then(Value::as_str).unwrap_or("");
                            Some(SlashItem::new(&n.replace(' ', "-"), d, SlashKind::Skill))
                        })
                        .collect();
                    self.loaded = Some(std::time::Instant::now());
                }
            }
            return;
        }
        if self.loaded.map_or(true, |t| t.elapsed().as_secs() >= 60) {
            self.job = Some(sh.spawn(|a| a.skills()));
        }
    }

    /// Choices for a menu command, the current one marked.
    pub fn options(menu: &str, sh: &Shared, perm: &str) -> Vec<SlashItem> {
        match menu {
            "background" => {
                let kind = crate::backdrop::kind();
                let cur_scene = crate::config::get().scene;
                let mut out = vec![
                    SlashItem::option(menu, "plain", "palette ground", "", kind == "palette"),
                    SlashItem::option(menu, "photo", "choose a picture…", "photo", kind == "photo"),
                ];
                for id in crate::backdrop::scene_ids() {
                    let name = if id.contains('/') {
                        std::path::Path::new(&id).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                    } else {
                        id.clone()
                    };
                    out.push(SlashItem::option(menu, &name.replace(' ', "-"), &crate::backdrop::display_name(&id), &id, kind == "scene" && cur_scene == id));
                }
                out
            }
            "palette" => theme::JbPalette::ALL
                .iter()
                .map(|pal| SlashItem::option(menu, pal.id(), pal.label(), pal.id(), theme::palette() == *pal))
                .collect(),
            "panels" => [40, 60, 80, 100]
                .iter()
                .map(|v| {
                    SlashItem::option(
                        menu,
                        &v.to_string(),
                        &if *v == 100 { "solid".to_string() } else { format!("{v}% opaque") },
                        &v.to_string(),
                        (theme::solidity() * 100.0 - *v as f32).abs() < 5.0,
                    )
                })
                .collect(),
            "permissions" => [("sandbox", "bound-folder", "Bound folder"), ("readonly", "read-only", "Read-only"), ("full", "full-access", "Full access")]
                .iter()
                .map(|(v, n, l)| SlashItem::option(menu, n, l, v, perm == *v))
                .collect(),
            "open" => OPEN_TABS
                .iter()
                .map(|(n, l, _)| SlashItem::option(menu, n, l, n, false))
                .collect(),
            _ => {
                let _ = sh;
                vec![]
            }
        }
    }

    /// Skills only (the coding-agent composer's slash menu), prefix matches first.
    pub fn skill_matches(&self, query: &str) -> Vec<SlashItem> {
        let q = query.to_lowercase();
        let (mut starts, rest): (Vec<_>, Vec<_>) = self.skills.iter().cloned().partition(|i| i.name.to_lowercase().starts_with(&q));
        starts.extend(rest.into_iter().filter(|i| i.name.to_lowercase().contains(&q)));
        starts
    }

    pub fn matches(&self, query: &str, sh: &Shared, perm: &str) -> Vec<SlashItem> {
        if let Some((menu, q)) = query.split_once(' ') {
            let (menu, q) = (menu.to_lowercase(), q.to_lowercase());
            let opts = Self::options(&menu, sh, perm);
            if q.is_empty() {
                return opts;
            }
            let (mut a, b): (Vec<_>, Vec<_>) = opts.into_iter().partition(|o| o.name.to_lowercase().starts_with(&q));
            a.extend(b.into_iter().filter(|o| format!("{} {}", o.name, o.label).to_lowercase().contains(&q)));
            return a;
        }
        let q = query.to_lowercase();
        let mut all = builtins();
        all.extend(self.skills.iter().cloned());
        if q.is_empty() {
            return all;
        }
        let (mut starts, rest): (Vec<_>, Vec<_>) = all.into_iter().partition(|i| i.name.to_lowercase().starts_with(&q));
        starts.extend(rest.into_iter().filter(|i| i.name.to_lowercase().contains(&q)));
        starts
    }
}

/// The floating suggestion list, growing upward from `anchor_top` (composer's top-left).
/// Returns (hovered index, picked index).
pub fn slash_popover(
    ctx: &egui::Context,
    p: &Palette,
    items: &[SlashItem],
    selected: usize,
    anchor_top_left: Pos2,
    scroll_to: bool,
) -> (Option<usize>, Option<usize>) {
    let mut hovered = None;
    let mut picked = None;
    egui::Area::new(egui::Id::new("chat_slash_popover"))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor_top_left - Vec2::new(0.0, 8.0))
        .pivot(Align2::LEFT_BOTTOM)
        .interactable(true)
        .show(ctx, |ui| {
            widgets::floating_frame(p, theme::R_CARD).show(ui, |ui| {
                ui.set_width(460.0);
                ui.spacing_mut().item_spacing.y = 1.0;
                egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.add_space(5.0);
                    for (i, item) in items.iter().enumerate() {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width() - 10.0, 28.0), Sense::click());
                        let rect = rect.translate(Vec2::new(5.0, 0.0));
                        let sel = i == selected;
                        if resp.hovered() && ctx.input(|s| s.pointer.delta() != Vec2::ZERO) {
                            hovered = Some(i);
                        }
                        if resp.clicked() {
                            picked = Some(i);
                        }
                        if sel {
                            for s in widgets::hover_row_shapes(p, rect, theme::R_CTRL, true, 1.0) {
                                ui.painter().add(s);
                            }
                            if scroll_to {
                                ui.scroll_to_rect(rect, None);
                            }
                        }
                        let cy = rect.center().y;
                        let mut x = rect.left() + 10.0;
                        let paint = ui.painter();
                        if item.kind == SlashKind::Option {
                            let t = if item.label.is_empty() { &item.name } else { &item.label };
                            let g = paint.layout_no_wrap(t.clone(), theme::jb(13.0, W::Medium), p.ink);
                            paint.galley(Pos2::new(x, cy - g.size().y / 2.0), g, p.ink);
                        } else {
                            let g = paint.layout_no_wrap(format!("/{}", item.name), theme::jb(12.5, W::Medium), p.ink);
                            let gw = g.size().x.min(230.0);
                            paint.galley(Pos2::new(x, cy - g.size().y / 2.0), g, p.ink);
                            x += gw.max(0.0) + 8.0;
                            if item.kind == SlashKind::Skill && item.label.is_empty() {
                                let g = paint.layout_no_wrap("skill".into(), theme::jb(10.5, W::Medium), p.ink2);
                                let r = Rect::from_min_size(Pos2::new(x, cy - 9.0), g.size() + Vec2::new(12.0, 4.0));
                                paint.rect_filled(r, Rounding::same(5.0), p.raised);
                                paint.rect_stroke(r.shrink(0.5), Rounding::same(5.0), Stroke::new(1.0, p.line));
                                paint.galley(r.center() - g.size() / 2.0, g, p.ink2);
                            } else if !item.label.is_empty() {
                                // The Mac column sits at a fixed offset when names are short.
                                x = x.max(rect.left() + 10.0 + 238.0);
                                let avail = rect.right() - 40.0 - x;
                                let mut label = item.label.clone();
                                let mut g = paint.layout_no_wrap(label.clone(), theme::jb(11.0, W::Regular), p.ink3);
                                while g.size().x > avail && label.chars().count() > 1 {
                                    label.pop();
                                    g = paint.layout_no_wrap(format!("{}…", label.trim_end()), theme::jb(11.0, W::Regular), p.ink3);
                                }
                                paint.galley(Pos2::new(x, cy - g.size().y / 2.0), g, p.ink3);
                            }
                        }
                        let mut rx = rect.right() - 10.0;
                        if item.kind == SlashKind::Menu {
                            icons::paint_symbol(paint, Pos2::new(rx - 5.0, cy), "chevron.right", 10.0, p.ink3);
                        } else if sel {
                            widgets::paint_kbd(paint, p, rx, cy, "\u{21a9}");
                        }
                        if item.current {
                            rx -= if item.kind == SlashKind::Menu { 18.0 } else { 0.0 };
                            icons::paint_symbol(paint, Pos2::new(rx - 5.0, cy), "checkmark", 11.0, p.accent);
                        }
                    }
                    ui.add_space(5.0);
                });
            });
        });
    (hovered, picked)
}

// ---- Model picker ---------------------------------------------------------------------------

/// `ModelPickerMenu`: logo + name + chevron, a grouped menu below/above. Returns the picked model.
pub fn model_picker(ui: &mut Ui, sh: &mut Shared, popup_id: egui::Id) -> Option<Model> {
    let p = sh.palette;
    let cur = sh.selected_model.clone();
    let label = cur.as_ref().map(|m| m.display_with_tag()).unwrap_or_else(|| "Select model".into());
    let font = theme::jb(11.5, W::Regular);
    let g = ui.painter().layout_no_wrap(label, font, p.ink2);
    let logo = if cur.is_some() { 11.0 + 5.0 } else { 0.0 };
    let w = 6.0 + logo + g.size().x + 5.0 + 8.0 + 6.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(theme::R_CTRL), p.hover);
    }
    let cy = rect.center().y;
    let mut x = rect.left() + 6.0;
    if let Some(m) = &cur {
        crate::logos::paint_logo(ui, Rect::from_min_size(Pos2::new(x, cy - 5.5), Vec2::splat(11.0)), &m.model_id, p.ink2);
        x += logo;
    }
    let gw = g.size().x;
    ui.painter().galley(Pos2::new(x, cy - g.size().y / 2.0), g, p.ink2);
    icons::paint_symbol(ui.painter(), Pos2::new(x + gw + 5.0 + 4.0, cy), "chevron.down", 8.0, p.ink3);
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    let mut picked = None;
    let mut refresh = false;
    let above = rect.center().y > ui.ctx().screen_rect().center().y;
    egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        &resp,
        if above { egui::AboveOrBelow::Above } else { egui::AboveOrBelow::Below },
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            widgets::floating_frame(&p, theme::R_CARD).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
                ui.set_width(300.0);
                ui.spacing_mut().item_spacing.y = 1.0;
                let groups = sh.grouped_models();
                if groups.is_empty() {
                    ui.label(theme::ty::callout("Add a local endpoint in Settings").color(p.ink2));
                }
                egui::ScrollArea::vertical().max_height(460.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (i, (endpoint, models)) in groups.iter().enumerate() {
                        ui.add_space(if i == 0 { 4.0 } else { 10.0 });
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.label(theme::ty::sized(endpoint, 11.0, W::Semibold).color(p.ink3));
                        });
                        ui.add_space(3.0);
                        for m in models {
                            let (r, rs) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
                            if rs.hovered() {
                                ui.painter().rect_filled(r, Rounding::same(theme::R_CTRL - 2.0), p.hover);
                            }
                            let c = r.center().y;
                            crate::logos::paint_logo(ui, Rect::from_min_size(Pos2::new(r.left() + 8.0 + 2.0, c - 6.0), Vec2::splat(12.0)), &m.model_id, p.ink2);
                            ui.painter().text(Pos2::new(r.left() + 8.0 + 24.0, c), Align2::LEFT_CENTER, m.display_with_tag(), theme::jb(13.0, W::Regular), p.ink);
                            if cur.as_ref().map(|x| x.id == m.id).unwrap_or(false) {
                                icons::paint_symbol(ui.painter(), Pos2::new(r.right() - 14.0, c), "checkmark", 11.0, p.accent);
                            }
                            if rs.clicked() {
                                picked = Some(m.clone());
                                ui.memory_mut(|mem| mem.close_popup());
                            }
                        }
                    }
                });
                widgets::menu_separator(ui, &p);
                if widgets::menu_item(ui, &p, None, "Refresh models", false, true).clicked() {
                    refresh = true;
                    ui.memory_mut(|mem| mem.close_popup());
                }
            });
        },
    );
    if refresh {
        sh.reload_models = true;
    }
    picked
}

// ---- Thinking + steps -----------------------------------------------------------------------

fn open_state(ui: &Ui, id: egui::Id) -> bool {
    ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false)
}

/// `ThinkingSection`: scrambling "thinking" while live, else "thinking" + chevron; the text sits
/// behind a 1 px rule.
pub fn thinking_section(ui: &mut Ui, p: &Palette, id: egui::Id, text: &str, live: bool) {
    let mut open = open_state(ui, id);
    let font = theme::jb(11.5, W::Regular);
    let resp = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if live {
                crate::ascii::scramble_text(ui, p, "thinking", true, font.clone(), p.ink3);
            } else {
                ui.label(egui::RichText::new("thinking").font(font).color(p.ink3));
                icons::symbol(ui, if open { "chevron.down" } else { "chevron.right" }, 7.0, p.ink3);
            }
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    if open || live {
        ui.add_space(6.0);
        let top = ui.cursor().min;
        let out = egui::Frame::none().inner_margin(egui::Margin { left: 10.0, ..Default::default() }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(egui::RichText::new(text).font(theme::jb(12.0, W::Regular)).color(p.ink2)).selectable(true));
        });
        let r = out.response.rect;
        ui.painter().line_segment([Pos2::new(top.x + 0.5, r.top()), Pos2::new(top.x + 0.5, r.bottom())], Stroke::new(1.0, p.line));
    }
}

pub struct Call<'a> {
    pub tool: &'a str,
    pub cmd: &'a str,
    pub code: i64,
    pub output: &'a str,
}

fn running(c: &Call) -> bool {
    c.code == i64::MIN
}

/// Humane activity text beside the steps disclosure (Mac `toolActivityText`).
pub fn activity_text(raw: &str, running: bool) -> &'static str {
    let name = raw.rsplit("__").next().unwrap_or("").to_lowercase();
    let has = |s: &str| name.contains(s);
    let r = |a: &'static str, b: &'static str| if running { a } else { b };
    if has("spotify") && has("search") {
        r("Searching Spotify", "Searched Spotify")
    } else if has("email") || has("mail") {
        if has("send") || has("reply") {
            r("Sending email", "Sent email")
        } else {
            r("Reading email", "Checked email")
        }
    } else if has("weather") {
        r("Checking the weather", "Checked the weather")
    } else if has("plan") {
        r("Updating the plan", "Updated the plan")
    } else if has("reminder") {
        r("Working with reminders", "Checked reminders")
    } else if has("calendar") || has("event") {
        r("Working with the calendar", "Checked the calendar")
    } else if has("memory") || name.starts_with("muse_") {
        r("Working with memory", "Finished memory steps")
    } else if has("read") || has("inspect") {
        r("Reading information", "Read information")
    } else if has("search") || has("find") {
        r("Searching", "Finished searching")
    } else if has("create") || has("write") || has("edit") {
        r("Working on changes", "Finished change steps")
    } else if has("agent") {
        r("Working with agents", "Checked agents")
    } else if name == "bash" || name == "python" {
        r("Running a command", "Finished command steps")
    } else {
        r("Working", "Finished steps")
    }
}

struct Group {
    name: String,
    count: usize,
    cmd: String,
    output: String,
    done: bool,
    failed: bool,
}

fn groups(calls: &[Call]) -> Vec<Group> {
    let mut out: Vec<Group> = vec![];
    for c in calls {
        let done = !running(c);
        let failed = done && c.code != 0;
        if let Some(g) = out.last_mut().filter(|g| g.name == c.tool) {
            if g.count == 1 {
                g.output = [g.cmd.as_str(), g.output.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("\n");
            }
            g.count += 1;
            g.cmd.clear();
            let tail = [c.cmd, c.output].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("\n");
            g.output = format!("{}\n\nCall {}\n{}", g.output, g.count, tail);
            g.done &= done;
            g.failed |= failed;
        } else {
            out.push(Group { name: c.tool.to_string(), count: 1, cmd: c.cmd.to_string(), output: c.output.to_string(), done, failed });
        }
    }
    out
}

/// The turn-level steps line: one chip with a status dot, summary, count, chevron.
pub fn tool_activity(ui: &mut Ui, p: &Palette, id: egui::Id, calls: &[Call], pretty: &dyn Fn(&str) -> String) {
    if calls.is_empty() {
        return;
    }
    let is_running = calls.iter().any(running);
    let failed = calls.iter().any(|c| !running(c) && c.code != 0);
    let summary = if failed && !is_running {
        "Finished with an issue"
    } else {
        let last = calls.iter().rev().find(|c| running(c)).or(calls.last()).map(|c| c.tool).unwrap_or("");
        activity_text(last, is_running)
    };
    let mut open = open_state(ui, id);
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(10.0, 5.0)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let head = ui.horizontal(|ui| {
            ui.set_min_height(22.0);
            ui.spacing_mut().item_spacing.x = 8.0;
            let dot = if failed && !is_running {
                p.danger
            } else if is_running {
                p.live
            } else {
                p.ink3
            };
            widgets::status_dot(ui, dot, is_running, 6.0);
            ui.label(egui::RichText::new(summary).font(theme::jb(11.5, W::Regular)).color(p.ink2));
            ui.label(egui::RichText::new(calls.len().to_string()).font(theme::jb(11.0, W::Regular)).color(p.ink3));
            icons::symbol(ui, if open { "chevron.down" } else { "chevron.right" }, 7.0, p.ink3);
        });
        let r = head.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if r.clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_temp(id, open));
        }
        if open {
            let top = ui.cursor().min;
            let body = egui::Frame::none().inner_margin(egui::Margin { left: 10.0, ..Default::default() }).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (i, g) in groups(calls).iter().enumerate() {
                    step(ui, p, id.with(i), g, pretty);
                }
            });
            let br = body.response.rect;
            ui.painter().line_segment([Pos2::new(top.x + 0.5, br.top()), Pos2::new(top.x + 0.5, br.bottom())], Stroke::new(1.0, p.line));
        }
    });
    let rect = inner.response.rect;
    let rr = Rounding::same(theme::R_CTRL + 1.0);
    ui.painter().set(
        slot,
        egui::Shape::Vec(vec![egui::Shape::rect_filled(rect, rr, p.panel_fill), egui::Shape::rect_stroke(rect.shrink(0.5), rr, Stroke::new(1.0, p.line))]),
    );
}

fn step(ui: &mut Ui, p: &Palette, id: egui::Id, g: &Group, pretty: &dyn Fn(&str) -> String) {
    let mut open = open_state(ui, id);
    let head = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(0.0);
        let col = if g.failed {
            p.danger
        } else if g.done {
            p.ink3
        } else {
            p.live
        };
        widgets::status_dot(ui, col, !g.done, 5.0);
        let name = pretty(&g.name) + &if g.count > 1 { format!(" ({})", g.count) } else { String::new() };
        ui.label(egui::RichText::new(name).font(theme::jb(11.5, W::Regular)).color(p.ink2));
        if !open && !g.cmd.is_empty() {
            let first = g.cmd.lines().next().unwrap_or("");
            let mut s: String = first.chars().take(80).collect();
            if first.chars().count() > 80 {
                s.push('…');
            }
            ui.label(egui::RichText::new(s).font(theme::jb(11.0, W::Regular)).color(p.ink3));
        }
    });
    if head.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    if open && (!g.cmd.is_empty() || !g.output.is_empty()) {
        widgets::surface(ui, p, widgets::Level::Raised, Some(theme::R_CTRL), |ui| {
            egui::Frame::none().inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if !g.cmd.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(&g.cmd).font(theme::jb(11.0, W::Regular)).color(p.ink)).selectable(true));
                }
                if !g.output.is_empty() {
                    egui::ScrollArea::vertical().id_salt(id.with("out")).max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&g.output).font(theme::jb(11.0, W::Regular)).color(p.ink2)).selectable(true));
                    });
                }
            });
        });
    }
}

/// Context ring (10 pt) + percentage, `ctx` in 0..100.
pub fn context_ring(ui: &mut Ui, p: &Palette, ctx: f64) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    let c = rect.center();
    ui.painter().circle_stroke(c, 4.2, Stroke::new(1.6, p.line));
    let frac = (ctx / 100.0).clamp(0.0, 1.0) as f32;
    let col = if ctx > 85.0 { p.danger } else { p.ink3 };
    let n = 24;
    let pts: Vec<Pos2> = (0..=((n as f32 * frac) as usize))
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * frac * (i as f32 / ((n as f32 * frac).max(1.0)));
            c + Vec2::angled(a) * 4.2
        })
        .collect();
    if pts.len() > 1 {
        ui.painter().add(egui::Shape::line(pts, Stroke::new(1.6, col)));
    }
    resp.on_hover_text("Context window used");
    ui.label(egui::RichText::new(format!("{}%", ctx as i64)).font(theme::jb(10.5, W::Regular)).color(p.ink3));
}

/// One row of the file-access menu: colour dot, icon, title, accent check.
pub fn perm_row(ui: &mut Ui, p: &Palette, title: &str, icon: &str, color: Color32, checked: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width().max(200.0), 26.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(theme::R_CTRL - 2.0), p.hover);
    }
    let cy = rect.center().y;
    ui.painter().circle_filled(Pos2::new(rect.left() + 11.0, cy), 3.5, color);
    icons::paint_symbol(ui.painter(), Pos2::new(rect.left() + 28.0, cy), icon, 11.0, p.ink2);
    ui.painter().text(Pos2::new(rect.left() + 44.0, cy), Align2::LEFT_CENTER, title, theme::jb(13.0, W::Regular), p.ink);
    if checked {
        icons::paint_symbol(ui.painter(), Pos2::new(rect.right() - 12.0, cy), "checkmark", 11.0, p.accent);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
