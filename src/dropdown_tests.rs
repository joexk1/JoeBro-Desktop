//! Headless interaction tests for every kind of dropdown the app uses: a fake
//! pointer clicks through real egui frames (with the app theme applied) and
//! checks the popup opens, the pick is written back, and it closes.
use crate::state::{Model, Shared};
use crate::widgets;
use eframe::egui::{self, Event, PointerButton, Pos2, Rect};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

struct H {
    ctx: egui::Context,
    sh: Shared,
    t: f64,
}

type Frame<'a> = dyn FnMut(&egui::Context, &mut Shared) + 'a;

impl H {
    fn new() -> H {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let api = Arc::new(crate::api::Api::new("http://127.0.0.1:9"));
        let mut sh = Shared::new(api, ctx.clone(), true, true);
        crate::theme::apply(&ctx, sh.palette);
        sh.models = vec![model("e1", "alpha"), model("e1", "beta"), model("e2", "gamma")];
        H { ctx, sh, t: 0.0 }
    }

    fn run(&mut self, events: Vec<Event>, f: &mut Frame) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 800.0))),
            time: Some(self.t),
            events,
            ..Default::default()
        };
        self.t += 0.05;
        let sh = &mut self.sh;
        let _ = self.ctx.run(input, |ctx| f(ctx, sh));
    }

    fn idle(&mut self, n: usize, f: &mut Frame) {
        for _ in 0..n {
            self.run(vec![], f);
        }
    }

    fn click(&mut self, pos: Pos2, f: &mut Frame) {
        let ev = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        self.run(vec![Event::PointerMoved(pos)], f);
        self.run(vec![ev(true)], f);
        self.run(vec![ev(false)], f);
        self.idle(2, f);
    }

    fn popup_open(&self, id: egui::Id) -> bool {
        self.ctx.memory(|m| m.is_popup_open(id))
    }

    fn any_popup(&self) -> bool {
        self.ctx.memory(|m| m.any_popup_open())
    }

    /// The topmost Foreground area (an open popup / menu), if any.
    fn foreground_area(&self) -> Option<Rect> {
        self.ctx.memory(|m| {
            m.layer_ids()
                .filter(|l| l.order == egui::Order::Foreground)
                .filter_map(|l| m.area_rect(l.id))
                .filter(|r| r.width() > 20.0 && r.height() > 20.0)
                .last()
        })
    }
}

fn model(ep: &str, name: &str) -> Model {
    Model {
        id: format!("{ep}:{name}"),
        model_id: name.into(),
        display: name.into(),
        tag: String::new(),
        endpoint_id: ep.into(),
        endpoint_name: ep.into(),
        endpoint_url: String::new(),
        category: "cloud".into(),
    }
}

/// Click down a popup's rows until `done()` (or the popup's bottom).
fn click_rows(h: &mut H, area: Rect, f: &mut Frame, done: &dyn Fn() -> bool) {
    let mut y = area.top() + 14.0;
    while !done() && y < area.bottom() - 4.0 {
        h.click(egui::pos2(area.left() + 40.0, y), f);
        y += 9.0;
    }
}

#[test]
fn model_picker_opens_selects_and_closes() {
    let mut h = H::new();
    let picked = Rc::new(RefCell::new(None::<Model>));
    let btn = Rc::new(Cell::new(Rect::NOTHING));
    let (p2, b2) = (picked.clone(), btn.clone());
    let mut f = move |ctx: &egui::Context, sh: &mut Shared| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(40.0);
            ui.horizontal(|ui| {
                let mut cur = p2.borrow().clone();
                widgets::model_picker(ui, "test_picker", sh, &mut cur);
                *p2.borrow_mut() = cur;
                b2.set(ui.min_rect());
            });
        });
    };
    h.idle(3, &mut f);
    let id = egui::Id::new(("model_picker", "test_picker"));
    let r = btn.get();
    h.click(egui::pos2(r.left() + 30.0, r.center().y), &mut f);
    assert!(h.popup_open(id), "picker popup should open on click");
    let area = h.foreground_area().expect("popup area");
    let pk = picked.clone();
    click_rows(&mut h, area, &mut f, &move || pk.borrow().is_some());
    assert!(picked.borrow().is_some(), "clicking a row writes the selection back");
    assert!(!h.popup_open(id), "popup closes after picking");
    // Reopen + click outside closes without changing the pick.
    let before = picked.borrow().clone().unwrap().id;
    h.click(egui::pos2(r.left() + 30.0, r.center().y), &mut f);
    assert!(h.popup_open(id));
    h.click(egui::pos2(1100.0, 700.0), &mut f);
    assert!(!h.popup_open(id), "click outside dismisses");
    assert_eq!(picked.borrow().clone().unwrap().id, before);
}

