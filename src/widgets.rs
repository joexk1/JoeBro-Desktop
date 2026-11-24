#![allow(dead_code)]
//! Flat JB building blocks (see the "JB components" section at the bottom) plus the older helpers (names kept from the glass era, restyled):
//! capsule controls, list rows, cards, the shared model picker. Panels build on
//! these (and on the visuals set in `theme::apply`) so the look stays uniform.
use crate::state::{Model, Shared};
use crate::theme::{Palette, R_CARD};
use eframe::egui::epaint::{Mesh, RectShape, Shadow, Tessellator};
use eframe::egui::{self, Color32, FontId, Rect, Response, RichText, Rounding, Sense, Ui, Vec2};

/// Linear blend between two colours (both already premultiplied, as Color32 is).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| ((x as f32) * (1.0 - t) + (y as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

/// Tessellate `shape` once and recolour its vertices by height — a smooth
/// vertical gradient that stays exactly inside the (rounded) outline and keeps
/// the tessellator's feathered anti-aliasing.
fn vgradient(shape: egui::Shape, rect: Rect, top: Color32, bottom: Color32) -> egui::Shape {
    let mut mesh = Mesh::default();
    Tessellator::new(1.0, Default::default(), [1, 1], vec![]).tessellate_shape(shape, &mut mesh);
    let h = rect.height().max(1.0);
    for v in &mut mesh.vertices {
        let t = ((v.pos.y - rect.top()) / h).clamp(0.0, 1.0);
        v.color = mix(top, bottom, t).gamma_multiply(v.color.a() as f32 / 255.0);
    }
    egui::Shape::Mesh(mesh)
}

/// A smooth top→bottom gradient fill inside a rounded rect.
pub fn gradient_shape(rect: Rect, rounding: Rounding, top: Color32, bottom: Color32) -> egui::Shape {
    if !rect.is_positive() || rect.height() < 0.5 {
        return egui::Shape::Noop;
    }
    vgradient(
        egui::Shape::Rect(RectShape::filled(rect, rounding, Color32::WHITE)),
        rect,
        top,
        bottom,
    )
}

/// The Liquid Glass rim: a 1 px hairline that is bright along the top edge and
/// fades toward the bottom — light caught by the glass edge.
pub fn rim_shape(rect: Rect, rounding: Rounding, top: Color32, bottom: Color32) -> egui::Shape {
    if !rect.is_positive() {
        return egui::Shape::Noop;
    }
    let r = rect.shrink(0.5);
    vgradient(
        egui::Shape::Rect(RectShape::stroke(r, rounding, egui::Stroke::new(1.0, Color32::WHITE))),
        r,
        top,
        bottom,
    )
}

/// Flat surface: `fill`, 1 px hairline, and (only for floating things) a soft
/// shadow. Name kept from the glass era; `shadow` = floating (menus, palette).
pub fn glass_surface(
    p: &Palette,
    rect: Rect,
    radius: f32,
    fill: Color32,
    shadow: bool,
) -> Vec<egui::Shape> {
    let r = Rounding::same(radius);
    let mut v = Vec::with_capacity(3);
    if shadow {
        v.push(Shadow { offset: Vec2::new(0.0, 12.0), blur: 24.0, spread: 0.0, color: p.shadow }.as_shape(rect, r).into());
    }
    v.push(egui::Shape::rect_filled(rect, r, fill));
    v.push(egui::Shape::rect_stroke(rect.shrink(0.5), r, egui::Stroke::new(1.0, if shadow { p.line_strong } else { p.line })));
    v
}

/// Paint a flat panel (surface at the user's opacity + hairline) into the
/// background layer, under everything else.
pub fn panel_glass(ctx: &egui::Context, p: &Palette, rect: Rect, fill: Color32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.extend(glass_surface(p, rect, crate::theme::R_PANEL, fill, false));
}

/// No-op kept for older call-sites: panels now get their glass from the
/// floating frames in `app.rs`.
pub fn aero_backdrop(_ui: &Ui, _sh: &Shared, _sidebar: bool) {}

/// Quiet section label: 11.5 px semibold, tracked, secondary ink.
pub fn header(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    let col = ui.visuals().override_text_color.unwrap_or(ui.visuals().text_color());
    ui.label(
        RichText::new(text)
            .font(FontId::new(11.5, crate::theme::display_font()))
            .extra_letter_spacing(0.4)
            .color(col.gamma_multiply(0.58)),
    );
    ui.add_space(2.0);
}

/// Panel title: accent glyph + Inter Display — the Mac PanelChrome header.
#[allow(dead_code)]
pub fn panel_title(ui: &mut Ui, glyph: &str, text: &str) -> Response {
    let accent = ui.visuals().hyperlink_color;
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(RichText::new(glyph).family(crate::theme::icon_font()).size(15.0).color(accent));
        ui.add_space(2.0);
        ui.label(
            RichText::new(text)
                .font(FontId::new(19.0, FontFamily::Name("display".into())))
                .extra_letter_spacing(-0.2),
        )
    })
    .inner
}
use eframe::egui::FontFamily;

/// Control face shared by chips/buttons (JBButtonStyle look): accent fill when
/// selected, else raised / hover / pressed with a hairline. `slot` is reserved
/// before the widget so the face lands beneath the label.
fn paint_capsule(ui: &Ui, slot: egui::layers::ShapeIdx, rect: Rect, p: &Palette, selected: bool, resp: &Response) {
    let r = Rounding::same(crate::theme::R_CTRL);
    let hov = ui.ctx().animate_bool_with_time(resp.id.with("hov"), resp.hovered(), 0.16);
    let mut v = Vec::with_capacity(2);
    if selected {
        let a = if resp.is_pointer_button_down_on() { 0.82 } else { 1.0 - 0.08 * hov };
        v.push(egui::Shape::rect_filled(rect, r, crate::theme::with_alpha(p.accent, a)));
    } else {
        let fill = if resp.is_pointer_button_down_on() { p.pressed } else if hov > 0.5 { p.hover } else { p.raised };
        v.push(egui::Shape::rect_filled(rect, r, fill));
        v.push(egui::Shape::rect_stroke(rect.shrink(0.5), r, egui::Stroke::new(1.0, p.line)));
    }
    ui.painter().set(slot, egui::Shape::Vec(v));
}

/// A selectable capsule chip: accent-filled when on, clear glass when off.
pub fn chip(ui: &mut Ui, label: &str, selected: bool, sh: &Shared) -> Response {
    let p = sh.palette;
    let slot = ui.painter().add(egui::Shape::Noop);
    let fg = if selected { p.on_accent } else { p.ink };
    let resp = ui.add(
        egui::Button::new(RichText::new(label).color(fg).family(crate::theme::medium_font()).size(12.5))
            .fill(Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE)
            .min_size(Vec2::new(0.0, 26.0))
            .rounding(Rounding::same(crate::theme::R_CTRL)),
    );
    paint_capsule(ui, slot, resp.rect, &p, selected, &resp);
    resp
}

/// Paint `glyph` so its *ink* (not its font box) is centred on `center`.
/// Nerd Font glyphs sit on wildly different baselines/side-bearings, so
/// centring the galley's row rect leaves many of them visibly off; the glyph
/// mesh bounds are the real painted extent. Snapped to whole pixels so it
/// stays crisp. Non-private-use glyphs (✕, ‹ …) use the UI font.
pub fn paint_glyph(ui: &Ui, center: egui::Pos2, glyph: &str, size: f32, color: Color32) {
    let family = if glyph.chars().next().map_or(false, |c| (c as u32) >= 0xE000) {
        crate::theme::icon_font()
    } else {
        crate::theme::medium_font()
    };
    let galley = ui.painter().layout_no_wrap(glyph.to_owned(), FontId::new(size, family), color);
    let b = galley.mesh_bounds;
    let ppp = ui.ctx().pixels_per_point();
    let snap = |v: f32| (v * ppp).round() / ppp;
    let pos = egui::pos2(snap(center.x - b.center().x), snap(center.y - b.center().y));
    ui.painter().galley(pos, galley, color);
}

/// Width of a glyph's painted ink at `size`.
fn glyph_ink_width(ui: &Ui, glyph: &str, size: f32) -> f32 {
    let g = ui.painter().layout_no_wrap(glyph.to_owned(), FontId::new(size, crate::theme::icon_font()), Color32::WHITE);
    g.mesh_bounds.width().max(1.0)
}

