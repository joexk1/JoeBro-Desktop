//! Shared kit for the Work panels (Tasks, Skills, Tools, Research, AI check): the Mac
//! `ExtrasShell`, `QuickAddBar`, `PanelSearchField`, `SelectionBar`, `SkillRowShell`,
//! `ExtrasField` and sheet chrome, built on the JB tokens.
use crate::icons::{self, JbGlyph};
use crate::theme::{self, ty, Palette, W};
use crate::widgets::{self, BtnKind, Level};
use eframe::egui::{self, Rect, Rounding, Sense, Ui, Vec2};

/// `ExtrasShell`: a chrome card header (glyph 16 accent, title 14 semibold, actions on the right,
/// added right to left) then `content` filling the rest, 12 pt below.
pub const MARKER: &str = "Work-panels-marker";
pub fn shell(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    glyph: JbGlyph,
    actions: impl FnOnce(&mut Ui),
    content: impl FnOnce(&mut Ui),
) {
    let w = ui.available_width();
    let _ = egui::Id::new(MARKER);
    ui.scope(|ui| widgets::surface(ui, p, Level::Chrome, Some(theme::R_PANEL), |ui| {
        ui.set_min_width(w);
        egui::Frame::none().inner_margin(egui::Margin::symmetric(16.0, 8.0)).show(ui, |ui| {
            ui.set_min_width(w - 32.0);
            ui.set_min_height(28.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                icons::glyph(ui, glyph, 16.0, p.accent);
                ui.label(ty::sized(title, 14.0, W::Semibold).extra_letter_spacing(-0.2).color(p.ink));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    actions(ui)
                });
            });
        });
    }));
    ui.add_space(theme::S3);
    content(ui);
}

/// Two adjacent compact buttons, the selected one `.secondary`, the other `.ghost` (Mac `HStack(spacing: 2)`).
/// Items are laid out left to right; returns the index clicked.
pub fn seg(ui: &mut Ui, p: &Palette, labels: &[&str], sel: usize) -> Option<usize> {
    let mut hit = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, l) in labels.iter().enumerate() {
            let k = if i == sel { BtnKind::Secondary } else { BtnKind::Ghost };
            if widgets::button_ex(ui, p, k, true, true, *l).clicked() {
                hit = Some(i);
            }
        }
    });
    hit
}

/// A `seg` for use inside a right-to-left actions row (adds the buttons reversed to keep order).
pub fn seg_rtl(ui: &mut Ui, p: &Palette, labels: &[&str], sel: usize) -> Option<usize> {
    let mut hit = None;
    let prev = ui.spacing().item_spacing.x;
    ui.spacing_mut().item_spacing.x = 2.0;
    for (i, l) in labels.iter().enumerate().rev() {
        let k = if i == sel { BtnKind::Secondary } else { BtnKind::Ghost };
        if widgets::button_ex(ui, p, k, true, true, *l).clicked() {
            hit = Some(i);
        }
    }
    ui.spacing_mut().item_spacing.x = prev;
    hit
}

pub fn eyebrow_field(ui: &mut Ui, p: &Palette, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 5.0;
        ui.label(ty::eyebrow(label).color(p.ink2));
        add(ui);
    });
}

/// `PanelSearchField`: magnifier that expands in place into a field with an x.
pub fn search(ui: &mut Ui, p: &Palette, text: &mut String, open: &mut bool, hint: &str) {
    let id = egui::Id::new(("wk_search", hint));
    if !*open {
        if widgets::icon_btn(ui, p, "magnifyingglass", 26.0, false).on_hover_text("Search").clicked() {
            *open = true;
            ui.memory_mut(|m| m.request_focus(id));
        }
        return;
    }
    let focused = ui.memory(|m| m.has_focus(id));
    // right_to_left parent: add the x first so it sits on the right.
    widgets::field_frame(p, focused).inner_margin(egui::Margin::symmetric(10.0, 5.0)).show(ui, |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            icons::symbol(ui, "magnifyingglass", 11.0, p.ink3);
            let r = ui.add(
                egui::TextEdit::singleline(text).id(id).frame(false).desired_width(150.0)
                    .hint_text(egui::RichText::new(hint).color(p.ink3)).font(theme::jb(12.0, W::Regular)),
            );
            if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                text.clear();
                *open = false;
            }
            let (rc, rs) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
            icons::paint_symbol(ui.painter(), rc.center(), "xmark", 10.0, p.ink3);
            if rs.clicked() {
                text.clear();
                *open = false;
            }
        });
    });
}

