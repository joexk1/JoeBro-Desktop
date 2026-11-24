//! App backdrop under the panels — the Mac's `WallpaperBackground`:
//! palette (dithered ground) -> photo -> animated scene (user video loops via
//! ffmpeg; the bundled ASCII/HTML scenes fall back to the palette).
use crate::config;
use eframe::egui::{self, Color32, ColorImage, Context, Pos2, Rect, TextureHandle};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

/// UV rect that crops a texture of `img` size to fill `target` (scaledToFill).
pub fn fill_uv(img: egui::Vec2, target: egui::Vec2) -> Rect {
    let (ia, ta) = (img.x / img.y.max(1.0), target.x / target.y.max(1.0));
    if ia > ta {
        let w = ta / ia;
        Rect::from_min_max(Pos2::new((1.0 - w) / 2.0, 0.0), Pos2::new((1.0 + w) / 2.0, 1.0))
    } else {
        let h = ia / ta;
        Rect::from_min_max(Pos2::new(0.0, (1.0 - h) / 2.0), Pos2::new(1.0, (1.0 + h) / 2.0))
    }
}

/// Bundled ASCII scene names (the Mac's `JBBackdropLibrary.bundledScenes`).
pub const BUNDLED_SCENES: &[&str] = &[
    "alpine-dawn", "amer-fort", "aurora-fjord", "black-prism", "cloud-sea", "command-city",
    "deep-reef", "desert-night", "earthrise", "edge-orbit", "gangaur-ghat", "green-storm",
    "jal-mahal", "jungle-falls", "kite-festival", "kyoto-dusk", "lake-pichola", "lantern-lake",
    "lava-wall", "marine-drive", "misty-forest", "night-coast", "night-highway", "ocean-sunset",
    "planet-scale", "prism-city", "salt-flat", "saturn-rise", "snow-train", "storm-plains",
    "taj-dawn", "tokyo-rain", "varanasi-ghats", "volcano-night",
];
pub const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];
pub const VIDEO_EXT: &[&str] = &["mp4", "mov", "m4v", "webm", "mkv"];
const USER_EXT: &[&str] = &["mp4", "mov", "m4v", "webm", "mkv"];

/// Active backdrop kind: `palette`, `photo` or `scene`. Unset + a wallpaper = photo (Mac rule).
pub fn kind() -> &'static str {
    let c = config::get();
    match c.backdrop.as_str() {
        "photo" => "photo",
        "scene" => "scene",
        "palette" => "palette",
        _ if c.wallpaper.is_some() => "photo",
        _ => "palette",
    }
}

/// `~/.config/joebro[-personal]/Backdrops` (created on demand).
pub fn folder() -> PathBuf {
    let d = config::dir().join("Backdrops");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn user_backdrops() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(folder())
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| USER_EXT.contains(&e.to_lowercase().as_str())))
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    v
}

/// "alpine-dawn" -> "Alpine Dawn"; a user file -> its stem.
pub fn display_name(id: &str) -> String {
    if id.contains('/') {
        return Path::new(id).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    }
    id.split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Copy a video into the Backdrops folder; returns the scene id (its path).
pub fn import_video(src: &Path) -> Result<String, String> {
    let name = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "backdrop".into());
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
    let dir = folder();
    let mut dest = dir.join(format!("{name}.{ext}"));
    let mut n = 2;
    while dest.exists() {
        dest = dir.join(format!("{name} {n}.{ext}"));
        n += 1;
    }
    std::fs::copy(src, &dest).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().into_owned())
}

/// Copy the bundled backdrop skill into the Claude Code and Codex skill folders.
pub fn install_skill() -> Result<Vec<String>, String> {
    const FILES: [(&str, &str); 2] = [
        ("SKILL.md", include_str!("../assets/BackdropSkill/SKILL.md")),
        ("template.html", include_str!("../assets/BackdropSkill/template.html")),
    ];
    let home = dirs::home_dir().ok_or("No home folder")?;
    let mut done = vec![];
    for root in [".claude/skills/joebro-backdrop", ".codex/skills/joebro-backdrop"] {
        let dest = home.join(root);
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        for (n, body) in FILES {
            std::fs::write(dest.join(n), body).map_err(|e| e.to_string())?;
        }
        done.push(format!("~/{root}"));
    }
    Ok(done)
}