/// Round glyph toggle (Nerd Font icon), same face as `chip`. The glyph is
/// ink-centred in the capsule both ways.
pub fn icon_chip(ui: &mut Ui, glyph: &str, selected: bool, sh: &Shared) -> Response {
    let p = sh.palette;
    let slot = ui.painter().add(egui::Shape::Noop);
    let fg = if selected { p.on_accent } else { p.ink_dim };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(30.0, 28.0), Sense::click());
    paint_capsule(ui, slot, rect, &p, selected, &resp);
    paint_glyph(ui, rect.center(), glyph, 13.0, fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Two-segment capsule switch (Chat | Agent) with a sliding accent thumb.
pub fn segmented(ui: &mut Ui, sh: &Shared, right: &mut bool, labels: (&str, &str)) -> Response {
    let p = sh.palette;
    let font = egui::FontId::new(12.5, crate::theme::medium_font());
    let w0 = ui.painter().layout_no_wrap(labels.0.into(), font.clone(), p.ink).size().x + 24.0;
    let w1 = ui.painter().layout_no_wrap(labels.1.into(), font.clone(), p.ink).size().x + 24.0;
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(w0 + w1 + 4.0, 28.0), egui::Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, *right, 0.22);
    let painter = ui.painter();
    let r = egui::Rounding::same(14.0);
    painter.rect_filled(rect, r, p.paper);
    painter.add(rim_shape(rect, r, p.rim_top.gamma_multiply(0.8), p.rim_bottom));
    let thumb_x = rect.left() + 2.0 + t * w0;
    let thumb_w = w0 + (w1 - w0) * t;
    let thumb = egui::Rect::from_min_size(egui::pos2(thumb_x, rect.top() + 2.0), egui::vec2(thumb_w, 24.0));
    painter.add(gradient_shape(thumb, egui::Rounding::same(12.0), p.accent_hi, p.accent));
    painter.add(rim_shape(thumb, egui::Rounding::same(12.0), egui::Color32::from_white_alpha(100), egui::Color32::TRANSPARENT));
    let c0 = egui::pos2(rect.left() + 2.0 + w0 / 2.0, rect.center().y);
    let c1 = egui::pos2(rect.left() + 2.0 + w0 + w1 / 2.0, rect.center().y);
    let mixc = |on: f32| {
        let a = p.ink;
        let b = p.on_accent;
        let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * on) as u8;
        egui::Color32::from_rgba_premultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), f(a.a(), b.a()))
    };
    painter.text(c0, egui::Align2::CENTER_CENTER, labels.0, font.clone(), mixc(1.0 - t));
    painter.text(c1, egui::Align2::CENTER_CENTER, labels.1, font, mixc(t));
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let pick = pos.x > rect.left() + 2.0 + w0;
            if pick != *right {
                *right = pick;
                resp.mark_changed();
            }
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// One "used" category under a chat reply — memories, skills, tools,
/// plugins — as a compact glass chip (icon + count) that opens a glass
/// popover listing the items. Mirrors the Mac app's MemoriesUsedView /
/// SkillsUsedView / PluginsUsedView, collapsed into a chip.
pub fn used_chip(
    ui: &mut Ui,
    sh: &Shared,
    id: egui::Id,
    glyph: &str,
    noun: (&str, &str),
    items: &[String],
) -> Response {
    let p = sh.palette;
    let popup_id = id.with("used_popover");
    let open = ui.memory(|m| m.is_popup_open(popup_id));
    let slot = ui.painter().add(egui::Shape::Noop);
    let mut job = egui::text::LayoutJob::default();
    let fmt = |font: FontId, color: Color32| egui::TextFormat {
        font_id: font,
        color,
        valign: egui::Align::Center,
        ..Default::default()
    };
    // On the accent fill (open) everything reads in `on_accent`.
    let (c_icon, c_text, c_chev) = if open {
        (p.on_accent, p.on_accent, p.on_accent.gamma_multiply(0.8))
    } else {
        (p.accent_hi, p.ink, p.ink_faint)
    };
    job.append(glyph, 0.0, fmt(FontId::new(10.5, crate::theme::icon_font()), c_icon));
    job.append(
        &items.len().to_string(),
        6.0,
        fmt(FontId::new(11.5, crate::theme::medium_font()), c_text),
    );
    job.append(
        crate::theme::icon::CHEVRON_DOWN,
        5.0,
        fmt(FontId::new(7.0, crate::theme::icon_font()), c_chev),
    );
    let resp = ui
        .add(
            egui::Button::new(job)
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE)
                .min_size(Vec2::new(0.0, 22.0))
                .rounding(Rounding::same(11.0)),
        )
        .on_hover_text(format!(
            "{} {} used",
            items.len(),
            if items.len() == 1 { noun.0 } else { noun.1 }
        ));
    paint_capsule(ui, slot, resp.rect, &p, open, &resp);
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        &resp,
        if resp.rect.center().y > ui.ctx().screen_rect().center().y {
            egui::AboveOrBelow::Above
        } else {
            egui::AboveOrBelow::Below
        },
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(220.0);
            ui.set_max_width(360.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.label(ico(glyph, 11.0, sh).color(p.accent_hi));
                ui.label(
                    RichText::new(format!(
                        "{} {} used",
                        items.len(),
                        if items.len() == 1 { noun.0 } else { noun.1 }
                    ))
                    .size(12.0)
                    .family(crate::theme::medium_font())
                    .color(p.ink),
                );
            });
            ui.add_space(2.0);
            egui::ScrollArea::vertical()
                .id_salt(popup_id.with("scroll"))
                .max_height(260.0)
                .show(ui, |ui| {
                    for it in items {
                        ui.horizontal_top(|ui| {
                            ui.add_space(2.0);
                            ui.label(ico("\u{f444}", 7.0, sh).color(p.accent_hi)); // small dot
                            ui.add(
                                egui::Label::new(RichText::new(it).size(12.0).color(p.ink_dim))
                                    .wrap()
                                    .selectable(true),
                            );
                        });
                    }
                });
        },
    );
    resp
}

/// Plain capsule push button.
pub fn btn(ui: &mut Ui, sh: &Shared, label: impl Into<RichText>) -> Response {
    let p = sh.palette;
    let slot = ui.painter().add(egui::Shape::Noop);
    let resp = ui.add(
        egui::Button::new(label.into().color(p.ink))
            .fill(Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE)
            .min_size(Vec2::new(0.0, 26.0))
            .rounding(Rounding::same(crate::theme::R_CTRL)),
    );
    paint_capsule(ui, slot, resp.rect, &p, false, &resp);
    resp
}

/// Capsule push button with a leading Nerd Font glyph (label stays in Inter).
pub fn btn_icon(ui: &mut Ui, sh: &Shared, glyph: &str, label: &str) -> Response {
    capsule_icon_btn(ui, sh, glyph, label, false, true)
}

/// Accent capsule button with a leading glyph (Mac `.glassProminent` + Label).
pub fn primary_icon(ui: &mut Ui, sh: &Shared, glyph: &str, label: &str, enabled: bool) -> Response {
    capsule_icon_btn(ui, sh, glyph, label, true, enabled)
}

/// Glyph + label are laid out as one group centred in the capsule; the glyph
/// is ink-centred on the capsule's middle.
fn capsule_icon_btn(ui: &mut Ui, sh: &Shared, glyph: &str, label: &str, prominent: bool, enabled: bool) -> Response {
    let p = sh.palette;
    let slot = ui.painter().add(egui::Shape::Noop);
    let gsize = 11.5;
    let gw = glyph_ink_width(ui, glyph, gsize);
    let (gcol, tcol) = if prominent { (p.on_accent, p.on_accent) } else { (p.accent_hi, p.ink) };
    let lab = ui.painter().layout_no_wrap(label.to_owned(), FontId::new(13.0, crate::theme::medium_font()), tcol);
    let gap = 6.0;
    let pad = 12.0;
    let w = pad * 2.0 + gw + gap + lab.size().x;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), if enabled { Sense::click() } else { Sense::hover() });
    paint_capsule(ui, slot, rect, &p, prominent, &resp);
    let fade = if enabled { 1.0 } else { 0.45 };
    let cy = rect.center().y;
    paint_glyph(ui, egui::pos2(rect.left() + pad + gw / 2.0, cy), glyph, gsize, gcol.gamma_multiply(fade));
    let ppp = ui.ctx().pixels_per_point();
    let ty = ((cy - lab.size().y / 2.0) * ppp).round() / ppp;
    let tx = ((rect.left() + pad + gw + gap) * ppp).round() / ppp;
    ui.painter().galley(egui::pos2(tx, ty), lab, tcol.gamma_multiply(fade));
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Primary (accent) capsule button.
pub fn primary(ui: &mut Ui, sh: &Shared, label: impl Into<RichText>, enabled: bool) -> Response {
    let p = sh.palette;
    let slot = ui.painter().add(egui::Shape::Noop);
    let resp = ui.add_enabled(
        enabled,
        egui::Button::new(label.into().color(p.on_accent))
            .fill(Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE)
            .min_size(Vec2::new(0.0, 26.0))
            .rounding(Rounding::same(crate::theme::R_CTRL)),
    );
    paint_capsule(ui, slot, resp.rect, &p, true, &resp);
    resp
}

/// A layered glass card. Returns the frame's response (callers opt into
/// card-level interaction with `.interact(...)`).
pub fn sheet(ui: &mut Ui, sh: &Shared, add: impl FnOnce(&mut Ui)) -> Response {
    let p = sh.palette;
    let avail = ui.available_width();
    let w = if avail.is_finite() && avail > 20.0 { avail } else { 300.0 };
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none()
        .inner_margin(egui::Margin::same(14.0))
        .rounding(Rounding::same(R_CARD))
        .show(ui, |ui| {
            ui.set_min_width((w - 28.0).max(24.0));
            add(ui);
        });
    let mut shapes = vec![Shadow {
        offset: Vec2::new(0.0, 6.0),
        blur: 22.0,
        spread: -4.0,
        color: p.shadow.gamma_multiply(0.7),
    }
    .as_shape(inner.response.rect, Rounding::same(R_CARD))
    .into()];
    shapes.extend(glass_surface(&p, inner.response.rect, R_CARD, p.paper, false));
    ui.painter().set(slot, egui::Shape::Vec(shapes));
    inner.response
}

