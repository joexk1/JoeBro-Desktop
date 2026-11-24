//! The bundled local backend: find Python, unpack the embedded backend, start
//! it as a child process on localhost, check its health, stop it again.
//!
//! The backend is the macOS app's `Backend/` folder (pure Python, stdlib only,
//! synced by `backend/sync_from_mac.sh`) plus a tiny shim, zipped as
//! `backend/joebro_backend.pyz` and embedded in the binary, so one executable
//! is all that ships. Like the Mac app it listens on 127.0.0.1, preferring port
//! 8765. Unlike the Mac app it also demands a random bearer token (generated
//! here on first start, kept in the data dir with mode 0600, passed through the
//! `JOEBRO_TOKEN` environment variable, never on the command line).
//!
//! Nothing here blocks the UI: `start`/`stop` are called from worker threads
//! (`Shared::spawn`), `snapshot()` is a cheap lock read.
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The backend, zipped by `backend/build_pyz.py`.
static PYZ: &[u8] = include_bytes!("../backend/joebro_backend.pyz");

/// Oldest Python the backend supports.
pub const MIN_PYTHON: (u32, u32) = (3, 10);
/// First port tried; the next free one is used when it is taken.
pub const PREFERRED_PORT: u16 = 8765;

#[derive(Clone, Debug)]
pub struct PythonInfo {
    pub cmd: PathBuf,
    /// Arguments that must precede the script (`-3` for the Windows `py` launcher).
    pub pre_args: Vec<String>,
    pub version: String,
}

impl PythonInfo {
    pub fn display(&self) -> String {
        let mut s = self.cmd.display().to_string();
        for a in &self.pre_args {
            s.push(' ');
            s.push_str(a);
        }
        format!("{s} ({})", self.version)
    }
}

/// Where the bundled backend keeps its data (database, token, log, files).
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("JOEBRO_BACKEND_DATA") {
        if !d.trim().is_empty() {
            return PathBuf::from(d);
        }
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::config::APP_DIR)
        .join("backend")
}

pub fn log_path() -> PathBuf {
    data_dir().join("backend.log")
}