/// The ComboBoxes in modal-ish windows (task form, tool/plugin form,
/// Telegram bot settings) — a Window, optionally inside a ScrollArea.
fn combo_in_window(scroll: bool, vscroll: bool) {
    let mut h = H::new();
    let value = Rc::new(RefCell::new("daily".to_string()));
    let btn = Rc::new(Cell::new(Rect::NOTHING));
    let (v2, b2) = (value.clone(), btn.clone());
    let mut f = move |ctx: &egui::Context, _sh: &mut Shared| {
        egui::CentralPanel::default().show(ctx, |_ui| {});
        egui::Window::new("Form")
            .id(egui::Id::new("form_win"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .vscroll(vscroll)
            .show(ctx, |ui| {
                let body = |ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        ui.label("Repeat");
                        let mut v = v2.borrow().clone();
                        let r = egui::ComboBox::from_id_salt("task_schedule")
                            .selected_text(v.clone())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut v, "daily".into(), "Daily");
                                ui.selectable_value(&mut v, "weekly".into(), "Weekly");
                                ui.selectable_value(&mut v, "monthly".into(), "Monthly");
                            });
                        b2.set(r.response.rect);
                        *v2.borrow_mut() = v;
                    });
                };
                if scroll {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(300.0).show(ui, body);
                } else {
                    body(ui);
                }
            });
    };
    h.idle(4, &mut f);
    let r = btn.get();
    h.click(r.center(), &mut f);
    assert!(h.any_popup(), "combo popup opens");
    let area = h.foreground_area().expect("combo popup area");
    // Last row ("Monthly") sits at the bottom of the popup.
    h.click(egui::pos2(area.left() + 40.0, area.bottom() - 16.0), &mut f);
    assert_ne!(*value.borrow(), "daily", "selection written back");
    assert!(!h.any_popup(), "combo closes after selecting");
}

#[test]
fn combo_in_window_selects() {
    combo_in_window(false, false);
}

#[test]
fn combo_in_vscroll_window_selects() {
    combo_in_window(false, true);
}

#[test]
fn combo_in_scrolling_window_selects() {
    combo_in_window(true, false);
}

#[test]
fn used_chip_popover_toggles() {
    let mut h = H::new();
    let btn = Rc::new(Cell::new(Rect::NOTHING));
    let b2 = btn.clone();
    let items = vec!["Prefers British spelling".to_string(), "Studies at King's".to_string()];
    let mut f = move |ctx: &egui::Context, sh: &mut Shared| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(300.0);
            ui.horizontal_wrapped(|ui| {
                let id = egui::Id::new(("used_chip", "m1", "memories"));
                let r = widgets::used_chip(ui, sh, id, crate::theme::icon::BRAIN, ("memory", "memories"), &items);
                b2.set(r.rect);
                // A second chip right next to it must not share its popover.
                let id2 = egui::Id::new(("used_chip", "m1", "skills"));
                widgets::used_chip(ui, sh, id2, "\u{f19d}", ("skill", "skills"), &items[..1]);
            });
        });
    };
    h.idle(3, &mut f);
    let pid = egui::Id::new(("used_chip", "m1", "memories")).with("used_popover");
    let pid2 = egui::Id::new(("used_chip", "m1", "skills")).with("used_popover");
    let r = btn.get();
    h.click(r.center(), &mut f);
    assert!(h.popup_open(pid), "chip opens its popover");
    assert!(!h.popup_open(pid2));
    // Clicking inside the popover keeps it open (text is selectable).
    let area = h.foreground_area().expect("popover");
    h.click(egui::pos2(area.left() + 20.0, area.bottom() - 10.0), &mut f);
    assert!(h.popup_open(pid), "click inside keeps it open");
    h.click(r.center(), &mut f);
    assert!(!h.popup_open(pid), "chip click closes it again");
    h.click(r.center(), &mut f);
    h.click(egui::pos2(1100.0, 50.0), &mut f);
    assert!(!h.popup_open(pid), "click outside closes it");
}

#[test]
fn segmented_picks_sides() {
    let mut h = H::new();
    let on = Rc::new(Cell::new(true));
    let rect = Rc::new(Cell::new(Rect::NOTHING));
    let changed = Rc::new(Cell::new(0));
    let (o2, r2, c2) = (on.clone(), rect.clone(), changed.clone());
    let mut f = move |ctx: &egui::Context, sh: &mut Shared| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut v = o2.get();
            let r = widgets::segmented(ui, sh, &mut v, ("Fast", "Think"));
            if r.changed() {
                c2.set(c2.get() + 1);
            }
            r2.set(r.rect);
            o2.set(v);
        });
    };
    h.idle(2, &mut f);
    let r = rect.get();
    h.click(egui::pos2(r.left() + 14.0, r.center().y), &mut f); // "Fast"
    assert!(!on.get());
    h.click(egui::pos2(r.left() + 14.0, r.center().y), &mut f); // again: no change
    assert!(!on.get());
    h.click(egui::pos2(r.right() - 10.0, r.center().y), &mut f); // "Think"
    assert!(on.get());
    h.click(egui::pos2(r.right() - 10.0, r.center().y), &mut f); // again: no change
    assert!(on.get());
    assert_eq!(changed.get(), 2);
}

#[test]
fn menu_button_opens_and_item_fires() {
    let mut h = H::new();
    let btn = Rc::new(Cell::new(Rect::NOTHING));
    let fired = Rc::new(Cell::new(false));
    let (b2, f2) = (btn.clone(), fired.clone());
    let mut f = move |ctx: &egui::Context, _sh: &mut Shared| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                let r = egui::menu::menu_custom_button(ui, egui::Button::new("Essay outline ▾").frame(false), |ui| {
                    ui.set_min_width(210.0);
                    if ui.add(egui::Button::new("Rename").frame(false).min_size(egui::vec2(200.0, 26.0))).clicked() {
                        f2.set(true);
                        ui.close_menu();
                    }
                });
                b2.set(r.response.rect);
            });
        });
    };
    h.idle(2, &mut f);
    let r = btn.get();
    h.click(r.center(), &mut f);
    let area = h.foreground_area().expect("menu opens");
    h.click(egui::pos2(area.left() + 40.0, area.top() + 20.0), &mut f);
    assert!(fired.get(), "menu item click fires");
}