/// A large clickable card showing `text` (flashcard study face).
pub fn sheet_button(ui: &mut Ui, sh: &Shared, text: &str) -> bool {
    let p = sh.palette;
    let slot = ui.painter().add(egui::Shape::Noop);
    let resp = egui::Frame::none()
        .inner_margin(egui::Margin::same(24.0))
        .show(ui, |ui| {
            ui.set_min_size(Vec2::new(ui.available_width(), 180.0));
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(text).size(19.0));
            });
        });
    ui.painter().set(
        slot,
        egui::Shape::Vec(glass_surface(&p, resp.response.rect, 22.0, p.paper_dim, true)),
    );
    resp.response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// Model picker as a capsule that opens a glass popover, endpoints grouped
/// (local first). Returns true if the selection changed.
pub fn model_picker(ui: &mut Ui, id: &str, sh: &Shared, current: &mut Option<Model>) -> bool {
    let p = sh.palette;
    let label = current
        .as_ref()
        .map(|m| m.display.clone())
        .unwrap_or_else(|| "Select model".into());
    let local = current.as_ref().map(|m| m.category == "local").unwrap_or(false);
    // A global id: stable however the caller's layout nests or reflows.
    let popup_id = egui::Id::new(("model_picker", id));
    let slot = ui.painter().add(egui::Shape::Noop);
    // Mac: ProviderLogoView(size 11, secondary) + 5 pt gap when a model is
    // selected; otherwise the generic glyph.
    const LOGO: f32 = 11.0;
    let logo_model = current.as_ref().map(|m| m.model_id.clone());
    let mut job = egui::text::LayoutJob::default();
    if logo_model.is_none() {
        job.append(
            if local { "\u{f2db}" } else { "\u{f0c2}" },
            0.0,
            egui::TextFormat {
                font_id: FontId::new(11.5, crate::theme::icon_font()),
                color: if local { p.success } else { p.accent_hi },
                valign: egui::Align::Center,
                ..Default::default()
            },
        );
    }
    job.append(
        &label,
        if logo_model.is_some() { LOGO + 5.0 } else { 7.0 },
        egui::TextFormat {
            font_id: FontId::new(12.5, crate::theme::medium_font()),
            color: p.ink,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    job.append(
        "\u{f078}",
        7.0,
        egui::TextFormat {
            font_id: FontId::new(8.0, crate::theme::icon_font()),
            color: p.ink_faint,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    let resp = ui
        .add(
            egui::Button::new(job)
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE)
                .min_size(Vec2::new(0.0, 26.0))
                .rounding(Rounding::same(13.0)),
        )
        .on_hover_text(current.as_ref().map(|m| m.display_with_tag()).unwrap_or_default());
    paint_capsule(ui, slot, resp.rect, &p, false, &resp);
    if let Some(mid) = &logo_model {
        let x = resp.rect.left() + ui.spacing().button_padding.x;
        let r = egui::Rect::from_min_size(egui::pos2(x, resp.rect.center().y - LOGO / 2.0), Vec2::splat(LOGO));
        crate::logos::paint_logo(ui, r, mid, p.ink_dim);
    }
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    let mut changed = false;
    egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        &resp,
        if resp.rect.center().y > ui.ctx().screen_rect().center().y {
            egui::AboveOrBelow::Above
        } else {
            egui::AboveOrBelow::Below
        },
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(300.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                if sh.models.is_empty() {
                    ui.label(RichText::new("No models — add an endpoint in Settings.").color(p.ink_dim));
                }
                for (endpoint, models) in sh.grouped_models() {
                    header(ui, &endpoint);
                    for m in models {
                        let sel = current.as_ref().map(|c| c.id == m.id).unwrap_or(false);
                        let r = row(ui, sh, sel, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(m.display.clone()).size(13.0));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let tag = if m.category == "local" { "Local" } else { "Cloud" };
                                    ui.label(RichText::new(tag).size(10.5).color(p.ink_faint));
                                });
                            });
                        });
                        if r.clicked() {
                            *current = Some(m.clone());
                            changed = true;
                            ui.memory_mut(|mem| mem.close_popup());
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        },
    );
    changed
}

/// A frameless glyph button (24x24 hit area, ink-centred glyph, hover wash).
pub fn icon_button(ui: &mut Ui, glyph: &str) -> Response {
    glyph_button(ui, glyph, 13.0, None)
}

/// Frameless glyph button with explicit size/colour; same centring as `icon_button`.
pub fn glyph_button(ui: &mut Ui, glyph: &str, size: f32, color: Option<Color32>) -> Response {
    let side = (size + 11.0).max(22.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let col = color.unwrap_or_else(|| ui.visuals().text_color().gamma_multiply(0.75));
    if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(side / 2.0), col.gamma_multiply(0.14));
    }
    paint_glyph(ui, rect.center(), glyph, size, col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Put `text` on the system clipboard. egui's own clipboard (arboard) is
/// unreliable under Wayland, so we also pipe to `wl-copy`/`xclip` when present —
/// whichever the session provides wins, and the egui path covers the rest.
pub fn set_clipboard(ctx: &egui::Context, text: &str) {
    ctx.copy_text(text.to_string());
    #[cfg(target_os = "linux")]
    let piped = |cmd: &str, args: &[&str], text: &str| -> bool {
        use std::io::Write;
        match std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                // Don't wait: wl-copy daemonizes to keep owning the selection.
                true
            }
            Err(_) => false,
        }
    };
    // wl-copy (Wayland) first, then xclip (X11) — stop at the first that spawns.
    #[cfg(target_os = "linux")]
    let _ = piped("wl-copy", &[], text) || piped("xclip", &["-selection", "clipboard"], text);
}

/// A visible, reliably-clickable copy button (bordered, explicit min size — a
/// frameless icon button had a near-zero hit area). Returns true when clicked.
pub fn copy_button(ui: &mut Ui, sh: &Shared, hover: &str) -> bool {
    // A check mark for 1.4 s after a click (Mac CopyResponseButton).
    let id = ui.next_auto_id().with("copied");
    let now = ui.input(|i| i.time);
    let copied_at: Option<f64> = ui.data(|d| d.get_temp(id));
    let copied = copied_at.map(|t| now - t < 1.4).unwrap_or(false);
    let (glyph, color) = if copied {
        ("\u{f00c}", sh.palette.success)
    } else {
        (crate::theme::icon::COPY, sh.palette.ink_faint)
    };
    let slot = ui.painter().add(egui::Shape::Noop);
    let resp = ui
        .add(
            egui::Button::new(ico(glyph, 11.5, sh).color(color))
                .frame(false)
                .min_size(Vec2::new(26.0, 22.0)),
        )
        .on_hover_text(if copied { "Copied" } else { hover });
    let hov = ui.ctx().animate_bool_with_time(id.with("hov"), resp.hovered(), 0.12);
    if hov > 0.0 {
        let fill = sh.palette.hover.gamma_multiply(0.7 * hov);
        ui.painter().set(slot, egui::Shape::rect_filled(resp.rect, Rounding::same(7.0), fill));
    }
    if resp.clicked() {
        ui.data_mut(|d| d.insert_temp(id, now));
    }
    if copied {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    }
    resp.clicked()
}

/// A Nerd Font glyph as RichText, for inline icons. Colour is inherited from
/// context (so it flips to the bg tone inside a selected/inverted row).
pub fn ico(glyph: &str, size: f32, _sh: &Shared) -> RichText {
    RichText::new(glyph)
        .family(crate::theme::icon_font())
        .size(size)
}

/// Rewrite GFM pipe-tables into aligned monospace code blocks. egui_commonmark
/// 0.18's own table renderer overlaps cells; a code block renders cleanly.
pub fn preprocess_md(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let is_sep = |l: &str| {
        let t = l.trim();
        t.contains('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
    };
    let parse_row = |l: &str| -> Vec<String> {
        l.trim()
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(|c| c.trim().to_string())
            .collect()
    };
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].contains('|') && i + 1 < lines.len() && is_sep(lines[i + 1]) {
            let header = parse_row(lines[i]);
            let mut body = vec![];
            let mut j = i + 2;
            while j < lines.len() && lines[j].contains('|') && !lines[j].trim().is_empty() {
                body.push(parse_row(lines[j]));
                j += 1;
            }
            let ncol = header
                .len()
                .max(body.iter().map(|r| r.len()).max().unwrap_or(0));
            let mut w = vec![0usize; ncol];
            for r in std::iter::once(&header).chain(body.iter()) {
                for (c, cell) in r.iter().enumerate() {
                    w[c] = w[c].max(cell.chars().count());
                }
            }
            let pad = |r: &[String]| -> String {
                (0..ncol)
                    .map(|c| {
                        let s = r.get(c).map(String::as_str).unwrap_or("");
                        format!("{:<width$}", s, width = w[c])
                    })
                    .collect::<Vec<_>>()
                    .join("  ")
            };
            out.push_str("```\n");
            out.push_str(pad(&header).trim_end());
            out.push('\n');
            out.push_str(
                &w.iter()
                    .map(|x| "-".repeat(*x))
                    .collect::<Vec<_>>()
                    .join("  "),
            );
            out.push('\n');
            for r in &body {
                out.push_str(pad(r).trim_end());
                out.push('\n');
            }
            out.push_str("```\n");
            i = j;
        } else {
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
        }
    }
    out
}

