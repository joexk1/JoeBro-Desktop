//! AI Check (Mac AICheckPanel): paste text, get an AI-likelihood gauge plus the
//! sentences that tripped the detector. Input card | 300 pt result card.
use crate::api::R;
use crate::state::{Job, Shared};
use crate::panels::workkit as wk;
use crate::theme::{self, ty, W};
use crate::widgets::{self, BtnKind};
use eframe::egui::{self, Align, Layout, Pos2, Rect, RichText, Stroke, Ui, Vec2};
use serde_json::Value;

#[derive(Default)]
pub struct AICheckPanel {
    text: String,
    job: Option<Job<R<Value>>>,
    score: Option<f64>,
    total_words: Option<i64>,
    ai_words: Option<i64>,
    flagged: Vec<String>,
    checked: bool,
    error: Option<String>,
}

fn word_count(t: &str) -> usize {
    t.split(' ').filter(|w| !w.is_empty()).count()
}

impl AICheckPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        if let Some(job) = &mut self.job {
            if let Some(res) = job.take() {
                match res {
                    Ok(v) => {
                        self.score = v
                            .get("ai_percent")
                            .or_else(|| v.get("ai_percentage"))
                            .or_else(|| v.get("score"))
                            .or_else(|| v.get("ai_probability"))
                            .and_then(|x| x.as_f64());
                        self.total_words = v.get("total_words").and_then(Value::as_i64);
                        self.ai_words = v.get("ai_words").and_then(Value::as_i64);
                        self.flagged = v
                            .get("flagged_sentences")
                            .or_else(|| v.get("flagged"))
                            .or_else(|| v.get("sentences"))
                            .and_then(|x| x.as_array())
                            .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        self.checked = true;
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
                self.job = None;
            }
        }
        let p = sh.palette;
        let checking = self.job.is_some();
        let has_state = !self.text.is_empty() || self.checked || self.error.is_some();
        let mut clear = false;
        let mut go = false;
        let rect = ui.max_rect();
        widgets::panel_chrome(
            ui,
            &p,
            "AI Check",
            "checkmark.shield",
            |ui| {
                widgets::chip_tag(ui, &p, "ZeroGPT", None);
                ui.add_space(10.0);
                if widgets::button_ex(ui, &p, BtnKind::Ghost, true, has_state, "Clear").clicked() {
                    clear = true;
                }
            },
            |ui| {
                let avail = Rect::from_min_max(ui.cursor().min, rect.max);
                ui.allocate_rect(avail, egui::Sense::hover());
                let narrow = avail.width() < 640.0;
                let right_w = if narrow { avail.width() } else { 300.0 };
                let (left, right) = if narrow {
                    let h = (avail.height() - 12.0) / 2.0;
                    (
                        Rect::from_min_size(avail.min, Vec2::new(avail.width(), h)),
                        Rect::from_min_size(Pos2::new(avail.left(), avail.top() + h + 12.0), Vec2::new(avail.width(), h)),
                    )
                } else {
                    (
                        Rect::from_min_max(avail.min, Pos2::new(avail.right() - right_w - 12.0, avail.bottom())),
                        Rect::from_min_max(Pos2::new(avail.right() - right_w, avail.top()), avail.max),
                    )
                };
                wk::box_in_rect(ui, &p, left, 0.0, |ui| {
                    let words = word_count(&self.text);
                    let footer_h = 46.0;
                    let edit_h = (ui.available_height() - footer_h).max(60.0);
                    egui::ScrollArea::vertical().id_salt("aicheck_in").max_height(edit_h).auto_shrink([false, false]).show(ui, |ui| {
                        egui::Frame::none().inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.text)
                                    .frame(false)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(((edit_h - 24.0) / 18.0).max(3.0) as usize)
                                    .font(theme::jb(13.0, W::Regular))
                                    .hint_text(RichText::new("Paste text, 20 words or more").color(p.ink3)),
                            );
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(ty::sized(format!("{words} words"), 11.0, W::Regular).color(p.ink3));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(12.0);
                            if checking {
                                crate::ascii::spinner(ui, 12.0, p.ink2);
                            } else if widgets::button_ex(ui, &p, BtnKind::Primary, false, words >= 20, "Check").clicked() && words >= 20 {
                                go = true;
                            }
                        });
                    });
                });
                wk::box_in_rect(ui, &p, right, 0.0, |ui| {
                    ui.add_space(14.0);
                    if let Some(e) = self.error.clone() {
                        widgets::panel_failure(ui, &p, "Check failed", Some(&e));
                    } else if let Some(score) = self.score.filter(|_| self.checked) {
                        ui.vertical_centered(|ui| {
                            gauge(ui, sh, score, self.ai_words, self.total_words);
                        });
                        ui.add_space(14.0);
                        if self.flagged.is_empty() {
                            ui.vertical_centered(|ui| ui.label(ty::sized("No sentences flagged", 11.0, W::Regular).color(p.ink3)));
                        } else {
                            egui::ScrollArea::vertical().id_salt("aicheck_flags").auto_shrink([false, false]).show(ui, |ui| {
                                egui::Frame::none().inner_margin(egui::Margin::symmetric(14.0, 0.0)).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.spacing_mut().item_spacing.y = 6.0;
                                    widgets::section_header(ui, &p, "Flagged", Some(self.flagged.len()), |_| {});
                                    for f in &self.flagged {
                                        egui::Frame::none()
                                            .fill(p.danger.gamma_multiply(0.08))
                                            .stroke(Stroke::new(1.0, p.danger.gamma_multiply(0.2)))
                                            .rounding(egui::Rounding::same(theme::R_CTRL))
                                            .inner_margin(egui::Margin::same(8.0))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.add(egui::Label::new(ty::sized(f, 12.0, W::Regular).color(p.ink)).wrap());
                                            });
                                    }
                                });
                            });
                        }
                    } else {
                        crate::ascii::empty_state(ui, &p, crate::ascii::Art::Search, "No result yet");
                    }
                });
            },
        );
        if clear {
            self.text.clear();
            self.score = None;
            self.total_words = None;
            self.ai_words = None;
            self.flagged.clear();
            self.checked = false;
            self.error = None;
        }
        if go {
            let t = self.text.clone();
            self.score = None;
            self.checked = false;
            self.flagged.clear();
            self.error = None;
            self.job = Some(sh.spawn(move |a| a.ai_check(&t)));
        }
    }
}

