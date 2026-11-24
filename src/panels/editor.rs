//! Editor pane: tabs over `sh.open_docs`, edit/preview, spreadsheet grid,
//! flashcard study/edit, binary view, AI-edit approval banner.
use crate::api::R;
use crate::state::{EditorDoc, Job, Shared};
use crate::widgets;
use super::editor_chrome as chrome;
use eframe::egui::{self, Align, Key, Layout, Rect, RichText, ScrollArea, Stroke, Ui};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct EditorPanel {
    pub(crate) preview: bool,
    // flashcard study session: queue of card indices + revealed state
    study_queue: Vec<usize>,
    study_revealed: bool,
    study_doc: String,
    pending_text: Vec<(String, String, String, Job<R<Value>>)>, // id, sid, sub, job
    pending_bin: Vec<(String, String, String, Job<R<Vec<u8>>>)>, // id, sub, job
    md: CommonMarkCache,
    // rendered PDF/SVG pages (PNG bytes) per doc id, + in-flight render jobs
    rendered: HashMap<String, R<Rendered>>,
    render_jobs: HashMap<String, Job<R<RenderOut>>>,
    save_jobs: HashMap<String, (String, Job<R<Value>>)>,
    save_due: HashMap<String, Instant>,
    save_errors: HashMap<String, String>,
    close_after_save: Option<String>,
    native_paths: HashMap<String, PathBuf>,
    native_jobs: HashMap<String, Job<R<PathBuf>>>,
    raw_save_jobs: HashMap<String, Job<R<(Vec<u8>, Option<FileRevision>)>>>,
    native_revisions: HashMap<String, Option<FileRevision>>,
    last_native_poll: Option<Instant>,
    study_learned: usize,
    study_again: usize,
    study_source: String,
    zoom: f32,
    render_revision: u64,
    export_jobs: HashMap<String, Job<R<Option<PathBuf>>>>,
    /// Doc id waiting for the in-app Save sheet.
    pending_copy: Option<String>,
    // Ctrl+F find-in-document (edit mode).
    find_open: bool,
    find_q: String,
    find_from: usize, // char index to resume the next search from
    find_focus: bool, // request focus on the find box next frame
    show_diff: bool,
    // Outline + stats cache, keyed by (doc id, content length).
    cache_key: (String, usize),
    heads: Vec<chrome::Heading>,
    stat: String,
    cursor_char: usize,
    goto: Option<usize>,
    preview_to: Option<f32>,
    preview_range: f32,
    sheet_focus: Option<(usize, usize)>,
}

/// Filename extension beats a server guess of "markdown"/empty (Mac `docLanguage`).
fn doc_language(d: &EditorDoc) -> String {
    let t = d.title.to_lowercase();
    let from_title = t.rsplit_once('.').map(|(_, e)| match e {
        "md" | "markdown" => "markdown",
        "txt" => "text",
        "html" | "htm" => "html",
        "svg" => "svg",
        "rs" => "rust",
        "py" => "python",
        "js" => "javascript",
        "ts" => "typescript",
        "json" => "json",
        "csv" => "csv",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "sh" => "bash",
        "css" => "css",
        "swift" => "swift",
        "eml" => "email",
        _ => "",
    });
    match from_title {
        Some(l) if !l.is_empty() && (d.language.is_empty() || d.language == "markdown" || l == "svg") => l.to_string(),
        _ if d.language.is_empty() && d.title.contains('.') => "text".to_string(),
        _ => d.language.clone(),
    }
}

fn is_markdown_lang(l: &str) -> bool {
    matches!(l.to_lowercase().as_str(), "markdown" | "md" | "")
}

fn is_monospaced(l: &str) -> bool {
    !matches!(l.to_lowercase().as_str(), "markdown" | "md" | "email" | "text" | "")
}

/// Mac `editorFont`: Geist 14 for prose, Geist Mono 12.5 for code.
fn editor_font(mono: bool) -> egui::FontId {
    if mono { crate::theme::mono(12.5, crate::theme::W::Regular) } else { crate::theme::jb(14.0, crate::theme::W::Regular) }
}

