//! Offscreen snapshot harness (feature `snapshot`, never in the shipped build).
//!
//! Runs the real `JoeBro::frame` against a fake in-process backend serving
//! fixture JSON, tessellates the result and renders it with egui-wgpu into an
//! offscreen texture on a headless adapter — no window, no surface. Each scene
//! is written twice: the raw premultiplied frame, and composited over a crop
//! of the Liquid Glass wallpaper (what Hyprland's blur would sit behind).
//!
//!   cargo run --release --features snapshot -- --snapshot <out_dir> [scene...]
use crate::app::JoeBro;
use crate::state::Tab;
use eframe::egui;
use eframe::egui_wgpu;
use eframe::wgpu;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Arc;

fn snap_w() -> u32 {
    std::env::var("JOEBRO_SNAP_W").ok().and_then(|v| v.parse().ok()).unwrap_or(1280)
}
/// Frame height; `JOEBRO_SNAP_H` overrides it (e.g. 600 to check overflow scrolling).
fn snap_h() -> u32 {
    std::env::var("JOEBRO_SNAP_H").ok().and_then(|v| v.parse().ok()).unwrap_or(820)
}
/// Pixel density; `JOEBRO_SNAP_SCALE=2` renders a retina-sharp frame of the same logical size.
fn scale() -> f32 {
    std::env::var("JOEBRO_SNAP_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0)
}
fn pw() -> u32 {
    (snap_w() as f32 * scale()).round() as u32
}
fn ph() -> u32 {
    (snap_h() as f32 * scale()).round() as u32
}
/// Seconds-ago timestamp for fixtures, so sidebar time labels are always populated.
fn ago(secs: u64) -> f64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    now - secs as f64
}

