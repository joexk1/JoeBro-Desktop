//! Regression tests for assistant markdown layout: long prose must wrap inside the column and
//! paragraphs / lists must stay on separate rows (chat and coding-agent bubbles share this path).
use crate::state::Shared;
use eframe::egui::{self, epaint::Shape, Pos2, Rect};
use std::sync::Arc;

/// NB: the column is an `allocate_ui` child, because `set_max_width` cannot shrink a panel ui below its min size.
/// Renders `content` in a column `width` wide; returns (rightmost text x, distinct text rows).
fn layout(content: &str, width: f32, agent_path: bool) -> (f32, usize) {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let api = Arc::new(crate::api::Api::new("http://127.0.0.1:9"));
    let sh = Shared::new(api, ctx.clone(), true, true);
    crate::theme::apply(&ctx, sh.palette);
    let mut cache = egui_commonmark::CommonMarkCache::default();
    let mut md = crate::panels::mdview::MdView::default();
    let mut out = None;
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width + 200.0, 4000.0))),
            ..Default::default()
        };
        out = Some(ctx.run(input, |ctx| {
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
                ui.allocate_ui(egui::vec2(width, 4000.0), |ui| {
                if agent_path {
                    md.show(ui, &sh, content);
                } else {
                    crate::panels::chat::render_md(ui, &sh, &mut cache, content);
                }
                });
            });
        }));
    }
    let mut right: f32 = 0.0;
    let mut rows: Vec<i32> = vec![];
    for cs in &out.unwrap().shapes {
        collect(&cs.shape, &mut right, &mut rows);
    }
    rows.sort();
    rows.dedup();
    (right, rows.len())
}

fn collect(s: &Shape, right: &mut f32, rows: &mut Vec<i32>) {
    match s {
        Shape::Text(t) => {
            *right = right.max(t.pos.x + t.galley.rect.width());
            for r in &t.galley.rows {
                rows.push((t.pos.y + r.rect.center().y).round() as i32);
            }
        }
        Shape::Vec(v) => v.iter().for_each(|s| collect(s, right, rows)),
        _ => {}
    }
}

const SAMPLE: &str = "First paragraph with a long run of words that has to wrap because the column is narrow and keeps going and going until it is clearly longer than one row of text could ever hold.\n\nSecond paragraph.\n\n- item one is also quite long so that it needs to wrap onto a second row inside the list\n- item two\n- item three\n\n1. numbered one\n2. numbered two\n\n**Bold lead.** Trailing prose.\nSoft broken line.";

#[test]
fn chat_markdown_wraps_and_keeps_structure() {
    let (right, rows) = layout(SAMPLE, 420.0, false);
    assert!(right <= 421.0, "text ran past the column: {right}");
    assert!(rows >= 11, "paragraphs/lists collapsed: {rows} rows");
}

#[test]
fn agent_markdown_wraps_and_keeps_structure() {
    let (right, rows) = layout(SAMPLE, 420.0, true);
    assert!(right <= 421.0, "text ran past the column: {right}");
    assert!(rows >= 11, "paragraphs/lists collapsed: {rows} rows");
}


#[test]
fn list_bullet_is_level_with_its_text() {
    // Row min-height used to exceed the text height, leaving the bullet ~5 px above the line.
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let api = Arc::new(crate::api::Api::new("http://127.0.0.1:9"));
    let sh = Shared::new(api, ctx.clone(), true, true);
    crate::theme::apply(&ctx, sh.palette);
    let mut cache = egui_commonmark::CommonMarkCache::default();
    let mut out = None;
    for _ in 0..2 {
        out = Some(ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 400.0))), ..Default::default() }, |ctx| {
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
                crate::panels::chat::render_md(ui, &sh, &mut cache, "- Hxample item");
            });
        }));
    }
    let (mut cy, mut mid) = (None, None);
    fn walk(s: &Shape, cy: &mut Option<f32>, mid: &mut Option<f32>) {
        match s {
            Shape::Circle(c) => *cy = Some(c.center.y),
            Shape::Text(t) => {
                if let Some(g) = t.galley.rows.first().and_then(|r| r.glyphs.iter().find(|g| g.chr == 'H')) {
                    *mid = Some(t.pos.y + g.pos.y - 5.0); // baseline minus half the cap height
                }
            }
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, cy, mid)),
            _ => {}
        }
    }
    for cs in &out.unwrap().shapes {
        walk(&cs.shape, &mut cy, &mut mid);
    }
    let (cy, mid) = (cy.expect("bullet"), mid.expect("text"));
    assert!((cy - mid).abs() < 2.0, "bullet at {cy}, text middle at {mid}");
}