impl EditorPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        // Service a sidebar open request, then land any in-flight opens.
        if let Some((sid, sub)) = sh.open_request.take() {
            self.open(sh, &sid, &sub);
        }
        self.drain_open(sh);
        self.drain_saves(sh);
        if let Some(path) = sh.take_picked("editor_save") {
            if let Some(id) = self.pending_copy.take() {
                self.save_copy_to(sh, &id, path);
            }
        }
        let p = sh.palette;
        if sh.open_docs.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("No document open").font(crate::theme::jb(13.0, crate::theme::W::Regular)).color(p.ink3))
            });
            return;
        }

        // Tab bar (Mac `tabBar`).
        for act in chrome::tab_bar(ui, sh, self.preview, sh.editor_popped_out) {
            match act {
                chrome::TabAct::SetPreview(v) => self.preview = v,
                chrome::TabAct::Select(id) => sh.active_doc = Some(id),
                chrome::TabAct::Close(id) => {
                    if sh.open_docs.iter().any(|d| d.id == id && d.dirty) {
                        self.close_after_save = Some(id.clone());
                        self.save_due.insert(id, Instant::now());
                    } else {
                        self.close_doc(sh, &id);
                    }
                }
                chrome::TabAct::PopOut => sh.editor_popped_out = true,
                chrome::TabAct::Hide => {
                    if sh.editor_popped_out {
                        sh.editor_popped_out = false;
                    }
                    sh.editor_visible = false;
                }
            }
        }

        let active = match sh.active_doc.clone() {
            Some(a) => a,
            None => return,
        };
        let idx = match sh.open_docs.iter().position(|d| d.id == active) {
            Some(i) => i,
            None => return,
        };

        if ui.input(|i| i.modifiers.command && i.key_pressed(Key::S)) {
            self.save_errors.remove(&active);
            self.save_due.insert(active.clone(), Instant::now());
        }
        if let Some(error) = self.save_errors.get(&active).cloned() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("Save failed: {error}")).font(crate::theme::jb(11.0, crate::theme::W::Regular)).color(p.danger));
                if !sh.open_docs[idx].is_binary
                    && widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, true, "Retry save").clicked()
                {
                    self.save_errors.remove(&active);
                    self.save_due.insert(active.clone(), Instant::now());
                }
            });
        }

        // Approval banner (Mac `approvalBanner`).
        if let Some(pending) = sh.open_docs[idx].pending_ai.clone() {
            let old = sh.open_docs[idx].content.clone();
            match chrome::approval_banner(ui, &p, &old, &pending, &mut self.show_diff) {
                Some(true) => {
                    sh.open_docs[idx].content = pending;
                    sh.open_docs[idx].pending_ai = None;
                    sh.open_docs[idx].dirty = true;
                    self.schedule_save(sh, idx);
                }
                Some(false) => sh.open_docs[idx].pending_ai = None,
                None => {}
            }
        }

        let (spreadsheet, flashcards, binary) = {
            let d = &sh.open_docs[idx];
            (d.spreadsheet, d.flashcards, d.is_binary)
        };
        let plain = !spreadsheet && !flashcards && !binary;

        // Footer (Mac `footer`), pinned to the bottom of the pane.
        let (lang, stat, dirty) = {
            let d = &sh.open_docs[idx];
            let lang = doc_language(d);
            let md = is_markdown_lang(&lang);
            if self.cache_key != (d.id.clone(), d.content.len()) {
                self.cache_key = (d.id.clone(), d.content.len());
                self.heads = if md { chrome::outline(&d.content) } else { vec![] };
                self.stat = if md { chrome::stats(&d.content) } else { chrome::line_stats(&d.content) };
            }
            (lang, self.stat.clone(), d.dirty)
        };
        let mut save_copy = false;
        egui::TopBottomPanel::bottom("editor-footer")
            .frame(egui::Frame::none())
            .show_separator_line(false)
            .exact_height(27.0)
            .show_inside(ui, |ui| {
                chrome::footer(ui, &p, if !plain { "" } else if lang.is_empty() { "text" } else { &lang }, if plain { &stat } else { "" }, 0, dirty);
                let r = ui.max_rect();
                let busy = self.export_jobs.contains_key(&active);
                let saving = self.save_jobs.contains_key(&active) || self.raw_save_jobs.contains_key(&active);
                let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(egui::pos2(r.right() - 150.0, r.top() + 1.0), egui::vec2(120.0, 26.0))).layout(Layout::right_to_left(Align::Center)));
                let label = if saving { "Saving…" } else { "Save a copy…" };
                let t = crate::theme::ty::mono(label, 10.5, crate::theme::W::Regular).color(p.ink3);
                if cu.add_enabled(!busy && !saving, egui::Label::new(t).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    save_copy = true;
                }
            });
        if save_copy {
            self.save_copy(sh, idx, &active);
        }

        if binary {
            self.binary_view(ui, sh, idx);
        } else if flashcards {
            self.flashcards_view(ui, sh, idx);
        } else if spreadsheet {
            self.spreadsheet_view(ui, sh, idx);
        } else {
            let show_outline = plain && !self.heads.is_empty();
            if show_outline {
                let wide = ui.available_width() >= 560.0;
                let (w, name) = if wide { (168.0, "editor-outline") } else { (26.0, "editor-rail") };
                let active_h = self.active_heading();
                let mut picked = None;
                egui::SidePanel::right(name)
                    .frame(egui::Frame::none())
                    .show_separator_line(false)
                    .resizable(false)
                    .exact_width(w)
                    .show_inside(ui, |ui| {
                        picked = if wide {
                            chrome::outline_panel(ui, &p, &self.heads, active_h)
                        } else {
                            chrome::outline_rail(ui, &p, &self.heads, active_h)
                        };
                    });
                if let Some(i) = picked {
                    let off = self.heads[i].offset;
                    self.goto = Some(off);
                    let total = sh.open_docs[idx].content.chars().count().max(1);
                    self.preview_to = Some(off as f32 / total as f32);
                }
            }
            self.text_view(ui, sh, idx);
        }
    }

    fn active_heading(&self) -> Option<usize> {
        let pos = self.cursor_char;
        let i = self.heads.iter().rposition(|h| h.offset <= pos);
        i.or(if self.heads.is_empty() { None } else { Some(0) })
    }

    fn save_copy(&mut self, sh: &mut Shared, idx: usize, active: &str) {
        let title = sh.open_docs[idx].title.clone();
        self.pending_copy = Some(active.to_string());
        sh.picker.open_save("editor_save", "Save a copy", &title, None);
    }

    fn save_copy_to(&mut self, sh: &mut Shared, active: &str, path: PathBuf) {
        let Some(doc) = sh.open_docs.iter().find(|d| d.id == active).cloned() else { return };
        let edited_path = self.native_paths.get(active).cloned();
        self.export_jobs.insert(
            active.to_string(),
            sh.spawn(move |api| {
                let bytes = if doc.is_binary {
                    if let Some(path) = edited_path { std::fs::read(path).map_err(|e| e.to_string())? }
                    else { doc.raw_bytes.ok_or("Document bytes are unavailable")? }
                } else if is_office_doc(&doc.title) {
                    return Err("Open the original Office file before exporting, so its formatting is preserved.".into());
                } else if doc.title.to_lowercase().ends_with(".xlsx") {
                    api.csv_to_xlsx(&doc.content)?
                } else if doc.flashcards {
                    api.text_to_apkg(&doc.content, &doc.title)?
                } else {
                    doc.content.into_bytes()
                };
                atomic_write(&path, &bytes)?;
                Ok(Some(path))
            }),
        );
    }

    fn open(&mut self, sh: &mut Shared, sid: &str, sub: &str) {
        let id = format!("local-{sid}:{sub}");
        if sh.open_docs.iter().any(|d| d.id == id) {
            sh.active_doc = Some(id);
            sh.editor_visible = true;
            return;
        }
        let lower = sub.to_lowercase();
        let is_bin = [
            ".doc", ".docx", ".xlsx", ".pdf", ".svg", ".png", ".jpg", ".jpeg", ".gif", ".webp",
            ".bmp",
        ]
        .iter()
        .any(|e| lower.ends_with(e));
        let sid_s = sid.to_string();
        let sub_s = sub.to_string();
        if is_bin {
            let job = sh.spawn(move |a| a.workdir_raw(&sid_s, &sub_s));
            // fire-and-forget: pull bytes, then push doc next frames via a poll
            self.pending_bin
                .push((id, sid.to_string(), sub.to_string(), job));
        } else {
            let job = sh.spawn(move |a| a.open_file(&sid_s, &sub_s));
            self.pending_text
                .push((id, sid.to_string(), sub.to_string(), job));
        }
        sh.editor_visible = true;
    }

    fn text_view(&mut self, ui: &mut Ui, sh: &mut Shared, idx: usize) {
        let p = sh.palette;
        let lang = doc_language(&sh.open_docs[idx]);
        let mono = is_monospaced(&lang);
        let doc_id = sh.open_docs[idx].id.clone();
        // Ctrl+F (Cmd+F on mac) opens the find bar. Find works on the editable
        // text, so drop out of preview if needed.
        if ui.input(|i| i.modifiers.command && i.key_pressed(Key::F)) {
            self.find_open = true;
            self.find_focus = true;
            self.preview = false;
        }
        // AI is streaming into the doc: read-only tail view with a "writing" badge.
        if sh.open_docs[idx].ai_writing {
            let content = sh.open_docs[idx].content.clone();
            let area = ui.available_rect_before_wrap();
            ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(content).font(editor_font(mono)).color(p.ink));
                });
            });
            let badge = Rect::from_min_size(egui::pos2(area.right() - 100.0, area.top() + 10.0), egui::vec2(90.0, 24.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(badge), |ui| {
                widgets::surface(ui, &p, widgets::Level::Raised, Some(crate::theme::R_CTRL), |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(9.0);
                        crate::ascii::spinner(ui, 10.0, p.accent);
                        ui.label(crate::theme::ty::sized("writing", 10.5, crate::theme::W::Regular).color(p.ink2));
                        ui.add_space(9.0);
                    });
                });
            });
            return;
        }
        if self.preview {
            let content = sh.open_docs[idx].content.clone();
            let title = sh.open_docs[idx].title.to_lowercase();
            let is_html = title.ends_with(".html") || title.ends_with(".htm");
            if is_html {
                self.html_preview(ui, sh, idx);
            } else {
                // Both axes: an unwrappable block (wide table, long code line) scrolls sideways
                // inside the pane instead of widening it past its clip.
                let mut area = ScrollArea::both().id_salt(("editor-preview", &doc_id)).auto_shrink([false, false]);
                if let Some(f) = self.preview_to.take() {
                    area = area.vertical_scroll_offset((f * self.preview_range).max(0.0));
                }
                let out = area.show(ui, |ui| {
                    // A fixed reading column (720 max), centred by explicit left padding so a
                    // wide child can only grow to the right, never off the left edge.
                    let avail = ui.available_width();
                    let col = (avail - 48.0).clamp(120.0, 720.0);
                    let pad = ((avail - col) / 2.0).max(16.0);
                    ui.horizontal_top(|ui| {
                        ui.add_space(pad);
                        ui.vertical(|ui| {
                            ui.set_width(col);
                            ui.add_space(20.0);
                            crate::panels::mdview::prose_style(ui, &sh.palette, true);
                            CommonMarkViewer::new().show(ui, &mut self.md, &crate::widgets::preprocess_md(&content));
                            ui.add_space(20.0);
                        });
                    });
                });
                self.preview_range = (out.content_size.y - out.inner_rect.height()).max(0.0);
                if self.preview_range > 1.0 && !self.heads.is_empty() {
                    let frac = (out.state.offset.y / self.preview_range).clamp(0.0, 1.0);
                    self.cursor_char = (frac * content.chars().count() as f32) as usize;
                }
            }
            return;
        }
        let mut content = sh.open_docs[idx].content.clone();

        // Format bar (markdown / prose docs only).
        let fmt = if mono { None } else { chrome::format_bar(ui, &p) };

        // Find bar.
        let mut do_find = false;
        let mut do_prev = false;
        if self.find_open {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                let mut q_resp = None;
                widgets::field_frame(&p, false).inner_margin(egui::Margin::symmetric(8.0, 3.0)).show(ui, |ui| {
                    q_resp = Some(ui.add(
                        egui::TextEdit::singleline(&mut self.find_q)
                            .frame(false)
                            .desired_width(180.0)
                            .font(crate::theme::jb(12.0, crate::theme::W::Regular))
                            .hint_text("Find"),
                    ));
                });
                let q = q_resp.unwrap();
                if self.find_focus {
                    q.request_focus();
                    self.find_focus = false;
                }
                let entered = q.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, true, "Next").clicked() || entered {
                    do_find = true;
                }
                if widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, true, "Prev").clicked() {
                    do_prev = true;
                }
                let n = count_matches(&content, &self.find_q);
                ui.label(crate::theme::ty::mono(format!("{n} match{}", if n == 1 { "" } else { "es" }), 10.5, crate::theme::W::Regular).color(p.ink3));
                if widgets::icon_btn(ui, &p, "xmark", 24.0, false).clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.find_open = false;
                }
            });
            ui.add_space(6.0);
            chrome::hairline(ui, &p, ui.cursor().top(), 12.0);
        }

        let te_id = ui.make_persistent_id(("editor-text", &doc_id));
        let font = editor_font(mono);
        let lh = if mono { None } else { Some(21.0) };
        let ink = p.ink;
        let mut layouter = |ui: &Ui, s: &str, wrap: f32| {
            let mut job = egui::text::LayoutJob::simple(s.to_string(), font.clone(), ink, wrap);
            for sec in &mut job.sections {
                sec.format.line_height = lh;
            }
            ui.fonts(|f| f.layout_job(job))
        };
        let mut goto_y = None;
        let goto = self.goto.take();
        // Toolbar / outline requests act on the current selection.
        let mut wrapped = false;
        let mut pending: Option<(String, (usize, usize))> = None;
        {
            let (bold, italic) = if ui.memory(|m| m.has_focus(te_id)) {
                ui.input_mut(|i| (i.consume_key(egui::Modifiers::COMMAND, Key::B), i.consume_key(egui::Modifiers::COMMAND, Key::I)))
            } else {
                (false, false)
            };
            let cmd = match (fmt, bold, italic) {
                (Some(f), _, _) => Some(f),
                (None, true, _) => Some(chrome::Fmt::Bold),
                (None, _, true) => Some(chrome::Fmt::Italic),
                _ => None,
            };
            if let Some(cmd) = cmd {
                if let Some(state) = egui::TextEdit::load_state(ui.ctx(), te_id) {
                    if let Some(range) = state.cursor.char_range() {
                        let (a, b) = (range.primary.index.min(range.secondary.index), range.primary.index.max(range.secondary.index));
                        pending = Some(match cmd {
                            chrome::Fmt::Bold => toggle_wrap(&content, a, b, "**"),
                            chrome::Fmt::Italic => toggle_wrap(&content, a, b, "*"),
                            chrome::Fmt::Bullet => chrome::prefix_lines(&content, a, b, false),
                            chrome::Fmt::Number => chrome::prefix_lines(&content, a, b, true),
                        });
                    }
                }
            }
        }
        let mut changed = false;
        let mut cursor_now = self.cursor_char;
        let margin = if mono { egui::vec2(8.0, 10.0) } else { egui::vec2(18.0, 14.0) };
        let area = ScrollArea::vertical().id_salt(("editor-scroll", &doc_id)).auto_shrink([false, false]);
        area.show(ui, |ui| {
            ui.visuals_mut().selection.bg_fill = p.accent_a(0.22);
            ui.visuals_mut().selection.stroke = Stroke::new(1.0, p.accent);
            ui.visuals_mut().text_cursor.stroke = Stroke::new(2.0, p.accent);
            let mut out = egui::TextEdit::multiline(&mut content)
                .id(te_id)
                .frame(false)
                .margin(margin)
                .lock_focus(true)
                .desired_width(f32::INFINITY)
                .desired_rows(24)
                .layouter(&mut layouter)
                .show(ui);
            if let Some(off) = goto {
                let c = egui::text::CCursor::new(off);
                out.state.cursor.set_char_range(Some(egui::text::CCursorRange::one(c)));
                goto_y = Some(out.galley.pos_from_ccursor(c).min.y + out.galley_pos.y);
                out.state.clone().store(ui.ctx(), te_id);
            }
            if let Some(y) = goto_y {
                let r = Rect::from_min_size(egui::pos2(out.response.rect.left(), y), egui::vec2(10.0, 24.0));
                ui.scroll_to_rect(r, Some(Align::Min));
            }
            if let Some(r) = out.state.cursor.char_range() {
                cursor_now = r.primary.index;
            }
            changed = out.response.changed();
        });
        self.cursor_char = cursor_now;
        if let Some((new, sel)) = pending {
            content = new;
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), te_id) {
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(sel.0), egui::text::CCursor::new(sel.1))));
                state.store(ui.ctx(), te_id);
            }
            ui.memory_mut(|m| m.request_focus(te_id));
            wrapped = true;
        }
        if changed || wrapped {
            sh.open_docs[idx].content = content.clone();
            sh.open_docs[idx].dirty = true;
            self.schedule_save(sh, idx);
        }
        if (do_find || do_prev) && !self.find_q.is_empty() {
            apply_find(ui.ctx(), te_id, &content, &self.find_q, &mut self.find_from, do_prev);
        }
    }

    fn spreadsheet_view(&mut self, ui: &mut Ui, sh: &mut Shared, idx: usize) {
        let mut rows = match parse_csv(&sh.open_docs[idx].content) {
            Ok(rows) => rows,
            Err(error) => {
                ui.label(RichText::new(format!("CSV could not be parsed: {error}. Repair the source below.")).font(crate::theme::jb(12.0, crate::theme::W::Regular)).color(sh.palette.danger));
                self.preview = false;
                self.text_view(ui, sh, idx);
                return;
            }
        };
        if rows.is_empty() {
            rows.push(vec![String::new()]);
        }
        let p = sh.palette;
        let mut cols = rows.iter().map(|r| r.len()).max().unwrap_or(1);
        let mut changed = false;
        let mut remove_row = None;
        let mut remove_col = None;
        // Toolbar (Mac `toolbar`): ghost "+ Row" / "+ Column", cell ref and size on the right.
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(0.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            if widgets::button_ex(ui, &p, widgets::BtnKind::Ghost, true, true, "+ Row").clicked() {
                rows.push(vec![String::new(); cols]);
                changed = true;
            }
            if widgets::button_ex(ui, &p, widgets::BtnKind::Ghost, true, true, "+ Column").clicked() {
                for row in &mut rows {
                    row.resize(cols + 1, String::new());
                }
                cols += 1;
                changed = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(crate::theme::ty::sized(format!("{} × {}", rows.len(), cols), 11.0, crate::theme::W::Regular).color(p.ink3));
                if let Some((r, c)) = self.sheet_focus {
                    ui.label(crate::theme::ty::mono(format!("{}{}", column_name(c), r + 1), 11.0, crate::theme::W::Regular).color(p.ink2));
                }
            });
        });
        ui.add_space(6.0);
        chrome::hairline(ui, &p, ui.cursor().top(), 12.0);
        ui.add_space(1.0);
        let (cw, ch, gutter) = (130.0, 26.0, 46.0);
        let focus = self.sheet_focus;
        let mut new_focus = None;
        let doc_id = sh.open_docs[idx].id.clone();
        ScrollArea::horizontal().id_salt(("sheet-h", &doc_id)).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            // Column header (stays put while rows scroll).
            let (hr, _) = ui.allocate_exact_size(egui::vec2(gutter + cw * cols as f32, 24.0), egui::Sense::hover());
            ui.painter().rect_filled(hr, egui::Rounding::ZERO, p.surface);
            ui.painter().hline(hr.x_range(), hr.bottom(), Stroke::new(1.0, p.line_strong));
            for c in 0..cols {
                let cr = Rect::from_min_size(egui::pos2(hr.left() + gutter + cw * c as f32, hr.top()), egui::vec2(cw, 24.0));
                let col = if focus.is_some_and(|f| f.1 == c) { p.accent } else { p.ink3 };
                ui.painter().text(cr.center(), egui::Align2::CENTER_CENTER, column_name(c), crate::theme::mono(10.0, crate::theme::W::Medium), col);
                ui.interact(cr, ui.id().with(("colhead", c)), egui::Sense::click()).context_menu(|ui| {
                    if ui.button("Delete column").clicked() {
                        remove_col = Some(c);
                        ui.close_menu();
                    }
                });
            }
            ScrollArea::vertical().id_salt(("sheet-v", &doc_id)).auto_shrink([false, false]).show_rows(ui, ch, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                for r in range {
                    let (rr, rresp) = ui.allocate_exact_size(egui::vec2(gutter + cw * cols as f32, ch), egui::Sense::click());
                    let rcol = if focus.is_some_and(|f| f.0 == r) { p.accent } else { p.ink3 };
                    ui.painter().text(egui::pos2(rr.left() + gutter - 8.0, rr.center().y), egui::Align2::RIGHT_CENTER, (r + 1).to_string(), crate::theme::mono(10.0, crate::theme::W::Regular), rcol);
                    rresp.context_menu(|ui| {
                        if ui.button("Delete row").clicked() {
                            remove_row = Some(r);
                            ui.close_menu();
                        }
                    });
                    for c in 0..cols {
                        let cr = Rect::from_min_size(egui::pos2(rr.left() + gutter + cw * c as f32, rr.top()), egui::vec2(cw, ch));
                        let mut cell = rows[r].get(c).cloned().unwrap_or_default();
                        let numeric = {
                            let t = cell.trim_matches(|c| " $€£%,".contains(c));
                            !t.is_empty() && t.parse::<f64>().is_ok()
                        };
                        let selected = focus == Some((r, c));
                        if selected {
                            ui.painter().rect_filled(cr, egui::Rounding::ZERO, p.accent_soft);
                        }
                        ui.painter().rect_stroke(cr, egui::Rounding::ZERO, Stroke::new(0.5, p.line));
                        let font = if numeric { crate::theme::mono(12.0, crate::theme::W::Regular) } else { crate::theme::jb(12.0, crate::theme::W::Regular) };
                        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(cr.shrink2(egui::vec2(8.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
                        let te = cui.add(
                            egui::TextEdit::singleline(&mut cell)
                                .id_salt((r, c))
                                .frame(false)
                                .font(font)
                                .text_color(p.ink)
                                .horizontal_align(if numeric { Align::RIGHT } else { Align::LEFT })
                                .desired_width(cw - 16.0),
                        );
                        if te.has_focus() {
                            new_focus = Some((r, c));
                            ui.painter().rect_stroke(cr, egui::Rounding::ZERO, Stroke::new(1.5, p.accent));
                        }
                        if te.changed() {
                            if rows[r].len() <= c {
                                rows[r].resize(c + 1, String::new());
                            }
                            rows[r][c] = cell;
                            changed = true;
                        }
                    }
                }
            });
        });
        if new_focus.is_some() {
            self.sheet_focus = new_focus;
        }
        if let Some(r) = remove_row {
            rows.remove(r);
            changed = true;
        }
        if let Some(c) = remove_col {
            for row in &mut rows {
                if c < row.len() {
                    row.remove(c);
                }
            }
            changed = true;
        }

        if changed {
            sh.open_docs[idx].content = serialize_csv(&rows);
            sh.open_docs[idx].dirty = true;
            self.schedule_save(sh, idx);
        }
    }

    fn flashcards_view(&mut self, ui: &mut Ui, sh: &mut Shared, idx: usize) {
        use crate::theme::{jb, W};
        let p = sh.palette;
        let mut cards = parse_cards(&sh.open_docs[idx].content);
        let empty = |ui: &mut Ui| {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    crate::icons::symbol(ui, "rectangle.on.rectangle.slash", 28.0, p.ink3);
                    ui.label(RichText::new("No cards yet").font(jb(15.0, W::Semibold)).color(p.ink2));
                });
            });
        };
        if self.preview {
            // Study mode (Mac `studyView`).
            if self.study_doc != sh.open_docs[idx].id || self.study_source != sh.open_docs[idx].content {
                self.study_doc = sh.open_docs[idx].id.clone();
                self.study_queue = (0..cards.len()).filter(|&i| !cards[i].2).collect();
                self.study_revealed = false;
                self.study_learned = 0;
                self.study_again = 0;
                self.study_source = sh.open_docs[idx].content.clone();
                shuffle(&mut self.study_queue);
            }
            if cards.is_empty() {
                empty(ui);
                return;
            }
            if self.study_queue.is_empty() {
                let still = cards.iter().filter(|c| !c.2).count();
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() / 2.0 - 70.0).max(20.0));
                    crate::ascii::dot_number(ui, &self.study_learned.to_string(), 3.0, p.live);
                    ui.add_space(14.0);
                    let title = if self.study_learned == 0 && self.study_again == 0 { format!("All {} learned", cards.len()) } else if still == 0 { format!("All {} learned", cards.len()) } else { "Session done".to_string() };
                    ui.label(RichText::new(title).font(jb(17.0, W::Semibold)).color(p.ink));
                    if self.study_learned > 0 || self.study_again > 0 {
                        let mut t = format!("{} learned this round", self.study_learned);
                        if self.study_again > 0 { t += &format!(" · {} retries", self.study_again); }
                        if still > 0 { t += &format!(" · {still} still to learn"); }
                        ui.label(RichText::new(t).font(jb(11.5, W::Regular)).color(p.ink3));
                    }
                    ui.add_space(14.0);
                    let mut start: Option<bool> = None;
                    ui.allocate_ui_with_layout(egui::vec2(300.0, 30.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.add_space(if still > 0 { 10.0 } else { 80.0 });
                        if still > 0 {
                            if widgets::button_ex(ui, &p, widgets::BtnKind::Primary, true, true, format!("Continue learning ({still})")).clicked() { start = Some(false); }
                            if widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, true, format!("Study all {}", cards.len())).clicked() { start = Some(true); }
                        } else if widgets::button_ex(ui, &p, widgets::BtnKind::Primary, true, true, "Study again").clicked() {
                            start = Some(true);
                        }
                    });
                    if let Some(all) = start {
                        self.study_queue = (0..cards.len()).filter(|&i| all || !cards[i].2).collect();
                        shuffle(&mut self.study_queue);
                        self.study_learned = 0;
                        self.study_again = 0;
                        self.study_revealed = false;
                    }
                });
                return;
            }
            ui.add_space(14.0);
            let total = (self.study_learned + self.study_queue.len()).max(1);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.spacing_mut().item_spacing.x = 10.0;
                crate::ascii::dot_number(ui, &self.study_queue.len().to_string(), 2.0, p.accent);
                ui.label(RichText::new(format!("/ {total}")).font(jb(11.0, W::Regular)).color(p.ink3));
                if self.study_learned > 0 {
                    ui.label(RichText::new(format!("+{}", self.study_learned)).font(jb(11.0, W::Regular)).color(p.live));
                }
                if self.study_again > 0 {
                    ui.label(RichText::new(format!("↺{}", self.study_again)).font(jb(11.0, W::Regular)).color(p.warn));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(14.0);
                    if widgets::icon_btn(ui, &p, "shuffle", 24.0, false).on_hover_text("Shuffle").clicked() {
                        shuffle(&mut self.study_queue);
                        self.study_revealed = false;
                    }
                });
            });
            ui.add_space(10.0);
            {
                let w = ui.available_width() - 28.0;
                let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 2.0), egui::Sense::hover());
                let bar = Rect::from_min_size(egui::pos2(r.left() + 14.0, r.top()), egui::vec2(w, 2.0));
                ui.painter().rect_filled(bar, egui::Rounding::same(1.0), p.line);
                let fw = w * self.study_learned as f32 / total as f32;
                ui.painter().rect_filled(Rect::from_min_size(bar.min, egui::vec2(fw, 2.0)), egui::Rounding::same(1.0), p.accent);
            }
            let ci = self.study_queue[0];
            let (front, back, _) = cards[ci].clone();
            let (advance, left, right) = ui.input(|i| {
                (i.key_pressed(Key::Enter), i.key_pressed(Key::ArrowLeft), i.key_pressed(Key::ArrowRight))
            });
            let space = ui.input(|i| i.key_pressed(Key::Space));
            let was_revealed = self.study_revealed;
            // Card face: raised, 14 pt radius, eyebrow label and centred 20 pt text.
            let avail = ui.available_size();
            let cw = (avail.x - 28.0).min(560.0);
            let top_pad = ((avail.y - 240.0 - 40.0) / 2.0).max(8.0);
            ui.add_space(top_pad);
            let mut flipped = false;
            ui.vertical_centered(|ui| {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(cw, 240.0), egui::Sense::click());
                ui.painter().rect_filled(rect, egui::Rounding::same(crate::theme::R_PANEL), p.raised);
                ui.painter().rect_stroke(rect, egui::Rounding::same(crate::theme::R_PANEL), Stroke::new(1.0, p.line));
                let (label, text) = if self.study_revealed { ("ANSWER", &back) } else { ("QUESTION", &front) };
                ui.painter().text(egui::pos2(rect.center().x, rect.top() + 24.0 + 6.0), egui::Align2::CENTER_CENTER, label, jb(11.0, W::Semibold), p.ink3);
                let shown = if text.is_empty() { "(blank)" } else { text.as_str() };
                let g = ui.painter().layout(shown.to_string(), jb(20.0, W::Medium), p.ink, rect.width() - 48.0);
                let pos = egui::pos2(rect.center().x - g.size().x / 2.0, rect.center().y - g.size().y / 2.0 + 10.0);
                ui.painter().galley(pos, g, p.ink);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    flipped = true;
                }
            });
            if flipped || space {
                self.study_revealed = !self.study_revealed;
            }
            if advance && !was_revealed {
                self.study_revealed = true;
            }
            if (left || right) && self.study_queue.len() > 1 {
                if right { let c = self.study_queue.remove(0); self.study_queue.push(c); }
                else { let c = self.study_queue.pop().unwrap(); self.study_queue.insert(0, c); }
                self.study_revealed = false;
            }
            ui.add_space(12.0);
            if self.study_revealed {
                ui.vertical_centered(|ui| {
                    ui.allocate_ui_with_layout(egui::vec2(260.0, 30.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        if widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, true, "Still learning").on_hover_text("Show again in a few cards (1)").clicked()
                            || ui.input(|i| i.key_pressed(Key::Num1))
                        {
                            self.study_again += 1;
                            let c = self.study_queue.remove(0);
                            let pos = 5.min(self.study_queue.len());
                            self.study_queue.insert(pos, c);
                            self.study_revealed = false;
                        } else if widgets::button_ex(ui, &p, widgets::BtnKind::Primary, true, true, "Learned").on_hover_text("Mark learned (2 or Return)").clicked()
                            || ui.input(|i| i.key_pressed(Key::Num2))
                            || (advance && was_revealed)
                        {
                            self.study_learned += 1;
                            cards[ci].2 = true;
                            self.study_queue.remove(0);
                            self.study_revealed = false;
                            sh.open_docs[idx].content = serialize_cards(&cards);
                            self.study_source = sh.open_docs[idx].content.clone();
                            sh.open_docs[idx].dirty = true;
                            self.schedule_save(sh, idx);
                        }
                    });
                });
            }
            return;
        }
        // Edit mode: toolbar + scrollable list of raised cards (Mac `editList`).
        self.study_doc.clear();
        let mut changed = false;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if widgets::button_ex(ui, &p, widgets::BtnKind::Ghost, true, true, "+ Card").clicked() {
                cards.push(("New question".into(), String::new(), false));
                changed = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let learned = cards.iter().filter(|c| c.2).count();
                ui.label(RichText::new(format!("{}/{}", learned, cards.len())).font(jb(11.0, W::Regular)).color(p.ink3)).on_hover_text("Learned");
            });
        });
        ui.add_space(6.0);
        chrome::hairline(ui, &p, ui.cursor().top(), 12.0);
        ui.add_space(1.0);
        if cards.is_empty() {
            empty(ui);
        }
        let mut remove = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(12.0);
            for (i, card) in cards.iter_mut().enumerate() {
                egui::Frame::none()
                    .fill(p.raised)
                    .stroke(Stroke::new(1.0, p.line))
                    .rounding(egui::Rounding::same(crate::theme::R_CARD))
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new((i + 1).to_string()).font(jb(10.0, W::Medium)).color(p.ink3));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if widgets::icon_btn(ui, &p, "trash", 22.0, false).clicked() {
                                    remove = Some(i);
                                }
                                let (name, col, txt) = if card.2 { ("checkmark.seal.fill", p.live, "Learned") } else { ("seal", p.ink3, "Learning") };
                                let r = ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    ui.label(RichText::new(txt).font(jb(10.5, W::Regular)).color(col));
                                    crate::icons::symbol(ui, name, 11.0, col);
                                }).response.interact(egui::Sense::click()).on_hover_text(if card.2 { "Mark as still learning" } else { "Mark as learned" });
                                if r.clicked() {
                                    card.2 = !card.2;
                                    changed = true;
                                }
                            });
                        });
                        ui.add_space(6.0);
                        changed |= ui.add(egui::TextEdit::multiline(&mut card.0).frame(false).desired_rows(1).desired_width(f32::INFINITY).font(jb(13.0, W::Medium)).text_color(p.ink).hint_text("Question")).changed();
                        ui.add_space(6.0);
                        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, egui::Rounding::ZERO, p.line);
                        ui.add_space(6.0);
                        changed |= ui.add(egui::TextEdit::multiline(&mut card.1).frame(false).desired_rows(1).desired_width(f32::INFINITY).font(jb(12.5, W::Regular)).text_color(p.ink2).hint_text("Answer")).changed();
                    });
                ui.add_space(10.0);
            }
        });
        if let Some(i) = remove {
            cards.remove(i);
            changed = true;
        }
        if changed {
            sh.open_docs[idx].content = serialize_cards(&cards);
            sh.open_docs[idx].dirty = true;
            self.schedule_save(sh, idx);
        }
    }

    /// Service background work even when the editor pane is hidden.
    pub fn tick(&mut self, sh: &mut Shared) {
        if let Some((sid, sub)) = sh.open_request.take() {
            self.open(sh, &sid, &sub);
        }
        self.drain_open(sh);
        self.drain_saves(sh);
    }

    pub fn has_unsaved(&self, sh: &Shared) -> bool {
        sh.open_docs.iter().any(|d| d.dirty)
            || !self.save_jobs.is_empty()
            || !self.raw_save_jobs.is_empty()
    }

    fn close_doc(&mut self, sh: &mut Shared, id: &str) {
        sh.open_docs.retain(|d| d.id != id);
        sh.active_doc = sh.open_docs.last().map(|d| d.id.clone());
        self.rendered.remove(id);
        self.render_jobs.remove(id);
        self.save_due.remove(id);
        self.save_errors.remove(id);
        if sh.open_docs.is_empty() {
            sh.editor_visible = false;
        }
    }

    fn schedule_save(&mut self, sh: &Shared, idx: usize) {
        let id = sh.open_docs[idx].id.clone();
        self.save_errors.remove(&id);
        self.save_due
            .insert(id.clone(), Instant::now() + Duration::from_millis(650));
        self.rendered.remove(&id);
        // Do not land an old HTML render after the content changed.
        self.render_jobs.remove(&id);
        sh.ctx.request_repaint_after(Duration::from_millis(700));
    }

    fn drain_saves(&mut self, sh: &mut Shared) {
        let exported: Vec<_> = self
            .export_jobs
            .iter_mut()
            .filter_map(|(id, job)| job.take().map(|r| (id.clone(), r)))
            .collect();
        for (id, result) in exported {
            self.export_jobs.remove(&id);
            match result {
                Ok(Some(path)) => {
                    if let Some(doc) = sh.open_docs.iter_mut().find(|d| d.id == id) {
                        if doc.local_path.is_none()
                            && doc.remote_sub.is_none()
                            && !doc.id.starts_with("doc_")
                        {
                            doc.local_path = Some(path.to_string_lossy().into_owned());
                            // Schedule the current revision too if edits happened during the export.
                            self.save_errors.remove(&id);
                            self.save_due.insert(id.clone(), Instant::now());
                        }
                    }
                    sh.err(format!("Saved copy to {}", path.display()));
                }
                Ok(None) => {}
                Err(error) => sh.err(format!("Export failed: {error}")),
            }
        }
        for doc in &sh.open_docs {
            if doc.dirty
                && !doc.ai_writing
                && !doc.is_binary
                && !self.save_due.contains_key(&doc.id)
                && !self.save_jobs.contains_key(&doc.id)
                && !self.save_errors.contains_key(&doc.id)
            {
                self.save_due
                    .insert(doc.id.clone(), Instant::now() + Duration::from_millis(650));
            }
        }
        let mut finished = vec![];
        for (id, (content, job)) in &mut self.save_jobs {
            if let Some(result) = job.take() {
                finished.push((id.clone(), content.clone(), result));
            }
        }
        for (id, content, result) in finished {
            self.save_jobs.remove(&id);
            match result {
                Ok(_) => {
                    if let Some(doc) = sh.open_docs.iter_mut().find(|d| d.id == id) {
                        if doc.content == content {
                            doc.dirty = false;
                        }
                    }
                    self.save_errors.remove(&id);
                }
                Err(error) => {
                    self.save_errors.insert(id, error);
                }
            }
        }
        let ready: Vec<_> = self
            .save_due
            .iter()
            .filter(|(id, due)| **due <= Instant::now() && !self.save_jobs.contains_key(*id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ready {
            self.save_due.remove(&id);
            let Some(doc) = sh.open_docs.iter().find(|d| d.id == id).cloned() else {
                continue;
            };
            if !doc.dirty || doc.is_binary || doc.ai_writing {
                continue;
            }
            let content = doc.content.clone();
            let job = sh.spawn(move |api| {
                if is_office_doc(&doc.title) {
                    return Err("Open the original Office file and edit it in LibreOffice to preserve formatting.".into());
                }
                if let Some(path) = doc.local_path {
                    let bytes = if doc.spreadsheet && doc.title.to_lowercase().ends_with(".xlsx") {
                        api.csv_to_xlsx(&doc.content)?
                    } else if doc.flashcards {
                        api.text_to_apkg(&doc.content, &doc.title)?
                    } else {
                        doc.content.into_bytes()
                    };
                    atomic_write(std::path::Path::new(&path), &bytes)?;
                    Ok(json!({"ok": true}))
                } else if let (Some(sid), Some(sub)) = (doc.remote_session, doc.remote_sub) {
                    api.save_file(&sid, &sub, &doc.content)
                } else if doc.id.starts_with("doc_") {
                    api.put_json(
                        &format!("api/document/{}", doc.id),
                        json!({"content": doc.content}),
                    )
                } else {
                    Err(
                        "This document has no save location. Export it to keep your changes."
                            .into(),
                    )
                }
            });
            self.save_jobs.insert(id, (content, job));
        }
        if let Some(id) = self.close_after_save.clone() {
            if sh.open_docs.iter().any(|d| d.id == id && !d.dirty)
                && !self.save_jobs.contains_key(&id)
            {
                self.close_doc(sh, &id);
                self.close_after_save = None;
            }
        }
        if !self.save_due.is_empty() {
            sh.ctx.request_repaint_after(Duration::from_millis(150));
        }
    }

    fn html_preview(&mut self, ui: &mut Ui, sh: &mut Shared, idx: usize) {
        let id = sh.open_docs[idx].id.clone();
        ui.horizontal(|ui| {
            if widgets::button_ex(ui, &sh.palette, widgets::BtnKind::Secondary, true, true, "Open in browser").clicked() {
                let html = sh.open_docs[idx].content.clone();
                let job = sh.spawn(move |_| open_html_browser(&html).map(|_| PathBuf::new()));
                self.native_jobs.insert(format!("browser-{id}"), job);
            }
            ui.label(crate::theme::ty::mono("Browser preview", 10.5, crate::theme::W::Regular).color(sh.palette.ink3));
        });
        ui.add_space(12.0);
        ui.label("Open this document in your browser for its full layout and interactive content.");
    }

    fn binary_view(&mut self, ui: &mut Ui, sh: &mut Shared, idx: usize) {
        let (id, title, raw) = {
            let d = &sh.open_docs[idx];
            (d.id.clone(), d.title.clone(), d.raw_bytes.clone())
        };
        let lower = title.to_lowercase();
        let rich = is_office_doc(&title);
        if rich {
            ui.horizontal_wrapped(|ui| {
                if widgets::button_ex(ui, &sh.palette, widgets::BtnKind::Secondary, true, !self.native_jobs.contains_key(&id), if lower.ends_with(".xlsx") { "Edit in LibreOffice Calc" } else { "Edit in LibreOffice Writer" }).clicked() {
                    let bytes = raw.clone().unwrap_or_default();
                    let path = self.native_paths.get(&id).cloned();
                    let extension = if lower.ends_with(".xlsx") { "xlsx" } else if lower.ends_with(".docx") { "docx" } else { "doc" };
                    let mode = if lower.ends_with(".xlsx") { "--calc" } else { "--writer" };
                    self.native_jobs.insert(id.clone(), sh.spawn(move |_| {
                        let path = match path {
                            Some(path) => path,
                            None => {
                                let dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("joebro/document-edits").join(stamp().to_string());
                                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                                let path = dir.join(format!("document.{extension}"));
                                std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                                path
                            }
                        };
                        { let mut c = std::process::Command::new(crate::platform::libreoffice().ok_or("LibreOffice is not installed (needed to edit Word/Excel files)")?); crate::platform::hide_console(&mut c); c }.arg(mode).arg(&path)
                            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
                            .spawn().map_err(|e| format!("Could not start LibreOffice: {e}"))?;
                        Ok(path)
                    }));
                }
                if let Some(path) = self.native_paths.get(&id).cloned() {
                    if widgets::button_ex(ui, &sh.palette, widgets::BtnKind::Secondary, true, !self.raw_save_jobs.contains_key(&id), "Save back & refresh").clicked() {
                        let path = path.clone();
                        let doc = sh.open_docs[idx].clone();
                        sh.open_docs[idx].dirty = true;
                        self.raw_save_jobs.insert(id.clone(), sh.spawn(move |api| {
                            let revision = file_revision(&path);
                            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                            if let Some(local) = doc.local_path {
                                if let Some(original) = &doc.raw_bytes {
                                    let current = std::fs::read(&local).map_err(|e| e.to_string())?;
                                    if &current != original && current != bytes { return Err("The original document changed since opening. Your edited copy is retained; reopen the original before merging changes.".into()); }
                                }
                                atomic_write(std::path::Path::new(&local), &bytes)?;
                            } else if let (Some(sid), Some(sub)) = (doc.remote_session, doc.remote_sub) {
                                use base64::Engine;
                                if let Some(original) = &doc.raw_bytes {
                                    let current = api.workdir_raw(&sid, &sub)?;
                                    if &current != original && current != bytes { return Err("The original document changed since opening. Your edited copy is retained; reopen the original before merging changes.".into()); }
                                }
                                api.post_json(&format!("api/session/{sid}/workdir/save-raw"), json!({"sub":sub,"b64":base64::engine::general_purpose::STANDARD.encode(&bytes)}))?;
                            } else { return Err("Document has no save location".into()); }
                            Ok((bytes, revision))
                        }));
                    }
                    ui.label(crate::theme::ty::mono("Save in LibreOffice, then save back here.", 10.5, crate::theme::W::Regular).color(sh.palette.ink3));
                    ui.label(crate::theme::ty::mono(format!("Recovery copy: {}", path.display()), 10.5, crate::theme::W::Regular).color(sh.palette.ink3));
                }
            });
        }
        if raw.is_none() {
            ui.label("No document bytes available.");
            return;
        }
        if [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
            .iter()
            .any(|e| lower.ends_with(e))
        {
            self.zoom_controls(ui, sh.palette);
            ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::Image::from_bytes(format!("bytes://{id}"), raw.unwrap())
                            .fit_to_original_size(self.zoom),
                    );
                });
            return;
        }
        if !self.rendered.contains_key(&id) && !self.render_jobs.contains_key(&id) {
            let bytes = raw.unwrap();
            self.render_jobs.insert(
                id.clone(),
                sh.spawn(move |_| {
                    if lower.ends_with(".pdf") {
                        Ok(RenderOut::Pdf(bytes))
                    } else if lower.ends_with(".svg") {
                        render_svg_bytes(&bytes).map(RenderOut::Pages)
                    } else if rich {
                        office_to_pdf(
                            &bytes,
                            if lower.ends_with(".xlsx") {
                                "xlsx"
                            } else if lower.ends_with(".docx") {
                                "docx"
                            } else {
                                "doc"
                            },
                        )
                        .map(RenderOut::Pdf)
                    } else {
                        Err("No renderer available for this file type".into())
                    }
                }),
            );
        }
        self.pages_view(ui, sh, &id);
    }

    fn zoom_controls(&mut self, ui: &mut Ui, p: crate::theme::Palette) {
        if self.zoom <= 0.0 {
            self.zoom = 1.0;
        }
        ui.horizontal(|ui| {
            ui.label(crate::theme::ty::mono("Zoom", 10.5, crate::theme::W::Regular).color(p.ink3));
            ui.add(egui::Slider::new(&mut self.zoom, 0.25..=3.0).show_value(false));
            ui.label(format!("{:.0}%", self.zoom * 100.0));
            if widgets::button_ex(ui, &p, widgets::BtnKind::Secondary, true, true, "100%").clicked() {
                self.zoom = 1.0;
            }
        });
    }

    fn pages_view(&mut self, ui: &mut Ui, sh: &Shared, id: &str) {
        if let Some(Ok(Rendered::Pdf(viewer))) = self.rendered.get_mut(id) {
            viewer.ui(ui, sh, false, |_| {});
            return;
        }
        self.zoom_controls(ui, sh.palette);
        match self.rendered.get(id) {
            Some(Ok(Rendered::Pdf(_))) => {}
            Some(Ok(Rendered::Pages(pages))) => {
                ui.label(
                    RichText::new(format!("{} pages", pages.len()))
                        .small()
                        .weak(),
                );
                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (i, png) in pages.iter().enumerate() {
                            ui.label(crate::theme::ty::mono(format!("Page {}", i + 1), 10.5, crate::theme::W::Regular).color(sh.palette.ink3));
                            ui.add(
                                egui::Image::from_bytes(
                                    format!("bytes://{id}#{i}-{}", self.render_revision),
                                    png.clone(),
                                )
                                .fit_to_original_size(self.zoom),
                            );
                            ui.add_space(10.0);
                        }
                    });
            }
            Some(Err(error)) => {
                let error = error.clone();
                ui.label(format!("Preview unavailable: {error}"));
                if widgets::button_ex(ui, &sh.palette, widgets::BtnKind::Secondary, true, true, "Retry preview").clicked() {
                    self.rendered.remove(id);
                }
            }
            None => {
                ui.spinner();
                ui.label("Rendering document…");
            }
        }
    }

    fn drain_open(&mut self, sh: &mut Shared) {
        if self
            .last_native_poll
            .map(|t| t.elapsed() >= Duration::from_secs(1))
            .unwrap_or(true)
        {
            self.last_native_poll = Some(Instant::now());
            for (id, path) in &self.native_paths {
                if self.native_revisions.get(id).copied().flatten() != file_revision(path) {
                    if let Some(doc) = sh.open_docs.iter_mut().find(|d| &d.id == id) {
                        doc.dirty = true;
                    }
                }
            }
        }
        if !self.native_paths.is_empty() {
            sh.ctx.request_repaint_after(Duration::from_secs(1));
        }
        let finished: Vec<_> = self
            .render_jobs
            .iter_mut()
            .filter_map(|(id, job)| job.take().map(|r| (id.clone(), r)))
            .collect();
        for (id, result) in finished {
            self.render_jobs.remove(&id);
            self.render_revision = self.render_revision.wrapping_add(1);
            let rev = self.render_revision;
            let landed = result.map(|out| match out {
                RenderOut::Pages(pages) => Rendered::Pages(pages),
                RenderOut::Pdf(bytes) => Rendered::Pdf(Box::new(crate::pdfview::PdfViewer::new(
                    &sh.ctx,
                    &format!("doc-{id}-{rev}"),
                    bytes,
                ))),
            });
            self.rendered.insert(id, landed);
        }
        let finished: Vec<_> = self
            .native_jobs
            .iter_mut()
            .filter_map(|(id, job)| job.take().map(|r| (id.clone(), r)))
            .collect();
        for (id, result) in finished {
            self.native_jobs.remove(&id);
            match result {
                Ok(path) => {
                    if !path.as_os_str().is_empty() {
                        if let Some(doc) = sh.open_docs.iter_mut().find(|d| d.id == id) {
                            doc.dirty = true;
                        }
                        self.native_revisions
                            .insert(id.clone(), file_revision(&path));
                        self.native_paths.insert(id, path);
                    }
                }
                Err(e) => sh.err(e),
            }
        }
        let finished: Vec<_> = self
            .raw_save_jobs
            .iter_mut()
            .filter_map(|(id, job)| job.take().map(|r| (id.clone(), r)))
            .collect();
        for (id, result) in finished {
            self.raw_save_jobs.remove(&id);
            match result {
                Ok((bytes, revision)) => {
                    let current = self
                        .native_paths
                        .get(&id)
                        .and_then(|path| file_revision(path));
                    if let Some(doc) = sh.open_docs.iter_mut().find(|d| d.id == id) {
                        doc.raw_bytes = Some(bytes);
                        doc.dirty = current != revision;
                    }
                    self.native_revisions.insert(id.clone(), revision);
                    self.rendered.remove(&id);
                    self.render_jobs.remove(&id);
                    self.save_errors.remove(&id);
                }
                Err(e) => {
                    self.save_errors.insert(id, e);
                }
            }
        }
        let mut completed = vec![];
        for (i, (id, sid, sub, job)) in self.pending_text.iter_mut().enumerate() {
            if let Some(result) = job.take() {
                completed.push(i);
                match result {
                    Ok(v) => {
                        let Some(content) = v.get("current_content").and_then(Value::as_str) else {
                            sh.err("Open response did not include document content");
                            continue;
                        };
                        let lower = sub.to_lowercase();
                        if !sh.open_docs.iter().any(|d| d.id == *id) {
                            sh.open_docs.push(EditorDoc {
                                id: id.clone(),
                                title: sub.rsplit('/').next().unwrap_or(sub).into(),
                                language: v
                                    .get("language")
                                    .and_then(Value::as_str)
                                    .unwrap_or("text")
                                    .into(),
                                content: content.into(),
                                spreadsheet: lower.ends_with(".csv") || lower.ends_with(".xlsx"),
                                flashcards: lower.ends_with(".apkg"),
                                remote_sub: Some(sub.clone()),
                                remote_session: Some(sid.clone()),
                                ..Default::default()
                            });
                        }
                        sh.active_doc = Some(id.clone());
                    }
                    Err(error) => sh.err(format!("Could not open {sub}: {error}")),
                }
            }
        }
        for i in completed.into_iter().rev() {
            self.pending_text.remove(i);
        }
        let mut completed = vec![];
        for (i, (id, sid, sub, job)) in self.pending_bin.iter_mut().enumerate() {
            if let Some(result) = job.take() {
                completed.push(i);
                match result {
                    Ok(bytes) => {
                        if !sh.open_docs.iter().any(|d| d.id == *id) {
                            sh.open_docs.push(EditorDoc {
                                id: id.clone(),
                                title: sub.rsplit('/').next().unwrap_or(sub).into(),
                                is_binary: true,
                                raw_bytes: Some(bytes),
                                remote_session: Some(sid.clone()),
                                remote_sub: Some(sub.clone()),
                                ..Default::default()
                            });
                        }
                        sh.active_doc = Some(id.clone());
                    }
                    Err(error) => sh.err(format!("Could not open {sub}: {error}")),
                }
            }
        }
        for i in completed.into_iter().rev() {
            self.pending_bin.remove(i);
        }
    }
}