/// `QuickAddBar`: field (13) + trailing action; returns the submitted text (and clears it).
pub fn quick_add(ui: &mut Ui, p: &Palette, text: &mut String, placeholder: &str, icon: &str, busy: bool, err: Option<&str>) -> Option<String> {
    let id = egui::Id::new(("wk_quick", placeholder));
    let focused = ui.memory(|m| m.has_focus(id));
    let ready = !text.trim().is_empty();
    let mut fire = false;
    let w = ui.available_width();
    widgets::field_frame(p, focused).inner_margin(egui::Margin { left: 12.0, right: 6.0, top: 5.0, bottom: 5.0 }).show(ui, |ui| {
        ui.set_min_width(w - 18.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let trailing = if busy || err.is_some() { 90.0 } else if focused && ready { 62.0 } else { 26.0 };
            let r = ui.add_enabled(
                !busy,
                egui::TextEdit::singleline(text).id(id).frame(false)
                    .desired_width((ui.available_width() - trailing - 8.0).max(60.0))
                    .hint_text(egui::RichText::new(placeholder).color(p.ink3)).font(theme::jb(13.0, W::Regular)),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                fire = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if busy {
                    crate::ascii::spinner(ui, 12.0, p.ink2);
                } else if let Some(e) = err {
                    ui.add(egui::Label::new(ty::caption(e).color(p.danger)).truncate());
                } else {
                    if widgets::icon_btn(ui, p, icon, 24.0, ready).clicked() && ready {
                        fire = true;
                    }
                    if focused && ready {
                        widgets::kbd(ui, p, "\u{21a9}");
                    }
                }
            });
        });
    });
    if fire && ready && !busy {
        return Some(std::mem::take(text).trim().to_string());
    }
    None
}

pub enum SelAct {
    None,
    All,
    Clear,
    Delete,
}

/// `SelectionBar`: "N selected · All/None · trash · x".
pub fn selection_bar(ui: &mut Ui, p: &Palette, count: usize, total: usize) -> SelAct {
    let mut act = SelAct::None;
    let w = ui.available_width();
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(12.0, 5.0)).show(ui, |ui| {
        ui.set_min_width(w - 24.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(ty::sized(format!("{count} selected"), 11.0, W::Medium).color(p.ink));
            if widgets::button_ex(ui, p, BtnKind::Ghost, true, true, if count == total { "None" } else { "All" }).clicked() {
                act = if count == total { SelAct::Clear } else { SelAct::All };
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if widgets::icon_btn(ui, p, "xmark", 26.0, false).on_hover_text("Clear selection").clicked() {
                    act = SelAct::Clear;
                }
                if widgets::icon_btn(ui, p, "trash", 26.0, false).on_hover_text("Delete selected").clicked() {
                    act = SelAct::Delete;
                }
            });
        });
    });
    let r = inner.response.rect;
    ui.painter().set(slot, egui::Shape::rect_filled(r, Rounding::ZERO, p.accent_soft));
    ui.painter().hline(r.x_range(), r.bottom() - 0.5, egui::Stroke::new(1.0, p.line));
    act
}

pub struct RowOut {
    pub resp: egui::Response,
    pub toggled: bool,
    pub clicked: bool,
}

/// `SkillRowShell`: raised card r10, 12 pt padding, always-reserved 24 pt check slot that shows on
/// hover / selecting / checked; accent-soft + accent hairline when checked.
pub fn row_shell(ui: &mut Ui, p: &Palette, id: egui::Id, checked: bool, selecting: bool, add: impl FnOnce(&mut Ui)) -> RowOut {
    let w = ui.available_width();
    let prev: Option<Rect> = ui.ctx().data(|d| d.get_temp(id));
    let hov = prev.map_or(false, |r| ui.ctx().pointer_hover_pos().map_or(false, |pp| r.contains(pp)));
    let show_check = selecting || hov || checked;
    let slot = ui.painter().add(egui::Shape::Noop);
    let mut check_rect = Rect::NOTHING;
    let inner = egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
        ui.set_min_width(w - 24.0);
        ui.set_max_width(w - 24.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (r, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
            check_rect = r;
            if show_check {
                let c = if checked { p.accent } else { p.ink3 };
                icons::paint_symbol(ui.painter(), r.center() - egui::vec2(0.0, 3.0), if checked { "checkmark.circle.fill" } else { "circle" }, 15.0, c);
            }
            ui.vertical(|ui| {
                ui.set_max_width((w - 24.0 - 34.0).max(20.0));
                add(ui)
            });
        });
    });
    let rect = inner.response.rect;
    ui.ctx().data_mut(|d| d.insert_temp(id, rect));
    let fill = if checked { p.accent_soft } else if hov { p.hover } else { p.raised };
    let stroke = if checked { p.accent_a(0.4) } else { p.line };
    let r = Rounding::same(theme::R_CARD);
    ui.painter().set(slot, egui::Shape::Vec(vec![egui::Shape::rect_filled(rect, r, fill), egui::Shape::rect_stroke(rect.shrink(0.5), r, egui::Stroke::new(1.0, stroke))]));
    let resp = ui.interact(rect, id.with("shell"), Sense::click());
    let on_check = resp.clicked() && show_check && resp.interact_pointer_pos().map_or(false, |pp| check_rect.expand(3.0).contains(pp));
    RowOut { clicked: resp.clicked() && !on_check, toggled: on_check, resp: resp.on_hover_cursor(egui::CursorIcon::PointingHand) }
}

