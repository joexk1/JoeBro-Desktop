//! Floating Ctrl+K command palette (`CommandPalette.swift`): recent chats, panels and actions
//! with subsequence fuzzy filtering. Opened via `sh.palette_open`; returns the chosen `Action`.
use crate::state::{Shared, Tab};
use crate::theme::{self, ty, W};
use crate::widgets;
use eframe::egui::{self, Align2, Context, Key, Modifiers, Rect, Sense, Vec2};

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Chat(String),
    Panel(Tab),
    NewChat,
    Settings,
    ToggleSidebar,
}

#[derive(Clone, PartialEq)]
enum Kind {
    Chat,
    Panel,
    Action,
}

struct Item {
    title: String,
    icon: &'static str,
    kbd: Option<String>,
    kind: Kind,
    action: Action,
}

#[derive(Default)]
pub struct CommandPalette {
    query: String,
    selected: usize,
    was_open: bool,
    scroll_to_selected: bool,
}

/// Subsequence match: higher is better; None when the query isn't contained.
pub fn score(q: &str, text: &str) -> Option<i64> {
    let q: Vec<char> = q.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut qi, mut score, mut prev) = (0usize, 0i64, -2i64);
    for (i, ch) in t.iter().enumerate() {
        if qi < q.len() && *ch == q[qi] {
            score += 1;
            if i as i64 == prev + 1 {
                score += 3;
            }
            if i == 0 || t[i - 1] == ' ' {
                score += 4;
            }
            prev = i as i64;
            qi += 1;
        }
    }
    (qi == q.len()).then_some(score * 10 - t.len() as i64)
}

impl CommandPalette {
    fn items(&self, sh: &Shared) -> Vec<Item> {
        let mut chats: Vec<Item> = sh
            .sessions
            .iter()
            .take(40)
            .map(|s| Item { title: s.name.clone(), icon: "bubble.left", kbd: None, kind: Kind::Chat, action: Action::Chat(s.id.clone()) })
            .collect();
        if self.query.is_empty() {
            chats.truncate(6);
        }
        let panels: Vec<Item> = Tab::palette_tabs()
            .into_iter()
            .map(|t| Item { title: t.label().into(), icon: t.icon(), kbd: t.shortcut(), kind: Kind::Panel, action: Action::Panel(t) })
            .collect();
        let actions = vec![
            Item { title: "New chat".into(), icon: "plus", kbd: Some(widgets::shortcut_label("N")), kind: Kind::Action, action: Action::NewChat },
            Item { title: "Settings".into(), icon: "gearshape", kbd: Some(widgets::shortcut_label(",")), kind: Kind::Action, action: Action::Settings },
            Item { title: "Toggle sidebar".into(), icon: "sidebar.left", kbd: Some(widgets::shortcut_label("\\")), kind: Kind::Action, action: Action::ToggleSidebar },
        ];
        if self.query.is_empty() {
            return chats.into_iter().chain(actions).chain(panels).collect();
        }
        let mut all: Vec<(Item, i64)> = chats
            .into_iter()
            .chain(panels)
            .chain(actions)
            .filter_map(|i| score(&self.query, &i.title).map(|s| (i, s)))
            .collect();
        all.sort_by(|a, b| b.1.cmp(&a.1));
        all.into_iter().map(|(i, _)| i).collect()
    }