/// Move the TextEdit's selection to the next/previous match of `q`, wrapping
/// around, and scroll to it. Works in char indices (egui cursors are char-based).
fn apply_find(
    ctx: &egui::Context,
    id: egui::Id,
    content: &str,
    q: &str,
    from: &mut usize,
    prev: bool,
) {
    let hit = if prev {
        find_char(content, q, *from, true)
    } else {
        find_char(content, q, *from, false)
    };
    if let Some(start) = hit {
        let end = start + q.chars().count();
        *from = if prev { start.saturating_sub(1) } else { end };
        if let Some(mut st) = egui::text_edit::TextEditState::load(ctx, id) {
            let range = egui::text::CCursorRange::two(
                egui::text::CCursor::new(start),
                egui::text::CCursor::new(end),
            );
            st.cursor.set_char_range(Some(range));
            st.store(ctx, id);
        }
        ctx.memory_mut(|m| m.request_focus(id)); // focus so the view scrolls to it
    } else {
        *from = 0; // nothing ahead — wrap on the next press
    }
}

/// Case-insensitive char-index search, forward or backward, wrapping around.
fn find_char(hay: &str, needle: &str, from: usize, backward: bool) -> Option<usize> {
    let h: Vec<String> = hay
        .chars()
        .map(|c| c.to_lowercase().collect::<String>())
        .collect();
    let n: Vec<String> = needle
        .chars()
        .map(|c| c.to_lowercase().collect::<String>())
        .collect();
    if n.is_empty() || n.len() > h.len() {
        return None;
    }
    let last = h.len() - n.len();
    let matches = |i: usize| h[i..i + n.len()] == n[..];
    if backward {
        let start = from.min(last);
        (0..=start)
            .rev()
            .find(|&i| matches(i))
            .or_else(|| (start + 1..=last).rev().find(|&i| matches(i)))
    } else {
        let start = from.min(last + 1);
        (start..=last)
            .find(|&i| matches(i))
            .or_else(|| (0..start).find(|&i| matches(i)))
    }
}

