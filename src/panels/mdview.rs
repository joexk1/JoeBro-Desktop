//! Markdown reader with real inline pictures (Mac `MarkdownText` parity).
//!
//! egui_commonmark can't fetch http(s) images (no loader feature) and doesn't
//! know the backend's relative paths, so standalone `![alt](url)` lines are cut
//! out and drawn here. Images load lazily on worker threads (absolute URLs
//! over a bare client -- the backend token is never sent to third parties;
//! relative paths over the authenticated client), decode off-thread, are cached
//! as textures, show a size-reserving placeholder while loading and an error
//! chip on failure.
use crate::api::R;
use crate::state::{Job, Shared};
use crate::widgets;
use eframe::egui::{self, Color32, RichText, Sense, Ui, Vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::collections::HashMap;

enum Img {
    Loading(Job<R<egui::ColorImage>>),
    Ready(egui::TextureHandle),
    Failed(String),
}

#[derive(Default)]
pub struct MdView {
    md: CommonMarkCache,
    imgs: HashMap<String, Img>,
}

#[derive(Debug, PartialEq)]
enum Seg {
    Text(String),
    Image { alt: String, url: String },
}

/// Parse `![alt](url)` occupying a whole trimmed line.
fn standalone_image(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    let rest = t.strip_prefix("![")?;
    let close = rest.find("](")?;
    let alt = &rest[..close];
    let url = rest[close + 2..].strip_suffix(')')?;
    let url = url.split_whitespace().next()?; // drop an optional "title"
    if url.is_empty() || url.starts_with("data:") {
        return None;
    }
    Some((alt.to_string(), url.to_string()))
}

/// Inline images -> links, so the markdown viewer never tries to load them.
fn demote_inline_images(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(i) = rest.find("![") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        
        rest = after; // "[alt](url)" stays as an ordinary link
    }
    out.push_str(rest);
    out
}

fn split(src: &str) -> Vec<Seg> {
    let mut segs = vec![];
    let mut buf = String::new();
    let mut fence = false;
    let flush = |buf: &mut String, segs: &mut Vec<Seg>| {
        if !buf.trim().is_empty() {
            segs.push(Seg::Text(std::mem::take(buf)));
        } else {
            buf.clear();
        }
    };
    for line in src.lines() {
        if line.trim_start().starts_with("```") {
            fence = !fence;
            buf.push_str(line);
            buf.push('\n');
            continue;
        }
        if !fence {
            if let Some((alt, url)) = standalone_image(line) {
                flush(&mut buf, &mut segs);
                segs.push(Seg::Image { alt, url });
                continue;
            }
            if line.contains("![") {
                buf.push_str(&demote_inline_images(line));
                buf.push('\n');
                continue;
            }
        }
        buf.push_str(line);
        buf.push('\n');
    }
    flush(&mut buf, &mut segs);
    segs
}