    /// Draw the palette if `sh.palette_open`. Returns the action the user picked (palette closes).
    pub fn ui(&mut self, ctx: &Context, sh: &mut Shared) -> Option<Action> {
        if !sh.palette_open {
            self.was_open = false;
            return None;
        }
        let first = !self.was_open;
        if first {
            self.query.clear();
            self.selected = 0;
            self.was_open = true;
        }
        let p = sh.palette;
        let items = self.items(sh);
        // Keys first so the text field doesn't swallow them.
        let (down, up, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if !items.is_empty() {
            if down {
                self.selected = (self.selected + 1) % items.len();
                self.scroll_to_selected = true;
            }
            if up {
                self.selected = (self.selected + items.len() - 1) % items.len();
                self.scroll_to_selected = true;
            }
        }
        let mut picked: Option<Action> = None;
        if esc {
            sh.palette_open = false;
            return None;
        }
        if enter {
            picked = items.get(self.selected).map(|i| i.action.clone());
        }

        // Scrim (click to dismiss), fading in.
        let screen = ctx.screen_rect();
        let k = theme::anim(ctx, egui::Id::new("jb-palette-in"), true, theme::SNAP);
        egui::Area::new(egui::Id::new("jb-palette-scrim")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_black_alpha((0.22 * 255.0 * k) as u8));
            if resp.clicked() {
                sh.palette_open = false;
            }
        });
        let q_before = self.query.clone();
        egui::Area::new(egui::Id::new("jb-palette"))
            .order(egui::Order::Tooltip)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 96.0 - (1.0 - k) * 6.0))
            .show(ctx, |ui| {
                ui.multiply_opacity(k);
                widgets::floating_frame(&p, theme::R_CARD + 4.0).show(ui, |ui| {
                    ui.set_width(560.0);
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    // Search row (48 pt).
                    let (row, _) = ui.allocate_exact_size(Vec2::new(560.0, 48.0), Sense::hover());
                    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(row.shrink2(Vec2::new(14.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    inner.spacing_mut().item_spacing.x = 8.0;
                    crate::icons::symbol(&mut inner, "magnifyingglass", 14.0, p.ink3);
                    let te = inner.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .id(egui::Id::new("jb-palette-input"))
                            .frame(false)
                            .hint_text(egui::RichText::new("Jump to a panel or chat").color(p.ink3))
                            .font(theme::jb(15.0, W::Regular))
                            .desired_width(560.0 - 28.0 - 22.0 - 44.0),
                    );
                    if first {
                        te.request_focus();
                    }
                    widgets::kbd(&mut inner, &p, "esc");
                    ui.painter().hline(row.x_range(), row.bottom(), egui::Stroke::new(1.0, p.line));
                    ui.add_space(1.0);
                    // Results.
                    egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.add_space(6.0);
                        if items.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(20.0);
                                ui.label(ty::body("No matches").color(p.ink3));
                                ui.add_space(20.0);
                            });
                        }
                        for (idx, item) in items.iter().enumerate() {
                            let (rect, resp) = ui.allocate_exact_size(Vec2::new(560.0, 32.0), Sense::click());
                            let r = Rect::from_min_max(rect.min + Vec2::new(6.0, 0.0), rect.max - Vec2::new(6.0, 0.0));
                            let sel = idx == self.selected;
                            let hov = if resp.hovered() { 1.0 } else { 0.0 };
                            let shapes = widgets::hover_row_shapes(&p, r, theme::R_CTRL, sel, hov);
                            ui.painter().extend(shapes);
                            crate::icons::paint_symbol(ui.painter(), egui::pos2(r.left() + 20.0, r.center().y), item.icon, 14.0, if sel { p.ink } else { p.ink2 });
                            ui.painter().text(egui::pos2(r.left() + 40.0, r.center().y), Align2::LEFT_CENTER, &item.title, theme::jb(13.0, W::Regular), p.ink);
                            let right = r.right() - 10.0;
                            if let Some(kb) = &item.kbd {
                                widgets::paint_kbd(ui.painter(), &p, right, r.center().y, kb);
                            } else if item.kind == Kind::Chat {
                                ui.painter().text(egui::pos2(right, r.center().y), Align2::RIGHT_CENTER, "chat", theme::jb(10.5, W::Regular), p.ink3);
                            }
                            if sel && self.scroll_to_selected {
                                ui.scroll_to_rect(rect, None);
                            }
                            if resp.clicked() {
                                picked = Some(item.action.clone());
                            }
                        }
                        ui.add_space(6.0);
                    });
                    self.scroll_to_selected = false;
                });
            });
        if self.query != q_before {
            self.selected = 0;
        }
        if picked.is_some() {
            sh.palette_open = false;
        }
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::score;
    #[test]
    fn fuzzy_prefers_word_starts_and_runs() {
        assert!(score("ns", "Night Shift").is_some());
        assert!(score("zz", "Night Shift").is_none());
        assert!(score("chat", "Chats").unwrap() > score("chat", "Check calendar for hats").unwrap_or(i64::MIN));
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    use crate::state::SessionRef;

    #[test]
    fn palette_draws_and_filters_without_panicking() {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let mut sh = Shared::new(std::sync::Arc::new(crate::api::Api::new("http://127.0.0.1:1")), ctx.clone(), true, true);
        sh.sessions = vec![SessionRef { id: "a".into(), name: "Plan the launch".into() }];
        sh.palette_open = true;
        let mut pal = CommandPalette::default();
        for q in ["", "nig", "zzzz"] {
            pal.query = q.into();
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0))), ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                let _ = pal.ui(ctx, &mut sh);
            });
            sh.palette_open = true;
        }
        // "nig" finds Night Shift among the panels.
        pal.query = "nig".into();
        assert!(pal.items(&sh).iter().any(|i| i.title == "Night Shift"));
    }
}