/// Count non-overlapping case-insensitive matches (for the "N matches" label).
fn count_matches(hay: &str, needle: &str) -> usize {
    let h: Vec<String> = hay
        .chars()
        .map(|c| c.to_lowercase().collect::<String>())
        .collect();
    let n: Vec<String> = needle
        .chars()
        .map(|c| c.to_lowercase().collect::<String>())
        .collect();
    if n.is_empty() || n.len() > h.len() {
        return 0;
    }
    let mut c = 0;
    let mut i = 0;
    while i + n.len() <= h.len() {
        if h[i..i + n.len()] == n[..] {
            c += 1;
            i += n.len();
        } else {
            i += 1;
        }
    }
    c
}

type Card = (String, String, bool);

fn parse_cards(text: &str) -> Vec<Card> {
    let mut cards = vec![];
    let (mut front, mut back, mut learned, mut field): (Vec<String>, Vec<String>, bool, char) =
        (vec![], vec![], false, ' ');
    let flush = |front: &mut Vec<String>,
                 back: &mut Vec<String>,
                 learned: &mut bool,
                 cards: &mut Vec<Card>| {
        let f = front.join("\n").trim().to_string();
        let b = back.join("\n").trim().to_string();
        if !f.is_empty() || !b.is_empty() {
            cards.push((f, b, *learned));
        }
        front.clear();
        back.clear();
        *learned = false;
    };
    for line in text.lines() {
        let t = line.trim_start();
        let marker = t
            .split_once(':')
            .filter(|(tag, _)| matches!(tag.trim().to_ascii_uppercase().as_str(), "Q" | "A" | "S"));
        if let Some((tag, value)) = marker {
            match tag.trim().to_ascii_uppercase().as_str() {
                "Q" => {
                    flush(&mut front, &mut back, &mut learned, &mut cards);
                    front.push(value.trim().into());
                    field = 'q';
                }
                "A" => {
                    back = vec![value.trim().into()];
                    field = 'a';
                }
                _ => {
                    learned = value.trim().eq_ignore_ascii_case("learned");
                    field = ' ';
                }
            }
        } else if field == 'q' {
            front.push(line.to_string());
        } else if field == 'a' {
            back.push(line.to_string());
        }
    }
    flush(&mut front, &mut back, &mut learned, &mut cards);
    cards
}

