//! Editor chrome mirroring Mac `EditorChrome.swift`, `MarkdownOutline.swift` and the tab bar,
//! format bar, approval banner and footer of `EditorPane.swift`. Pure drawing: the caller owns
//! all state and applies the returned actions.
use crate::state::Shared;
use crate::theme::{self, with_alpha, W};
use crate::theme::Palette;
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Color32, Rect, Rounding, Sense, Stroke, Ui};

/// One markdown heading (`MDHeading`); `offset` is a char index into the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    pub level: usize,
    pub title: String,
    pub offset: usize,
}

/// `markdownOutline`: ATX headings outside code fences.
pub fn outline(text: &str) -> Vec<Heading> {
    let mut out = vec![];
    let mut in_fence = false;
    let mut offset = 0usize;
    for line in text.split('\n') {
        let start = offset;
        offset += line.chars().count() + 1;
        let t = line.trim();
        if t.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || !t.starts_with('#') {
            continue;
        }
        let level = t.chars().take_while(|c| *c == '#').count();
        if level > 6 || t.chars().count() <= level || t.chars().nth(level) != Some(' ') {
            continue;
        }
        let title: String = t.chars().skip(level + 1).collect::<String>().trim().to_string();
        out.push(Heading { level, title, offset: start });
    }
    out
}

/// `markdownStats`: "80 words · 1 min".
pub fn stats(text: &str) -> String {
    let words = text.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).count();
    let mins = ((words as f32 / 220.0).round() as usize).max(1);
    format!("{words} words · {mins} min")
}

/// Line count for non-markdown docs (Mac outline model).
pub fn line_stats(text: &str) -> String {
    format!("{} lines", text.bytes().filter(|b| *b == b'\n').count() + 1)
}

/// `prefixLines`: bullet or number every non-empty line of the lines touched by `a..b` (chars).
/// Returns the new text and the new selection (the whole replaced block).
pub fn prefix_lines(text: &str, a: usize, b: usize, numbered: bool) -> (String, (usize, usize)) {
    let chars: Vec<char> = text.chars().collect();
    let (a, b) = (a.min(chars.len()), b.min(chars.len()));
    let mut s = a;
    while s > 0 && chars[s - 1] != '\n' {
        s -= 1;
    }
    let mut e = b;
    // lineRange includes the trailing newline of the last touched line.
    while e < chars.len() && chars[e] != '\n' {
        e += 1;
    }
    if e < chars.len() {
        e += 1;
    }
    let block: String = chars[s..e].iter().collect();
    let mut n = 1;
    let out: Vec<String> = block
        .split('\n')
        .map(|l| {
            if l.is_empty() {
                l.to_string()
            } else if numbered {
                let r = format!("{n}. {l}");
                n += 1;
                r
            } else {
                n += 1;
                format!("- {l}")
            }
        })
        .collect();
    let replaced = out.join("\n");
    let len = replaced.chars().count();
    let mut new: String = chars[..s].iter().collect();
    new.push_str(&replaced);
    new.extend(chars[e..].iter());
    (new, (s, s + len))
}

/// Full-bleed hairline across the panel, past the shell's inner margin.
pub fn hairline(ui: &Ui, p: &Palette, y: f32, bleed: f32) {
    let r = ui.max_rect();
    ui.painter().hline(r.left() - bleed..=r.right() + bleed, y, Stroke::new(1.0, p.line));
}

#[derive(Debug, PartialEq)]
pub enum TabAct {
    Select(String),
    Close(String),
    PopOut,
    Hide,
    SetPreview(bool),
}