fn fetch(api: &crate::api::Api, url: &str) -> R<egui::ColorImage> {
    let bytes = if url.starts_with("http://") || url.starts_with("https://") {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent("JoeBro")
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(url).send().map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }
        if resp.content_length().map_or(false, |n| n > 25_000_000) {
            return Err("Image too large".into());
        }
        resp.bytes().map_err(|e| e.to_string())?.to_vec()
    } else {
        api.get_bytes(url.trim_start_matches('/'), &[])?
    };
    let img = image::load_from_memory(&bytes).map_err(|_| "Unsupported image format".to_string())?;
    let img = if img.width().max(img.height()) > 2048 {
        img.resize(2048, 2048, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

/// Chat-style line breaks: a single newline inside a paragraph is a visible break (CommonMark would
/// fold it into a space and run list-less agent text together). Fenced code, blank lines, list items,
/// headings, quotes and table rows are left alone.
pub fn soft_to_hard(src: &str) -> String {
    let norm = src.replace("\r\n", "\n");
    let lines: Vec<&str> = norm.lines().collect();
    let mut out = String::with_capacity(src.len() + 16);
    let mut fence = false;
    for (i, l) in lines.iter().enumerate() {
        out.push_str(l);
        let next = lines.get(i + 1).copied();
        if l.trim_start().starts_with("```") {
            fence = !fence;
        }
        if let Some(n) = next {
            let t = l.trim();
            let structural = |x: &str| {
                let x = x.trim_start();
                x.is_empty() || x.starts_with("```") || x.starts_with('#') || x.starts_with('>') || x.starts_with('|') || x.starts_with("- ") || x.starts_with("* ") || x.starts_with("+ ")
                    || x.split_once(". ").map_or(false, |(a, _)| !a.is_empty() && a.chars().all(|c| c.is_ascii_digit()))
            };
            if !fence && !structural(l) && !t.is_empty() && !structural(n) && !l.ends_with("  ") {
                out.push_str("  ");
            }
            out.push('\n');
        }
    }
    out
}

/// The coding-agent feed collapses all whitespace (`jb_coding._clip`), so a reply arrives as one line:
/// `**Fix:** - A - B 1. x 2. y`. Rebuild the obvious structure (list items, numbered steps, bold
/// run-in headings) so it renders as the author wrote it. Text that already has newlines is returned as is.
pub fn restore_structure(src: &str) -> String {
    if src.contains('\n') || src.len() < 60 {
        return src.to_string();
    }
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() + 32);
    let mut fence = false;
    let mut i = 0;
    let mut next_num = 1u32;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' && chars[i..].starts_with(&['`', '`', '`']) {
            fence = !fence;
        }
        if !fence && c == ' ' && i > 0 {
            let prev = chars[i - 1];
            let rest: String = chars[i + 1..].iter().take(6).collect();
            let list = rest.starts_with("- ") && rest.chars().nth(2).map_or(false, |n| n == '*' || n == '`' || n.is_uppercase() || n.is_ascii_digit()) && ".:!?)`*\"”".contains(prev);
            let num_marker = format!("{next_num}. ");
            let numbered = rest.starts_with(&num_marker) && (next_num > 1 || ".:!?)`*".contains(prev)) && rest.chars().nth(num_marker.len()).map_or(false, |n| n != ' ');
            if list {
                out.push_str("\n- ");
                i += 3;
                continue;
            }
            if numbered {
                out.push('\n');
                out.push_str(&num_marker);
                i += 1 + num_marker.chars().count();
                next_num += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    // A list that follows prose needs a blank line before it to be a list in CommonMark.
    out.replace("\n- ", "\n\n- ").replace("\n\n\n", "\n\n").replace("\n\n- ", "\n- ").replacen("\n- ", "\n\n- ", 1)
}

impl MdView {
    pub fn show(&mut self, ui: &mut Ui, sh: &Shared, source: &str) {
        let src = widgets::preprocess_md(&soft_to_hard(source));
        for (i, seg) in split(&src).into_iter().enumerate() {
            match seg {
                Seg::Text(t) => {
                    ui.push_id(("mdseg", i), |ui| {
                        ui.scope(|ui| {
                            prose_style(ui, &sh.palette, false);
                            CommonMarkViewer::new().show(ui, &mut self.md, &t);
                        });
                    });
                }
                Seg::Image { alt, url } => self.image(ui, sh, &alt, &url),
            }
        }
    }

    fn image(&mut self, ui: &mut Ui, sh: &Shared, alt: &str, url: &str) {
        let p = sh.palette;
        if !self.imgs.contains_key(url) {
            let u = url.to_string();
            self.imgs.insert(url.to_string(), Img::Loading(sh.spawn(move |api| fetch(api, &u))));
        }
        // Resolve a finished load into a texture.
        let done = match self.imgs.get_mut(url) {
            Some(Img::Loading(job)) => job.take(),
            _ => None,
        };
        if let Some(res) = done {
            let st = match res {
                Ok(ci) => Img::Ready(ui.ctx().load_texture(format!("md-img:{url}"), ci, Default::default())),
                Err(e) => Img::Failed(e),
            };
            self.imgs.insert(url.to_string(), st);
        }
        let w = ui.available_width().max(40.0);
        ui.add_space(4.0);
        match self.imgs.get(url) {
            Some(Img::Ready(tex)) => {
                let sz = tex.size_vec2();
                let scale = (w / sz.x).min(300.0 / sz.y).min(1.0).max(0.01);
                let size = sz * scale;
                ui.vertical_centered(|ui| {
                    let r = ui.add(
                        egui::Image::new((tex.id(), size)).rounding(12.0).sense(Sense::click()),
                    );
                    if r.clicked() && url.starts_with("http") {
                        let _ = open::that(url);
                    }
                    r.on_hover_text(if alt.is_empty() { url } else { alt });
                });
            }
            Some(Img::Failed(why)) => {
                let why = why.clone();
                let mut retry = false;
                let host = url.split('/').nth(2).unwrap_or(url).to_string();
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(16.0, 20.0), Sense::hover());
                    widgets::paint_glyph(ui, r.center(), "\u{f071}", 11.0, p.warning);
                    ui.label(
                        RichText::new(format!("Couldn't load image ({why})"))
                            .size(11.5)
                            .color(p.ink_dim),
                    )
                    .on_hover_text(url);
                    if !alt.is_empty() {
                        ui.label(RichText::new(format!("{alt} · {host}")).size(11.0).color(p.ink_faint));
                    }
                    if widgets::btn(ui, sh, "Retry").clicked() {
                        retry = true;
                    }
                });
                if retry {
                    self.imgs.remove(url);
                }
            }
            _ => {
                // Reserve the final-ish footprint so the page doesn't jump.
                let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 160.0), Sense::hover());
                ui.painter().rect_filled(rect, 12.0, Color32::from_gray(128).gamma_multiply(0.12));
                let c = rect.center();
                ui.put(
                    egui::Rect::from_center_size(c, Vec2::splat(18.0)),
                    egui::Spinner::new().size(16.0).color(p.accent_hi),
                );
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
            }
        }
        ui.add_space(4.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_standalone_images_out_of_text() {
        let s = split("# T\n\n![a](https://x/y.png)\n\ntext ![i](u) more\n\n```\n![n](no)\n```\n");
        assert!(matches!(&s[1], Seg::Image { url, .. } if url == "https://x/y.png"));
        assert_eq!(s.len(), 3);
        if let Seg::Text(t) = &s[2] {
            assert!(t.contains("[i](u)") && !t.contains("![i]") && t.contains("![n](no)"));
        } else {
            panic!()
        }
    }

    #[test]
    fn single_newlines_become_breaks() {
        assert_eq!(soft_to_hard("a\nb\n\n- x\n- y"), "a  \nb\n\n- x\n- y");
        assert_eq!(soft_to_hard("```\na\nb\n```"), "```\na\nb\n```");
    }

    #[test]
    fn flattened_feed_text_regains_list_structure() {
        let flat = "Done. **Fix:** - A new in-app picker, used by Settings. - `set_wallpaper` now sets the kind. **Left** 1. Save dialogs still block 2. Scenes fall back to plain";
        let r = restore_structure(flat);
        assert!(r.contains("\n- A new in-app picker"), "{r}");
        assert!(r.contains("\n- `set_wallpaper`"), "{r}");
        assert!(r.contains("\n1. Save dialogs") && r.contains("\n2. Scenes"), "{r}");
        // already structured text and plain hyphens are untouched
        assert_eq!(restore_structure("a - b\nline two is here and long enough to count as text for the check"), "a - b\nline two is here and long enough to count as text for the check");
        let plain = "Well - that was fine, and a long enough sentence that nothing here is a list at all, honestly.";
        assert_eq!(restore_structure(plain), plain);
    }

    #[test]
    fn relative_and_titled_urls() {
        assert_eq!(standalone_image("![](/api/x.png \"t\")"), Some(("".into(), "/api/x.png".into())));
        assert_eq!(standalone_image("![a](data:image/png;base64,AAA)"), None);
    }
}