/// The token the backend generated (empty until its first start).
pub fn read_token() -> Option<String> {
    std::fs::read_to_string(data_dir().join("token"))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

fn probe_python(cmd: &Path, pre: &[&str]) -> Option<PythonInfo> {
    let mut c = Command::new(cmd);
    c.args(pre)
        .args(["-c", "import sys;print('%d.%d.%d'%sys.version_info[:3])"])
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    no_window(&mut c);
    let out = c.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let mut it = version.split('.').filter_map(|p| p.parse::<u32>().ok());
    let (maj, min) = (it.next()?, it.next()?);
    if (maj, min) < MIN_PYTHON {
        return None;
    }
    Some(PythonInfo {
        cmd: cmd.to_path_buf(),
        pre_args: pre.iter().map(|s| s.to_string()).collect(),
        version,
    })
}

/// Result of looking for a usable Python.
pub enum PythonSearch {
    Found(PythonInfo),
    /// Python exists but is older than `MIN_PYTHON` (carries what was found).
    TooOld(String),
    Missing,
}

/// Find Python 3.10+: the configured override, then `python3`, `python` and
/// (Windows) the `py -3` launcher. The Microsoft Store "App execution alias"
/// stubs in `WindowsApps` are skipped: running them opens the Store.
pub fn find_python_ex() -> PythonSearch {
    let over = crate::config::get().python;
    let mut tried_old: Option<String> = None;
    let mut consider = |p: Option<PythonInfo>, cmd: &Path, pre: &[&str]| -> Option<PythonInfo> {
        if p.is_none() {
            // distinguish "too old" from "not python"
            let mut c = Command::new(cmd);
            c.args(pre).arg("--version").stdin(Stdio::null()).stderr(Stdio::null());
            no_window(&mut c);
            if let Ok(o) = c.output() {
                let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if v.starts_with("Python 3") && o.status.success() {
                    tried_old = Some(v);
                }
            }
        }
        p
    };
    if !over.trim().is_empty() {
        let c = PathBuf::from(over.trim());
        let p = probe_python(&c, &[]);
        if let Some(p) = consider(p, &c, &[]) {
            return PythonSearch::Found(p);
        }
    }
    let mut cands: Vec<(PathBuf, Vec<&str>)> = Vec::new();
    for n in ["python3", "python"] {
        if let Some(p) = crate::platform::which(n) {
            if !p.to_string_lossy().contains("WindowsApps") {
                cands.push((p, vec![]));
            }
        }
    }
    if cfg!(windows) {
        if let Some(p) = crate::platform::which("py") {
            cands.push((p, vec!["-3"]));
        }
    }
    for (c, pre) in cands {
        let p = probe_python(&c, &pre);
        if let Some(p) = consider(p, &c, &pre) {
            return PythonSearch::Found(p);
        }
    }
    match tried_old {
        Some(v) => PythonSearch::TooOld(v),
        None => PythonSearch::Missing,
    }
}

/// Clear, OS-specific guidance when Python is missing or too old.
pub fn python_help(s: &PythonSearch) -> String {
    let head = match s {
        PythonSearch::TooOld(v) => format!("Found {v}, but JoeBro needs Python {}.{} or newer.", MIN_PYTHON.0, MIN_PYTHON.1),
        _ => format!("Python {}.{}+ was not found on this computer.", MIN_PYTHON.0, MIN_PYTHON.1),
    };
    let how = if cfg!(windows) {
        "Install it from python.org/downloads (tick \"Add python.exe to PATH\") or run `winget install Python.Python.3.12`, then press Check again."
    } else if cfg!(target_os = "macos") {
        "Install it with `brew install python` or from python.org/downloads, then press Check again."
    } else {
        "Install it with your package manager (for example `sudo apt install python3`, `sudo dnf install python3` or `sudo pacman -S python`), then press Check again."
    };
    format!("{head} {how}")
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Stopped,
    Starting,
    Running { port: u16, pid: u32 },
    Failed(String),
}

struct Mgr {
    child: Option<Child>,
    state: State,
    port: u16,
}

fn mgr() -> &'static Mutex<Mgr> {
    static M: std::sync::OnceLock<Mutex<Mgr>> = std::sync::OnceLock::new();
    M.get_or_init(|| Mutex::new(Mgr { child: None, state: State::Stopped, port: 0 }))
}

/// Current state, noticing a child that died on its own.
pub fn snapshot() -> State {
    let mut m = match mgr().lock() {
        Ok(m) => m,
        Err(_) => return State::Stopped,
    };
    if let Some(c) = m.child.as_mut() {
        if let Ok(Some(status)) = c.try_wait() {
            m.child = None;
            m.state = State::Failed(format!("The backend exited ({status}). See {}", log_path().display()));
        }
    }
    m.state.clone()
}

/// True when this app started the backend and it is still alive.
pub fn started_by_app() -> bool {
    mgr().lock().map(|m| m.child.is_some()).unwrap_or(false)
}

pub fn url_for(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn pick_port() -> u16 {
    if port_free(PREFERRED_PORT) {
        return PREFERRED_PORT;
    }
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(PREFERRED_PORT + 1)
}

#[cfg(windows)]
fn no_window(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}
#[cfg(not(windows))]
fn no_window(_c: &mut Command) {}

/// Write the embedded backend next to its data (once per content hash) and
/// return the path to run. `JOEBRO_BACKEND_PYZ` (a freshly built archive) wins
/// for development.
fn script_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("JOEBRO_BACKEND_PYZ") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
    }
    let mut h: u64 = 0xcbf29ce484222325; // FNV-1a over the archive
    for b in PYZ {
        h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
    }
    let dir = data_dir().join("runtime");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
    let p = dir.join(format!("joebro_backend-{h:016x}.pyz"));
    if !p.is_file() {
        std::fs::write(&p, PYZ).map_err(|e| format!("Can't write {}: {e}", p.display()))?;
        // Drop archives of older app versions.
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                if e.path() != p && e.file_name().to_string_lossy().starts_with("joebro_backend-") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    Ok(p)
}