/// Mac `tabBar` + `EditorTab`: 36 pt row, edit/eye mode buttons, flat tabs with an accent
/// underline, then pop-out, copy and hide.
pub fn tab_bar(ui: &mut Ui, sh: &Shared, preview: bool, popped: bool) -> Vec<TabAct> {
    let p = sh.palette;
    let mut acts = vec![];
    let row = ui.available_rect_before_wrap();
    let h = 36.0;
    let rect = Rect::from_min_size(row.min, vec2(row.width(), h));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.spacing_mut().item_spacing = vec2(2.0, 0.0);
    let ui2 = &mut child;
    ui2.add_space(8.0);
    if widgets::icon_btn(ui2, &p, "square.and.pencil", 26.0, !preview).on_hover_text("Edit").clicked() {
        acts.push(TabAct::SetPreview(false));
    }
    if widgets::icon_btn(ui2, &p, "eye", 26.0, preview).on_hover_text("Preview").clicked() {
        acts.push(TabAct::SetPreview(true));
    }
    ui2.add_space(6.0);
    let right_w = 26.0 * 3.0 + 6.0 + 4.0;
    let tabs_w = (ui2.available_width() - right_w).max(40.0);
    let tabs_rect = Rect::from_min_size(ui2.cursor().min, vec2(tabs_w, h));
    let mut tabs_ui = ui2.new_child(egui::UiBuilder::new().max_rect(tabs_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    egui::ScrollArea::horizontal()
        .id_salt("editor-tabs")
        .max_width(tabs_w)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(&mut tabs_ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for d in &sh.open_docs {
                let active = sh.active_doc.as_deref() == Some(d.id.as_str());
                tab(ui, &p, &d.id, &d.title, d.dirty, d.ai_writing, active, &mut acts);
            }
        });
    // Right cluster pinned to the right edge.
    let rr = Rect::from_min_size(pos2(rect.right() - right_w, rect.top()), vec2(right_w, h));
    let mut ru = ui.new_child(egui::UiBuilder::new().max_rect(rr).layout(egui::Layout::left_to_right(egui::Align::Center)));
    ru.spacing_mut().item_spacing.x = 0.0;
    if !popped
        && widgets::icon_btn(&mut ru, &p, "macwindow.on.rectangle", 26.0, false).on_hover_text("Open in window").clicked()
    {
        acts.push(TabAct::PopOut);
    }
    if popped {
        ru.add_space(26.0);
    }
    if let Some(d) = sh.active_doc.as_ref().and_then(|a| sh.open_docs.iter().find(|d| &d.id == a)) {
        widgets::copy_check_button(&mut ru, &p, &d.content, 26.0).on_hover_text("Copy document");
    } else {
        ru.add_space(26.0);
    }
    let (name, tip) = if popped { ("xmark", "Close window") } else { ("sidebar.trailing", "Hide editor") };
    if widgets::icon_btn(&mut ru, &p, name, 26.0, false).on_hover_text(tip).clicked() {
        acts.push(TabAct::Hide);
    }
    ui.advance_cursor_after_rect(rect);
    hairline(ui, &p, rect.bottom(), 12.0);
    ui.add_space(1.0);
    acts
}

#[allow(clippy::too_many_arguments)]
fn tab(ui: &mut Ui, p: &Palette, id: &str, title: &str, dirty: bool, writing: bool, active: bool, acts: &mut Vec<TabAct>) {
    let font = theme::jb(12.0, if active { W::Medium } else { W::Regular });
    let mut job = egui::text::LayoutJob::simple_singleline(title.to_string(), font, p.ink);
    job.wrap = egui::text::TextWrapping { max_width: 150.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = ui.fonts(|f| f.layout_job(job));
    let spin = if writing { 16.0 } else { 0.0 };
    let w = 12.0 + spin + galley.size().x + 6.0 + 14.0 + 6.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 36.0), Sense::click());
    let hov = resp.hovered();
    let paint = ui.painter();
    if hov && !active {
        paint.rect_filled(rect, Rounding::ZERO, with_alpha(p.hover, 0.6));
    }
    let col = if active || hov { p.ink } else { p.ink2 };
    let tx = rect.left() + 12.0 + spin;
    paint.galley_with_override_text_color(pos2(tx, rect.center().y - galley.size().y / 2.0), galley.clone(), col);
    let slot = Rect::from_center_size(pos2(tx + galley.size().x + 6.0 + 7.0, rect.center().y), vec2(14.0, 14.0));
    let mut close_hit = false;
    if hov {
        let over = ui.input(|i| i.pointer.hover_pos()).is_some_and(|q| slot.expand(3.0).contains(q));
        crate::icons::paint_symbol(ui.painter(), slot.center(), "xmark", 8.0, if over { p.ink } else { p.ink2 });
        close_hit = over && resp.clicked();
    } else if dirty {
        ui.painter().circle_filled(slot.center(), 2.5, p.accent);
    }
    if active {
        ui.painter().rect_filled(Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), rect.max), Rounding::ZERO, p.accent);
    }
    if writing {
        let r = Rect::from_center_size(pos2(rect.left() + 12.0 + 5.0, rect.center().y), vec2(10.0, 10.0));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| crate::ascii::spinner(ui, 10.0, p.accent));
    }
    if close_hit {
        acts.push(TabAct::Close(id.to_string()));
    } else if resp.clicked() {
        acts.push(TabAct::Select(id.to_string()));
    } else if resp.middle_clicked() {
        acts.push(TabAct::Close(id.to_string()));
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fmt {
    Bold,
    Italic,
    Bullet,
    Number,
}

fn divider(ui: &mut Ui, p: &Palette) {
    ui.add_space(4.0);
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 14.0), Sense::hover());
    ui.painter().rect_filled(r, Rounding::ZERO, p.line);
    ui.add_space(4.0);
}