fn serialize_cards(cards: &[Card]) -> String {
    let mut out = String::new();
    for (f, b, learned) in cards {
        out.push_str(&format!("Q: {f}\nA: {b}"));
        if *learned {
            out.push_str("\nS: learned");
        }
        out.push_str("\n\n");
    }
    out
}

/// Unique temp path fragment from the clock (no rand crate).
fn stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// RFC 4180 fields, including embedded newlines, commas and escaped quotes.
fn parse_csv(text: &str) -> R<Vec<Vec<String>>> {
    if text.is_empty() {
        return Ok(vec![]);
    }
    let mut rows = vec![];
    let mut row = vec![];
    let mut cell = String::new();
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    let (mut quoted, mut closed, mut ended_row) = (false, false, false);
    while let Some(c) = chars.next() {
        ended_row = false;
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() && !closed => quoted = true,
            ',' => {
                row.push(std::mem::take(&mut cell));
                closed = false;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                closed = false;
                ended_row = true;
            }
            _ if closed => return Err("unexpected character after a closing quote".into()),
            '"' => return Err("quote inside an unquoted field".into()),
            _ => cell.push(c),
        }
    }
    if quoted {
        return Err("unclosed quoted field".into());
    }
    if !ended_row {
        row.push(cell);
        rows.push(row);
    }
    Ok(rows)
}