pub fn run(args: &[String]) -> Result<(), String> {
    let out = std::path::PathBuf::from(args.first().cloned().unwrap_or_else(|| "snapshots".into()));
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let wanted: Vec<String> = args.iter().skip(1).cloned().collect();
    let base = serve_fixtures();
    // Isolate config: the harness must never read the user's real token/prefs or
    // write his remembered model.
    let tmp_cfg = std::env::temp_dir().join(format!("joebro-snap-{}", std::process::id()));
    std::fs::create_dir_all(tmp_cfg.join(crate::config::APP_DIR)).ok();
    std::env::set_var("XDG_CONFIG_HOME", &tmp_cfg);
    std::env::set_var("JOEBRO_TOKEN", "snapshot");
    std::env::set_var("JOEBRO_BACKEND", &base);
    // A synthetic wallpaper for the wallpaper scenes (never a real photo).
    let wall = tmp_cfg.join("sample-wall.png");
    image::RgbaImage::from_fn(640, 400, |x, y| image::Rgba([(x / 3) as u8, (y / 2) as u8, 150, 255]))
        .save(&wall)
        .ok();
    // PDF page renders go to a throwaway cache, never the user's ~/.cache.
    std::env::set_var("XDG_CACHE_HOME", tmp_cfg.join("cache"));

    let gpu = Gpu::new()?;
    let scenes: &[(&str, Tab, bool)] = &[
        ("chat", Tab::Chats, false),
        ("chat-empty", Tab::Chats, false),
        ("chat-stream", Tab::Chats, false),
        ("email", Tab::Email, false),
        ("calendar", Tab::Calendar, false),
        ("brain", Tab::Brain, false),
        ("notes", Tab::Notes, false),
        ("memory", Tab::Memory, false),
        ("inbox", Tab::Inbox, false),
        ("tasks", Tab::Tasks, false),
        ("skills", Tab::Skills, false),
        ("tools", Tab::Tools, false),
        ("research", Tab::Research, false),
        ("research-report", Tab::Research, false),
        ("aicheck", Tab::AICheck, false),
        ("agents", Tab::Agents, false),
        ("coding-agent", Tab::Chats, false),
        ("night-shift", Tab::NightShift, false),
        ("new-coding-agent", Tab::Chats, false),
        ("chips", Tab::Chats, false),
        ("palette", Tab::Chats, false),
        ("slash", Tab::Chats, false),
        ("settings", Tab::Chats, true),
        ("settings-models", Tab::Chats, true),
        ("settings-appearance", Tab::Chats, true),
        ("settings-wallpaper", Tab::Chats, true),
        ("settings-bot", Tab::Chats, true),
        ("settings-agents", Tab::Chats, true),
        ("settings-mobile", Tab::Chats, true),
        ("settings-integrations", Tab::Chats, true),
        ("settings-advanced", Tab::Chats, true),
        ("settings-email-sheet", Tab::Chats, true),
        ("setup", Tab::Chats, false),
        ("setup-mac", Tab::Chats, false),
        ("setup-scratch", Tab::Chats, false),
        ("setup-nopython", Tab::Chats, false),
        ("setup-services", Tab::Chats, false),
        ("setup-server", Tab::Chats, false),
        ("preset-ocean", Tab::Chats, false),
        ("wallpaper", Tab::Chats, false),
        ("picker", Tab::Chats, false),
        ("editor", Tab::Chats, false),
        ("attach", Tab::Chats, false),
        ("pdf-preview", Tab::Chats, false),
        ("pdf-editor", Tab::Chats, false),
        ("calendar-agenda", Tab::Calendar, false),
        ("calendar", Tab::Calendar, false),
        ("calendar-week", Tab::Calendar, false),
    ];
    for dark in [false, true] {
        for (name, tab, settings) in scenes {
            if !wanted.is_empty() && !wanted.iter().any(|w| w == name) {
                continue;
            }
            let mode = if dark { "dark" } else { "light" };
            std::env::set_var("JOEBRO_THEME_MODE", mode);
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(scale());
            crate::theme::install_fonts(&ctx);
            egui_extras::install_image_loaders(&ctx);
            crate::backdrop::clear_wallpaper();
            crate::config::update(|c| c.background = crate::config::DEFAULT_BACKGROUND.to_string());
            match *name {
                "preset-ocean" => crate::config::update(|c| c.background = "ocean".into()),
                "wallpaper" | "settings-wallpaper" => {
                    crate::backdrop::set_wallpaper(&wall).ok();
                }
                _ => {}
            }
            if let Ok(id) = std::env::var("JOEBRO_SNAP_PALETTE") {
                if let Some(pal) = crate::theme::JbPalette::ALL.iter().find(|x| x.id() == id) {
                    crate::theme::set_palette(*pal);
                }
            }
            let api = Arc::new(crate::api::Api::new(&base));
            let mut app = JoeBro::new(api, ctx.clone());
            app.tab = *tab;
            if matches!(*name, "coding-agent" | "new-coding-agent") {
                app.sh.agent_nav.open_id = Some("agt_code1".into());
            }
            if *name != "chat-empty" {
                app.sh.current_sid = Some("s_essay".into());
                app.sh.current_title = Some("Essay outline".into());
            }
            let files_scene = matches!(*name, "attach" | "pdf-preview" | "pdf-editor");
            if files_scene {
                app.sh.current_sid = Some("s_files".into());
                app.sh.current_title = Some("Contract review".into());
            }
            // pdftoppm runs on worker threads — give file scenes time to land.
            let frames = if files_scene { 160 } else if std::env::var("JOEBRO_SNAP_FRAMES").is_ok() { std::env::var("JOEBRO_SNAP_FRAMES").unwrap().parse().unwrap_or(90) } else { 90 };
            app.show_settings = *settings;
            if name.starts_with("setup") {
                let mut st = crate::setup::Setup::default();
                st.demo_stage(match *name {
                    "setup-mac" => 1,
                    "setup-scratch" => 2,
                    "setup-nopython" => 3,
                    "setup-services" => 4,
                    "setup-server" => 5,
                    _ => 0,
                });
                app.setup = Some(st);
            }
            app.settings.demo_tab(match *name {
                "settings-appearance" | "settings-wallpaper" => 1,
                "settings-models" => 2,
                "settings-agents" => 3,
                "settings-mobile" => 4,
                "settings-integrations" => 5,
                "settings-bot" => 6,
                "settings-advanced" => 7,
                _ => 0,
            });
            let mut time = 0.0f64;
            let mut shapes = None;
            let mut renderer = gpu.renderer();
            for frame in 0..frames {
                if frame == 40 {
                    match *name {
                        "chat-stream" => app.chat.demo_streaming(),
                        "picker" => app.chat.demo_open_picker(&ctx),
                        "editor" => app.demo_editor(),
                        "research-report" => app.research.demo_open("r1", &app.sh),
                        "agents" => app.agents.demo_select("agt_r1", &app.sh),
                        "new-coding-agent" => app.sh.new_coding_agent_requested = true,
                        "chips" => app.chat.demo_open_used(&ctx),
                        "palette" => app.sh.palette_open = true,
                        "slash" => app.chat.demo_slash(),
                        "pdf-preview" => app.chat.demo_preview("f1a2.pdf", &ctx),
                        "pdf-editor" => app.demo_editor_pdf(),
                        "calendar-agenda" => app.calendar.demo_agenda(),
                        "calendar" => app.calendar.demo_month(),
                        "calendar-week" => app.calendar.demo_week(),
                        "settings-email-sheet" => app.settings.demo_email_sheet(),
                        _ => {}
                    }
                }
                // JOEBRO_SNAP_CLICKS="frame:x:y,…" — real pointer clicks on
                // the real app, to check popups/dropdowns end to end.
                let mut events = vec![];
                for c in std::env::var("JOEBRO_SNAP_CLICKS").unwrap_or_default().split(',') {
                    let v: Vec<f32> = c.split(':').filter_map(|x| x.trim().parse().ok()).collect();
                    if v.len() != 3 {
                        continue;
                    }
                    let pos = egui::pos2(v[1], v[2]);
                    let at = v[0] as usize;
                    let button = |pressed| egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    };
                    if frame + 1 == at {
                        events.push(egui::Event::PointerMoved(pos));
                    } else if frame == at {
                        events.push(egui::Event::PointerMoved(pos));
                        events.push(button(true));
                    } else if frame == at + 1 {
                        events.push(button(false));
                    }
                }
                let input = egui::RawInput {
                    events,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(snap_w() as f32, snap_h() as f32),
                    )),
                    time: Some(time),
                    predicted_dt: 1.0 / 60.0,
                    focused: true,
                    ..Default::default()
                };
                time += 1.0 / 30.0;
                let mut output = ctx.run(input, |ctx| app.frame(ctx));
                if frame < frames - 1 {
                    gpu.apply(&mut renderer, &output.textures_delta);
                    output.textures_delta = Default::default();
                }
                shapes = Some(output);
                let nap = if files_scene { 14 } else if frame < 40 { 12 } else { 4 };
                std::thread::sleep(std::time::Duration::from_millis(nap));
            }
            let output = shapes.unwrap();
            let clear = app.clear_rgba();
            let raw = gpu.render(&mut renderer, &ctx, output, clear)?;
            let img = image::RgbaImage::from_raw(pw(), ph(), raw).ok_or("bad frame")?;
            if std::env::var("SNAP_RAW").is_ok() {
                img.save(out.join(format!("{name}-{mode}-raw.png"))).ok();
            }
            let path = out.join(format!("{name}-{mode}.png"));
            composite(&img, dark).save(&path).map_err(|e| e.to_string())?;
            eprintln!("wrote {}", path.display());
        }
    }
    Ok(())
}