/// Mac `MarkdownText` look for egui_commonmark: Geist body (14 chat / 15 document), Geist Mono 12.5 code,
/// accent links, `raised` inline-code wash, 10 pt block spacing. Apply inside a `ui.scope`.
pub fn prose_style(ui: &mut Ui, p: &crate::theme::Palette, document: bool) {
    use crate::theme::{jb, mono, W};
    use egui::TextStyle;
    let (body, head) = if document { (15.0, 24.0) } else { (13.5, 19.0) };
    let body_h = ui.fonts(|f| f.row_height(&jb(body, W::Regular)));
    let style = ui.style_mut();
    style.text_styles.insert(TextStyle::Body, jb(body, W::Regular));
    style.text_styles.insert(TextStyle::Heading, jb(head, W::Semibold));
    style.text_styles.insert(TextStyle::Monospace, mono(12.5, W::Regular));
    style.spacing.item_spacing.y = if document { 14.0 } else { 10.0 };
    // Row min-height would otherwise exceed the text height and leave list bullets above their line.
    style.spacing.interact_size.y = body_h;
    let v = &mut style.visuals;
    v.override_text_color = None;
    v.widgets.noninteractive.fg_stroke.color = p.ink;
    v.widgets.active.fg_stroke.color = p.ink; // `strong` text
    v.code_bg_color = p.raised;
    v.hyperlink_color = p.accent;
}
