//! Phone memory (Muse files): plain Markdown memory files the phone agent reads
//! (Mac `PhoneMemoryView`, embedded content with no panel chrome). Autosaves 800 ms after typing.
#![allow(dead_code)] // the Mac defines this view but no panel hosts it either
use crate::api::R;
use crate::ascii::{self, Art};
use crate::state::{Job, Shared};
use crate::theme::{self, ty, W, R_PANEL};
use crate::widgets::{self, BtnKind};
use eframe::egui::{self, Align, Layout, Rect, Ui, Vec2};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct PhoneMemoryPanel {
    files: Vec<String>,
    started: bool,
    selected: Option<String>,
    text: String,
    loaded_text: String,
    error: Option<String>,
    new_name: Option<String>,
    dirty_at: Option<Instant>,
    list: Option<Job<R<Vec<String>>>>,
    open: Option<(String, Job<R<String>>)>,
    saves: Vec<Job<R<serde_json::Value>>>,
    pending_select: Option<String>,
}

impl PhoneMemoryPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        let p = sh.palette;
        if !self.started {
            self.started = true;
            self.reload(sh);
        }
        self.poll(sh);
        let rect = ui.max_rect();
        let left = Rect::from_min_size(rect.min, Vec2::new(190.0, rect.height()));
        let right = Rect::from_min_max(egui::pos2(left.right() + 12.0, rect.top()), rect.max);
        ui.painter().extend(widgets::glass_surface(&p, left, R_PANEL, p.panel_fill, false));
        ui.painter().extend(widgets::glass_surface(&p, right, R_PANEL, p.panel_fill, false));
        let mut pick: Option<String> = None;
        let (mut add, mut del) = (false, false);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(left), |ui| {
            let list_rect = Rect::from_min_max(left.min, egui::pos2(left.right(), left.bottom() - 38.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(list_rect.shrink(6.0)), |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for f in &self.files {
                        let sel = self.selected.as_deref() == Some(f.as_str());
                        let r = widgets::hover_row(ui, &p, sel, 8.0, |ui| {
                            ui.add_space(2.0);
                            ui.add(egui::Label::new(ty::sized(f.as_str(), 12.0, W::Regular).color(if sel { p.ink } else { p.ink2 })).truncate());
                            ui.add_space(2.0);
                        });
                        if r.clicked() {
                            pick = Some(f.clone());
                        }
                    }
                });
            });
            let bar = Rect::from_min_max(egui::pos2(left.left() + 6.0, left.bottom() - 32.0), egui::pos2(left.right() - 6.0, left.bottom() - 6.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar).layout(Layout::left_to_right(Align::Center)), |ui| {
                add = widgets::icon_btn(ui, &p, "plus", 26.0, false).on_hover_text("New file").clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_enabled_ui(self.selected.is_some() && self.selected.as_deref() != Some("about-me"), |ui| {
                        del = widgets::icon_btn(ui, &p, "trash", 26.0, false).on_hover_text("Delete file").clicked();
                    });
                });
            });
        });
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right), |ui| {
            if self.selected.is_none() {
                ascii::empty_state(ui, &p, Art::Files, "Pick a file");
                return;
            }
            let h = if self.error.is_some() { right.height() - 28.0 } else { right.height() };
            let body = Rect::from_min_size(right.min, Vec2::new(right.width(), h)).shrink(8.0);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body), |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut self.text)
                            .frame(false)
                            .font(theme::jb(12.5, W::Regular))
                            .text_color(p.ink)
                            .desired_width(f32::INFINITY)
                            .desired_rows(30),
                    );
                    if r.changed() && self.text != self.loaded_text {
                        self.dirty_at = Some(Instant::now());
                    }
                });
            });
            if let Some(e) = &self.error {
                ui.painter().text(
                    egui::pos2(right.left() + 10.0, right.bottom() - 14.0),
                    egui::Align2::LEFT_CENTER,
                    e,
                    theme::jb(11.0, W::Regular),
                    p.danger,
                );
            }
        });
        ui.advance_cursor_after_rect(rect);

        if let Some(n) = pick {
            if self.selected.as_deref() != Some(n.as_str()) {
                self.save_now(sh);
                self.select(sh, n);
            }
        }
        if add {
            self.new_name = Some(String::new());
        }
        if del {
            if let Some(s) = self.selected.take() {
                self.text.clear();
                self.loaded_text.clear();
                self.dirty_at = None;
                self.saves.push(sh.spawn(move |a| a.muse_delete(&s)));
            }
        }
        if let Some(at) = self.dirty_at {
            if at.elapsed() >= Duration::from_millis(800) {
                self.save_now(sh);
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
        }
        self.name_sheet(ui.ctx(), sh);
    }

    fn reload(&mut self, sh: &mut Shared) {
        if self.list.is_none() {
            self.list = Some(sh.spawn(|a| a.muse_files()));
        }
    }

    fn select(&mut self, sh: &mut Shared, name: String) {
        self.selected = Some(name.clone());
        self.text.clear();
        self.loaded_text.clear();
        let n = name.clone();
        self.open = Some((name, sh.spawn(move |a| a.muse_read(&n))));
    }

    fn save_now(&mut self, sh: &mut Shared) {
        self.dirty_at = None;
        if let Some(s) = self.selected.clone() {
            if self.text != self.loaded_text && self.open.is_none() {
                let t = self.text.clone();
                self.loaded_text = t.clone();
                self.saves.push(sh.spawn(move |a| a.muse_write(&s, &t)));
            }
        }
    }

    fn poll(&mut self, sh: &mut Shared) {
        if let Some(j) = &mut self.list {
            if let Some(r) = j.take() {
                self.list = None;
                match r {
                    Ok(f) => {
                        self.files = f;
                        if let Some(n) = self.pending_select.take() {
                            if let Some(m) = self.files.iter().find(|x| **x == n || **x == format!("{n}.md")).cloned() {
                                self.select(sh, m);
                            }
                        } else if self.selected.is_none() {
                            let first = self.files.iter().find(|f| *f == "about-me").or(self.files.first()).cloned();
                            if let Some(f) = first {
                                self.select(sh, f);
                            }
                        }
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
        if let Some((_, j)) = &mut self.open {
            if let Some(r) = j.take() {
                let c = r.unwrap_or_default();
                self.loaded_text = c.clone();
                self.text = c;
                self.open = None;
            }
        }
        let mut done = false;
        let mut err = None;
        self.saves.retain_mut(|j| match j.take() {
            Some(r) => {
                done = true;
                if let Err(e) = r {
                    err = Some(e);
                }
                false
            }
            None => true,
        });
        if err.is_some() {
            self.error = err;
        } else if done {
            self.error = None;
            self.reload(sh);
        }
    }

    fn name_sheet(&mut self, ctx: &egui::Context, sh: &mut Shared) {
        let p = sh.palette;
        let Some(name) = &mut self.new_name else { return };
        let (create, cancel) = (std::cell::Cell::new(false), std::cell::Cell::new(false));
        let esc = widgets::dialog(
            ctx,
            "muse_new",
            "New memory file",
            None,
            360.0,
            |ui| {
                if widgets::button_ex(ui, &p, BtnKind::Primary, true, true, "Create").clicked() {
                    create.set(true);
                }
                if widgets::button_ex(ui, &p, BtnKind::Ghost, true, true, "Cancel").clicked() {
                    cancel.set(true);
                }
            },
            |ui| {
                let r = ui.add(egui::TextEdit::singleline(name).hint_text("Name").desired_width(f32::INFINITY));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    create.set(true);
                }
            },
        );
        if create.get() {
            let n = name.trim().to_string();
            self.new_name = None;
            if !n.is_empty() {
                self.pending_select = Some(n.clone());
                self.saves.push(sh.spawn(move |a| a.muse_create(&n)));
            }
        } else if cancel.get() || esc {
            self.new_name = None;
        }
    }
}
