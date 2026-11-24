//! Thin OS-integration layer: opening files/URLs with the default app and
//! locating optional helper programs, so the rest of the code never shells
//! out to `xdg-open` (or anything else Linux-only) directly.
use std::path::Path;

/// Open a file or URL with the system default application.
pub fn open_path(target: impl AsRef<std::ffi::OsStr>) -> Result<(), String> {
    open::that_detached(target).map_err(|e| e.to_string())
}

/// Open a URL in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    open_path(url)
}

/// Find an executable on PATH (adds `.exe` on Windows).
pub fn which(name: &str) -> Option<std::path::PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        for ext in exts {
            let p = dir.join(format!("{name}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// LibreOffice's command line tool, if installed: `libreoffice`/`soffice` on
/// PATH, then the usual per-OS install locations.
pub fn libreoffice() -> Option<std::path::PathBuf> {
    for n in ["libreoffice", "soffice"] {
        if let Some(p) = which(n) {
            return Some(p);
        }
    }
    let fixed: &[&str] = &[
        r"C:\Program Files\LibreOffice\program\soffice.exe",
        r"C:\Program Files (x86)\LibreOffice\program\soffice.exe",
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",
    ];
    fixed.iter().map(Path::new).find(|p| p.is_file()).map(|p| p.to_path_buf())
}

/// `file://` URL for a local path (LibreOffice's `-env:UserInstallation`).
pub fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

/// `pdftoppm`/`pdfinfo` (poppler) are only needed for PDF thumbnails/pages.
pub fn poppler(tool: &str) -> std::process::Command {
    let mut c = std::process::Command::new(which(tool).unwrap_or_else(|| tool.into()));
    hide_console(&mut c);
    c
}

/// Windows: don't flash a console window when a GUI app starts a helper.
/// No-op elsewhere. Returns the command for chaining.
pub fn hide_console(c: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

/// How to install a missing helper program on this OS (for error messages).
pub fn install_hint(what: &str) -> String {
    let how = if cfg!(windows) {
        "winget install"
    } else if cfg!(target_os = "macos") {
        "brew install"
    } else {
        "your package manager"
    };
    format!("{what} is not installed (try {how}).")
}

/// System font files worth loading as glyph fallbacks (symbols, icons), per OS.
pub fn symbol_fonts() -> &'static [&'static str] {
    if cfg!(windows) {
        &[r"C:\Windows\Fonts\seguisym.ttf"]
    } else if cfg!(target_os = "macos") {
        &["/System/Library/Fonts/Apple Symbols.ttf"]
    } else {
        &["/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf", "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf"]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn which_finds_nothing_for_garbage() {
        assert!(super::which("definitely-not-a-real-binary-xyz").is_none());
    }
}