/// A full-width clickable row that inverts when `selected`. Returns a
/// click-sensing Response over the whole row (for clicks + context menus).
///
/// Fills its width by adding trailing space AFTER the content (finite) rather
/// than `set_min_width(available_width())` — the latter can be a huge value for
/// the first item in a fresh ScrollArea and blows the row up.
pub fn row(ui: &mut Ui, sh: &Shared, selected: bool, add: impl FnOnce(&mut Ui)) -> Response {
    row_with_sense(ui, sh, selected, Sense::click(), add)
}

pub fn draggable_row(
    ui: &mut Ui,
    sh: &Shared,
    selected: bool,
    add: impl FnOnce(&mut Ui),
) -> Response {
    row_with_sense(ui, sh, selected, Sense::click_and_drag(), add)
}

fn row_with_sense(
    ui: &mut Ui,
    sh: &Shared,
    selected: bool,
    sense: Sense,
    add: impl FnOnce(&mut Ui),
) -> Response {
    let p = sh.palette;
    // Reserve a paint slot before the contents so the wash lands beneath them.
    let slot = ui.painter().add(egui::Shape::Noop);
    let avail = ui.available_width();
    let w = if avail.is_finite() && avail > 20.0 { avail } else { 200.0 };
    let inner = egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(10.0, 4.0))
        .rounding(Rounding::same(10.0))
        .show(ui, |ui| {
            // Keep the OUTER frame at exactly `w` (subtract the margins).
            ui.set_min_width((w - 20.0).max(24.0));
            ui.spacing_mut().interact_size.y = 20.0;
            add(ui);
        });
    let resp = ui.interact(inner.response.rect, inner.response.id, sense);
    let rect = inner.response.rect;
    let hov = ui.ctx().animate_bool_with_time(resp.id.with("rowhov"), resp.hovered() && !selected, 0.14);
    let shapes = hover_row_shapes(&p, rect, crate::theme::R_CTRL, selected, hov);
    if !shapes.is_empty() {
        ui.painter().set(slot, egui::Shape::Vec(shapes));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---------------------------------------------------------------------------
// Page chrome shared by the workspace panels (Mac PanelKit equivalents).
// ---------------------------------------------------------------------------

/// Mac `PanelChrome` header: a full-width glass capsule holding the accent
/// glyph, the rounded semibold title and trailing actions (right-aligned,
/// laid out right-to-left, so the first action added is the right-most).
pub fn page_header<R>(
    ui: &mut Ui,
    sh: &Shared,
    glyph: &str,
    title: &str,
    actions: impl FnOnce(&mut Ui) -> R,
) -> R {
    let p = sh.palette;
    let w = ui.available_width();
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(16.0, 6.0))
        .show(ui, |ui| {
            ui.set_min_width((w - 32.0).max(24.0));
            ui.set_min_height(30.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (r, _) = ui.allocate_exact_size(Vec2::new(18.0, 26.0), Sense::hover());
                paint_glyph(ui, r.center(), glyph, 14.0, p.accent);
                ui.label(
                    RichText::new(title)
                        .font(FontId::new(15.0, FontFamily::Name("display".into()))),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions).inner
            })
            .inner
        });
    let r = inner.response.rect;
    ui.painter().set(slot, egui::Shape::Vec(glass_surface(&p, r, r.height() / 2.0, p.control, false)));
    ui.add_space(8.0);
    inner.inner
}

/// A recessed glass capsule (launcher / quick-add bars, Mac `joeGlassCapsuleInteractive`).
pub fn glass_capsule<R>(ui: &mut Ui, sh: &Shared, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = sh.palette;
    let w = ui.available_width();
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(14.0, 5.0))
        .show(ui, |ui| {
            ui.set_min_width((w - 28.0).max(24.0));
            ui.set_min_height(28.0);
            ui.horizontal(|ui| add(ui)).inner
        });
    let r = inner.response.rect;
    ui.painter().set(slot, egui::Shape::Vec(glass_surface(&p, r, r.height() / 2.0, p.control, false)));
    ui.add_space(8.0);
    inner.inner
}

/// Mac `ContentUnavailableView`: big faint glyph, semibold title, secondary description.
pub fn empty_state(ui: &mut Ui, sh: &Shared, glyph: &str, title: &str, desc: &str) {
    let p = sh.palette;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.5 - 60.0).clamp(12.0, 220.0));
        let (r, _) = ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
        paint_glyph(ui, r.center(), glyph, 30.0, p.ink_faint);
        ui.add_space(4.0);
        ui.label(RichText::new(title).size(15.0).family(crate::theme::medium_font()).color(p.ink));
        if !desc.is_empty() {
            ui.label(RichText::new(desc).size(12.5).color(p.ink_dim));
        }
    });
}

/// Result of `selection_bar`.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum SelAction {
    None,
    ToggleAll,
    Delete,
    Clear,
}

/// Mac `SelectionBar`: "N selected · All/None ... trash · x" on an accent wash.
pub fn selection_bar(ui: &mut Ui, sh: &Shared, count: usize, total: usize) -> SelAction {
    let p = sh.palette;
    let mut act = SelAction::None;
    let w = ui.available_width();
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(12.0, 4.0))
        .show(ui, |ui| {
            ui.set_min_width((w - 24.0).max(24.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{count} selected")).size(11.0).family(crate::theme::medium_font()));
                let all = if count == total { "None" } else { "All" };
                if ui.add(egui::Button::new(RichText::new(all).size(11.0).color(p.accent)).frame(false)).clicked() {
                    act = SelAction::ToggleAll;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if glyph_button(ui, "\u{f00d}", 11.0, Some(p.ink_dim)).on_hover_text("Clear selection").clicked() {
                        act = SelAction::Clear;
                    }
                    if glyph_button(ui, "\u{f1f8}", 12.0, Some(p.ink_dim))
                        .on_hover_text("Delete selected")
                        .clicked()
                    {
                        act = SelAction::Delete;
                    }
                });
            });
        });
    ui.painter().set(
        slot,
        egui::Shape::rect_filled(inner.response.rect, Rounding::same(8.0), p.accent.gamma_multiply(0.10)),
    );
    act
}

/// A small secondary-ink capsule tag (category / count badges).
pub fn tag(ui: &mut Ui, sh: &Shared, text: &str) {
    let p = sh.palette;
    let g = ui.painter().layout_no_wrap(text.to_owned(), FontId::new(10.0, crate::theme::medium_font()), p.ink_dim);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(g.size().x + 12.0, 15.0), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(7.5), p.control);
    let pos = egui::pos2(rect.left() + 6.0, rect.center().y - g.size().y / 2.0);
    ui.painter().galley(pos, g, p.ink_dim);
}

/// Result of `row_shell`: the card response plus "the check circle was hit".
pub struct ShellRow {
    pub resp: Response,
    /// The check circle was hit.
    pub toggled: bool,
    /// The card body (not the check circle) was clicked.
    pub clicked: bool,
}

/// Mac `SkillRowShell`: a glass card (r14, 14 pt padding) with a reserved 24 pt
/// check-circle slot that appears on hover / while selecting / when checked.
pub fn row_shell(
    ui: &mut Ui,
    sh: &Shared,
    id: egui::Id,
    checked: bool,
    selecting: bool,
    add: impl FnOnce(&mut Ui),
) -> ShellRow {
    let p = sh.palette;
    let w = ui.available_width();
    let w = if w.is_finite() && w > 40.0 { w } else { 300.0 };
    let prev: Option<Rect> = ui.ctx().data(|d| d.get_temp(id));
    let hovering = prev.map_or(false, |r| ui.ctx().pointer_hover_pos().map_or(false, |pp| r.contains(pp)));
    let show_check = selecting || hovering || checked;
    let slot = ui.painter().add(egui::Shape::Noop);
    let mut check_rect = Rect::NOTHING;
    let inner = egui::Frame::none().inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
        ui.set_min_width(w - 28.0);
        ui.set_max_width(w - 28.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (r, cr) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
            check_rect = r;
            if show_check {
                let g = "\u{f058}";
                let col = if checked { p.accent } else { p.ink_dim.gamma_multiply(if cr.hovered() { 1.0 } else { 0.8 }) };
                let g = if checked { g } else { "\u{f10c}" };
                paint_glyph(ui, r.center(), g, 15.0, col);
            }
            ui.vertical(|ui| {
                ui.set_max_width((w - 28.0 - 34.0).max(20.0));
                add(ui)
            });
        });
    });
    let rect = inner.response.rect;
    ui.ctx().data_mut(|d| d.insert_temp(id, rect));
    let mut shapes: Vec<egui::Shape> = glass_surface(&p, rect, 14.0, p.paper, false);
    if checked {
        shapes.push(egui::Shape::rect_filled(rect, Rounding::same(14.0), p.accent.gamma_multiply(0.14)));
    }
    ui.painter().set(slot, egui::Shape::Vec(shapes));
    let resp = ui.interact(rect, id.with("shell"), Sense::click());
    let on_check = resp.clicked() && resp.interact_pointer_pos().map_or(false, |pp| show_check && check_rect.expand(3.0).contains(pp));
    let clicked = resp.clicked() && !on_check;
    ShellRow { resp: resp.on_hover_cursor(egui::CursorIcon::PointingHand), toggled: on_check, clicked }
}

