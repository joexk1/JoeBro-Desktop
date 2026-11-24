//! HTTP/SSE client for the Pi-hosted JoeBro backend. Mirrors the Swift
//! APIClient one-for-one; the Rust GUI is a second client of the same backend.
//!
//! Everything here is blocking (called from worker threads, never the UI
//! thread). Panels call these and post results back over a channel.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;

/// Which flavour of JoeBro backend we are talking to. All of them speak the
/// same core API; the differences (see `Api::probe`) are small and handled in
/// the few methods that care.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackendKind {
    /// Not probed yet (or the probe failed): behave like `Server`.
    Unknown = 0,
    /// A full JoeBro server (the Pi-class backend).
    Server = 1,
    /// The bundled cross-platform backend shipped with this app.
    Bundled = 2,
    /// The backend embedded in the JoeBro Mac app (older/leaner route set).
    MacLocal = 3,
}

impl BackendKind {
    fn from_u8(n: u8) -> Self {
        match n {
            1 => Self::Server,
            2 => Self::Bundled,
            3 => Self::MacLocal,
            _ => Self::Unknown,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "JoeBro backend",
            Self::Server => "JoeBro server",
            Self::Bundled => "bundled local backend",
            Self::MacLocal => "JoeBro Mac app backend",
        }
    }
}

#[derive(Clone, Debug)]
pub struct BackendInfo {
    pub kind: BackendKind,
    pub version: String,
}

#[derive(Clone)]
pub struct Api {
    kind: std::sync::Arc<std::sync::atomic::AtomicU8>,
    base: String,
    http: reqwest::blocking::Client,
    stream_http: reqwest::blocking::Client,
}

pub type R<T> = Result<T, String>;

fn e<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

// Thin client over the backend routes; not every route has UI yet.
#[allow(dead_code)]
impl Api {
    pub fn new(base: &str) -> Self {
        Self::with_token(base, crate::config::load_token().as_deref())
    }

    /// Identify the backend: `GET /api/health` (open on every flavour), then
    /// `GET /api/devices`, which the full server has and the Mac app's leaner
    /// backend does not (404). The bundled backend names itself in /health.
    pub fn probe(base: &str, token: &str) -> R<BackendInfo> {
        let base = base.trim().trim_end_matches('/');
        if base.is_empty() {
            return Err("Enter the backend URL first.".into());
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .map_err(e)?;
        let auth = |req: reqwest::blocking::RequestBuilder| {
            let t = token.trim();
            if t.is_empty() { req } else { req.header(reqwest::header::AUTHORIZATION, format!("Bearer {t}")) }
        };
        let health = auth(client.get(format!("{base}/api/health")))
            .send()
            .map_err(|m| format!("Could not reach the backend: {}", m.without_url()))?;
        if matches!(health.status().as_u16(), 401 | 403) {
            return Err("The server is reachable, but rejected the token.".into());
        }
        if !health.status().is_success() {
            return Err(format!("The server answered HTTP {} - is this a JoeBro backend?", health.status().as_u16()));
        }
        let hv: Value = health.json().map_err(|_| "That address answered, but it is not a JoeBro backend.".to_string())?;
        if hv.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err("That address answered, but it is not a JoeBro backend.".into());
        }
        let version = hv.get("version").and_then(Value::as_str).unwrap_or("").to_string();
        if hv.get("backend").and_then(Value::as_str) == Some("joebro-bundled") {
            return Ok(BackendInfo { kind: BackendKind::Bundled, version });
        }
        // Authenticated read-only calls: verify the token and tell Mac from server.
        let prefs = auth(client.get(format!("{base}/api/prefs"))).send().map_err(e)?;
        if matches!(prefs.status().as_u16(), 401 | 403) {
            return Err("The server is reachable, but rejected the token.".into());
        }
        let dev = auth(client.get(format!("{base}/api/devices"))).send().map_err(e)?;
        let kind = if dev.status().as_u16() == 404 { BackendKind::MacLocal } else { BackendKind::Server };
        Ok(BackendInfo { kind, version })
    }

    /// "Test connection": probe + verify the token with a read-only call.
    pub fn test_connection(base: &str, token: &str) -> R<String> {
        let info = Self::probe(base, token)?;
        let base = base.trim().trim_end_matches('/');
        let mut req = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .map_err(e)?
            .get(format!("{base}/api/prefs"));
        let token = token.trim();
        if !token.is_empty() {
            req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
        }
        match req.send() {
            Ok(r) if r.status().is_success() => {
                let v = if info.version.is_empty() { String::new() } else { format!(" v{}", info.version) };
                Ok(format!("Connected to a {}{v}.", info.kind.label()))
            }
            Ok(r) if matches!(r.status().as_u16(), 401 | 403) => {
                Err("The server is reachable, but rejected the token.".into())
            }
            Ok(r) => Err(format!("The server answered HTTP {} - is this a JoeBro backend?", r.status().as_u16())),
            Err(m) => Err(format!("Could not reach the backend: {}", m.without_url())),
        }
    }

