//! Provider logos — a port of the Mac app's `ProviderLogos.swift` (itself the
//! web UI's providers.js, Simple Icons set). The SVGs are pre-rendered to 32 px
//! white alpha masks (`assets/logos/*.png`) and drawn tinted, like the Mac's
//! template images, so they follow the text colour in light and dark mode.
use eframe::egui::{self, Color32, FontId, Rect, Response, Sense, TextureHandle, Ui, Vec2};
use std::sync::Arc;

macro_rules! logo {
    ($n:literal) => {
        ($n, include_bytes!(concat!("../assets/logos/", $n, ".png")) as &[u8])
    };
}

/// Same order as the Mac's `providerLogoTable` (first match wins).
const LOGOS: &[(&str, &[u8])] = &[
    logo!("openai"),
    logo!("anthropic"),
    logo!("google"),
    logo!("meta"),
    logo!("mistral"),
    logo!("qwen"),
    logo!("deepseek"),
    logo!("x-ai"),
    logo!("cohere"),
    logo!("perplexity"),
    logo!("nous"),
    logo!("microsoft"),
    logo!("zhipu"),
    logo!("minimax"),
    logo!("kimi"),
    logo!("nvidia"),
];

/// The Mac's regex table, hand-matched (no regex crate):
/// `openai|gpt-|^o[13]-|chatgpt|dall-e`, `anthropic|claude`,
/// `google|gemini|gemma`, `meta|llama(?![.\-_ ]?cpp)`, `mistral`,
/// `qwen|alibaba`, `deepseek`, `x-ai|xai|grok`, `cohere|command-r`,
/// `perplexity|sonar`, `nous|hermes`, `microsoft|phi-`, `zhipu|glm|chatglm`,
/// `minimax`, `kimi|moonshot`, `nvidia|nemotron`.
pub fn logo_key(model: &str) -> Option<&'static str> {
    let m = model.to_lowercase();
    let any = |xs: &[&str]| xs.iter().any(|x| m.contains(x));
    let llama = m.match_indices("llama").any(|(i, _)| {
        let rest = &m[i + 5..];
        let rest = rest.strip_prefix(['.', '-', '_', ' ']).unwrap_or(rest);
        !rest.starts_with("cpp")
    });
    let key = if any(&["openai", "gpt-", "chatgpt", "dall-e"]) || m.starts_with("o1-") || m.starts_with("o3-") {
        "openai"
    } else if any(&["anthropic", "claude"]) {
        "anthropic"
    } else if any(&["google", "gemini", "gemma"]) {
        "google"
    } else if m.contains("meta") || llama {
        "meta"
    } else if m.contains("mistral") {
        "mistral"
    } else if any(&["qwen", "alibaba"]) {
        "qwen"
    } else if m.contains("deepseek") {
        "deepseek"
    } else if any(&["x-ai", "xai", "grok"]) {
        "x-ai"
    } else if any(&["cohere", "command-r"]) {
        "cohere"
    } else if any(&["perplexity", "sonar"]) {
        "perplexity"
    } else if any(&["nous", "hermes"]) {
        "nous"
    } else if any(&["microsoft", "phi-"]) {
        "microsoft"
    } else if any(&["zhipu", "glm", "chatglm"]) {
        "zhipu"
    } else if m.contains("minimax") {
        "minimax"
    } else if any(&["kimi", "moonshot"]) {
        "kimi"
    } else if any(&["nvidia", "nemotron"]) {
        "nvidia"
    } else {
        return None;
    };
    Some(key)
}

type Cache = Arc<Vec<Option<TextureHandle>>>;

/// The provider logo texture for a model id, loaded once per context.
pub fn model_logo(ctx: &egui::Context, model: &str) -> Option<TextureHandle> {
    let key = logo_key(model)?;
    let idx = LOGOS.iter().position(|(n, _)| *n == key)?;
    let cache_id = egui::Id::new("joebro_provider_logos");
    let cache: Option<Cache> = ctx.data(|d| d.get_temp(cache_id));
    let cache = cache.unwrap_or_else(|| {
        let opts = egui::TextureOptions {
            magnification: egui::TextureFilter::Linear,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::ClampToEdge,
            mipmap_mode: Some(egui::TextureFilter::Linear),
        };
        let v: Cache = Arc::new(
            LOGOS
                .iter()
                .map(|(n, bytes)| {
                    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
                    let size = [img.width() as usize, img.height() as usize];
                    let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    Some(ctx.load_texture(format!("logo-{n}"), ci, opts))
                })
                .collect(),
        );
        ctx.data_mut(|d| d.insert_temp(cache_id, v.clone()));
        v
    });
    cache.get(idx).cloned().flatten()
}