/// "Oct 18, 2026" from unix seconds (Mac `.formatted(date: .abbreviated, time: .omitted)`).
pub fn fmt_date_abbr(secs: f64) -> String {
    let z = (secs / 86400.0).floor() as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {}, {}", MON[(m - 1) as usize], d, y)
}

/// Parse an ISO-8601 prefix ("2026-10-18T10:00:00Z" or a bare date) to unix
/// seconds, treating the stamp as UTC.
pub fn iso_to_secs(s: &str) -> Option<f64> {
    let b = s.get(..10)?;
    let mut it = b.split('-');
    let (y, m, d): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let mut secs = ((era * 146097 + doe - 719468) * 86400) as f64;
    if let Some(t) = s.get(11..19) {
        let mut p = t.split(':');
        if let (Some(h), Some(mi), Some(se)) = (p.next(), p.next(), p.next()) {
            secs += h.parse::<f64>().unwrap_or(0.0) * 3600.0 + mi.parse::<f64>().unwrap_or(0.0) * 60.0 + se.parse::<f64>().unwrap_or(0.0);
        }
    }
    Some(secs)
}

/// "in 3 hours" / "tomorrow" / "2 days ago" (Mac `.relative(presentation: .named)`).
pub fn relative_from_now(secs: f64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let d = secs - now;
    let (n, future) = (d.abs(), d >= 0.0);
    let (v, unit) = if n < 90.0 {
        return "now".into();
    } else if n < 3600.0 {
        ((n / 60.0).round() as i64, "minute")
    } else if n < 86400.0 {
        ((n / 3600.0).round() as i64, "hour")
    } else {
        let days = (n / 86400.0).round() as i64;
        if days == 1 {
            return if future { "tomorrow".into() } else { "yesterday".into() };
        }
        (days, "day")
    };
    let pl = if v == 1 { "" } else { "s" };
    if future { format!("in {v} {unit}{pl}") } else { format!("{v} {unit}{pl} ago") }
}

/// Mac-style sheet: centred dialog, title on the left and the action buttons on
/// the right of a header row, a divider, then the (scrollable) form body.
/// `header` is laid out right-to-left (first button added = right-most).
/// Returns true when Esc was pressed (callers treat it as Cancel).
pub fn dialog(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    glyph: Option<&str>,
    width: f32,
    header: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui),
) -> bool {
    let screen = ctx.screen_rect();
    let w = (screen.width() - 32.0).clamp(280.0, width);
    let max_h = (screen.height() - 120.0).max(160.0);
    egui::Window::new(RichText::new("").size(1.0))
        .id(egui::Id::new(("dialog", id)))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .fixed_size(Vec2::new(w, 0.0))
        .show(ctx, |ui| {
            ui.set_width(w - 20.0);
            ui.horizontal(|ui| {
                if let Some(g) = glyph {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(18.0, 24.0), Sense::hover());
                    paint_glyph(ui, r.center(), g, 13.0, ui.visuals().hyperlink_color);
                }
                ui.label(RichText::new(title).font(FontId::new(14.0, crate::theme::display_font())));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), header);
            });
            ui.add_space(4.0);
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(max_h - 60.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 8.0;
                    body(ui);
                });
        });
    ctx.input(|i| i.key_pressed(egui::Key::Escape))
}

/// Small secondary caption above a form field.
pub fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(ui.visuals().text_color().gamma_multiply(0.6)));
}

/// Mac `PanelSearchField`: a magnifier that expands in place into a search
/// field with an x to collapse and clear. Call inside a right-to-left header.
pub fn search_field(ui: &mut Ui, sh: &Shared, text: &mut String, open: &mut bool, hint: &str) {
    let p = sh.palette;
    if !*open {
        if glyph_button(ui, crate::theme::icon::SEARCH, 12.0, None).on_hover_text("Search").clicked() {
            *open = true;
            ui.memory_mut(|m| m.request_focus(egui::Id::new(("search_field", hint))));
        }
        return;
    }
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(10.0, 3.0)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (r, _) = ui.allocate_exact_size(Vec2::new(12.0, 20.0), Sense::hover());
            paint_glyph(ui, r.center(), crate::theme::icon::SEARCH, 11.0, p.ink_dim);
            ui.add(
                egui::TextEdit::singleline(text)
                    .id(egui::Id::new(("search_field", hint)))
                    .frame(false)
                    .desired_width(150.0)
                    .hint_text(hint),
            );
            if glyph_button(ui, "\u{f057}", 11.0, Some(p.ink_dim)).clicked() {
                text.clear();
                *open = false;
            }
        });
    });
    let r = inner.response.rect;
    ui.painter().set(slot, egui::Shape::rect_filled(r, Rounding::same(r.height() / 2.0), p.field));
}

/// Mac `QuickAddBar`: a glass capsule with a plain field and a trailing action
/// glyph (spinner while busy, error text instead of the glyph when set).
/// Returns the submitted text (trimmed) on Enter / click.
pub fn quick_add(
    ui: &mut Ui,
    sh: &Shared,
    text: &mut String,
    placeholder: &str,
    glyph: &str,
    busy: bool,
    error: Option<&str>,
) -> Option<String> {
    let p = sh.palette;
    let mut fired = false;
    glass_capsule(ui, sh, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if busy {
                ui.add(egui::Spinner::new().size(14.0).color(p.accent_hi));
            } else if let Some(e) = error {
                ui.label(RichText::new(e).size(11.0).color(p.danger));
            } else {
                let enabled = !text.trim().is_empty();
                let col = if enabled { p.accent } else { p.ink_faint };
                if glyph_button(ui, glyph, 13.0, Some(col)).clicked() && enabled {
                    fired = true;
                }
            }
            let te = ui.add_enabled(
                !busy,
                egui::TextEdit::singleline(text)
                    .frame(false)
                    .desired_width(ui.available_width().max(60.0))
                    .hint_text(placeholder)
                    .text_color(p.ink),
            );
            if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !text.trim().is_empty() {
                fired = true;
            }
        });
    });
    if fired && !busy {
        // Keep the text while drafting so a failure can be retried; callers
        // that want it cleared do so themselves.
        return Some(text.trim().to_string());
    }
    None
}

/// A glyph button that opens a small popup menu of `(glyph, label)` items
/// (Mac `Menu { Label… } label: Image(systemName: "plus")`). Returns the picked index.
pub fn menu_glyph(ui: &mut Ui, sh: &Shared, id: &str, glyph: &str, hover: &str, items: &[(&str, &str)]) -> Option<usize> {
    let p = sh.palette;
    let resp = glyph_button(ui, glyph, 13.0, None).on_hover_text(hover);
    let pid = ui.make_persistent_id(("menu_glyph", id));
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(pid));
    }
    let mut pick = None;
    egui::popup_below_widget(ui, pid, &resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(190.0);
        for (i, (g, label)) in items.iter().enumerate() {
            let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width().max(190.0), 26.0), Sense::click());
            if r.hovered() {
                ui.painter().rect_filled(rect, Rounding::same(8.0), p.accent_soft);
            }
            paint_glyph(ui, egui::pos2(rect.left() + 14.0, rect.center().y), g, 12.0, p.accent_hi);
            let gal = ui.painter().layout_no_wrap((*label).to_owned(), FontId::new(13.0, FontFamily::Proportional), p.ink);
            ui.painter().galley(egui::pos2(rect.left() + 30.0, rect.center().y - gal.size().y / 2.0), gal, p.ink);
            if r.clicked() {
                pick = Some(i);
            }
        }
    });
    pick
}

/// Fill the rest of the page with a glass card (Mac `.joeGlassRect(16)`) and
/// lay `add` out inside it, clipped to the card.
pub fn card_area(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui)) {
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());
    card_in_rect(ui, p, rect, add);
}