const VW: usize = 640;
const VH: usize = 360;

/// A looping video decoded by an `ffmpeg` child process into a small RGBA texture.
struct VideoPlayer {
    path: String,
    rx: Receiver<ColorImage>,
    child: std::process::Child,
    tex: Option<TextureHandle>,
}

impl VideoPlayer {
    fn start(path: &str, ctx: &Context) -> Option<VideoPlayer> {
        use std::io::Read;
        use std::process::{Command, Stdio};
        let mut child = crate::platform::hide_console(&mut Command::new(crate::platform::which("ffmpeg").unwrap_or_else(|| "ffmpeg".into())))
            .args(["-loglevel", "quiet", "-stream_loop", "-1", "-re", "-i", path, "-an", "-vf"])
            .arg(format!("fps=24,scale={VW}:{VH}:force_original_aspect_ratio=increase,crop={VW}:{VH}"))
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .ok()?;
        let mut out = child.stdout.take()?;
        let (tx, rx) = std::sync::mpsc::sync_channel::<ColorImage>(2);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; VW * VH * 4];
            while out.read_exact(&mut buf).is_ok() {
                let img = ColorImage::from_rgba_unmultiplied([VW, VH], &buf);
                if tx.send(img).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Some(VideoPlayer { path: path.to_string(), rx, child, tex: None })
    }
    fn frame(&mut self, ctx: &Context) -> Option<&TextureHandle> {
        let mut last = None;
        while let Ok(img) = self.rx.try_recv() {
            last = Some(img);
        }
        if let Some(img) = last {
            match &mut self.tex {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => self.tex = Some(ctx.load_texture("backdrop-video", img, egui::TextureOptions::LINEAR)),
            }
        }
        self.tex.as_ref()
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
pub struct Backdrop {
    video: Option<VideoPlayer>,
    /// A scene whose ffmpeg start failed (don't respawn every frame).
    video_failed: Option<String>,
    path: Option<String>,
    tex: Option<TextureHandle>,
    loading: Option<Receiver<Option<ColorImage>>>,
    /// Last load error, shown in Settings.
    pub error: Option<String>,
}

impl Backdrop {
    /// Texture + pixel size of the current wallpaper, once decoded.
    pub fn wallpaper(&self) -> Option<(&TextureHandle, egui::Vec2)> {
        self.tex.as_ref().map(|t| (t, t.size_vec2()))
    }

    /// Keep the texture in step with `config.wallpaper` (decoded off-thread).
    pub fn sync(&mut self, ctx: &Context) {
        let want = config::get().wallpaper;
        if want != self.path {
            self.path = want.clone();
            self.tex = None;
            self.loading = None;
            self.error = None;
            if let Some(p) = want {
                let (tx, rx) = channel();
                self.loading = Some(rx);
                let ctx2 = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(decode(Path::new(&p)));
                    ctx2.request_repaint();
                });
            }
        }
        if let Some(rx) = &self.loading {
            if let Ok(img) = rx.try_recv() {
                self.loading = None;
                match img {
                    Some(img) => self.tex = Some(ctx.load_texture("wallpaper", img, egui::TextureOptions::LINEAR)),
                    None => self.error = Some("Couldn't read that image.".into()),
                }
            }
        }
    }