    pub fn kind(&self) -> BackendKind {
        BackendKind::from_u8(self.kind.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub fn set_kind(&self, k: BackendKind) {
        self.kind.store(k as u8, std::sync::atomic::Ordering::Relaxed);
    }

    /// Fire-and-forget backend detection; fills `kind()` when it lands.
    pub fn detect_in_background(&self, token: Option<String>) {
        let (base, me) = (self.base.clone(), self.clone());
        std::thread::spawn(move || {
            if let Ok(info) = Api::probe(&base, token.as_deref().unwrap_or("")) {
                me.set_kind(info.kind);
            }
        });
    }

    pub fn with_token(base: &str, token: Option<&str>) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(token) = token.map(str::trim).filter(|token| !token.is_empty()) {
            if let Ok(mut value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
                value.set_sensitive(true);
                headers.insert(reqwest::header::AUTHORIZATION, value);
            }
        }
        Api {
            kind: Default::default(),
            stream_http: reqwest::blocking::Client::builder()
                .default_headers(headers.clone())
                .connect_timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap(),
            base: base.trim_end_matches('/').to_string(),
            http: reqwest::blocking::Client::builder()
                .default_headers(headers)
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base, path.trim_start_matches('/'))
    }

    // ---- primitives --------------------------------------------------------
    pub fn get(&self, path: &str) -> R<Value> {
        let r = self.http.get(self.url(path)).send().map_err(e)?;
        json_body(r)
    }
    pub fn get_q(&self, path: &str, q: &[(&str, &str)]) -> R<Value> {
        let r = self.http.get(self.url(path)).query(q).send().map_err(e)?;
        json_body(r)
    }
    pub fn post_json(&self, path: &str, body: Value) -> R<Value> {
        let r = self
            .http
            .post(self.url(path))
            .json(&body)
            .send()
            .map_err(e)?;
        json_body(r)
    }
    pub fn put_json(&self, path: &str, body: Value) -> R<Value> {
        let r = self
            .http
            .put(self.url(path))
            .json(&body)
            .send()
            .map_err(e)?;
        json_body(r)
    }
    pub fn post_form(&self, path: &str, fields: &[(&str, &str)]) -> R<Value> {
        let r = self
            .http
            .post(self.url(path))
            .form(fields)
            .send()
            .map_err(e)?;
        json_body(r)
    }
    pub fn put_form(&self, path: &str, fields: &[(&str, &str)]) -> R<Value> {
        let r = self
            .http
            .put(self.url(path))
            .form(fields)
            .send()
            .map_err(e)?;
        json_body(r)
    }
    pub fn delete(&self, path: &str) -> R<Value> {
        let r = self.http.delete(self.url(path)).send().map_err(e)?;
        json_body(r)
    }
    pub fn post_bytes(&self, path: &str, ct: &str, bytes: Vec<u8>) -> R<Vec<u8>> {
        let r = self
            .http
            .post(self.url(path))
            .header("Content-Type", ct)
            .body(bytes)
            .send()
            .map_err(e)?;
        checked_bytes(r)
    }
    pub fn get_bytes(&self, path: &str, q: &[(&str, &str)]) -> R<Vec<u8>> {
        let r = self.http.get(self.url(path)).query(q).send().map_err(e)?;
        checked_bytes(r)
    }

    // ---- documents library ---------------------------------------------------
    /// Save text into the Documents library (chat "Save to Documents").
    pub fn create_document(&self, title: &str, content: &str, language: &str) -> R<Value> {
        self.post_json(
            "api/document",
            json!({ "title": title, "content": content, "language": language }),
        )
    }

    /// Render markdown to PDF through a throwaway server document (the Mac
    /// app's exportChatPDF): create → export-pdf → delete.
    pub fn chat_pdf(&self, title: &str, content: &str) -> R<Vec<u8>> {
        let doc = self.create_document(&format!("__pdf_export__{title}"), content, "markdown")?;
        let id = doc
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "PDF export: backend returned no document id".to_string())?
            .to_string();
        let bytes = self.get_bytes(&format!("api/document/{id}/export-pdf"), &[]);
        let _ = self.delete(&format!("api/document/{id}"));
        bytes
    }

    // ---- sessions / chats --------------------------------------------------
    /// This device is the Arch PC — the backend filters sessions to those bound
    /// here or to Nextcloud/Pi (hiding Mac-bound chats).
    pub fn sessions(&self) -> R<Value> {
        self.get_q("api/sessions", &[("device", &crate::config::device_id())])
    }
    pub fn history(&self, sid: &str) -> R<Value> {
        self.get(&format!("api/history/{sid}"))
    }
    pub fn create_session(&self, name: &str, model: &str, ep_url: &str, ep_id: &str) -> R<Value> {
        self.post_form(
            "api/session",
            &[
                ("name", name),
                ("model", model),
                ("endpoint_url", ep_url),
                ("endpoint_id", ep_id),
            ],
        )
    }
    pub fn delete_session(&self, sid: &str) -> R<Value> {
        self.delete(&format!("api/session/{sid}"))
    }
    pub fn rename_session(&self, sid: &str, name: &str) -> R<Value> {
        self.put_form(&format!("api/session/{sid}"), &[("name", name)])
    }
    pub fn set_session_mode(&self, sid: &str, mode: &str) -> R<Value> {
        self.put_form(&format!("api/session/{sid}"), &[("mode", mode)])
    }
    pub fn set_session_folder(&self, sid: &str, folder: &str) -> R<Value> {
        self.put_form(&format!("api/session/{sid}"), &[("folder", folder)])
    }
    pub fn move_session(&self, sid: &str, folder: &str, sort: f64) -> R<Value> {
        self.put_form(
            &format!("api/session/{sid}"),
            &[("folder", folder), ("sort_order", &sort.to_string())],
        )
    }
    pub fn rename_folder(&self, old: &str, new: &str) -> R<Value> {
        self.put_form("api/folder/rename", &[("old", old), ("new", new)])
    }
    pub fn set_session_important(&self, sid: &str, important: bool) -> R<Value> {
        self.post_form(
            &format!("api/session/{sid}/important"),
            &[("important", if important { "true" } else { "false" })],
        )
    }
    pub fn archive_session(&self, sid: &str) -> R<Value> {
        self.put_form(&format!("api/session/{sid}"), &[("archived", "true")])
    }
    pub fn compact_session(&self, sid: &str) -> R<Value> {
        self.post_json(&format!("api/session/{sid}/compact"), json!({}))
    }
    pub fn update_session_model(
        &self,
        sid: &str,
        model: &str,
        ep_url: &str,
        ep_id: &str,
    ) -> R<Value> {
        self.put_form(
            &format!("api/session/{sid}"),
            &[
                ("model", model),
                ("endpoint_url", ep_url),
                ("endpoint_id", ep_id),
            ],
        )
    }
    pub fn session_tip(&self, sid: &str) -> R<Value> {
        self.get(&format!("api/session/{sid}/tip"))
    }
    pub fn delete_messages(&self, sid: &str, ids: &[String]) -> R<Value> {
        self.post_json(
            &format!("api/session/{sid}/delete-messages"),
            json!({ "ids": ids }),
        )
    }
    pub fn append_message(&self, sid: &str, role: &str, content: &str, meta: Value) -> R<Value> {
        let r = self.post_json(
            &format!("api/session/{sid}/append"),
            json!({ "role": role, "content": content, "metadata": meta }),
        );
        // Not every backend can append (Mac app): keep the message client-side.
        match r {
            Err(m) if m.starts_with("HTTP 404") => Ok(json!({ "ok": false, "unsupported": true })),
            other => other,
        }
    }