/// Same, into an explicit rect (side-by-side columns).
pub fn card_in_rect(ui: &mut Ui, p: &Palette, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.painter().extend(glass_surface(p, rect, 16.0, p.paper, false));
    let inner = rect.shrink(1.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(inner.intersect(ui.clip_rect()));
    add(&mut child);
}

/// Small iOS-style switch (Mac `.toggleStyle(.switch)`).
pub fn switch(ui: &mut Ui, sh: &Shared, on: &mut bool) -> Response {
    let size = Vec2::new(32.0, 18.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
    let p = sh.palette;
    let bg = mix(p.ink_faint.gamma_multiply(0.55), p.accent, t);
    ui.painter().rect_filled(rect, rect.height() / 2.0, bg);
    let x = rect.left() + 9.0 + t * (size.x - 18.0);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 7.0, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// N-segment capsule control (Mac `.pickerStyle(.segmented)`) with a sliding
/// accent thumb; labels are centred in their cells. Returns true on change.
pub fn segmented_n(ui: &mut Ui, sh: &Shared, id: &str, labels: &[&str], sel: &mut usize) -> bool {
    let p = sh.palette;
    let font = FontId::new(12.5, crate::theme::medium_font());
    let ws: Vec<f32> = labels
        .iter()
        .map(|l| ui.painter().layout_no_wrap((*l).to_owned(), font.clone(), p.ink).size().x + 24.0)
        .collect();
    let total: f32 = ws.iter().sum();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(total + 4.0, 28.0), Sense::click());
    let rid = resp.id.with(id);
    let tpos = ui.ctx().animate_value_with_time(rid, ws.iter().take(*sel).sum::<f32>(), 0.2);
    let twid = ui.ctx().animate_value_with_time(rid.with("w"), ws.get(*sel).copied().unwrap_or(40.0), 0.2);
    let r = Rounding::same(14.0);
    ui.painter().rect_filled(rect, r, p.paper);
    ui.painter().add(rim_shape(rect, r, p.rim_top.gamma_multiply(0.8), p.rim_bottom));
    let thumb = Rect::from_min_size(egui::pos2(rect.left() + 2.0 + tpos, rect.top() + 2.0), Vec2::new(twid, 24.0));
    ui.painter().add(gradient_shape(thumb, Rounding::same(12.0), p.accent_hi, p.accent));
    ui.painter().add(rim_shape(thumb, Rounding::same(12.0), Color32::from_white_alpha(100), Color32::TRANSPARENT));
    let mut x = rect.left() + 2.0;
    let mut changed = false;
    for (i, l) in labels.iter().enumerate() {
        let cell = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(ws[i], rect.height()));
        let on = i == *sel;
        let col = if on { p.on_accent } else { p.ink };
        let g = ui.painter().layout_no_wrap((*l).to_owned(), font.clone(), col);
        let ppp = ui.ctx().pixels_per_point();
        let pos = egui::pos2(
            ((cell.center().x - g.size().x / 2.0) * ppp).round() / ppp,
            ((cell.center().y - g.size().y / 2.0) * ppp).round() / ppp,
        );
        ui.painter().galley(pos, g, col);
        if resp.clicked() && resp.interact_pointer_pos().map_or(false, |pp| cell.contains(pp)) && !on {
            *sel = i;
            changed = true;
        }
        x += ws[i];
    }
    changed
}

/// Borderless "glyph + label" button (Mac `.buttonStyle(.borderless)` Label):
/// accent text, hover wash, glyph ink-centred against the label.
pub fn link_button(ui: &mut Ui, sh: &Shared, glyph: &str, label: &str) -> Response {
    let p = sh.palette;
    let gsize = 11.0;
    let gw = glyph_ink_width(ui, glyph, gsize);
    let lab = ui.painter().layout_no_wrap(label.to_owned(), FontId::new(12.0, crate::theme::medium_font()), p.accent);
    let w = 8.0 * 2.0 + gw + 6.0 + lab.size().x;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 24.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(12.0), p.accent.gamma_multiply(0.12));
    }
    let cy = rect.center().y;
    paint_glyph(ui, egui::pos2(rect.left() + 8.0 + gw / 2.0, cy), glyph, gsize, p.accent);
    let ppp = ui.ctx().pixels_per_point();
    let pos = egui::pos2(
        ((rect.left() + 8.0 + gw + 6.0) * ppp).round() / ppp,
        ((cy - lab.size().y / 2.0) * ppp).round() / ppp,
    );
    ui.painter().galley(pos, lab, p.accent);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A recessed list row surface (Mac `.background(.quaternary.opacity(0.25), r10)`).
pub fn inset_row<R>(ui: &mut Ui, sh: &Shared, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = sh.palette;
    let w = ui.available_width();
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(ui, |ui| {
        ui.set_min_width((w - 24.0).max(20.0));
        ui.set_max_width((w - 24.0).max(20.0));
        add(ui)
    });
    ui.painter().set(slot, egui::Shape::rect_filled(inner.response.rect, Rounding::same(10.0), p.control.gamma_multiply(0.7)));
    inner.inner
}

/// Accent-tinted capsule tag (e.g. BUNDLED).
pub fn accent_tag(ui: &mut Ui, sh: &Shared, text: &str) {
    let p = sh.palette;
    let g = ui.painter().layout_no_wrap(text.to_owned(), FontId::new(9.0, crate::theme::medium_font()), p.accent);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(g.size().x + 10.0, 15.0), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(7.5), p.accent.gamma_multiply(0.15));
    ui.painter().galley(egui::pos2(rect.left() + 5.0, rect.center().y - g.size().y / 2.0), g, p.accent);
}

// ===========================================================================
// JB components (ports of JBDesign.swift). Take `p: &Palette` (`sh.palette`).
// ===========================================================================

/// Selected row: pressed fill + 2 pt accent tick; hovered: hover fill (`jbHoverRow`).
pub fn hover_row_shapes(p: &Palette, rect: Rect, radius: f32, selected: bool, hov: f32) -> Vec<egui::Shape> {
    let r = Rounding::same(radius);
    let mut v = vec![];
    if selected {
        v.push(egui::Shape::rect_filled(rect, r, p.pressed));
        let tick = Rect::from_min_max(
            egui::pos2(rect.left() - 1.0, rect.top() + 7.0),
            egui::pos2(rect.left() + 1.0, rect.bottom() - 7.0),
        );
        v.push(egui::Shape::rect_filled(tick, Rounding::same(1.0), p.accent));
    } else if hov > 0.01 {
        v.push(egui::Shape::rect_filled(rect, r, crate::theme::with_alpha(p.hover_base(), hov)));
    }
    v
}

/// A full-width hover/selected row with free content (`.jbHoverRow`). Returns the click response.
pub fn hover_row(ui: &mut Ui, p: &Palette, selected: bool, radius: f32, add: impl FnOnce(&mut Ui)) -> Response {
    let slot = ui.painter().add(egui::Shape::Noop);
    let avail = ui.available_width();
    let w = if avail.is_finite() && avail > 20.0 { avail } else { 200.0 };
    let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(10.0, 4.0)).show(ui, |ui| {
        ui.set_min_width((w - 20.0).max(24.0));
        add(ui);
    });
    let rect = inner.response.rect;
    let resp = ui.interact(rect, inner.response.id, Sense::click());
    let hov = ui.ctx().animate_bool_with_time(resp.id.with("hrow"), resp.hovered() && !selected, crate::theme::FADE);
    let shapes = hover_row_shapes(p, rect, radius, selected, hov);
    if !shapes.is_empty() {
        ui.painter().set(slot, egui::Shape::Vec(shapes));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BtnKind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// `JBButtonStyle`: primary = accent fill, secondary = raised + hairline,
/// ghost = text only, danger = ghost in danger ink. `compact` = 12 pt / tighter.
pub fn button_ex(ui: &mut Ui, p: &Palette, kind: BtnKind, compact: bool, enabled: bool, label: impl Into<String>) -> Response {
    let text: String = label.into();
    let font = crate::theme::jb(if compact { 12.0 } else { 13.0 }, crate::theme::W::Medium);
    let fg = match kind {
        BtnKind::Primary => p.on_accent,
        BtnKind::Danger => p.danger,
        _ => p.ink,
    };
    let pad = Vec2::new(if compact { 9.0 } else { 12.0 }, if compact { 4.0 } else { 7.0 });
    let galley = ui.painter().layout_no_wrap(text, font, fg);
    let size = galley.size() + pad * 2.0;
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let hov = ui.ctx().animate_bool_with_time(resp.id.with("bh"), resp.hovered() && enabled, crate::theme::FADE);
    let down = resp.is_pointer_button_down_on() && enabled;
    let fill = match kind {
        BtnKind::Primary => crate::theme::with_alpha(p.accent, if down { 0.82 } else { 1.0 - 0.08 * hov }),
        BtnKind::Secondary => if down { p.pressed } else if hov > 0.5 { p.hover } else { p.raised },
        BtnKind::Ghost | BtnKind::Danger => if down { p.pressed } else if hov > 0.5 { p.hover } else { Color32::TRANSPARENT },
    };
    // Press: scale .97 about the centre.
    let r = if down { Rect::from_center_size(rect.center(), rect.size() * 0.97) } else { rect };
    let op = if enabled { 1.0 } else { 0.4 };
    let painter = ui.painter();
    painter.rect_filled(r, Rounding::same(crate::theme::R_CTRL), fill.gamma_multiply(op));
    if kind == BtnKind::Secondary {
        painter.rect_stroke(r.shrink(0.5), Rounding::same(crate::theme::R_CTRL), egui::Stroke::new(1.0, p.line.gamma_multiply(op)));
    }
    painter.galley(r.center() - galley.size() / 2.0, galley, fg.gamma_multiply(op));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn button(ui: &mut Ui, p: &Palette, kind: BtnKind, label: impl Into<String>) -> Response {
    button_ex(ui, p, kind, false, true, label)
}

/// Square icon button (`JBIconButtonStyle`): SF name, hover fill, press scale .92.
pub fn icon_btn(ui: &mut Ui, p: &Palette, name: &str, size: f32, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let hov = ui.ctx().animate_bool_with_time(resp.id.with("ib"), resp.hovered() || active, crate::theme::FADE);
    let down = resp.is_pointer_button_down_on();
    let r = if down { Rect::from_center_size(rect.center(), rect.size() * 0.92) } else { rect };
    let fill = if down { p.pressed } else if hov > 0.5 { p.hover } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(r, Rounding::same(crate::theme::R_CTRL), fill);
    let col = if active { p.accent } else if resp.hovered() { p.ink } else { p.ink2 };
    crate::icons::paint_symbol(ui.painter(), r.center(), name, size * 0.48 / 0.9 + 1.0, col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Panel,
    Raised,
    Chrome,
}

/// The fill for a surface level (`jbSurface`).
pub fn level_fill(p: &Palette, l: Level) -> Color32 {
    match l {
        Level::Panel | Level::Chrome => p.panel_fill,
        Level::Raised => p.raised,
    }
}

/// `jbSurface`: flat fill + 1 px hairline. `radius` None = capsule.
pub fn surface<R>(ui: &mut Ui, p: &Palette, level: Level, radius: Option<f32>, add: impl FnOnce(&mut Ui) -> R) -> R {
    let slot = ui.painter().add(egui::Shape::Noop);
    let out = add(ui);
    let rect = ui.min_rect();
    let r = Rounding::same(radius.unwrap_or(rect.height() / 2.0));
    ui.painter().set(
        slot,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(rect, r, level_fill(p, level)),
            egui::Shape::rect_stroke(rect.shrink(0.5), r, egui::Stroke::new(1.0, p.line)),
        ]),
    );
    out
}

/// `jbCard`: padded raised card at rCard (full width).
pub fn card<R>(ui: &mut Ui, p: &Palette, padding: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let w = ui.available_width();
    let w = if w.is_finite() { w } else { 300.0 };
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none().inner_margin(egui::Margin::same(padding)).show(ui, |ui| {
        ui.set_min_width(w - padding * 2.0);
        add(ui)
    });
    let rect = inner.response.rect;
    let r = Rounding::same(crate::theme::R_CARD);
    ui.painter().set(
        slot,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(rect, r, p.raised),
            egui::Shape::rect_stroke(rect.shrink(0.5), r, egui::Stroke::new(1.0, p.line)),
        ]),
    );
    inner.inner
}