/// Sheet window (Mac `.sheet`): opaque surface, `ExtrasSheetHeader` (title 14 semibold, actions right, hairline).
/// Returns true when Escape was pressed.
pub fn sheet(ctx: &egui::Context, p: &Palette, id: &str, title: &str, width: f32, actions: impl FnOnce(&mut Ui), body: impl FnOnce(&mut Ui)) -> bool {
    egui::Window::new("")
        .id(egui::Id::new(("wk_sheet", id)))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(widgets::floating_frame(p, theme::R_PANEL))
        .fixed_size(Vec2::new(width, 0.0))
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                ui.set_min_width(width - 24.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(ty::sized(title, 14.0, W::Semibold).color(p.ink));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| actions(ui));
                });
            });
            let (r, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, p.line);
            egui::Frame::none().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                ui.set_min_width(width - 32.0);
                ui.spacing_mut().item_spacing.y = 12.0;
                body(ui)
            });
        });
    ctx.input(|i| i.key_pressed(egui::Key::Escape))
}

/// Mac `Picker` as a JB-styled combo box over `(value, label)` options.
pub fn picker(ui: &mut Ui, p: &Palette, id: &str, cur: &mut String, opts: &[(&str, &str)]) {
    let label = opts.iter().find(|o| o.0 == cur.as_str()).map(|o| o.1).unwrap_or("");
    let trig = ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(id).selected_text(egui::RichText::new(label).font(theme::jb(13.0, W::Regular)).color(p.ink)).show_ui(ui, |ui| {
            for (v, l) in opts {
                if ui.selectable_label(cur.as_str() == *v, *l).clicked() {
                    *cur = v.to_string();
                }
            }
        });
    });
    let _ = trig;
}

pub fn danger_text(ui: &mut Ui, p: &Palette, s: &str) {
    ui.label(ty::sized(s, 11.0, W::Regular).color(p.danger));
}


/// Same signature as `widgets::dialog`, drawn as a JB sheet (`header` is laid out right to left).
pub fn dialog(
    ctx: &egui::Context,
    p: &Palette,
    id: &str,
    title: &str,
    width: f32,
    header: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui),
) -> bool {
    sheet(ctx, p, id, title, width, header, |ui| {
        egui::ScrollArea::vertical()
            .max_height((ctx.screen_rect().height() - 200.0).max(160.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 10.0;
                body(ui)
            });
    })
}

pub fn primary(ui: &mut Ui, p: &Palette, label: &str, enabled: bool) -> egui::Response {
    widgets::button_ex(ui, p, BtnKind::Primary, true, enabled, label)
}

pub fn ghost(ui: &mut Ui, p: &Palette, label: &str) -> egui::Response {
    widgets::button_ex(ui, p, BtnKind::Ghost, true, true, label)
}

pub fn label(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(ty::eyebrow(text).color(p.ink2));
}

/// A `.jbSurface(.panel)` box over `rect` whose content sits `pad` inside it.
pub fn box_in_rect(ui: &mut Ui, p: &Palette, rect: Rect, pad: f32, add: impl FnOnce(&mut Ui)) {
    let r = Rounding::same(theme::R_PANEL);
    ui.painter().rect_filled(rect, r, widgets::level_fill(p, Level::Panel));
    ui.painter().rect_stroke(rect.shrink(0.5), r, egui::Stroke::new(1.0, p.line));
    let inner = rect.shrink(pad);
    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    c.set_clip_rect(inner.expand(2.0).intersect(ui.clip_rect()));
    add(&mut c);
}

/// `.jbSurface(.raised, radius: rCard)` row with 12 / 10 padding.
pub fn raised_row<R>(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    let w = ui.available_width();
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(ui, |ui| {
        ui.set_min_width(w - 24.0);
        ui.set_max_width(w - 24.0);
        add(ui)
    });
    let r = Rounding::same(theme::R_CARD);
    let rect = inner.response.rect;
    ui.painter().set(slot, egui::Shape::Vec(vec![egui::Shape::rect_filled(rect, r, p.raised), egui::Shape::rect_stroke(rect.shrink(0.5), r, egui::Stroke::new(1.0, p.line))]));
    inner.inner
}

/// Secondary compact "+ Add" button.
pub fn add_button(ui: &mut Ui, p: &Palette, label: &str) -> egui::Response {
    let font = theme::jb(12.0, W::Medium);
    let g = ui.painter().layout_no_wrap(label.to_string(), font, p.ink);
    let size = Vec2::new(9.0 + 12.0 + 5.0 + g.size().x + 9.0, g.size().y + 8.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let down = resp.is_pointer_button_down_on();
    let fill = if down { p.pressed } else if resp.hovered() { p.hover } else { p.raised };
    ui.painter().rect_filled(rect, Rounding::same(theme::R_CTRL), fill);
    ui.painter().rect_stroke(rect.shrink(0.5), Rounding::same(theme::R_CTRL), egui::Stroke::new(1.0, p.line));
    icons::paint_symbol(ui.painter(), egui::pos2(rect.left() + 9.0 + 6.0, rect.center().y), "plus", 12.0, p.ink);
    ui.painter().galley(egui::pos2(rect.left() + 9.0 + 12.0 + 5.0, rect.center().y - g.size().y / 2.0), g, p.ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