/// Composite the premultiplied frame over a centred crop of the mode's
/// wallpaper, roughly blurred the way Hyprland's glass blur would show it.
fn composite(frame: &image::RgbaImage, dark: bool) -> image::RgbaImage {
    let home = dirs::home_dir().unwrap_or_default();
    let wp = if dark {
        home.join("wallpapers/liquid-glass-dark.png")
    } else {
        home.join("wallpapers/liquid-glass-light.png")
    };
    let mut bg = match image::open(&wp) {
        Ok(i) => {
            let i = i.resize_to_fill(pw(), ph(), image::imageops::FilterType::Triangle);
            image::imageops::blur(&i.to_rgba8(), 9.0)
        }
        Err(_) => image::RgbaImage::from_pixel(pw(), ph(), image::Rgba([90, 120, 160, 255])),
    };
    for (x, y, px) in bg.enumerate_pixels_mut() {
        let f = frame.get_pixel(x, y);
        let a = f[3] as f32 / 255.0;
        for c in 0..3 {
            // premultiplied over
            let v = f[c] as f32 + px[c] as f32 * (1.0 - a);
            px[c] = v.round().clamp(0.0, 255.0) as u8;
        }
        px[3] = 255;
    }
    bg
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    struct T(std::thread::Thread);
    impl std::task::Wake for T {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(T(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park_timeout(std::time::Duration::from_millis(5));
    }
}

impl Gpu {
    fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..Default::default()
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or("no wgpu adapter")?;
        let (device, queue) = block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("snapshot"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits { max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d.min(8192), ..wgpu::Limits::downlevel_defaults() },
                memory_hints: Default::default(),
            },
            None,
        ))
        .map_err(|e| e.to_string())?;
        Ok(Gpu { device, queue })
    }

    fn renderer(&self) -> egui_wgpu::Renderer {
        egui_wgpu::Renderer::new(&self.device, wgpu::TextureFormat::Rgba8Unorm, None, 1, true)
    }

    /// Texture deltas arrive incrementally (font atlas growth, images), so
    /// every frame's delta must reach the renderer, not just the last one.
    fn apply(&self, renderer: &mut egui_wgpu::Renderer, delta: &egui::TexturesDelta) {
        for (id, d) in &delta.set {
            renderer.update_texture(&self.device, &self.queue, *id, d);
        }
    }

    fn render(
        &self,
        renderer: &mut egui_wgpu::Renderer,
        ctx: &egui::Context,
        output: egui::FullOutput,
        clear: [f32; 4],
    ) -> Result<Vec<u8>, String> {
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: pw(), height: ph(), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
        self.apply(renderer, &output.textures_delta);
        let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [pw(), ph()], pixels_per_point: scale() };
        let mut enc = self.device.create_command_encoder(&Default::default());
        let cbs = renderer.update_buffers(&self.device, &self.queue, &mut enc, &jobs, &sd);
        {
            let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear[0] as f64,
                            g: clear[1] as f64,
                            b: clear[2] as f64,
                            a: clear[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut pass = pass.forget_lifetime();
            renderer.render(&mut pass, &jobs, &sd);
        }
        let row = (pw() * 4 + 255) / 256 * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * ph()) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &buf,
                layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(ph()) },
            },
            wgpu::Extent3d { width: pw(), height: ph(), depth_or_array_layers: 1 },
        );
        self.queue.submit(cbs.into_iter().chain(std::iter::once(enc.finish())));
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::Maintain::Wait);
        let data = slice.get_mapped_range();
        let mut out = Vec::with_capacity((pw() * ph() * 4) as usize);
        for y in 0..ph() {
            let s = (y * row) as usize;
            out.extend_from_slice(&data[s..s + (pw() * 4) as usize]);
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Fake backend
// ---------------------------------------------------------------------------

fn serve_fixtures() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || handle(stream));
        }
    });
    base
}