/// Mac markdown format bar: B I | bullets numbers, 24 pt buttons, hairline below.
pub fn format_bar(ui: &mut Ui, p: &Palette) -> Option<Fmt> {
    let mut out = None;
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(0.0);
        if widgets::icon_btn(ui, p, "bold", 24.0, false).on_hover_text("Bold (Ctrl+B)").clicked() {
            out = Some(Fmt::Bold);
        }
        if widgets::icon_btn(ui, p, "italic", 24.0, false).on_hover_text("Italic (Ctrl+I)").clicked() {
            out = Some(Fmt::Italic);
        }
        divider(ui, p);
        if widgets::icon_btn(ui, p, "list.bullet", 24.0, false).on_hover_text("Bullets").clicked() {
            out = Some(Fmt::Bullet);
        }
        if widgets::icon_btn(ui, p, "list.number", 24.0, false).on_hover_text("Numbers").clicked() {
            out = Some(Fmt::Number);
        }
    });
    ui.add_space(3.0);
    hairline(ui, p, ui.cursor().top(), 12.0);
    ui.add_space(1.0);
    out
}

/// Mac `footer`: mono 10.5 ink3, language · stats · suggested, then a check or spinner. 26 pt.
pub fn footer(ui: &mut Ui, p: &Palette, lang: &str, stats: &str, suggested: usize, dirty: bool) {
    hairline(ui, p, ui.cursor().top(), 12.0);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 26.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(12.0);
        let m = |s: &str| theme::ty::mono(s, 10.5, W::Regular).color(p.ink3);
        if !lang.is_empty() {
            ui.label(m(lang));
        }
        if !stats.is_empty() {
            if !lang.is_empty() {
                ui.label(m("·"));
            }
            ui.label(m(stats));
        }
        if suggested > 0 {
            ui.label(m("·"));
            ui.label(theme::ty::mono(format!("{suggested} suggested"), 10.5, W::Regular).color(p.accent));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(12.0);
            if dirty {
                crate::ascii::spinner(ui, 10.0, p.ink3);
            } else {
                crate::icons::symbol(ui, "checkmark", 10.0, p.ink3);
            }
        });
    });
}

/// `DocOutlineView`: 168 pt column, eyebrow header, indented rows, accent on the current one.
pub fn outline_panel(ui: &mut Ui, p: &Palette, heads: &[Heading], active: Option<usize>) -> Option<usize> {
    let mut picked = None;
    let r = ui.max_rect();
    ui.painter().rect_filled(r, Rounding::ZERO, with_alpha(p.raised, 0.5));
    ui.painter().vline(r.left(), r.y_range(), Stroke::new(1.0, p.line));
    ui.add_space(9.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(theme::ty::eyebrow("OUTLINE").size(10.5).color(p.ink3));
    });
    ui.add_space(8.0);
    hairline_in(ui, p);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(6.0);
        ui.spacing_mut().item_spacing.y = 1.0;
        for (i, h) in heads.iter().enumerate() {
            let sel = active == Some(i);
            let indent = (h.level.saturating_sub(1)) as f32 * 10.0 + 8.0;
            let col = if sel { p.accent } else if h.level <= 2 { p.ink2 } else { p.ink3 };
            let w = if h.level <= 2 { W::Medium } else { W::Regular };
            let title = if h.title.is_empty() { "—" } else { h.title.as_str() };
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 6.0, 24.0), Sense::click());
            let rect = rect.translate(vec2(6.0, 0.0));
            if sel || resp.hovered() {
                ui.painter().rect_filled(rect, Rounding::same(theme::R_CTRL), if sel { p.pressed } else { p.hover });
            }
            let mut job = egui::text::LayoutJob::simple_singleline(title.to_string(), theme::jb(12.0, w), col);
            job.wrap = egui::text::TextWrapping { max_width: rect.width() - indent - 8.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
            let g = ui.fonts(|f| f.layout_job(job));
            ui.painter().galley(pos2(rect.left() + indent, rect.center().y - g.size().y / 2.0), g, col);
            if resp.clicked() {
                picked = Some(i);
            }
            if sel && ui.is_rect_visible(rect) {
                // keep it in view
            }
        }
    });
    picked
}

fn hairline_in(ui: &mut Ui, p: &Palette) {
    let r = ui.max_rect();
    let y = ui.cursor().top();
    ui.painter().hline(r.x_range(), y, Stroke::new(1.0, p.line));
    ui.add_space(1.0);
}