/// 110 pt ring gauge: faint track, coloured round-capped arc from 12 o'clock,
/// big rounded percentage with a small "AI" under it.
fn gauge(ui: &mut Ui, sh: &Shared, pct: f64, ai: Option<i64>, total: Option<i64>) {
    let p = sh.palette;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(110.0), egui::Sense::hover());
    let c = rect.center();
    let r = 55.0 - 3.0;
    let col = if pct < 25.0 {
        p.live
    } else if pct < 60.0 {
        p.warn
    } else {
        p.danger
    };
    ui.painter().circle_stroke(c, r, Stroke::new(6.0, p.line));
    let frac = (pct / 100.0).clamp(0.0, 1.0) as f32;
    if frac > 0.0 {
        let n = ((frac * 96.0) as usize).max(2);
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + frac * std::f32::consts::TAU * i as f32 / n as f32;
                Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
            })
            .collect();
        ui.painter().add(egui::Shape::line(pts.clone(), Stroke::new(6.0, col)));
        for e in [pts[0], pts[n]] {
            ui.painter().circle_filled(e, 3.0, col);
        }
    }
    let big = ui.painter().layout_no_wrap(
        format!("{}%", pct as i64),
        theme::jb(26.0, W::Medium),
        p.ink,
    );
    let small = ui.painter().layout_no_wrap("AI".into(), theme::jb(12.0, W::Semibold), p.ink2);
    let total_h = big.size().y + small.size().y - 4.0;
    let top = c.y - total_h / 2.0;
    let bp = Pos2::new(c.x - big.size().x / 2.0, top);
    let sp = Pos2::new(c.x - small.size().x / 2.0, top + big.size().y - 4.0);
    ui.painter().galley(bp, big, p.ink);
    ui.painter().galley(sp, small, p.ink_dim);
    if let (Some(ai), Some(words)) = (ai, total) {
        ui.add_space(6.0);
        ui.label(ty::sized(format!("{ai} of {words} words read as AI"), 11.0, W::Regular).color(p.ink2));
    }
}