fn handle(mut s: std::net::TcpStream) {
    let mut reader = BufReader::new(s.try_clone().unwrap());
    let mut first = String::new();
    if reader.read_line(&mut first).is_err() {
        return;
    }
    let mut len = 0usize;
    loop {
        let mut l = String::new();
        if reader.read_line(&mut l).is_err() || l == "\r\n" || l.is_empty() {
            break;
        }
        if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; len];
    let _ = reader.read_exact(&mut body);
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let path = target.split('?').next().unwrap_or("").to_string();
    if method == "GET" && path.starts_with("/api/upload/") {
        let bytes = upload_fixture(&path);
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        );
        let _ = s.write_all(&bytes);
        return;
    }
    let v = if method == "GET" { fixture(&path) } else { json!({"ok": true}) };
    let b = v.to_string();
    let _ = write!(
        s,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        b.len(),
        b
    );
}

/// Uploaded-file bytes: the PDF from `SNAP_PDF`, or a generated gradient PNG.
fn upload_fixture(path: &str) -> Vec<u8> {
    if path.ends_with(".pdf") {
        return std::env::var("SNAP_PDF").ok().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
    }
    if path.contains("missing") {
        return b"<html>not an image</html>".to_vec();
    }
    let img = image::RgbaImage::from_fn(480, 300, |x, y| {
        let t = x as f32 / 480.0;
        let u = y as f32 / 300.0;
        image::Rgba([(90.0 + 120.0 * t) as u8, (60.0 + 90.0 * u) as u8, (170.0 + 60.0 * (1.0 - t)) as u8, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = img.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

fn fixture(path: &str) -> Value {
    let p = path.trim_start_matches('/');
    match p {
        "api/models" => json!({"models": [], "items": [
            {"url":"http://pc:11434/v1","endpoint_id":"ep_pc","endpoint_name":"Ollama (workstation)","category":"local",
             "models":["qwen3:14b","gemma3:12b","llama3.2:3b"]},
            {"url":"https://api.deepseek.com/v1","endpoint_id":"ep_ds","endpoint_name":"DeepSeek","category":"cloud",
             "models":["deepseek-chat","deepseek-reasoner"]},
            {"url":"https://openrouter.ai/api/v1","endpoint_id":"ep_or","endpoint_name":"OpenRouter","category":"cloud",
             "models":["anthropic/claude-sonnet-4.5","google/gemini-2.5-pro"]}]}),
        "api/prefs" => json!({"cloak_mode": true, "require_doc_approval": true, "ask_before_commands": true, "max_tool_calls": 25, "email_load_count": 100}),
        "api/sessions" => json!([
            {"id":"s_essay","updated_at":ago(1800),"name":"Essay outline","model":"deepseek-chat","mode":"agent","folder":"University","is_important":true,"sort_order":1.0,"workdir":"~/Documents/essays","archived":false},
            {"id":"s_hope","updated_at":ago(93600),"name":"Hope & Solnit notes","model":"qwen3:14b","mode":"chat","folder":"University","is_important":false,"sort_order":2.0,"workdir":"","archived":false},
            {"id":"s_gpu","updated_at":ago(259200),"name":"Used GPU shortlist","model":"deepseek-chat","mode":"agent","folder":"","is_important":false,"sort_order":3.0,"workdir":"","archived":false},
            {"id":"s_trip","updated_at":ago(432000),"name":"Weekend in Utrecht","model":"gemma3:12b","mode":"chat","folder":"","is_important":false,"sort_order":4.0,"workdir":"","archived":false},
            {"id":"s_rust","updated_at":ago(777600),"name":"egui glass shader ideas","model":"deepseek-chat","mode":"agent","folder":"Projects","is_important":false,"sort_order":5.0,"workdir":"","archived":false},
            {"id":"s_cv","updated_at":ago(1296000),"name":"CV polish","model":"deepseek-chat","mode":"chat","folder":"","is_important":false,"sort_order":6.0,"workdir":"","archived":false}
        ]),
        "api/history/s_essay" => json!({"mode":"agent","model":"deepseek-chat","endpoint_id":"ep_ds","permission_mode":"sandbox","history":[
            {"role":"user","content":"Can you outline my essay on hope as an atmosphere? Keep my thesis, three sections.","_db_id":11},
            {"role":"assistant","content":"Here’s a tight three-part outline that keeps your thesis front and centre.\n\n## 1. Hope as atmosphere\n- Open with the *room* image: hope that asks nothing of you.\n- Contrast with hope as a **plan** (Bloch) — forward-leaning, demanding.\n\n## 2. Solnit’s counter-position\nSolnit makes hope an act of the dark: it lives *because* the future is unknown.\n\n## 3. Synthesis\nArgue the two meet in shared spaces.\n\n```markdown\nThesis: hope is less a forecast than a weather —\nsomething you stand inside, together.\n```\n\nWant me to draft the introduction next?",
             "_db_id":12,"metadata":{"_db_id":12,"thinking":"The user wants structure, not prose. Keep their thesis.","tool_events":[{"tool":"read_file","command":"essays/hope-draft.md","exit_code":0,"output":"# Hope draft\n…"}],"memories_used":["Essay thesis: hope as an atmosphere conferred on a room.","Prefers honest uncertainty over confident guesses.","Studying law; exam on 26 November."],"skills_used":["Final-pass essay edits"],"plugins_used":["Computer use"],"tokens_per_second":41.2,"context_percent":18}},
            {"role":"user","content":"Yes — and keep it under 200 words.","_db_id":13},
            {"role":"assistant","content":"Hope is usually described as a forecast: a reasoned bet that tomorrow will be kinder than today. This essay argues for something quieter. Hope, at its most durable, is an **atmosphere** — a condition we stand inside rather than a claim we defend.","_db_id":14,"metadata":{"tokens_per_second":39.8}}
        ]}),
        "api/session/s_essay/tip" => json!({"last_id":14}),
        "api/history/s_files" => json!({"mode":"chat","model":"deepseek-chat","endpoint_id":"ep_ds","history":[
            {"role":"system","content":"[Conversation summary] earlier turns","metadata":{"compacted":true}},
            {"role":"user","content":"Here's the lease and a photo of the meter — anything odd?","metadata":{"_db_id":21,"attachments":[
                {"id":"f1a2.pdf","name":"Lease agreement 2026.pdf","mime":"application/pdf","size":5120},
                {"id":"img9.png","name":"meter.png","mime":"image/png","width":480,"height":300},
                {"id":"n3.csv","name":"costs.csv","mime":"text/csv"}]}},
            {"role":"assistant","content":"Two things stand out in the lease:\n\n1. **Clause 4.2** lets the landlord raise rent twice a year.\n2. The deposit is held for *90 days* after move-out.\n\nHere's the deposit maths:\n\n```python\ndef refund(deposit, damages):\n    # held for 90 days, minus damages\n    return max(0, deposit - damages)  # never negative\n\nprint(refund(1450, 120))\n```\n\nI saved a summary as [Lease notes](document-doc_77) and added a reminder [Deposit check](task-t9).","metadata":{"_db_id":22,"tokens_per_second":38.0,"context_percent":24}}
        ]}),
        "api/session/s_files/tip" => json!({"last_id":22}),
        "api/session/s_essay/workdir" => json!({"workdir":"~/Documents/essays","device":""}),
        "api/session/s_essay/workdir/list" => json!({"ok":true,"entries":[
            {"name":"hope-draft.md","is_dir":false,"size":4210},{"name":"sources","is_dir":true},{"name":"outline.md","is_dir":false,"size":980},{"name":"reading-list.csv","is_dir":false,"size":512}]}),
        "api/email/status" => json!({"configured":true,"type":"imap","display":"alex@example.com","sources":[{"id":"m1","display":"alex@example.com","type":"imap"}]}),
        "api/email/folders" => json!({"folders":["INBOX","Archive","Drafts","Sent","Spam","Trash"],"configured":true}),
        "api/email/list" => json!({"configured":true,"total":6,"emails":[
            {"uid":"1","subject":"Exam cover sheet: Law & Economics I","from_name":"Course Office","from_address":"office@uni.example","date":"2026-11-19T09:12:00+02:00","is_read":false,"has_attachments":true,"is_flagged":false,"size":48211},
            {"uid":"2","subject":"Your Sunday briefing","from_name":"JoeBro","from_address":"bot@example.com","date":"2026-11-19T07:00:00+02:00","is_read":false,"has_attachments":false,"is_flagged":true,"size":3011},
            {"uid":"3","subject":"Re: seminar reading for week 6","from_name":"Dr. Ellis","from_address":"ellis@uni.example","date":"2026-11-18T16:40:00+02:00","is_read":true,"has_attachments":false,"is_flagged":false,"size":9020},
            {"uid":"4","subject":"Order shipped — RTX 3060 12GB","from_name":"Marketplace","from_address":"noreply@shop.example","date":"2026-11-17T11:05:00+02:00","is_read":true,"has_attachments":false,"is_flagged":false,"size":21000},
            {"uid":"5","subject":"Gym: new class schedule","from_name":"Sportcity","from_address":"info@gym.example","date":"2026-11-16T08:30:00+02:00","is_read":true,"has_attachments":true,"is_flagged":false,"size":120332},
            {"uid":"6","subject":"Coffee Thursday?","from_name":"Ashley","from_address":"ash@example.com","date":"2026-11-15T19:02:00+02:00","is_read":true,"has_attachments":false,"is_flagged":false,"size":1200}]}),
        p if p.starts_with("api/email/read/") => json!({"uid":"1","subject":"Exam cover sheet: Law & Economics I","from_name":"Course Office","from_address":"office@uni.example","to":"alex@example.com","date":"2026-11-19T09:12:00+02:00",
            "body":"Dear students,\n\nPlease find attached the cover sheet for tomorrow’s exam. Bring your student card and arrive 15 minutes early.\n\nGood luck!\nCourse Office","html":"","attachments":[{"index":0,"filename":"cover-sheet.pdf","size":48000}]}),
        "api/calendar/status" => json!({"configured":true,"type":"caldav","display":"Nextcloud"}),
        "api/calendar/events" => json!({"events":[
            {"uid":"e1","summary":"Law & Econ exam","dtstart":"2026-11-26T09:00:00+02:00","dtend":"2026-11-26T12:00:00+02:00","location":"REC A1.02"},
            {"uid":"e2","summary":"Work","dtstart":"2026-11-21T18:00:00+02:00","dtend":"2026-11-21T22:00:00+02:00"},
            {"uid":"e3","summary":"Coffee with Ashley","dtstart":"2026-11-23T10:30:00+02:00","dtend":"2026-11-23T11:30:00+02:00"},
            {"uid":"e4","summary":"Essay deadline","dtstart":"2026-11-24","dtend":"2026-11-25","all_day":true},
            {"uid":"e5","summary":"Gym","dtstart":"2026-11-19T17:00:00+02:00","dtend":"2026-11-19T18:00:00+02:00"},
            {"uid":"e6","summary":"Seminar week 6","dtstart":"2026-11-14T13:00:00+02:00","dtend":"2026-11-14T15:00:00+02:00"}]}),
        "api/shared-memory/projects" => json!({"projects":[
            {"key":"global","name":"","active":2,"archived":0},{"key":"user","name":"","active":3,"archived":0},
            {"key":"atlas","name":"Atlas API","active":4,"archived":1}]}),
        "api/shared-memory" => json!({"facts":[
            {"id":"f1","scope":"user","topic":"style","kind":"preference","text":"Prefers short answers with a clear next step, and code over prose.","pinned":true,"strength":0.9,"recalls":12,"updated_at":"2026-11-19T04:00:00Z","provenance":{"agent":"app","host":"laptop"}},
            {"id":"f2","scope":"project","project":"atlas","topic":"commits","kind":"fact","text":"Small commits with a imperative subject line; never push without being asked.","pinned":true,"strength":0.7,"recalls":5,"updated_at":"2026-11-13T10:00:00Z","provenance":{"agent":"claude","model":"claude-opus-4","host":"laptop","source":"mcp"}},
            {"id":"f3","scope":"project","project":"atlas","topic":"testing","kind":"gotcha","text":"Run tests with `make test`; the suite needs Postgres on port 5433.","pinned":false,"strength":0.6,"recalls":3,"updated_at":"2026-11-18T10:00:00Z","provenance":{"agent":"claude","model":"claude-opus-4","host":"laptop","source":"mcp"}},
            {"id":"f4","scope":"global","topic":"machines","kind":"fact","text":"Three machines are linked: laptop (daily driver), home server (always on, runs Night Shift), workstation (GPU, local models).","pinned":false,"strength":0.4,"recalls":1,"updated_at":"2026-11-14T10:00:00Z","provenance":{"agent":"app","host":"laptop"}}]}),
        "api/memory" => json!({"memory":[
            {"id":"m1","text":"Prefers honest uncertainty over confident guesses.","category":"preferences","kind":"fact","pinned":true},
            {"id":"m2","text":"Studying law; exam on 26 November.","category":"uva-student","kind":"fact","pinned":true},
            {"id":"m3","text":"Essay thesis: hope as an atmosphere conferred on a room.","category":"essay-projects","kind":"fact","pinned":false},
            {"id":"m4","text":"Hunting a used GPU with ≥12 GB VRAM under €250.","category":"gpu-hunting","kind":"fact","pinned":false},
            {"id":"m5","text":"Lives in Utrecht; prefers metric units.","category":"location-amsterdam","kind":"fact","pinned":false}]}),
        "api/notes" => json!({"notes":[
            {"id":"n1","title":"Exam checklist","note_type":"checklist","pinned":true,"content":"","items":[{"text":"Student card","done":true},{"text":"Calculator","done":false},{"text":"Two pens","done":false}]},
            {"id":"n2","title":"Essay quotes","note_type":"note","pinned":false,"content":"“Hope is an embrace of the unknown.” — Solnit\n“The not-yet.” — Bloch","items":[]},
            {"id":"n3","title":"Groceries","note_type":"checklist","pinned":false,"content":"","items":[{"text":"Oat milk","done":false},{"text":"Coffee beans","done":false}]}]}),
        "api/tasks" => json!({"tasks":[
            {"id":"t1","name":"Morning briefing","title":"Morning briefing","prompt":"Weather, calendar and unread mail summary.","schedule":"daily","scheduled_time":"07:00","status":"active","permission_mode":"sandbox","last_run":"2026-11-19T07:00:00+02:00"},
            {"id":"t2","name":"Memory audit","title":"Memory audit","prompt":"Reflect over long-term memory and merge duplicates.","schedule":"weekly","scheduled_time":"21:00","repeat_day":"sunday","status":"active","permission_mode":"sandbox"},
            {"id":"t3","name":"GPU watcher","title":"GPU watcher","prompt":"Check marketplaces for 12GB GPUs under €250.","schedule":"daily","scheduled_time":"12:00","status":"paused","permission_mode":"readonly"}]}),
        "api/skills" => json!({"skills":[
            {"id":"k1","name":"Final-pass essay edits","description":"Review a finished essay for typos, stale metadata and citations.","when_to_use":"After a full draft exists.","procedure":"1. Read\n2. Mark\n3. Fix","status":"active","confidence":0.9,"category":"writing"},
            {"id":"k2","name":"Quiz me one at a time","description":"Ask exam questions one by one and grade each answer.","when_to_use":"Revision sessions.","procedure":"…","status":"active","confidence":0.8,"category":"study"},
            {"id":"k3","name":"GPU price sanity check","description":"Compare a listing to recent sold prices.","when_to_use":"Marketplace links.","procedure":"…","status":"draft","confidence":0.6,"category":"shopping"}]}),
        "api/tools" => json!({"tools":[{"id":"tl1","name":"DataCite","func_name":"datacite","base_url":"https://api.datacite.org/dois?query={query}","method":"GET","description":"Search DOIs","is_enabled":true,"has_key":false},
            {"id":"tl2","name":"Weather","func_name":"weather","base_url":"https://api.open-meteo.com/v1/forecast","method":"GET","description":"Forecasts","is_enabled":true,"has_key":false}]}),
        "api/mcp" => json!({"servers":[{"id":"mc1","name":"Pinterest","command":"/usr/bin/python3","args":"mcp_pinterest.py","is_enabled":true}]}),
        "api/plugins" => json!({"plugins":[{"id":"pl1","name":"Computer use","kind":"foreground","description":"Drive desktop apps.","is_enabled":false,"permission_mode":"full"}]}),
        "api/research" | "api/research/library" => json!({"research":[
            {"id":"r1","query":"Cheapest used GPUs in the EU that run a capable local LLM","status":"completed","category":"hardware","source_count":12,"completed_at":1760781600.0,"created_at":"2026-11-18T10:00:00Z","content":"# Findings\n\nThe **RTX 3060 12GB** remains the value pick…"},
            {"id":"r2","query":"Hope as atmosphere: Bloch vs Solnit","status":"completed","source_count":9,"created_at":"2026-11-15T10:00:00Z","content":"# Summary\n…"}]}),
        "api/research/r1" => {
            let base = std::env::var("JOEBRO_BACKEND").unwrap_or_default();
            json!({"id":"r1","query":"Cheapest used GPUs in the EU that run a capable local LLM",
              "report": format!("# Findings\n\nThe **RTX 3060 12GB** remains the value pick under 250 euro.\n\n![Price chart (relative path, authed client)](api/upload/chart.png)\n\nListing photo from a remote host:\n\n![Listing photo]({base}/api/upload/photo.png)\n\n![Dead link]({base}/api/upload/missing.webm)\n\n- 12 GB VRAM fits 14B models at Q4\n- Inline ![tiny](http://x/y.png) images become links\n"),
              "sources":[{"title":"Tweakers Pricewatch - RTX 3060 12GB","url":"https://tweakers.net/"},{"title":"r/LocalLLaMA budget GPU thread","url":"https://reddit.com/r/LocalLLaMA"}]})
        }
        "api/agents" => json!({"ok":true,"count":4,"active":1,"live":1,
            "by_kind":{"chat":1,"research":1,"task":1,"briefing":0,"subagent":1},"kinds":["chat","research","task","briefing","subagent"],
            "agents":[
            {"id":"agt_code1","kind":"coding","title":"Codex: add retry with backoff to ledger-api uploader","status":"waiting","live":true,"open":true,"source":"codex","host":"laptop","progress":"Waiting for approval to run go test","created_at":"2026-11-19T08:00:00Z"},
            {"id":"agt_s1","kind":"subagent","title":"Compare 3 used RTX 3060 listings","status":"running","live":true,"progress":"Reading listing 2 of 3…","session_id":"s_gpu","model":"deepseek-chat","created_at":"2026-11-19T11:58:00Z","started_at":"2026-11-19T11:58:01Z","finished_at":"","error":"","result":"","result_truncated":false},
            {"id":"agt_r1","kind":"research","title":"Cheapest used GPUs in the EU that run a capable local LLM","status":"done","live":false,"progress":"Finished","session_id":"","model":"deepseek-reasoner","created_at":"2026-11-18T10:00:00Z","started_at":"2026-11-18T10:00:02Z","finished_at":"2026-11-18T10:06:41Z","error":"","source_count":12,"result":"# Findings\n\nThe **RTX 3060 12GB** remains the value pick…","result_truncated":true},
            {"id":"tsk_1","kind":"task","title":"Morning briefing","status":"done","live":false,"progress":"Finished","session_id":"task_session_t1","model":"deepseek-chat","created_at":"2026-11-19T07:00:00Z","started_at":"2026-11-19T07:00:01Z","finished_at":"2026-11-19T07:00:24Z","error":"","result":"Sunny, 11°C. Exam tomorrow 09:00.","result_truncated":false},
            {"id":"agt_c1","kind":"chat","title":"Essay outline","status":"failed","live":false,"progress":"","session_id":"s_essay","model":"deepseek-chat","created_at":"2026-11-17T19:00:00Z","started_at":"2026-11-17T19:00:00Z","finished_at":"2026-11-17T19:00:09Z","error":"Endpoint timed out after 60 s","result":"","result_truncated":false}]}),
        "api/agents/agt_code1" => json!({"ok":true,"agent":{"id":"agt_code1","kind":"coding","title":"Codex: add retry with backoff to ledger-api uploader","status":"waiting","live":true,"open":true,"source":"codex","host":"laptop","cwd":"/home/dev/atlas","model":"gpt-5-codex","created_at":"2026-11-19T08:00:00Z",
            "prompt":{"header":"","question":"Would you like to run the following command?  go test ./...","options":[{"key":"1","label":"Yes, proceed"},{"key":"2","label":"Yes, and don't ask again for go test"},{"key":"3","label":"Type your own answer","freeform":true}]},
            "messages":[{"role":"user","content":"The uploader gives up after one failed chunk. Add retry with exponential backoff to ledger-api, and make it configurable."},
            {"role":"assistant","content":"I will add a small retry wrapper around `uploadChunk` with jitter, capped at five attempts."},
            {"role":"tool","content":"Read: internal/config/config.go"},{"role":"tool","content":"Read: internal/upload/uploader.go"},
            {"role":"assistant","content":"Implemented. The wrapper retries only on network errors and `503`, never on `4xx`, so a bad request still fails fast.

- Config: `UploadRetries` and `UploadBackoffMs`, with sane defaults.
- Helper: `retry.go` next to the uploader, with jitter capped at 30 s.
- Tests: six new cases, all green.

Next I want to run the full suite, including the integration tests."},{"role":"approval","content":"Question: Would you like to run the following command?"}]}}),
        "api/night-shift" => json!({"ok":true,"config":{"enabled":true,"default_host":"laptop"},"hosts":[{"id":"laptop","name":"Laptop","local":true,"up":true},{"id":"server","name":"Home server","local":false,"up":true}],"tasks":[
            {"id":"n1","title":"Add retry with backoff to the uploader","repo_name":"atlas","agent":"claude","model":"opus","status":"review","result":{"summary":"Added exponential backoff with full jitter (cap 30s) to the uploader. Six new tests.","duration_s":1320,"diff":{"files":6,"insertions":212,"deletions":40,"file_list":[{"path":"src/upload.py","added":120,"removed":12},{"path":"src/backoff.py","added":54,"removed":0}]},"tests":{"passed":true}}},
            {"id":"n2","title":"Upgrade to React 19","repo_name":"web","agent":"opencode","model":"qwen3-coder:30b","status":"failed","error":"Two tests still fail","result":{"summary":"Codemod replaced 38 wrappers.","diff":{"files":38,"insertions":540,"deletions":505,"file_list":[]},"tests":{"passed":false}}},
            {"id":"n3","title":"Write the changelog","repo_name":"atlas","agent":"codex","status":"queued"}]}),
        "api/shared-memory/sessions" => json!({"sessions":[{"id":"x1","repo":"/home/dev/atlas","host":"laptop","agent":"claude","summary":"Migrate auth middleware","at":"2026-11-19T09:00:00Z","resume_id":"r1"},{"id":"x2","repo":"/home/dev/web","host":"laptop","agent":"codex","summary":"Fix types","at":"2026-11-18T09:00:00Z","resume_id":"r2"}]}),
        "api/agents/agt_r1" => json!({"ok":true,"agent":{"id":"agt_r1","kind":"research","title":"Cheapest used GPUs in the EU that run a capable local LLM","status":"done","live":false,"progress":"Finished","session_id":"s_gpu","model":"deepseek-reasoner","created_at":"2026-11-18T10:00:00Z","started_at":"2026-11-18T10:00:02Z","finished_at":"2026-11-18T10:06:41Z","error":"","source_count":2,
            "sources":[{"title":"Tweakers Pricewatch — RTX 3060 12GB","url":"https://tweakers.net/"},{"title":"r/LocalLLaMA budget GPU thread","url":"https://reddit.com/r/LocalLLaMA"}],
            "result":"# Findings\n\nThe **RTX 3060 12GB** remains the value pick for local LLMs under €250.\n\n- 12 GB VRAM fits 14B models at Q4\n- Used prices: €200–240 in NL/DE\n- Avoid mining cards without receipts","result_truncated":false,
            "messages":[{"role":"user","content":"Cheapest used GPUs in the EU that run a capable local LLM"},{"role":"assistant","content":"Searching marketplaces and benchmarks…"}]}}),
        "api/client-settings" => json!({"ok":true,"settings":{"mode":"agent","thinking":true,"web_search":true,"terminal":false,"permission_mode":"sandbox","updated_at":"2026-11-19T10:00:00Z"}}),
        "api/research/active" => json!({"active":[]}),
        "api/inbox" => json!({"inbox":[
            {"id":"i1","kind":"personal_task","title":"Sunday briefing","body":"Utrecht 11°C, drizzle until 13:00. Exam tomorrow at 09:00 — cover sheet is in your inbox.","read":false,"created_at":"2026-11-19T07:00:00Z","source":"task"},
            {"id":"i2","kind":"agent","title":"GPU watcher","body":"New listing: RTX 3060 12GB, €219, Utrecht.","read":false,"created_at":"2026-11-18T12:00:00Z","source":"agent"},
            {"id":"i3","kind":"notice","title":"Memory audit finished","body":"Merged 4 duplicate memories.","read":true,"created_at":"2026-11-12T21:05:00Z","source":"task"}]}),
        "api/model-endpoints" => json!([{"id":"ep_pc","name":"Ollama (workstation)","base_url":"http://pc:11434/v1","model_type":"local","is_enabled":true},
            {"id":"ep_ds","name":"DeepSeek","base_url":"https://api.deepseek.com/v1","model_type":"cloud","is_enabled":true}]),
        "api/devices" => json!({"devices":[{"id":"server","name":"Home server (this machine)","is_self":true},{"id":"workstation","name":"Workstation","remote":true}]}),
        _ => json!({}),
    }
}