/// Frame for popovers, menus, palettes (`jbFloating`): opaque surface, lineStrong, shadow.
pub fn floating_frame(p: &Palette, radius: f32) -> egui::Frame {
    egui::Frame::none()
        .fill(p.surface)
        .stroke(egui::Stroke::new(1.0, p.line_strong))
        .rounding(Rounding::same(radius))
        .shadow(Shadow { offset: Vec2::new(0.0, 12.0), blur: 24.0, spread: 0.0, color: p.shadow })
}

/// Uppercase-free mono-ish eyebrow label (`jbEyebrow`: 12 semibold ink2).
pub fn eyebrow(ui: &mut Ui, p: &Palette, text: &str) -> Response {
    ui.label(crate::theme::ty::eyebrow(text).color(p.ink2))
}

/// `JBSectionHeader`: eyebrow + optional count + trailing accessory (right-aligned).
pub fn section_header(ui: &mut Ui, p: &Palette, title: &str, count: Option<usize>, trailing: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        eyebrow(ui, p, title);
        if let Some(c) = count {
            ui.label(crate::theme::ty::sized(c.to_string(), 12.0, crate::theme::W::Regular).color(p.ink3));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), trailing);
    });
}

/// Text field chrome (`jbField`): raised fill, hairline, accent ring on focus.
pub fn field_frame(p: &Palette, focused: bool) -> egui::Frame {
    egui::Frame::none()
        .fill(p.raised)
        .stroke(egui::Stroke::new(1.0, if focused { crate::theme::with_alpha(p.accent, 0.7) } else { p.line }))
        .rounding(Rounding::same(crate::theme::R_CTRL))
        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
}

/// Single-line field with the JB chrome. Returns the TextEdit response.
pub fn field(ui: &mut Ui, p: &Palette, text: &mut String, hint: &str) -> Response {
    let id = ui.next_auto_id();
    let focused = ui.memory(|m| m.has_focus(id.with("te")));
    field_frame(p, focused)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(text)
                    .id(id.with("te"))
                    .hint_text(RichText::new(hint).color(p.ink3))
                    .frame(false)
                    .desired_width(f32::INFINITY)
                    .font(crate::theme::jb(13.0, crate::theme::W::Regular)),
            )
        })
        .inner
}

/// `JBChip`: tag / status text on a faint tint with a hairline.
pub fn chip_tag(ui: &mut Ui, p: &Palette, text: &str, tint: Option<Color32>) -> Response {
    let t = tint.unwrap_or(p.ink2);
    let base = tint.unwrap_or(p.ink);
    let galley = ui.painter().layout_no_wrap(text.to_string(), crate::theme::jb(12.0, crate::theme::W::Medium), t);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + Vec2::new(14.0, 6.0), Sense::hover());
    let r = Rounding::same(5.0);
    ui.painter().rect_filled(rect, r, crate::theme::with_alpha(base, 0.08));
    ui.painter().rect_stroke(rect.shrink(0.5), r, egui::Stroke::new(1.0, crate::theme::with_alpha(base, 0.12)));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t);
    resp
}

/// Paint a `JBKbd` right-aligned at `right_x`, centred on `cy`. Returns its width.
pub fn paint_kbd(painter: &egui::Painter, p: &Palette, right_x: f32, cy: f32, keys: &str) -> f32 {
    let galley = painter.layout_no_wrap(keys.to_string(), crate::theme::jb(10.5, crate::theme::W::Medium), p.ink3);
    let size = galley.size() + Vec2::new(10.0, 3.0);
    let rect = Rect::from_min_size(egui::pos2(right_x - size.x, cy - size.y / 2.0), size);
    painter.rect_stroke(rect.shrink(0.5), Rounding::same(4.0), egui::Stroke::new(1.0, p.line));
    painter.galley(rect.center() - galley.size() / 2.0, galley, p.ink3);
    size.x
}

/// `JBKbd`: shortcut hint, e.g. `kbd(ui, p, &shortcut_label("K"))`.
pub fn kbd(ui: &mut Ui, p: &Palette, keys: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(keys.to_string(), crate::theme::jb(10.5, crate::theme::W::Medium), p.ink3);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + Vec2::new(10.0, 3.0), Sense::hover());
    ui.painter().rect_stroke(rect.shrink(0.5), Rounding::same(4.0), egui::Stroke::new(1.0, p.line));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, p.ink3);
    resp
}

/// `JBStatusDot`; `live` breathes (an expanding faded ring).
pub fn status_dot(ui: &mut Ui, color: Color32, live: bool, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if live && !crate::theme::reduce_motion() {
        let ph = ((ui.ctx().input(|i| i.time) / 1.6) % 1.0) as f32;
        ui.painter().circle_filled(rect.center(), size / 2.0 * (1.0 + 1.4 * ph), crate::theme::with_alpha(color, 0.35 * (1.0 - ph)));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
    resp
}

/// `JBSwitchStyle`: live-tinted switch.
pub fn jb_switch(ui: &mut Ui, p: &Palette, on: &mut bool) -> Response {
    let size = Vec2::new(32.0, 18.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = crate::theme::anim(ui.ctx(), resp.id, *on, crate::theme::SNAP);
    let bg = mix(p.ink3.gamma_multiply(0.55), p.live, t);
    ui.painter().rect_filled(rect, rect.height() / 2.0, bg);
    let x = rect.left() + 9.0 + t * (size.x - 18.0);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 7.0, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Palette swatch (`JBPaletteSwatch`): mono is half black / half white.
pub fn palette_swatch(ui: &mut Ui, p: &Palette, pal: crate::theme::JbPalette, size: f32, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let c = rect.center();
    if pal == crate::theme::JbPalette::Mono {
        ui.painter().circle_filled(c, size / 2.0, Color32::BLACK);
        // right half white
        let mut pts = vec![];
        for i in 0..=16 {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 16.0;
            pts.push(c + Vec2::new(a.cos(), a.sin()) * size / 2.0);
        }
        ui.painter().add(egui::Shape::convex_polygon(pts, Color32::WHITE, egui::Stroke::NONE));
    } else {
        ui.painter().circle_filled(c, size / 2.0, pal.accent(p.dark));
    }
    ui.painter().circle_stroke(c, size / 2.0 - 0.5, egui::Stroke::new(1.0, p.line));
    if selected {
        ui.painter().circle_stroke(c, size / 2.0 + 3.0, egui::Stroke::new(1.5, p.ink));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Fade-and-rise on first appear (`jbAppear`). Stagger lists with `delay = idx * 0.03` (cap ~0.3).
/// `id` must be stable per element.
pub fn appear<R>(ui: &mut Ui, id: impl std::hash::Hash, delay: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let id = ui.id().with(("appear", id));
    let t0: f64 = ui.ctx().data_mut(|d| *d.get_temp_mut_or_insert_with(id, || ui.ctx().input(|i| i.time)));
    let since = (ui.ctx().input(|i| i.time) - t0) as f32 - delay;
    let k = if crate::theme::reduce_motion() { 1.0 } else { crate::theme::ease_out(since / crate::theme::SMOOTH) };
    if k < 1.0 {
        ui.ctx().request_repaint();
    }
    ui.scope(|ui| {
        ui.multiply_opacity(k);
        ui.add_space((1.0 - k) * 6.0);
        add(ui)
    })
    .inner
}

/// Copy feedback: swaps a copy icon for a check for ~1.4 s after `trigger`.
/// Returns true while the check is showing. Usage: `if copy_check(ui, id, clicked) {..}`.
pub fn copy_check(ctx: &egui::Context, id: egui::Id, trigger: bool) -> bool {
    let now = ctx.input(|i| i.time);
    if trigger {
        ctx.data_mut(|d| d.insert_temp(id, now));
    }
    let t: Option<f64> = ctx.data(|d| d.get_temp(id));
    match t {
        Some(t0) if now - t0 < 1.4 => {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            true
        }
        _ => false,
    }
}

/// Undo toast ("Deleted X" + Undo), bottom centre. Returns true when Undo was clicked.
/// Call every frame while `msg` is Some; clear it yourself after `dur` seconds or on undo.
pub fn undo_toast(ctx: &egui::Context, p: &Palette, msg: &str) -> bool {
    let mut undo = false;
    egui::Area::new(egui::Id::new("jb-undo-toast"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -28.0))
        .show(ctx, |ui| {
            floating_frame(p, crate::theme::R_CARD).inner_margin(egui::Margin::symmetric(14.0, 8.0)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::theme::ty::body(msg).color(p.ink));
                    if button_ex(ui, p, BtnKind::Ghost, true, true, "Undo").clicked() {
                        undo = true;
                    }
                });
            });
        });
    undo
}

// ---- shell-level helpers ----------------------------------------------------

/// "Ctrl+K" (Linux/Windows) or "⌘K" (macOS) for a key name (`JBKbd` text).
pub fn shortcut_label(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("\u{2318}{key}") } else { format!("Ctrl+{key}") }
}