    // ---- workdir / files ---------------------------------------------------
    pub fn session_workdir(&self, sid: &str) -> R<Value> {
        self.get(&format!("api/session/{sid}/workdir"))
    }
    pub fn bind_workdir(&self, sid: &str, path: &str, device: Option<&str>) -> R<Value> {
        let mut b = json!({ "workdir": path });
        if let Some(d) = device {
            b["device"] = json!(d);
        }
        self.put_json(&format!("api/session/{sid}/workdir"), b)
    }
    pub fn workdir_list(&self, sid: &str, sub: &str) -> R<Value> {
        self.get_q(&format!("api/session/{sid}/workdir/list"), &[("sub", sub)])
    }
    pub fn open_file(&self, sid: &str, sub: &str) -> R<Value> {
        self.post_json(
            &format!("api/session/{sid}/workdir/open-file"),
            json!({ "sub": sub }),
        )
    }
    pub fn save_file(&self, sid: &str, sub: &str, content: &str) -> R<Value> {
        self.post_json(
            &format!("api/session/{sid}/workdir/save-file"),
            json!({ "sub": sub, "content": content }),
        )
    }
    pub fn delete_file(&self, sid: &str, sub: &str) -> R<Value> {
        self.post_json(
            &format!("api/session/{sid}/workdir/delete-file"),
            json!({ "sub": sub }),
        )
    }
    pub fn rename_file(&self, sid: &str, sub: &str, new_name: &str) -> R<Value> {
        self.post_json(
            &format!("api/session/{sid}/workdir/rename-file"),
            json!({ "sub": sub, "new_name": new_name }),
        )
    }
    pub fn workdir_raw(&self, sid: &str, sub: &str) -> R<Vec<u8>> {
        self.get_bytes(&format!("api/session/{sid}/workdir/raw"), &[("sub", sub)])
    }
    pub fn fs_browse(&self, path: &str, device: Option<&str>) -> R<Value> {
        let d = device.unwrap_or("");
        self.get_q("api/fs/browse", &[("path", path), ("device", d)])
    }
    pub fn devices(&self) -> R<Value> {
        // The Mac app's backend has no device registry: everything is local.
        match self.get("api/devices") {
            Err(m) if m.starts_with("HTTP 404") => Ok(json!({ "devices": [] })),
            other => other,
        }
    }
    pub fn docx_extras(&self, sid: &str, sub: &str) -> R<Value> {
        self.get_q(
            &format!("api/session/{sid}/workdir/docx-extras"),
            &[("sub", sub)],
        )
    }