/// The Mac's `ProviderBrand` monogram fallback: initial + brand colour.
pub fn provider_brand(model: &str) -> (String, Color32) {
    let m = model.to_lowercase();
    let rgb = |r: f32, g: f32, b: f32| Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8);
    let (i, c) = if m.contains("deepseek") {
        ("D", rgb(0.30, 0.42, 1.0))
    } else if m.contains("glm") || m.contains("zai") {
        ("Z", rgb(0.36, 0.33, 0.87))
    } else if m.contains("gpt") || m.contains("openai") || m.starts_with("o1") || m.starts_with("o3") || m.contains("oss") {
        ("O", rgb(0.10, 0.65, 0.55))
    } else if m.contains("claude") {
        ("C", rgb(0.85, 0.47, 0.34))
    } else if m.contains("gemini") || m.contains("gemma") {
        ("G", rgb(0.26, 0.52, 0.96))
    } else if m.contains("qwen") {
        ("Q", rgb(0.55, 0.36, 0.96))
    } else if m.contains("llama") {
        ("L", rgb(0.0, 0.50, 0.95))
    } else if m.contains("mistral") || m.contains("mixtral") {
        ("M", rgb(0.99, 0.44, 0.13))
    } else if m.contains("grok") {
        ("X", rgb(0.25, 0.25, 0.25))
    } else if m.contains("kimi") || m.contains("moonshot") {
        ("K", rgb(0.15, 0.15, 0.2))
    } else {
        return (model.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(), Color32::GRAY);
    };
    (i.into(), c)
}

/// Paint `ProviderLogoView` into `rect`: Apple glyph, tinted logo, or the
/// brand monogram circle.
pub fn paint_logo(ui: &Ui, rect: Rect, model: &str, tint: Color32) {
    let size = rect.width().min(rect.height());
    let painter = ui.painter();
    if model.to_lowercase().contains("apple") {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "\u{f179}",
            FontId::new(size * 0.9, crate::theme::icon_font()),
            tint,
        );
    } else if let Some(tex) = model_logo(ui.ctx(), model) {
        let r = Rect::from_center_size(rect.center(), Vec2::splat(size));
        painter.image(tex.id(), r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
    } else {
        let (initial, color) = provider_brand(model);
        painter.circle_filled(rect.center(), size / 2.0, color);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            initial,
            FontId::new(size * 0.58, crate::theme::display_font()),
            Color32::WHITE,
        );
    }
}

/// `ProviderLogoView(model:size:)` as an inline widget.
pub fn logo(ui: &mut Ui, model: &str, size: f32, tint: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_logo(ui, rect, model, tint);
    resp
}

#[cfg(test)]
mod tests {
    use super::logo_key;

    #[test]
    fn mapping_matches_mac() {
        for (m, k) in [
            ("anthropic/claude-sonnet-4.5", Some("anthropic")),
            ("gpt-4o", Some("openai")),
            ("o3-mini", Some("openai")),
            ("gpt-oss:20b", Some("openai")),
            ("google/gemini-2.5-pro", Some("google")),
            ("gemma3:12b", Some("google")),
            ("llama3.2:3b", Some("meta")),
            ("llama.cpp-server", None),
            ("llama-cpp", None),
            ("qwen3:14b", Some("qwen")),
            ("deepseek-chat", Some("deepseek")),
            ("x-ai/grok-4", Some("x-ai")),
            ("glm-4.6", Some("zhipu")),
            ("kimi-k2", Some("kimi")),
            ("nemotron-70b", Some("nvidia")),
            ("phi-4", Some("microsoft")),
            ("hermes-3", Some("nous")),
            ("bonsai-27b", None),
            ("omni", None),
        ] {
            assert_eq!(logo_key(m), k, "{m}");
        }
    }
}