fn serialize_csv(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|cell| {
                    if cell.contains([',', '"', '\n', '\r']) {
                        format!("\"{}\"", cell.replace('"', "\"\""))
                    } else {
                        cell.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\r\n")
        + if rows.is_empty() { "" } else { "\r\n" }
}

fn is_office_doc(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.ends_with(".doc") || lower.ends_with(".docx") || lower.ends_with(".xlsx")
}

fn column_name(mut index: usize) -> String {
    let mut name = String::new();
    loop {
        name.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    name
}

fn shuffle(queue: &mut [usize]) {
    let mut seed = stamp() as u64 | 1;
    for i in (1..queue.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        queue.swap(i, seed as usize % (i + 1));
    }
}

fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> R<()> {
    let temp = path.with_extension(format!("joebro-{}", stamp()));
    let result = (|| {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        if let Ok(metadata) = std::fs::metadata(path) {
            file.set_permissions(metadata.permissions())
                .map_err(|e| e.to_string())?;
        }
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

type FileRevision = (u64, std::time::SystemTime);
fn file_revision(path: &std::path::Path) -> Option<FileRevision> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()?))
}

struct RenderDir(PathBuf);
impl RenderDir {
    fn new() -> R<Self> {
        let dir =
            std::env::temp_dir().join(format!("joebro-render-{}-{}", std::process::id(), stamp()));
        std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
        Ok(Self(dir))
    }
}
impl Drop for RenderDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_renderer(command: &mut std::process::Command, label: &str, dir: &std::path::Path) -> R<()> {
    let log_path = dir.join("renderer.log");
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let mut child = command
        .stdout(std::process::Stdio::null())
        .stderr(log)
        .spawn()
        .map_err(|e| {
            format!("{label} could not start ({e}). Install {label} to render this format.")
        })?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if status.success() {
                return Ok(());
            }
            let details = std::fs::read_to_string(&log_path).unwrap_or_default();
            return Err(format!(
                "{label} failed: {}",
                details.chars().take(600).collect::<String>()
            ));
        }
        if start.elapsed() > Duration::from_secs(90) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{label} timed out after 90 seconds"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn render_svg_bytes(bytes: &[u8]) -> R<Vec<Vec<u8>>> {
    let dir = RenderDir::new()?;
    let src = dir.0.join("in.svg");
    let out = dir.0.join("out.png");
    std::fs::write(&src, bytes).map_err(|e| e.to_string())?;
    run_renderer(
        crate::platform::hide_console(&mut std::process::Command::new("rsvg-convert"))
            .args(["-z", "2"])
            .arg(&src)
            .arg("-o")
            .arg(&out),
        "rsvg-convert",
        &dir.0,
    )?;
    Ok(vec![std::fs::read(out).map_err(|e| e.to_string())?])
}

/// Wrap chars `a..b` of `text` in `marker`, or unwrap if already wrapped.
/// Returns the new text and the new selection (char indices).
fn toggle_wrap(text: &str, a: usize, b: usize, marker: &str) -> (String, (usize, usize)) {
    let chars: Vec<char> = text.chars().collect();
    let m: Vec<char> = marker.chars().collect();
    let k = m.len();
    let (a, b) = (a.min(chars.len()), b.min(chars.len()));
    let before = a >= k && chars[a - k..a] == m[..];
    let after = b + k <= chars.len() && chars[b..b + k] == m[..];
    let mut out: Vec<char> = Vec::with_capacity(chars.len() + 2 * k);
    if before && after {
        out.extend_from_slice(&chars[..a - k]);
        out.extend_from_slice(&chars[a..b]);
        out.extend_from_slice(&chars[b + k..]);
        (out.into_iter().collect(), (a - k, b - k))
    } else {
        out.extend_from_slice(&chars[..a]);
        out.extend_from_slice(&m);
        out.extend_from_slice(&chars[a..b]);
        out.extend_from_slice(&m);
        out.extend_from_slice(&chars[b..]);
        (out.into_iter().collect(), (a + k, b + k))
    }
}

/// What a background render produces: page images, or a PDF for the viewer.
enum RenderOut {
    Pages(Vec<Vec<u8>>),
    Pdf(Vec<u8>),
}

enum Rendered {
    Pages(Vec<Vec<u8>>),
    Pdf(Box<crate::pdfview::PdfViewer>),
}

/// Convert an Office/HTML document to PDF with a throwaway LibreOffice profile.
pub(crate) fn office_to_pdf(bytes: &[u8], extension: &str) -> R<Vec<u8>> {
    let dir = RenderDir::new()?;
    let src = dir.0.join(format!("document.{extension}"));
    std::fs::write(&src, bytes).map_err(|e| e.to_string())?;
    // A separate profile permits rendering while the user's Writer is open.
    let profile = format!(
        "-env:UserInstallation={}",
        crate::platform::file_url(&dir.0.join("profile"))
    );
    let office = crate::platform::libreoffice()
        .ok_or("LibreOffice is not installed (needed to preview Office files)")?;
    run_renderer(
        crate::platform::hide_console(&mut std::process::Command::new(office))
            .arg(profile)
            .args(["--headless", "--convert-to", "pdf", "--outdir"])
            .arg(&dir.0)
            .arg(&src),
        "libreoffice",
        &dir.0,
    )?;
    std::fs::read(dir.0.join("document.pdf"))
        .map_err(|e| format!("LibreOffice did not produce a PDF: {e}"))
}

pub(crate) fn open_html_browser(html: &str) -> R<()> {
    let dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("joebro/html-previews");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.html", stamp()));
    std::fs::write(&path, html).map_err(|e| e.to_string())?;
    crate::platform::open_path(&path).map_err(|e| format!("Could not open browser: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renderer_reports_stderr_and_cleans_temporary_files() {
        let path;
        {
            let dir = RenderDir::new().unwrap();
            path = dir.0.clone();
            let error = run_renderer(
                std::process::Command::new("sh")
                    .args(["-c", "echo useful-render-error >&2; exit 2"]),
                "test renderer",
                &dir.0,
            )
            .unwrap_err();
            assert!(error.contains("useful-render-error"));
        }
        assert!(!path.exists());
    }
    #[test]
    fn csv_round_trip_preserves_quotes_newlines_unicode_and_spaces() {
        let rows = vec![
            vec![
                "a,b".into(),
                "a\"b".into(),
                "first\r\nsecond".into(),
                "  café  ".into(),
                "".into(),
            ],
            vec!["".into()],
        ];
        assert_eq!(parse_csv(&serialize_csv(&rows)).unwrap(), rows);
        assert_eq!(
            parse_csv("\u{feff}one,two\r\n\"three,four\",\"five\"\"six\"\r\n").unwrap(),
            vec![vec!["one", "two"], vec!["three,four", "five\"six"]]
        );
    }
    #[test]
    fn invalid_csv_reports_error_instead_of_rewriting_data() {
        assert!(parse_csv("\"unclosed").is_err());
        assert!(parse_csv("\"closed\"extra").is_err());
    }
    #[test]
    fn ctrl_b_wraps_and_unwraps_selection() {
        let (t, sel) = toggle_wrap("make this bold", 5, 9, "**");
        assert_eq!(t, "make **this** bold");
        assert_eq!(sel, (7, 11));
        let (t2, sel2) = toggle_wrap(&t, 7, 11, "**");
        assert_eq!(t2, "make this bold");
        assert_eq!(sel2, (5, 9));
    }

    #[test]
    fn find_preserves_original_unicode_cursor_offsets() {
        assert_eq!(find_char("İ abc Café", "café", 0, false), Some(6));
        assert_eq!(find_char("abc abc", "abc", 4, true), Some(4));
        assert_eq!(count_matches("Café café", "CAFÉ"), 2);
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(701), "ZZ");
    }
    #[test]
    fn card_roundtrip_keeps_multiline_and_progress() {
        let cards = vec![(
            "Question\ncontinued".into(),
            "Answer\ncontinued".into(),
            true,
        )];
        assert_eq!(parse_cards(&serialize_cards(&cards)), cards);
    }
}