    // ---- converters --------------------------------------------------------
    pub fn xlsx_to_csv(&self, bytes: Vec<u8>) -> R<String> {
        self.post_bytes("api/spreadsheet/to_csv", "application/octet-stream", bytes)
            .map(|b| String::from_utf8_lossy(&b).to_string())
    }
    pub fn csv_to_xlsx(&self, csv: &str) -> R<Vec<u8>> {
        self.post_bytes(
            "api/spreadsheet/to_xlsx",
            "text/csv",
            csv.as_bytes().to_vec(),
        )
    }
    pub fn apkg_to_text(&self, bytes: Vec<u8>) -> R<String> {
        self.post_bytes("api/flashcards/to_text", "application/octet-stream", bytes)
            .map(|b| String::from_utf8_lossy(&b).to_string())
    }
    pub fn text_to_apkg(&self, text: &str, name: &str) -> R<Vec<u8>> {
        let enc: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_string()
                } else {
                    format!("%{:02X}", c as u32)
                }
            })
            .collect();
        self.post_bytes(
            &format!("api/flashcards/to_apkg?name={enc}"),
            "text/plain",
            text.as_bytes().to_vec(),
        )
    }

    // ---- email -------------------------------------------------------------
    pub fn email_status(&self) -> R<Value> {
        self.get("api/email/status")
    }
    pub fn email_folders(&self) -> R<Value> {
        self.get("api/email/folders")
    }
    pub fn email_list(&self, folder: &str, filter: &str, limit: i64) -> R<Value> {
        self.get_q(
            "api/email/list",
            &[
                ("folder", folder),
                ("filter", filter),
                ("limit", &limit.to_string()),
            ],
        )
    }
    pub fn email_read(&self, uid: &str, folder: &str) -> R<Value> {
        self.get_q(&format!("api/email/read/{uid}"), &[("folder", folder)])
    }
    pub fn email_mark_read(&self, uid: &str, folder: &str, read: bool) -> R<Value> {
        let verb = if read { "mark-read" } else { "mark-unread" };
        self.email_action(&format!("api/email/{verb}/{uid}"), folder)
    }
    pub fn email_archive(&self, uid: &str, folder: &str) -> R<Value> {
        self.email_action(&format!("api/email/archive/{uid}"), folder)
    }
    pub fn email_delete(&self, uid: &str, folder: &str) -> R<Value> {
        self.email_action(&format!("api/email/delete/{uid}"), folder)
    }
    fn email_action(&self, path: &str, folder: &str) -> R<Value> {
        json_body(
            self.http
                .post(self.url(path))
                .query(&[("folder", folder)])
                .json(&json!({}))
                .send()
                .map_err(e)?,
        )
    }
    pub fn email_compose_upload(&self, filename: &str, bytes: Vec<u8>) -> R<Value> {
        let part = reqwest::blocking::multipart::Part::bytes(bytes).file_name(filename.to_string());
        let form = reqwest::blocking::multipart::Form::new().part("file", part);
        json_body(
            self.http
                .post(self.url("api/email/compose-upload"))
                .multipart(form)
                .send()
                .map_err(e)?,
        )
    }
    pub fn email_attachment_bytes(&self, uid: &str, folder: &str, index: i64) -> R<Vec<u8>> {
        self.get_bytes(
            &format!("api/email/attachment/{uid}"),
            &[("folder", folder), ("index", &index.to_string())],
        )
    }
    pub fn email_send(&self, fields: Value) -> R<Value> {
        self.post_json("api/email/send", fields)
    }
    /// Unread badge. The Personal backend has no `unread-total` route; an
    /// unread-filtered INBOX listing of one message carries the `total`.
    pub fn email_unread_total(&self) -> R<Value> {
        let v = self.email_list("INBOX", "unread", 1)?;
        let n = v.get("total").and_then(Value::as_i64).unwrap_or(0);
        Ok(json!({ "unread": n, "total": n }))
    }
    pub fn connect_email(&self, email: &str, password: &str) -> R<Value> {
        // JSON like the Mac client (the backend reads these routes as JSON).
        self.post_json("api/email/connect", json!({ "email": email, "password": password }))
    }
    pub fn connect_imap(&self, fields: &[(&str, &str)]) -> R<Value> {
        let mut m = serde_json::Map::new();
        for (k, v) in fields {
            let val = if k.ends_with("_port") { v.trim().parse::<i64>().map(Value::from).unwrap_or(Value::from(*v)) } else { Value::from(*v) };
            m.insert(k.to_string(), val);
        }
        self.post_json("api/email/connect-imap", Value::Object(m))
    }
    pub fn disconnect_email(&self) -> R<Value> {
        self.post_json("api/email/disconnect", json!({}))
    }
    pub fn remove_email_source(&self, id: &str) -> R<Value> {
        self.delete(&format!("api/email/source/{id}"))
    }

    // ---- calendar ----------------------------------------------------------
    pub fn calendar_status(&self) -> R<Value> {
        self.get("api/calendar/status")
    }
    pub fn calendar_events(&self, start: &str, end: &str) -> R<Value> {
        self.get_q("api/calendar/events", &[("start", start), ("end", end)])
    }
    pub fn create_event(&self, fields: Value) -> R<Value> {
        self.post_json("api/calendar/events", fields)
    }
    pub fn update_event(&self, uid: &str, fields: Value) -> R<Value> {
        self.put_json(&format!("api/calendar/events/{uid}"), fields)
    }
    pub fn delete_event(&self, uid: &str) -> R<Value> {
        self.delete(&format!("api/calendar/events/{uid}"))
    }
    pub fn quick_parse_event(&self, text: &str) -> R<Value> {
        self.post_json("api/calendar/quick-parse", json!({ "text": text }))
    }
    pub fn connect_caldav(&self, url: &str, principal: &str, password: &str) -> R<Value> {
        self.post_json(
            "api/calendar/connect-caldav",
            json!({ "url": url, "principal": principal, "password": password }),
        )
    }
    pub fn disconnect_calendar(&self) -> R<Value> {
        self.post_json("api/calendar/disconnect", json!({}))
    }

    // ---- memory / brain ----------------------------------------------------
    pub fn memory(&self) -> R<Value> {
        self.get("api/memory")
    }
    pub fn add_memory(&self, text: &str, category: &str) -> R<Value> {
        self.post_json(
            "api/memory/add",
            json!({ "text": text, "category": category, "source": "user" }),
        )
    }
    pub fn update_memory(&self, id: &str, text: &str, category: &str) -> R<Value> {
        self.put_form(
            &format!("api/memory/{id}"),
            &[("text", text), ("category", category)],
        )
    }
    pub fn pin_memory(&self, id: &str) -> R<Value> {
        self.post_json(&format!("api/memory/{id}/pin"), json!({}))
    }
    pub fn delete_memory(&self, id: &str) -> R<Value> {
        self.delete(&format!("api/memory/{id}"))
    }

    // ---- skills ------------------------------------------------------------
    pub fn skills(&self) -> R<Value> {
        self.get("api/skills")
    }
    pub fn add_skill(&self, body: Value) -> R<Value> {
        self.post_json("api/skills/add", body)
    }
    pub fn update_skill(&self, id: &str, body: Value) -> R<Value> {
        self.put_json(&format!("api/skills/{id}"), body)
    }
    pub fn delete_skill(&self, id: &str) -> R<Value> {
        self.delete(&format!("api/skills/{id}"))
    }
    pub fn learn_skills(&self, path: &str, device: &str) -> R<Value> {
        self.post_json(
            "api/skills/learn",
            json!({ "path": path, "device": device }),
        )
    }
    pub fn learn_skills_status(&self, job: &str) -> R<Value> {
        self.get_q("api/skills/learn-status", &[("job", job)])
    }

    // ---- tasks -------------------------------------------------------------
    pub fn tasks(&self) -> R<Value> {
        self.get("api/tasks")
    }
    pub fn create_task(&self, body: Value) -> R<Value> {
        self.post_json("api/tasks/add", body)
    }
    pub fn update_task(&self, id: &str, body: Value) -> R<Value> {
        self.put_json(&format!("api/tasks/{id}"), body)
    }
    pub fn delete_task(&self, id: &str) -> R<Value> {
        self.delete(&format!("api/tasks/{id}"))
    }
    pub fn run_task(&self, id: &str) -> R<Value> {
        self.post_json(&format!("api/tasks/{id}/run"), json!({}))
    }
    pub fn task_action(&self, id: &str, action: &str) -> R<Value> {
        self.post_json(&format!("api/tasks/{id}/{action}"), json!({}))
    }

    // ---- tools / plugins / mcp --------------------------------------------
    pub fn api_tools(&self) -> R<Value> {
        self.get("api/tools")
    }
    pub fn create_api_tool(&self, fields: &[(&str, &str)]) -> R<Value> {
        self.post_form("api/tools/add", fields)
    }
    pub fn update_api_tool(&self, id: &str, fields: &[(&str, &str)]) -> R<Value> {
        self.put_form(&format!("api/tools/{id}"), fields)
    }
    pub fn delete_api_tool(&self, id: &str) -> R<Value> {
        self.delete(&format!("api/tools/{id}"))
    }
    pub fn plugins(&self) -> R<Value> {
        self.get("api/plugins")
    }
    pub fn create_plugin(&self, fields: &[(&str, &str)]) -> R<Value> {
        self.post_form("api/plugins/add", fields)
    }
    pub fn update_plugin(&self, id: &str, fields: &[(&str, &str)]) -> R<Value> {
        self.put_form(&format!("api/plugins/{id}"), fields)
    }
    pub fn delete_plugin(&self, id: &str) -> R<Value> {
        self.delete(&format!("api/plugins/{id}"))
    }
    pub fn mcp_servers(&self) -> R<Value> {
        self.get("api/mcp")
    }
    pub fn create_mcp(&self, fields: &[(&str, &str)]) -> R<Value> {
        self.post_form("api/mcp/add", fields)
    }
    pub fn update_mcp(&self, id: &str, fields: &[(&str, &str)]) -> R<Value> {
        self.put_form(&format!("api/mcp/{id}"), fields)
    }
    pub fn delete_mcp(&self, id: &str) -> R<Value> {
        self.delete(&format!("api/mcp/{id}"))
    }

    // ---- deep research -----------------------------------------------------
    pub fn research_library(&self) -> R<Value> {
        // `/library` exists on every flavour (the bare route only on servers).
        self.get("api/research/library")
    }
    pub fn research_start(&self, query: &str, ep_id: &str, model: &str) -> R<Value> {
        self.post_json(
            "api/research/start",
            json!({ "query": query, "endpoint_id": ep_id, "model": model }),
        )
    }
    pub fn research_status(&self, id: &str) -> R<Value> {
        self.get(&format!("api/research/status/{id}"))
    }
    pub fn research_detail(&self, id: &str) -> R<Value> {
        self.get(&format!("api/research/detail/{id}"))
    }
    pub fn research_active(&self) -> R<Value> {
        self.get("api/research/active")
    }
    pub fn research_cancel(&self, id: &str) -> R<Value> {
        self.post_json(&format!("api/research/cancel/{id}"), json!({}))
    }
    pub fn research_archive(&self, id: &str) -> R<Value> {
        self.put_json(&format!("api/research/{id}/archive"), json!({}))
    }
    pub fn research_delete(&self, id: &str) -> R<Value> {
        self.delete(&format!("api/research/{id}"))
    }

    // ---- models / endpoints ------------------------------------------------
    pub fn models(&self) -> R<Value> {
        self.get("api/models")
    }
    pub fn model_endpoints(&self) -> R<Value> {
        self.get("api/model-endpoints")
    }
    pub fn add_endpoint(&self, name: &str, base_url: &str, api_key: &str) -> R<Value> {
        self.post_form(
            "api/model-endpoints",
            &[("name", name), ("base_url", base_url), ("api_key", api_key)],
        )
    }
    pub fn delete_endpoint(&self, id: &str) -> R<Value> {
        self.delete(&format!("api/model-endpoints/{id}"))
    }
    pub fn endpoint_models(&self, ep: &str) -> R<Value> {
        self.get(&format!("api/model-endpoints/{ep}/models"))
    }
    pub fn set_hidden_models(&self, ep: &str, hidden: &[String]) -> R<Value> {
        self.put_json(
            &format!("api/model-endpoints/{ep}/models"),
            json!({ "hidden": hidden }),
        )
    }

    // ---- prefs / misc ------------------------------------------------------
    pub fn prefs(&self) -> R<Value> {
        self.get("api/prefs")
    }
    pub fn set_pref(&self, key: &str, value: Value) -> R<Value> {
        self.put_json(&format!("api/prefs/{key}"), json!({ "value": value }))
    }
    /// Composer settings synced across PC / Mac / iOS (agent vs chat, think vs
    /// fast, enabled tools). Uses `/api/client-settings`; a backend that
    /// predates that route falls back to the same record in the prefs table.
    pub fn client_settings(&self) -> R<Value> {
        match self.get("api/client-settings") {
            Ok(v) => Ok(v.get("settings").cloned().unwrap_or(v)),
            Err(err) if err.starts_with("HTTP 404") => Ok(self
                .prefs()?
                .get("client_settings")
                .cloned()
                .unwrap_or(Value::Null)),
            Err(err) => Err(err),
        }
    }
    /// Merge-update the synced composer settings. `full` is the whole record,
    /// written verbatim only on the prefs fallback (which can't merge).
    pub fn put_client_settings(&self, changed: Value, full: Value) -> R<Value> {
        match self.put_json("api/client-settings", changed) {
            Ok(v) => Ok(v.get("settings").cloned().unwrap_or(v)),
            Err(err) if err.starts_with("HTTP 404") => {
                self.set_pref("client_settings", full.clone())?;
                Ok(full)
            }
            Err(err) => Err(err),
        }
    }

    // ---- agents registry (chat runs, research, tasks, briefings, subagents) --
    pub fn agents(&self, kind: &str, status: &str) -> R<Value> {
        self.get_q(
            "api/agents",
            &[("kind", kind), ("status", status), ("limit", "150")],
        )
    }
    pub fn agent_detail(&self, id: &str) -> R<Value> {
        self.get(&format!("api/agents/{id}"))
    }
    pub fn agent_cancel(&self, id: &str) -> R<Value> {
        self.post_json(&format!("api/agents/{id}/cancel"), json!({}))
    }
    pub fn spawn_agents(&self, body: Value) -> R<Value> {
        self.post_json("api/agents", body)
    }

    pub fn ai_check(&self, text: &str) -> R<Value> {
        self.post_json("api/ai-check", json!({ "text": text }))
    }
    pub fn ping(&self) -> R<Value> {
        self.get("api/ping")
    }
    pub fn respond_permission(&self, rid: &str, decision: &str) -> R<Value> {
        // The id is the path segment on every backend flavour.
        self.post_json(
            &format!("api/permission/{rid}"),
            json!({ "request_id": rid, "decision": decision }),
        )
    }
    pub fn upload(&self, filename: &str, bytes: Vec<u8>) -> R<Value> {
        let part = reqwest::blocking::multipart::Part::bytes(bytes).file_name(filename.to_string());
        let form = reqwest::blocking::multipart::Form::new().part("file", part);
        let r = self
            .http
            .post(self.url("api/upload"))
            .multipart(form)
            .send()
            .map_err(e)?;
        json_body(r)
    }

    // ---- chat streaming (SSE) ---------------------------------------------
    /// Streams a chat turn. Each backend SSE event is forwarded verbatim as a
    /// parsed JSON `Value` on `tx`; the worker ends by sending `Value::Null`.
    #[allow(clippy::too_many_arguments)]
    pub fn chat_stream(&self, fields: Vec<(String, String)>, tx: Sender<Value>) {
        let result = self
            .stream_http
            .post(self.url("api/chat_stream"))
            .form(&fields)
            .send();
        match result {
            Ok(resp) if resp.status().is_success() => forward_sse(BufReader::new(resp), &tx),
            Ok(resp) => {
                if let Err(err) = json_body(resp) {
                    let _ = tx.send(json!({ "error": err }));
                }
            }
            Err(err) => {
                let _ = tx.send(json!({ "error": err.to_string() }));
            }
        }
        let _ = tx.send(Value::Null);
    }

    pub fn stop_stream(&self, sid: &str) -> R<Value> {
        self.post_json(&format!("api/session/{sid}/stop"), json!({}))
    }
}