/// The bearer token for the bundled backend: the stored one, else a fresh
/// random one (OS entropy) saved with mode 0600.
fn ensure_token() -> Result<String, String> {
    if let Some(t) = read_token() {
        return Ok(t);
    }
    let mut raw = [0u8; 32];
    getrandom::getrandom(&mut raw).map_err(|e| format!("No system randomness for the token: {e}"))?;
    let tok: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    crate::config::write_private(&data_dir().join("token"), tok.as_bytes())
        .map_err(|e| format!("Can't save the backend token: {e}"))?;
    Ok(tok)
}

/// Start the bundled backend (no-op returning the port if already running).
/// Blocks until `/api/health` answers (max ~25 s). Call from a worker thread.
pub fn start() -> Result<u16, String> {
    if let State::Running { port, .. } = snapshot() {
        return Ok(port);
    }
    let py = match find_python_ex() {
        PythonSearch::Found(p) => p,
        other => {
            let msg = python_help(&other);
            set_state(State::Failed(msg.clone()));
            return Err(msg);
        }
    };
    set_state(State::Starting);
    let r = start_inner(&py);
    if let Err(e) = &r {
        kill_child();
        set_state(State::Failed(e.clone()));
    }
    r
}

fn set_state(s: State) {
    if let Ok(mut m) = mgr().lock() {
        m.state = s;
    }
}

fn start_inner(py: &PythonInfo) -> Result<u16, String> {
    let data = data_dir();
    std::fs::create_dir_all(&data).map_err(|e| format!("Can't create {}: {e}", data.display()))?;
    let script = script_path()?;
    let token = ensure_token()?;
    let port = pick_port();
    let log = std::fs::File::create(log_path()).map_err(|e| format!("Can't write the log: {e}"))?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&py.cmd);
    cmd.args(&py.pre_args).arg("-u").arg(&script);
    cmd.args(["--host", "127.0.0.1", "--port", &port.to_string(), "--data-dir"])
        .arg(&data)
        .args(["--parent-pid", &std::process::id().to_string()])
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("JOEBRO_TOKEN", &token)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log2));
    no_window(&mut cmd);
    let child = cmd.spawn().map_err(|e| format!("Couldn't start Python ({}): {e}", py.display()))?;
    let pid = child.id();
    if let Ok(mut m) = mgr().lock() {
        m.child = Some(child);
        m.port = port;
    }
    let deadline = Instant::now() + Duration::from_secs(25);
    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    loop {
        if let State::Failed(m) = snapshot() {
            return Err(format!("{m}\n{}", log_tail(12)));
        }
        if let Ok(r) = http.get(format!("{}/api/health", url_for(port))).send() {
            if r.status().is_success() {
                set_state(State::Running { port, pid });
                return Ok(port);
            }
        }
        if Instant::now() > deadline {
            return Err(format!("The backend did not answer within 25 s.\n{}", log_tail(12)));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn kill_child() {
    let child = mgr().lock().ok().and_then(|mut m| m.child.take());
    if let Some(mut c) = child {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// Stop the backend if this app started it: ask politely (`POST /api/shutdown`
/// so SQLite closes cleanly), then kill after a short grace period.
pub fn stop() {
    let (has_child, port) = match mgr().lock() {
        Ok(m) => (m.child.is_some(), m.port),
        Err(_) => return,
    };
    if !has_child {
        return;
    }
    if let (Some(tok), true) = (read_token(), port != 0) {
        if let Ok(http) = reqwest::blocking::Client::builder().timeout(Duration::from_secs(2)).build() {
            let _ = http
                .post(format!("{}/api/shutdown", url_for(port)))
                .header(reqwest::header::AUTHORIZATION, format!("Bearer {tok}"))
                .send();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let done = mgr()
            .lock()
            .ok()
            .and_then(|mut m| m.child.as_mut().map(|c| matches!(c.try_wait(), Ok(Some(_)))))
            .unwrap_or(true);
        if done || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    kill_child();
    set_state(State::Stopped);
}

/// Last `n` lines of the backend log (for error messages / the UI).
pub fn log_tail(n: usize) -> String {
    let text = std::fs::read_to_string(log_path()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}
