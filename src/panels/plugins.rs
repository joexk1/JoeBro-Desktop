//! Plugins panel: hosts the UI panels that joebro.plugin/1 plugins contribute.
//! Mirrors PluginPanels.swift: a 210 pt list of panels (icon, title, plugin name) beside the
//! selected panel. There is no embedded web view in egui, so the panel's HTML is fetched through
//! the same authenticated route and shown as its readable text.
use crate::api::R;
use crate::models::{Plugin, PluginPanel};
use crate::panels::workkit as wk;
use crate::state::{Job, Shared};
use crate::theme::{self, ty, W};
use crate::widgets;
use eframe::egui::{self, Rect, Sense, Ui, Vec2};

#[derive(Default)]
pub struct PluginPanelsView {
    selected: Option<String>,
    load: Option<Job<R<Vec<Plugin>>>>,
    loaded: bool,
    plugins: Vec<Plugin>,
    page_key: String,
    page_job: Option<Job<R<String>>>,
    page_text: Option<R<String>>,
}

struct Entry {
    plugin: Plugin,
    panel: PluginPanel,
}

impl Entry {
    fn id(&self) -> String {
        format!("{}/{}", if self.plugin.slug.is_empty() { &self.plugin.id } else { &self.plugin.slug }, self.panel.id)
    }
}

/// Readable text out of a plugin page: drop script/style/head, strip tags, collapse blank runs.
fn html_text(html: &str) -> String {
    let lower = html.to_lowercase();
    let mut out = String::new();
    let mut i = 0;
    let b = html.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            let rest = &lower[i..];
            let skip = ["script", "style", "head"].iter().find(|t| rest.starts_with(&format!("<{t}")));
            if let Some(t) = skip {
                if let Some(e) = lower[i..].find(&format!("</{t}>")) {
                    i += e + t.len() + 3;
                    continue;
                }
            }
            let block = ["<br", "<p", "</p", "<div", "</div", "<li", "<h1", "<h2", "<h3", "</h", "<tr"].iter().any(|t| rest.starts_with(t));
            match html[i..].find('>') {
                Some(e) => i += e + 1,
                None => break,
            }
            if block && !out.ends_with('\n') {
                out.push('\n');
            }
        } else {
            let ch = html[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    let out = out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ").replace("&quot;", "\"").replace("&#39;", "'");
    let mut lines: Vec<String> = Vec::new();
    for l in out.lines() {
        let t = l.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.is_empty() && lines.last().map_or(true, |p| p.is_empty()) {
            continue;
        }
        lines.push(t);
    }
    lines.join("\n").trim().to_string()
}

impl PluginPanelsView {
    fn entries(&self) -> Vec<Entry> {
        self.plugins
            .iter()
            .filter(|p| p.is_enabled && !p.slug.is_empty())
            .flat_map(|p| {
                p.panels
                    .iter()
                    .filter(|pp| pp.surfaces.is_empty() || pp.surfaces.iter().any(|s| s == "mac"))
                    .map(move |pp| Entry { plugin: p.clone(), panel: pp.clone() })
            })
            .collect()
    }

    fn reload(&mut self, sh: &Shared) {
        self.load = Some(sh.spawn(|a| a.plugins_typed()));
    }

    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        if !self.loaded && self.load.is_none() {
            self.reload(sh);
        }
        if let Some(j) = &mut self.load {
            if let Some(r) = j.take() {
                self.load = None;
                self.loaded = true;
                if let Ok(list) = r {
                    sh.plugins = list.clone();
                    self.plugins = list;
                }
            }
        }
        if let Some(j) = &mut self.page_job {
            if let Some(r) = j.take() {
                self.page_job = None;
                self.page_text = Some(r);
            }
        }
        let p = sh.palette;
        let rect = ui.max_rect();
        let mut refresh = false;
        let entries = self.entries();
        let cur = entries.iter().find(|e| Some(e.id()) == self.selected).or(entries.first()).map(|e| e.id());
        // Start fetching the current panel's page.
        if let Some(e) = entries.iter().find(|e| Some(e.id()) == cur) {
            let key = e.id();
            if self.page_key != key {
                self.page_key = key;
                self.page_text = None;
                let slug = e.plugin.slug.clone();
                let rel = e.panel.url.split("/ui/").skip(1).collect::<Vec<_>>().join("/ui/");
                let rel = if rel.is_empty() { "index.html".to_string() } else { rel };
                self.page_job = Some(sh.spawn(move |a| a.plugin_ui_file(&slug, &rel).map(|(b, _)| html_text(&String::from_utf8_lossy(&b)))));
            }
        }
        let mut pick = None;
        widgets::panel_chrome(
            ui,
            &p,
            "Plugins",
            "puzzlepiece.extension",
            |ui| {
                refresh = widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).on_hover_text("Reload plugins").clicked();
            },
            |ui| {
                let avail = Rect::from_min_max(ui.cursor().min, rect.max);
                ui.allocate_rect(avail, Sense::hover());
                if entries.is_empty() {
                    wk::box_in_rect(ui, &p, avail, 0.0, |ui| {
                        crate::ascii::empty_state(ui, &p, crate::ascii::Art::Plug, "No plugin panels");
                    });
                    return;
                }
                let left = Rect::from_min_size(avail.min, Vec2::new(210.0, avail.height()));
                let right = Rect::from_min_max(egui::pos2(avail.left() + 222.0, avail.top()), avail.max);
                wk::box_in_rect(ui, &p, left, 8.0, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for e in &entries {
                        let sel = Some(e.id()) == cur;
                        let r = widgets::hover_row(ui, &p, sel, theme::R_CTRL, |ui| {
                            ui.set_min_height(32.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let name = if e.panel.icon.is_empty() { "square.grid.2x2" } else { e.panel.icon.as_str() };
                                let (ir, _) = ui.allocate_exact_size(Vec2::new(18.0, 30.0), Sense::hover());
                                crate::icons::paint_symbol(ui.painter(), ir.center(), name, 14.0, if sel { p.accent } else { p.ink2 });
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    ui.add(egui::Label::new(ty::sized(&e.panel.title, 13.0, if sel { W::Semibold } else { W::Medium }).color(p.ink)).truncate());
                                    ui.add(egui::Label::new(ty::sized(&e.plugin.name, 10.5, W::Regular).color(p.ink3)).truncate());
                                });
                            });
                        });
                        if r.clicked() {
                            pick = Some(e.id());
                        }
                    }
                });
                wk::box_in_rect(ui, &p, right, 0.0, |ui| {
                    egui::ScrollArea::vertical().id_salt("plugin_page").auto_shrink([false, false]).show(ui, |ui| {
                        egui::Frame::none().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            match &self.page_text {
                                None => {
                                    widgets::panel_loading(ui, &p);
                                }
                                Some(Err(e)) => widgets::panel_failure(ui, &p, "Couldn't load panel", Some(e)),
                                Some(Ok(t)) => {
                                    ui.add(egui::Label::new(ty::sized(t, 13.0, W::Regular).color(p.ink)).wrap());
                                }
                            }
                        });
                    });
                });
            },
        );
        if let Some(id) = pick {
            self.selected = Some(id);
        }
        if refresh {
            self.reload(sh);
        }
    }
}