/// Percent-encode a path segment (Swift `seg`): keeps unreserved chars, drops `/ ? # %`.
fn seg(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':' | b'@') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// Routes added for Mac parity (APIClient.swift / MobileAPI.swift / FeatureModels.swift /
// AgentChat.swift / NewCodingAgentSheet.swift / MobileSettingsTab.swift). All blocking.
#[allow(dead_code)]
impl Api {
    // ---- chat ----------------------------------------------------------------
    /// The Mac's stop route (`POST api/chat/stop/{sid}`); `stop_stream` is the older session route.
    pub fn chat_stop(&self, sid: &str) -> R<Value> {
        self.post_json(&format!("api/chat/stop/{}", seg(sid)), json!({}))
    }
    /// Transcribe recorded audio (`POST api/stt/transcribe`, multipart `file`).
    pub fn transcribe(&self, filename: &str, bytes: Vec<u8>) -> R<String> {
        let part = reqwest::blocking::multipart::Part::bytes(bytes).file_name(filename.to_string());
        let form = reqwest::blocking::multipart::Form::new().part("file", part);
        let r = self.http.post(self.url("api/stt/transcribe")).multipart(form).send().map_err(e)?;
        let v = json_body(r)?;
        Ok(v.get("text").and_then(Value::as_str).unwrap_or("").to_string())
    }
    /// Recent documents for the library / palette (`api/documents/library`).
    pub fn documents_library(&self, search: &str) -> R<Value> {
        let mut q = vec![("limit", "50"), ("sort", "recent")];
        if !search.is_empty() {
            q.push(("search", search));
        }
        self.get_q("api/documents/library", &q)
    }
    pub fn update_document(&self, id: &str, content: &str) -> R<Value> {
        self.put_json(&format!("api/document/{id}"), json!({ "content": content }))
    }
    pub fn export_document_pdf(&self, id: &str) -> R<Vec<u8>> {
        self.get_bytes(&format!("api/document/{id}/export-pdf"), &[])
    }
    pub fn update_docx_extra(&self, sid: &str, sub: &str, region: &str, content: &str) -> R<Value> {
        self.post_json(
            &format!("api/session/{sid}/workdir/docx-extra"),
            json!({ "sub": sub, "region": region, "content": content }),
        )
    }
    pub fn save_workdir_raw(&self, sid: &str, sub: &str, bytes: Vec<u8>) -> R<Value> {
        let r = self
            .http
            .post(self.url(&format!("api/session/{sid}/workdir/save-raw")))
            .query(&[("sub", sub)])
            .header("Content-Type", "application/octet-stream")
            .body(bytes)
            .send()
            .map_err(e)?;
        json_body(r)
    }