/// `PanelChrome`: the flat panel every workspace panel sits in: surface at the user's
/// opacity + hairline, a 44 pt header (icon, title 15 semibold, right-aligned `actions`),
/// a hairline, then `content` filling the rest. Fills `ui.max_rect()`.
pub fn panel_chrome(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    icon: &str,
    actions: impl FnOnce(&mut Ui),
    content: impl FnOnce(&mut Ui),
) {
    use crate::theme::{ty, W};
    let rect = ui.max_rect();
    ui.painter().extend(glass_surface(p, rect, crate::theme::R_PANEL, p.panel_fill, false));
    let hdr = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 44.0));
    ui.allocate_new_ui(
        egui::UiBuilder::new().max_rect(hdr.shrink2(Vec2::new(16.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            crate::icons::symbol(ui, icon, 15.0, p.ink2);
            ui.label(ty::sized(title, 15.0, W::Semibold).extra_letter_spacing(-0.2).color(p.ink));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                actions(ui)
            });
        },
    );
    ui.painter().hline(rect.x_range(), hdr.bottom(), egui::Stroke::new(1.0, p.line));
    let body = Rect::from_min_max(egui::pos2(rect.left(), hdr.bottom() + 1.0), rect.max);
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body), |ui| {
        ui.set_clip_rect(body.intersect(ui.clip_rect()));
        content(ui)
    });
    ui.advance_cursor_after_rect(rect);
}

/// The Appearance popover body (`GlassTunerPopover`): palette swatches + opacity slider.
/// Applies changes immediately (`theme::apply`) and refreshes `sh.palette`.
pub fn appearance_tuner(ui: &mut Ui, sh: &mut Shared) {
    use crate::theme::{self, ty, JbPalette};
    let p = sh.palette;
    let mut changed = false;
    ui.spacing_mut().item_spacing.y = 12.0;
    ui.label(ty::eyebrow("Palette").color(p.ink2));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for pal in JbPalette::ALL {
            let sel = theme::palette() == pal;
            if palette_swatch(ui, &p, pal, 18.0, sel).on_hover_text(pal.label()).clicked() {
                theme::set_palette(pal);
                changed = true;
            }
        }
    });
    ui.label(ty::eyebrow("Opacity").color(p.ink2));
    let mut sol = theme::solidity();
    ui.spacing_mut().slider_width = 8.0 * 24.0 + 7.0 * 8.0;
    if ui.add(egui::Slider::new(&mut sol, 0.35..=1.0).show_value(false)).changed() {
        theme::set_solidity(sol);
        changed = true;
    }
    if ui.ctx().input(|i| i.pointer.any_released()) && changed {
        theme::save_solidity(theme::solidity());
    }
    // Light / dark (follows the desktop until chosen here; kept from the previous sidebar toggle).
    ui.label(ty::eyebrow("Mode").color(p.ink2));
    ui.horizontal(|ui| {
        for (label, dark) in [("Light", false), ("Dark", true)] {
            let kind = if sh.dark == dark { BtnKind::Secondary } else { BtnKind::Ghost };
            if button_ex(ui, &p, kind, true, true, label).clicked() && sh.dark != dark {
                sh.dark = dark;
                theme::request_mode(dark);
                changed = true;
            }
        }
    });
    if changed {
        sh.palette = theme::Palette::for_state(sh.aero, sh.dark);
        theme::apply(&sh.ctx, sh.palette);
    }
}

// ---- PanelParts.swift / JBMenu.swift equivalents -------------------------------------------

/// `PanelLoading`: a centred 14 pt spinner filling the space.
pub fn panel_loading(ui: &mut Ui, p: &Palette) {
    let avail = ui.available_size();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(avail.x.max(20.0), avail.y.max(60.0)), Sense::hover());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_center_size(rect.center(), Vec2::splat(14.0))));
    crate::ascii::spinner(&mut child, 14.0, p.ink2);
}

/// `PanelFailure`: plug art + title + optional 11 pt detail.
pub fn panel_failure(ui: &mut Ui, p: &Palette, title: &str, detail: Option<&str>) {
    crate::ascii::empty_state(ui, p, crate::ascii::Art::Plug, title);
    if let Some(d) = detail.filter(|d| !d.is_empty()) {
        ui.vertical_centered(|ui| {
            ui.add(egui::Label::new(crate::theme::ty::sized(d, 11.0, crate::theme::W::Regular).color(p.ink3)).wrap());
        });
    }
}

/// `CopyCheckButton`: copy icon that turns into a green check for 1.2 s.
pub fn copy_check_button(ui: &mut Ui, p: &Palette, text: &str, size: f32) -> Response {
    let id = ui.id().with(("copy-check", text.len()));
    let done = copy_check(ui.ctx(), id, false);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if resp.clicked() {
        set_clipboard(ui.ctx(), text);
        copy_check(ui.ctx(), id, true);
    }
    let hov = resp.hovered();
    if hov {
        ui.painter().rect_filled(rect, Rounding::same(crate::theme::R_CTRL), p.hover);
    }
    let (name, col) = if done { ("checkmark", p.live) } else { ("doc.on.doc", if hov { p.ink } else { p.ink2 }) };
    crate::icons::paint_symbol(ui.painter(), rect.center(), name, size * 0.48 / 0.9 + 1.0, col);
    resp.on_hover_text("Copy").on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A dropdown menu (`JBMenu`) anchored to `trigger`: toggles on click, floating surface,
/// closes on click outside. Put `menu_item`s / `menu_separator`s inside `add`.
pub fn menu_popup(ui: &mut Ui, p: &Palette, trigger: &Response, add: impl FnOnce(&mut Ui)) {
    let id = trigger.id.with("jb-menu");
    if trigger.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    egui::popup::popup_below_widget(ui, id, trigger, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        floating_frame(p, crate::theme::R_CARD).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
            ui.set_min_width(180.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
    });
}

/// A `JBMenuRowStyle` row: optional leading icon (SF name), label, accent check when `checked`.
pub fn menu_item(ui: &mut Ui, p: &Palette, icon: Option<&str>, text: &str, checked: bool, enabled: bool) -> Response {
    let w = ui.available_width().max(160.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), if enabled { Sense::click() } else { Sense::hover() });
    let op = if enabled { 1.0 } else { 0.4 };
    if enabled && (resp.hovered() || resp.is_pointer_button_down_on()) {
        let f = if resp.is_pointer_button_down_on() { p.pressed } else { p.hover };
        ui.painter().rect_filled(rect, Rounding::same(crate::theme::R_CTRL - 2.0), f);
    }
    let mut x = rect.left() + 8.0;
    if checked || icon.is_some() {
        let c = egui::pos2(x + 8.0, rect.center().y);
        if checked {
            crate::icons::paint_symbol(ui.painter(), c, "checkmark", 11.0, p.accent);
        } else if let Some(i) = icon {
            crate::icons::paint_symbol(ui.painter(), c, i, 12.0, p.ink2.gamma_multiply(op));
        }
        x += 24.0;
    }
    ui.painter().text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, text, crate::theme::jb(13.0, crate::theme::W::Regular), p.ink.gamma_multiply(op));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn menu_separator(ui: &mut Ui, p: &Palette) {
    ui.add_space(3.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.line);
    ui.add_space(3.0);
}