    /// Paint the backdrop at the very bottom of the background layer. Call
    /// before any panel paints its glass.
    pub fn paint(&mut self, ctx: &Context, dark: bool) {
        self.sync(ctx);
        let rect = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::background());
        let kind = kind();
        // Animated: a user video loop (needs ffmpeg). ASCII/HTML scenes fall back to the palette.
        let scene = config::get().scene;
        if kind == "scene" && scene.contains('/') && Path::new(&scene).exists() {
            if self.video.as_ref().map(|v| v.path.as_str()) != Some(scene.as_str()) && self.video_failed.as_deref() != Some(scene.as_str()) {
                self.video = VideoPlayer::start(&scene, ctx);
                if self.video.is_none() {
                    self.video_failed = Some(scene.clone());
                    self.error = Some("Couldn't play that video (is ffmpeg installed?).".into());
                }
            }
            if let Some(v) = &mut self.video {
                if let Some(tex) = v.frame(ctx) {
                    painter.image(tex.id(), rect, fill_uv(egui::vec2(VW as f32, VH as f32), rect.size()), Color32::WHITE);
                    let a = if dark { 97 } else { 77 }; // ground at 0.38 / 0.30
                    painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(if dark { 0 } else { 255 }, if dark { 0 } else { 255 }, if dark { 0 } else { 255 }, a));
                    ctx.request_repaint_after(std::time::Duration::from_millis(40));
                    return;
                }
            }
        } else {
            self.video = None;
        }
        let wall = if kind == "photo" { self.wallpaper() } else { None };
        if let Some((tex, size)) = wall {
            painter.image(tex.id(), rect, fill_uv(size, rect.size()), Color32::WHITE);
            // Tone toward the app ground (dark 0.38 / light 0.30) so ink always reads.
            let g = crate::theme::Palette::for_state(true, dark).ground;
            painter.rect_filled(rect, 0.0, crate::theme::with_alpha(g, if dark { 0.38 } else { 0.30 }));
            return;
        }
        // No picture: the palette backdrop (flat ground + a dithered accent glow).
        let pal = crate::theme::Palette::for_state(true, dark);
        crate::ascii::dither_backdrop(ctx, &pal, rect);
    }
}

/// Decode and downscale (max 2560 px on the long side) to bound texture size.
fn decode(path: &Path) -> Option<ColorImage> {
    let mut img = image::open(path).ok()?;
    if img.width().max(img.height()) > 2560 {
        img = img.resize(2560, 2560, image::imageops::FilterType::Triangle);
    }
    let rgba = img.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

/// Copy `src` into the config dir as `wallpaper-<ts>.<ext>` (new name each
/// time so the texture cache is busted), drop older copies, record it.
pub fn set_wallpaper(src: &Path) -> Result<(), String> {
    image::open(src).map_err(|e| format!("Couldn't read that image: {e}"))?;
    let dir = config::dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    clear_files(&dir);
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("png");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest: PathBuf = dir.join(format!("wallpaper-{ts}.{ext}"));
    std::fs::copy(src, &dest).map_err(|e| format!("Couldn't set wallpaper: {e}"))?;
    config::update(|c| {
        c.wallpaper = Some(dest.to_string_lossy().into_owned());
        c.backdrop = "photo".into();
    });
    Ok(())
}

pub fn clear_wallpaper() {
    clear_files(&config::dir());
    config::update(|c| {
        c.wallpaper = None;
        if c.backdrop == "photo" {
            c.backdrop = "palette".into();
        }
    });
}

/// Plain palette ground (the Mac's `backdrop = .palette`).
pub fn use_palette() {
    config::update(|c| c.backdrop = "palette".into());
}

/// Animated scene by id (bundled name or a user video path).
pub fn use_scene(id: &str) {
    config::update(|c| {
        c.scene = id.to_string();
        c.backdrop = "scene".into();
    });
}

/// Scene id from a slash-menu name (spaces become dashes).
pub fn scene_ids() -> Vec<String> {
    BUNDLED_SCENES.iter().map(|s| s.to_string()).chain(user_backdrops().into_iter().map(|p| p.to_string_lossy().into_owned())).collect()
}

fn clear_files(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for f in rd.flatten() {
            if f.file_name().to_string_lossy().starts_with("wallpaper-") {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fill_uv_crops_wide_and_tall() {
        let r = fill_uv(egui::vec2(200.0, 100.0), egui::vec2(100.0, 100.0));
        assert!((r.width() - 0.5).abs() < 1e-4 && r.height() == 1.0);
        let r = fill_uv(egui::vec2(100.0, 200.0), egui::vec2(100.0, 100.0));
        assert!((r.height() - 0.5).abs() < 1e-4 && r.width() == 1.0);
    }
}