    // ---- email AI -------------------------------------------------------------
    pub fn email_summarize(&self, uid: &str, folder: &str, subject: &str, from: &str, body: &str) -> R<Value> {
        self.post_json(
            "api/email/summarize",
            json!({ "uid": uid, "folder": folder, "subject": subject, "from": from, "body": body }),
        )
    }
    pub fn email_ai_reply(&self, uid: &str, folder: &str, subject: &str, from: &str, body: &str) -> R<Value> {
        self.post_json(
            "api/email/ai-reply",
            json!({ "uid": uid, "folder": folder, "subject": subject, "from": from, "body": body }),
        )
    }

    // ---- calendar mirror (EventKit sync; unused on Linux) -----------------------------
    pub fn calendar_pending(&self, ack: &[String]) -> R<Value> {
        self.post_json("api/calendar/pending", json!({ "ack": ack }))
    }
    pub fn calendar_mirror(&self, events: Value) -> R<Value> {
        self.post_json("api/calendar/mirror", json!({ "events": events }))
    }

    // ---- notes extras -----------------------------------------------------------------
    pub fn archive_note(&self, id: &str) -> R<Value> {
        self.post_json(&format!("api/notes/{id}/archive"), json!({}))
    }

    // ---- tasks extras ----------------------------------------------------------------
    /// Phone-agent (personal) tasks.
    pub fn personal_tasks(&self) -> R<Value> {
        self.get_q("api/tasks", &[("scope", "personal")])
    }
    pub fn create_starter_task(&self, id: &str) -> R<Value> {
        self.post_json("api/tasks/starters", json!({ "id": id }))
    }

    // ---- agents registry (Mac flavour: includes coding sessions) --------------------------
    pub fn agents_with_coding(&self, kind: &str, status: &str) -> R<Value> {
        let mut q = vec![("include", "coding")];
        if !kind.is_empty() {
            q.push(("kind", kind));
        }
        if !status.is_empty() {
            q.push(("status", status));
        }
        self.get_q("api/agents", &q)
    }
    pub fn agents_typed(&self, kind: &str, status: &str) -> R<crate::models::AgentsEnvelope> {
        self.agents_with_coding(kind, status).map(|v| crate::models::AgentsEnvelope::from_value(&v))
    }
    pub fn agent_detail_typed(&self, id: &str) -> R<crate::models::AgentRun> {
        let v = self.agent_detail(id)?;
        crate::models::AgentRun::from_value(v.get("agent").unwrap_or(&v)).ok_or_else(|| "Agent not found".to_string())
    }

    // ---- coding agents --------------------------------------------------------------------
    /// Start (or resume) a coding agent. `body`: repo, host, agent, permission, prompt?, resume (false | id).
    pub fn coding_start(&self, body: Value) -> R<Value> {
        self.post_json("api/coding-agents/start", body)
    }
    pub fn coding_resume(&self, run_id: &str) -> R<Value> {
        self.post_json("api/coding-agents/start", json!({ "resume": run_id }))
    }
    /// Type an instruction into a coding agent. `mode`: auto | terminal | queue. Returns "delivered".
    pub fn coding_send(&self, id: &str, text: &str, mode: &str) -> R<String> {
        let v = self.post_json(
            &format!("api/coding-agents/{}/send", seg(id)),
            json!({ "text": text, "sender": "app", "mode": mode }),
        )?;
        Ok(v.get("delivered").and_then(Value::as_str).unwrap_or("queued").to_string())
    }
    /// Switch the model; falls back to `/model <m>` in the terminal on older backends.
    pub fn coding_set_model(&self, id: &str, model: &str, harness: &str) -> R<String> {
        match self.post_json(&format!("api/coding-agents/{}/model", seg(id)), json!({ "model": model })) {
            Ok(v) => Ok(v.get("delivered").and_then(Value::as_str).unwrap_or("terminal").to_string()),
            Err(_) => {
                let mode = if harness == "opencode" { "queue" } else { "terminal" };
                self.coding_send(id, &format!("/model {model}"), mode)
            }
        }
    }
    /// The agent's terminal screen (or its last hook lines).
    pub fn coding_screen(&self, id: &str, lines: u32) -> R<String> {
        let v = self.get_q(&format!("api/coding-agents/{}/screen", seg(id)), &[("lines", &lines.to_string())])?;
        if let Some(s) = v.get("screen").and_then(Value::as_str) {
            return Ok(s.to_string());
        }
        Ok(v.get("lines")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default())
    }
    /// Pick an option of the agent's on-screen question (`key`) or type free text. Returns the next prompt, if any.
    pub fn coding_answer(&self, id: &str, key: Option<&str>, text: Option<&str>) -> R<Option<crate::models::AgentPrompt>> {
        let mut body = serde_json::Map::new();
        if let Some(k) = key {
            body.insert("key".into(), json!(k));
        }
        if let Some(t) = text {
            body.insert("text".into(), json!(t));
        }
        let v = self.post_json(&format!("api/coding-agents/{}/answer", seg(id)), Value::Object(body))?;
        Ok(v.get("prompt").and_then(crate::models::AgentPrompt::from_value))
    }
    /// Approve / deny a coding agent's pending permission.
    pub fn coding_decide(&self, id: &str, approve: bool) -> R<Value> {
        self.post_json(
            &format!("api/coding-agents/{}/decide", seg(id)),
            json!({ "decision": if approve { "approve" } else { "deny" } }),
        )
    }
    /// Sessions that can be resumed from the New Coding Agent sheet.
    pub fn resumable_sessions(&self, limit: u32) -> R<Vec<crate::models::ResumableSession>> {
        let v = self.get_q("api/shared-memory/sessions", &[("limit", &limit.to_string())])?;
        Ok(crate::models::arr(&v, "sessions").iter().map(crate::models::ResumableSession::from_value).collect())
    }