/// `DocOutlineRail`: one tick per heading for narrow editors; the current one in accent.
pub fn outline_rail(ui: &mut Ui, p: &Palette, heads: &[Heading], active: Option<usize>) -> Option<usize> {
    let mut picked = None;
    ui.add_space(14.0);
    ui.spacing_mut().item_spacing.y = 5.0;
    for (i, h) in heads.iter().take(40).enumerate() {
        let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 6.0), Sense::click());
        let act = active == Some(i);
        let col = if act { p.accent } else { with_alpha(p.ink3, if h.level <= 2 { 0.9 } else { 0.5 }) };
        let w = (14.0 - (h.level as f32 - 1.0) * 3.0).max(4.0);
        ui.painter().rect_filled(Rect::from_min_max(pos2(rect.right() - w, rect.center().y - 1.0), pos2(rect.right(), rect.center().y + 1.0)), Rounding::same(1.0), col);
        if resp.on_hover_text(&h.title).clicked() {
            picked = Some(i);
        }
    }
    picked
}

/// Mac `approvalBanner`: accent-soft strip with a 2 pt accent edge. `Some(true)` = Apply.
pub fn approval_banner(ui: &mut Ui, p: &Palette, old: &str, pending: &str, show_diff: &mut bool) -> Option<bool> {
    let mut out = None;
    let delta = pending.bytes().filter(|b| *b == b'\n').count() as i64 - old.bytes().filter(|b| *b == b'\n').count() as i64;
    let summary = format!("{}{} lines", if delta >= 0 { "+" } else { "−" }, delta.abs());
    let top = ui.cursor().top();
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(12.0, 8.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let head = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    crate::icons::symbol(ui, "sparkles", 13.0, p.accent);
                    ui.label(theme::ty::sized("JoeBro edited this", 12.0, W::Medium).color(p.ink));
                    ui.label(theme::ty::sized(summary, 11.0, W::Regular).color(p.ink3));
                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                    crate::icons::paint_symbol(ui.painter(), r.center(), if *show_diff { "chevron.down" } else { "chevron.right" }, 9.0, p.ink3);
                })
                .response
                .interact(Sense::click())
                .on_hover_text(if *show_diff { "Hide changes" } else { "View changes" });
            if head.clicked() {
                *show_diff = !*show_diff;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::button_ex(ui, p, widgets::BtnKind::Primary, true, true, "Apply").clicked() {
                    out = Some(true);
                }
                if widgets::button_ex(ui, p, widgets::BtnKind::Secondary, true, true, "Reject").clicked() {
                    out = Some(false);
                }
            });
        });
        if *show_diff {
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(220.0).id_salt("diff").show(ui, |ui| diff_view(ui, p, old, pending));
        }
    });
    let r = inner.response.rect;
    ui.painter().set(slot, egui::Shape::rect_filled(r, Rounding::ZERO, p.accent_soft));
    ui.painter().rect_filled(Rect::from_min_max(pos2(r.left(), top), pos2(r.left() + 2.0, r.bottom())), Rounding::ZERO, p.accent);
    ui.painter().hline(r.x_range(), r.bottom(), Stroke::new(1.0, p.line));
    out
}

/// Line diff (LCS): removed lines in danger, added in live, mono 11.
fn diff_view(ui: &mut Ui, p: &Palette, old: &str, new: &str) {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let (n, m) = (a.len().min(2000), b.len().min(2000));
    let mut t = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i][j] = if a[i] == b[j] { t[i + 1][j + 1] + 1 } else { t[i + 1][j].max(t[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let line = |ui: &mut Ui, sign: &str, s: &str, c: Color32, bg: Color32| {
        egui::Frame::none().fill(bg).inner_margin(egui::Margin::symmetric(6.0, 1.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::ty::mono(format!("{sign} {s}"), 11.0, W::Regular).color(c));
        });
    };
    ui.spacing_mut().item_spacing.y = 0.0;
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            line(ui, " ", a[i], p.ink3, Color32::TRANSPARENT);
            i += 1;
            j += 1;
        } else if j < m && (i >= n || t[i][j + 1] >= t[i + 1][j]) {
            line(ui, "+", b[j], p.live, with_alpha(p.live, 0.10));
            j += 1;
        } else {
            line(ui, "−", a[i], p.danger, with_alpha(p.danger, 0.10));
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outline_skips_fences_and_reports_char_offsets() {
        let h = outline("# A\n```\n# no\n```\n## é B\n#x\n");
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].level, h[0].offset), (1, 0));
        assert_eq!((h[1].title.as_str(), h[1].offset), ("é B", 17));
    }
    #[test]
    fn stats_and_prefix() {
        assert_eq!(stats("one two three"), "3 words · 1 min");
        let (t, sel) = prefix_lines("a\nb\n\nc", 0, 3, true);
        assert_eq!(t, "1. a\n2. b\n\nc");
        assert_eq!(sel, (0, 10));
        assert_eq!(prefix_lines("x", 0, 0, false).0, "- x");
    }
}
