#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! JoeBro — native desktop client (egui) of the JoeBro backend.
//! Liquid Glass theme. A sibling of the macOS JoeBroPERSONAL app;
//! same backend, same features, different skin.
mod api;
#[cfg(test)]
mod dropdown_tests;
#[cfg(test)]
mod wrap_tests;
mod app;
mod backend;
mod backend_ui;
#[cfg(test)]
mod backend_tests;
mod backdrop;
mod config;
mod panels;
mod logos;
mod pdfview;
mod pickers;
mod platform;
#[cfg(feature = "snapshot")]
mod snapshot;
mod setup;
mod state;
mod theme;
mod icons;
mod intro;
mod ascii;
mod models;
#[cfg(test)]
mod live_check;
mod voice;
mod widgets;

use std::sync::Arc;

fn main() -> eframe::Result<()> {
    #[cfg(feature = "snapshot")]
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--snapshot") {
            if let Err(e) = snapshot::run(&args[i + 1..]) {
                eprintln!("snapshot failed: {e}");
                std::process::exit(1);
            }
            return Ok(());
        }
    }
    let base = config::server_url();
    let api = Arc::new(api::Api::new(&base));

    let opts = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 560.0])
            .with_app_id("JoeBro")
            .with_title(config::APP_TITLE)
            // Real glass: an alpha surface (premultiplied composite alpha);
            // Hyprland blurs the wallpaper behind the translucent pixels. The
            // app repaints at 4 Hz when idle, which keeps the NVIDIA/Wayland
            // path happy; JOEBRO_OPAQUE=1 falls back to an opaque window.
            .with_transparent(!theme::opaque_window()),
        ..Default::default()
    };

    let result = eframe::run_native(
        "JoeBro",
        opts,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app::JoeBro::new(api, cc.egui_ctx.clone())))
        }),
    );
    backend::stop(); // only does anything if this app started the bundled backend
    result
}