    // ---- night shift ---------------------------------------------------------------------
    pub fn night_shift(&self) -> R<crate::models::NightShiftState> {
        self.get("api/night-shift").map(|v| crate::models::NightShiftState::from_value(&v))
    }
    pub fn night_hosts(&self) -> Vec<crate::models::NightHost> {
        self.night_shift().map(|s| s.hosts).unwrap_or_default()
    }
    pub fn night_add(&self, body: Value) -> R<Value> {
        self.post_json("api/night-shift", body)
    }
    pub fn night_reorder(&self, ids: &[String]) -> R<Value> {
        self.post_json("api/night-shift/reorder", json!({ "ids": ids }))
    }
    pub fn night_cancel(&self, id: &str) -> R<Value> {
        self.post_json(&format!("api/night-shift/{}/cancel", seg(id)), json!({}))
    }
    pub fn night_run(&self, id: &str) -> R<Value> {
        self.post_json(&format!("api/night-shift/{}/run", seg(id)), json!({}))
    }
    pub fn night_config_default_host(&self, host: &str) -> R<Value> {
        self.put_json("api/night-shift/config", json!({ "default_host": host }))
    }
    pub fn night_config_enabled(&self, enabled: bool) -> R<Value> {
        self.put_json("api/night-shift/config", json!({ "enabled": enabled }))
    }
    /// `decision`: approve | reject | followup ...; `mode`/`text` optional.
    pub fn night_decide(&self, id: &str, decision: &str, mode: Option<&str>, text: Option<&str>) -> R<Value> {
        let mut b = serde_json::Map::new();
        b.insert("decision".into(), json!(decision));
        if let Some(m) = mode {
            b.insert("mode".into(), json!(m));
        }
        if let Some(t) = text {
            b.insert("text".into(), json!(t));
        }
        self.post_json(&format!("api/night-shift/{}/decide", seg(id)), Value::Object(b))
    }
    pub fn night_diff(&self, id: &str) -> R<String> {
        let v = self.get(&format!("api/night-shift/{}/diff", seg(id)))?;
        Ok(v.get("diff").and_then(Value::as_str).unwrap_or("").to_string())
    }

    // ---- shared memory ----------------------------------------------------------------
    pub fn memory_projects(&self) -> R<Vec<crate::models::MemProject>> {
        let v = self.get("api/shared-memory/projects")?;
        Ok(crate::models::arr(&v, "projects").iter().map(crate::models::MemProject::from_value).collect())
    }
    /// `project` "__all" = every project. `status`: active | archived.
    pub fn memory_facts(&self, project: &str, status: &str, query: &str) -> R<Vec<crate::models::MemFact>> {
        let mut q = vec![("status", status), ("limit", "200")];
        if project != "__all" {
            q.push(("project", project));
        }
        if !query.is_empty() {
            q.push(("q", query));
        }
        let v = self.get_q("api/shared-memory", &q)?;
        Ok(crate::models::arr(&v, "facts").iter().map(crate::models::MemFact::from_value).collect())
    }
    pub fn memory_history(&self, id: &str) -> R<Vec<crate::models::MemFact>> {
        let v = self.get(&format!("api/shared-memory/{}", seg(id)))?;
        Ok(crate::models::arr(&v, "history").iter().map(crate::models::MemFact::from_value).collect())
    }
    /// Edits save as a new version: returns the new version's id.
    pub fn memory_edit(&self, id: &str, text: &str, topic: &str, kind: &str) -> R<Option<String>> {
        let v = self.put_json(
            &format!("api/shared-memory/{}", seg(id)),
            json!({ "text": text, "topic": topic, "kind": kind }),
        )?;
        Ok(v.get("fact").and_then(|f| f.get("id")).and_then(Value::as_str).map(String::from))
    }
    pub fn memory_pin(&self, id: &str, pinned: bool) -> R<Value> {
        self.post_json(&format!("api/shared-memory/{}/pin", seg(id)), json!({ "pinned": pinned }))
    }
    pub fn memory_archive(&self, id: &str) -> R<Value> {
        self.post_json(&format!("api/shared-memory/{}/archive", seg(id)), json!({}))
    }
    pub fn memory_restore(&self, id: &str) -> R<Value> {
        self.post_json(&format!("api/shared-memory/{}/restore", seg(id)), json!({}))
    }
    pub fn memory_add(&self, project: &str, text: &str, kind: &str) -> R<Value> {
        let mut b = json!({ "text": text, "kind": kind, "source": "app", "agent": "app" });
        if project != "__all" {
            b["project"] = json!(project);
        }
        self.post_json("api/shared-memory", b)
    }

    // ---- inbox / notifications -------------------------------------------------------------
    pub fn inbox(&self) -> R<Vec<crate::models::InboxItem>> {
        let v = self.get("api/inbox")?;
        Ok(crate::models::arr(&v, "inbox").iter().filter_map(crate::models::InboxItem::from_value).collect())
    }
    /// `None` marks everything read.
    pub fn mark_inbox_read(&self, id: Option<&str>) -> R<Value> {
        self.post_json("api/inbox/read", id.map_or(json!({}), |i| json!({ "id": i })))
    }
    pub fn delete_inbox(&self, ids: &[String]) -> R<Value> {
        self.post_json("api/inbox/delete", json!({ "ids": ids }))
    }
    pub fn clear_inbox(&self) -> R<Value> {
        self.post_json("api/inbox/delete", json!({ "all": true }))
    }

    // ---- phone memory (Muse files) -------------------------------------------------------------
    pub fn muse_files(&self) -> R<Vec<String>> {
        let v = self.get("api/muse")?;
        Ok(crate::models::arr(&v, "files").iter().filter_map(crate::models::muse_name).collect())
    }
    pub fn muse_read(&self, name: &str) -> R<String> {
        let v = self.get(&format!("api/muse/{}", seg(name)))?;
        Ok(v.get("content").and_then(Value::as_str).unwrap_or("").to_string())
    }
    pub fn muse_write(&self, name: &str, content: &str) -> R<Value> {
        self.put_json(&format!("api/muse/{}", seg(name)), json!({ "content": content }))
    }
    pub fn muse_create(&self, name: &str) -> R<Value> {
        self.post_json("api/muse", json!({ "name": name, "content": "" }))
    }
    pub fn muse_delete(&self, name: &str) -> R<Value> {
        self.delete(&format!("api/muse/{}", seg(name)))
    }

