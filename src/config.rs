//! Build flavour + persisted configuration.
//!
//! One codebase, two builds. `--features personal` is the owner's build (a remote
//! backend, token file and desktop-glass look as defaults); the default build
//! is the generic production client: no baked-in address, first-run setup.
//!
//! Everything user-specific lives in the per-OS config dir
//! (`~/.config/joebro[-personal]`, `%APPDATA%\joebro`, `~/Library/Application
//! Support/joebro`): `config.json` plus a `token` file (0600 on Unix).
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// True in the Personal build.
pub const PERSONAL: bool = cfg!(feature = "personal");
/// Config sub-directory name (Personal keeps its historical directory).
pub const APP_DIR: &str = if PERSONAL { "joebro-personal" } else { "joebro" };
pub const APP_TITLE: &str = if PERSONAL { "JoeBro Personal" } else { "JoeBro" };
/// Backend address used when nothing is configured. Empty in Production.
/// Personal builds take it from `JOEBRO_DEFAULT_SERVER` at build time (set it in the untracked
/// `.cargo/config.toml` `[env]` table), so no private address lives in the source.
pub const DEFAULT_SERVER: &str = if PERSONAL {
    match option_env!("JOEBRO_DEFAULT_SERVER") { Some(s) => s, None => "" }
} else {
    ""
};
/// Default app backdrop: Personal keeps the see-through desktop glass; the
/// production default is the Mac's default (the Purple preset).
pub const DEFAULT_BACKGROUND: &str = if PERSONAL { "desktop" } else { "purple" };
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Backend base URL. Empty = not configured yet (first run).
    pub server_url: String,
    /// Backdrop preset id (`purple`, `ocean`, `forest`, `sunset`, `graphite`,
    /// or `desktop` in the Personal build). Used when no wallpaper is set.
    pub background: String,
    /// Wallpaper image copied into the config dir; overrides the preset.
    pub wallpaper: Option<String>,
    /// Id this machine is registered under on the backend ("This PC").
    /// Empty = the build default.
    pub device_id: String,
    /// How the backend was chosen at onboarding: `bundled` (started by this
    /// app), `mac` (the JoeBro Mac app's backend), `server` or empty.
    pub mode: String,
    /// Start the bundled backend automatically when the app opens.
    pub bundled_autostart: bool,
    /// Python interpreter override for the bundled backend (empty = detect).
    pub python: String,
    /// Swap the large soft shadows / sheen for a flatter, cheaper look.
    pub reduce_glass: bool,
    /// Backdrop kind (Settings > Appearance): `palette`, `photo` or `scene`. Empty = unset
    /// (a wallpaper then counts as a photo backdrop, as on the Mac).
    pub backdrop: String,
    /// Animated scene: bundled name or the path of a user video in the Backdrops folder.
    pub scene: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server_url: DEFAULT_SERVER.to_string(),
            background: DEFAULT_BACKGROUND.to_string(),
            wallpaper: None,
            device_id: String::new(),
            mode: String::new(),
            bundled_autostart: true,
            python: String::new(),
            reduce_glass: false,
            backdrop: String::new(),
            scene: String::new(),
        }
    }
}

/// The app's config directory.
pub fn dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join(APP_DIR)
}

fn store() -> &'static Mutex<Config> {
    static CFG: OnceLock<Mutex<Config>> = OnceLock::new();
    CFG.get_or_init(|| Mutex::new(read_disk()))
}

fn read_disk() -> Config {
    std::fs::read_to_string(dir().join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Snapshot of the current config.
pub fn get() -> Config {
    store().lock().map(|c| c.clone()).unwrap_or_default()
}

/// Mutate + persist the config.
pub fn update(f: impl FnOnce(&mut Config)) {
    if let Ok(mut c) = store().lock() {
        f(&mut c);
        let d = dir();
        if std::fs::create_dir_all(&d).is_ok() {
            if let Ok(text) = serde_json::to_string_pretty(&*c) {
                let _ = write_private(&d.join("config.json"), text.as_bytes());
            }
        }
    }
}

/// Effective backend URL: `JOEBRO_BACKEND` env, else config.
pub fn server_url() -> String {
    match std::env::var("JOEBRO_BACKEND") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => get().server_url,
    }
}

/// Backend device id of this machine (Personal: `JOEBRO_DEVICE_ID` at build time).
pub fn device_id() -> String {
    // A backend running on this machine (bundled / Mac app) owns the local
    // folders itself: there is no separate bridge device to name.
    if matches!(get().mode.as_str(), "bundled" | "mac") {
        return String::new();
    }
    let d = get().device_id;
    if !d.trim().is_empty() {
        d.trim().to_string()
    } else if let (true, Some(id)) = (PERSONAL, option_env!("JOEBRO_DEVICE_ID")) {
        id.into()
    } else {
        "pc".into()
    }
}

/// Normalise what the user typed: trim, default to http://, drop trailing `/`.
pub fn normalize_url(raw: &str) -> String {
    let t = raw.trim().trim_end_matches('/');
    if t.is_empty() || t.contains("://") {
        t.to_string()
    } else {
        format!("http://{t}")
    }
}

/// Stored bearer token (`JOEBRO_TOKEN` env wins), if any.
pub fn load_token() -> Option<String> {
    std::env::var("JOEBRO_TOKEN")
        .ok()
        .or_else(|| std::fs::read_to_string(dir().join("token")).ok())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

pub fn save_token(token: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir())?;
    let path = dir().join("token");
    if token.trim().is_empty() {
        let _ = std::fs::remove_file(path);
        return Ok(());
    }
    write_private(&path, token.trim().as_bytes())
}

/// True when the app has enough to talk to a backend.
pub fn configured() -> bool {
    !server_url().is_empty()
}

/// Write a file readable only by the current user (mode 0600 on Unix; on
/// Windows the per-user %APPDATA% ACL already restricts it).
pub fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_normalisation() {
        assert_eq!(normalize_url(" 10.0.0.2:8771/ "), "http://10.0.0.2:8771");
        assert_eq!(normalize_url("https://x.example/"), "https://x.example");
        assert_eq!(normalize_url(""), "");
    }
    #[cfg(unix)]
    #[test]
    fn private_files_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let p = std::env::temp_dir().join(format!("joebro-priv-{}", std::process::id()));
        write_private(&p, b"x").unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        let _ = std::fs::remove_file(&p);
        assert_eq!(mode, 0o600);
    }
}