    // ---- phone access / push (Settings > Phone) ---------------------------------------------------
    pub fn mobile_status(&self) -> R<Value> {
        self.get("api/mobile")
    }
    pub fn set_mobile(&self, enabled: bool, bind: &str) -> R<Value> {
        self.post_json("api/mobile/enable", json!({ "enabled": enabled, "bind": bind }))
    }
    pub fn rotate_mobile_token(&self) -> R<Value> {
        self.post_json("api/mobile/rotate", json!({}))
    }
    pub fn mobile_devices(&self) -> R<Vec<crate::models::MobileDevice>> {
        let v = self.get("api/mobile/devices")?;
        Ok(crate::models::arr(&v, "devices").iter().map(crate::models::MobileDevice::from_value).collect())
    }
    pub fn remove_mobile_device(&self, id: &str) -> R<Value> {
        self.delete(&format!("api/mobile/devices/{}", seg(id)))
    }
    pub fn push_status(&self) -> R<Value> {
        self.get("api/push/status")
    }
    pub fn remove_apns_key(&self) -> R<Value> {
        self.delete("api/mobile/apns")
    }
    pub fn send_test_notification(&self) -> R<Value> {
        self.post_json("api/notify", json!({ "message": "Test from JoeBro", "channel": "mobile" }))
    }
    /// Multipart upload of the APNs .p8 key plus team/key/bundle ids.
    pub fn save_apns(&self, key_name: &str, key: Vec<u8>, team_id: &str, key_id: &str, bundle_id: &str) -> R<Value> {
        let part = reqwest::blocking::multipart::Part::bytes(key)
            .file_name(key_name.to_string())
            .mime_str("application/octet-stream")
            .map_err(e)?;
        let form = reqwest::blocking::multipart::Form::new()
            .text("team_id", team_id.to_string())
            .text("key_id", key_id.to_string())
            .text("bundle_id", bundle_id.to_string())
            .part("key", part);
        let r = self.http.post(self.url("api/mobile/apns")).multipart(form).send().map_err(e)?;
        json_body(r)
    }
    pub fn heartbeat(&self) -> R<Value> {
        self.get("api/heartbeat")
    }
    /// `action`: pause | resume.
    pub fn set_heartbeat(&self, action: &str) -> R<Value> {
        self.post_json("api/heartbeat", json!({ "action": action }))
    }
    pub fn pulse(&self) -> R<Value> {
        self.get("api/pulse")
    }
    pub fn set_pulse(&self, body: Value) -> R<Value> {
        self.post_json("api/pulse", body)
    }

    // ---- plugins runtime ---------------------------------------------------------------------
    pub fn plugins_typed(&self) -> R<Vec<crate::models::Plugin>> {
        let v = self.plugins()?;
        Ok(crate::models::arr(&v, "plugins").iter().filter_map(crate::models::Plugin::from_value).collect())
    }
    pub fn call_plugin(&self, slug: &str, tool: &str, arguments: Value) -> R<Value> {
        self.post_json(&format!("api/plugins/{}/call", seg(slug)), json!({ "tool": tool, "arguments": arguments }))
    }
    /// A plugin's UI file: (bytes, content-type).
    pub fn plugin_ui_file(&self, slug: &str, path: &str) -> R<(Vec<u8>, String)> {
        let r = self
            .http
            .get(self.url(&format!("api/plugins/{}/ui/{}", seg(slug), path.trim_start_matches('/'))))
            .send()
            .map_err(e)?;
        let ct = r
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        Ok((checked_bytes(r)?, ct))
    }
}

fn json_body(r: reqwest::blocking::Response) -> R<Value> {
    let status = r.status();
    let text = r.text().map_err(e)?;
    if !status.is_success() {
        // surface the backend's {detail|error} message when present
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            let msg = v
                .get("detail")
                .or_else(|| v.get("error"))
                .and_then(|x| x.as_str())
                .unwrap_or(&text);
            return Err(format!("HTTP {}: {}", status.as_u16(), msg));
        }
        return Err(format!("HTTP {}: {}", status.as_u16(), text));
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(e)
}

fn checked_bytes(r: reqwest::blocking::Response) -> R<Vec<u8>> {
    if !r.status().is_success() {
        return Err(json_body(r)
            .err()
            .unwrap_or_else(|| "HTTP request failed".into()));
    }
    r.bytes().map(|b| b.to_vec()).map_err(e)
}

/// SSE records can contain multiple data lines; comments are heartbeats.
fn forward_sse(reader: impl BufRead, tx: &Sender<Value>) {
    let mut data = String::new();
    let dispatch = |data: &mut String| -> bool {
        if data.is_empty() {
            return true;
        }
        let text = std::mem::take(data);
        if text.trim() == "[DONE]" {
            return false;
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(v) => tx.send(v).is_ok(),
            Err(err) => tx
                .send(json!({"error": format!("Invalid stream event: {err}")}))
                .is_ok(),
        }
    };
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                let _ = tx.send(json!({"error": format!("Stream interrupted: {err}")}));
                return;
            }
        };
        if line.is_empty() {
            if !dispatch(&mut data) {
                return;
            }
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    dispatch(&mut data);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Test connection" distinguishes reachable+authorised, bad token and
    /// unreachable, using a throwaway local server (read-only GET /api/prefs).
    #[test]
    fn test_connection_reports_ok_rejected_and_unreachable() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            // good: health, prefs, devices, prefs; bad: health, prefs
            for _ in 0..6 {
                let (mut socket, _) = listener.accept().unwrap();
                let mut req = Vec::new();
                let mut byte = [0];
                while !req.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    req.push(byte[0]);
                }
                let req = String::from_utf8(req).unwrap().to_lowercase();
                let (status, body) = if req.starts_with("get /api/health ") {
                    ("200 OK", "{\"ok\":true}")
                } else {
                    assert!(req.starts_with("get /api/prefs ") || req.starts_with("get /api/devices "));
                    (if req.contains("bearer good") { "200 OK" } else { "401 Unauthorized" }, "{}")
                };
                write!(socket, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        assert!(Api::test_connection(&base, "good").unwrap().starts_with("Connected"));
        assert!(Api::test_connection(&base, "bad").unwrap_err().contains("rejected"));
        server.join().unwrap();
        assert!(Api::test_connection("http://127.0.0.1:1", "x").unwrap_err().contains("reach"));
        assert!(Api::test_connection("", "x").is_err());
    }

    #[test]
    fn authentication_reaches_regular_and_streaming_requests() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for streaming in [false, true] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap().to_lowercase();
                assert!(request.contains("authorization: bearer test-secret\r\n"));
                let body = if streaming {
                    "data: {\"delta\":\"ok\"}\n\n"
                } else {
                    "{}"
                };
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let api = Api::with_token(&base, Some("test-secret"));
        assert_eq!(api.get("api/sessions").unwrap(), json!({}));
        let (tx, rx) = std::sync::mpsc::channel();
        api.chat_stream(Vec::new(), tx);
        assert_eq!(rx.recv().unwrap(), json!({"delta":"ok"}));
        assert_eq!(rx.recv().unwrap(), Value::Null);
        server.join().unwrap();
    }

    #[test]
    fn sse_records_support_comments_multiline_and_done() {
        let (tx, rx) = std::sync::mpsc::channel();
        forward_sse(std::io::Cursor::new(": ping\n\ndata: {\ndata: \"delta\":\"hello\"}\n\ndata: [DONE]\n\ndata: {\"delta\":\"ignored\"}\n\n"), &tx);
        assert_eq!(rx.try_recv().unwrap(), json!({"delta":"hello"}));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn invalid_stream_data_is_visible() {
        let (tx, rx) = std::sync::mpsc::channel();
        forward_sse(std::io::Cursor::new("data: invalid\n\n"), &tx);
        assert!(rx.try_recv().unwrap().get("error").is_some());
    }
}
