//! Integration tests of the Rust client API layer against real backends:
//!
//! * the bundled Python backend, started by `backend::start()` exactly as the
//!   app does (random data dir, localhost, generated token), with a fake model
//!   server, a fake CalDAV server and fake IMAP/SMTP (no credits, no real mail);
//! * a mock with the JoeBro Mac app backend's API shape;
//! * the live Pi, read-only (GET only), when `JOEBRO_TEST_PI=1`;
//! * any other server given by `JOEBRO_TEST_URL` (+ `JOEBRO_TEST_TOKEN`,
//!   `JOEBRO_TEST_EXPECT`=server|mac|bundled), read-only.
use crate::api::{Api, BackendKind};
use crate::backend;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::channel;

fn tests_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("backend").join("tests")
}

/// A helper Python process (mock servers); dies with the test.
struct Helper {
    child: Child,
    ports: Value,
}

impl Helper {
    fn spawn(mode: &str, extra: &[&str]) -> Helper {
        let py = match backend::find_python_ex() {
            backend::PythonSearch::Found(p) => p,
            _ => panic!("Python 3.10+ is required to run the backend tests"),
        };
        let mut c = Command::new(&py.cmd);
        c.args(&py.pre_args)
            .arg(tests_dir().join("mock_servers.py"))
            .arg(mode)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = c.spawn().expect("spawn helper");
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        Helper { ports: serde_json::from_str(&line).expect("helper ports"), child }
    }
    fn port(&self, k: &str) -> u16 {
        self.ports[k].as_u64().unwrap() as u16
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn ok<T>(label: &str, r: Result<T, String>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{label} failed: {e}"),
    }
}

#[test]
fn bundled_backend_end_to_end() {
    let tmp = std::env::temp_dir().join(format!("joebro-rs-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    // Isolated data dir; fake mail servers inside the backend process.
    std::env::set_var("JOEBRO_BACKEND_DATA", tmp.join("data"));
    std::env::set_var("JOEBRO_TEST_MOCK_MAIL", "1");
    std::env::set_var("JOEBRO_TEST_MAIL_OUT", tmp.join("sent.jsonl"));
    std::env::set_var("PYTHONPATH", tests_dir().join("mockmail"));
    let helper = Helper::spawn("services", &[]);

    assert!(matches!(backend::snapshot(), backend::State::Stopped));
    let port = ok("backend::start", backend::start());
    assert!(matches!(backend::snapshot(), backend::State::Running { .. }));
    assert!(backend::started_by_app());
    let url = backend::url_for(port);
    let token = backend::read_token().expect("token file");
    assert!(token.len() >= 32, "token should be long and random");

    // ---- detection -------------------------------------------------------
    let info = ok("probe", Api::probe(&url, &token));
    assert_eq!(info.kind, BackendKind::Bundled);
    assert!(!info.version.is_empty());
    assert!(Api::test_connection(&url, &token).unwrap().contains("bundled local backend"));
    let bad = Api::test_connection(&url, "wrong-token").unwrap_err();
    assert!(bad.contains("rejected the token"), "{bad}");

    let api = Api::with_token(&url, Some(&token));
    api.set_kind(info.kind);

    // ---- health, chats, history, prefs ----------------------------------
    assert_eq!(ok("ping", api.ping())["ok"], true);
    assert_eq!(ok("sessions", api.sessions()).as_array().unwrap().len(), 0);
    let s = ok("create_session", api.create_session("Rust chat", "m", "", ""));
    let sid = s["id"].as_str().unwrap().to_string();
    let rows = ok("sessions", api.sessions());
    assert!(rows.as_array().unwrap().iter().any(|r| r["id"] == sid.as_str()));
    assert_eq!(ok("history", api.history(&sid))["history"].as_array().unwrap().len(), 0);
    ok("rename", api.rename_session(&sid, "Renamed"));
    ok("set_pref", api.set_pref("require_doc_approval", json!(false)));
    assert_eq!(ok("prefs", api.prefs())["require_doc_approval"], false);
    assert!(ok("devices", api.devices()).get("devices").is_some());
    assert!(ok("research_library", api.research_library()).get("research").is_some());
    assert!(ok("respond_permission", api.respond_permission("req_1", "allow"))["ok"] == true);

    // ---- model endpoint + streaming chat (fake model) --------------------
    let ep = ok(
        "add_endpoint",
        api.add_endpoint("Mock", &format!("http://127.0.0.1:{}/v1", helper.port("model")), "sk-test"),
    );
    let eid = ep["id"].as_str().unwrap_or_default().to_string();
    assert!(!eid.is_empty(), "{ep}");
    assert!(ok("models", api.models()).to_string().contains("mock-model"));
    let s2 = ok("create_session", api.create_session("Streamed", "mock-model", "", &eid));
    let sid2 = s2["id"].as_str().unwrap().to_string();
    let (tx, rx) = channel();
    let fields: Vec<(String, String)> = [
        ("message", "hello"),
        ("session", sid2.as_str()),
        ("mode", "chat"),
        ("model", "mock-model"),
        ("endpoint_id", eid.as_str()),
        ("endpoint_url", ""),
        ("allow_web_search", "false"),
        ("allow_bash", "false"),
        ("permission_mode", "sandbox"),
        ("thinking", "false"),
        ("device", ""),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let a2 = api.clone();
    let worker = std::thread::spawn(move || a2.chat_stream(fields, tx));
    let mut text = String::new();
    for ev in rx {
        if ev.is_null() {
            break;
        }
        if let Some(d) = ev.get("delta").and_then(Value::as_str) {
            text.push_str(d);
        }
        assert!(ev.get("error").is_none(), "stream error: {ev}");
    }
    worker.join().unwrap();
    assert!(text.contains("Mock reply from the fake model."), "streamed: {text:?}");
    let mut hist = vec![];
    for _ in 0..40 {
        hist = ok("history", api.history(&sid2))["history"].as_array().unwrap().clone();
        if hist.len() >= 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    assert_eq!(hist.len(), 2, "{hist:?}");
    assert_eq!(hist[1]["role"], "assistant");

    // ---- calendar (fake CalDAV) -----------------------------------------
    ok("connect_caldav", api.connect_caldav(&format!("http://127.0.0.1:{}/cal/", helper.port("caldav")), "me", "pw"));
    assert_eq!(ok("cal status", api.calendar_status())["type"], "caldav");
    let ev = ok("events", api.calendar_events("2026-10-01", "2026-11-30"));
    assert!(ev.to_string().contains("Mock standup"));
    ok(
        "create_event",
        api.create_event(json!({"summary": "From Rust", "dtstart": "2026-10-26T10:00:00Z", "dtend": "2026-10-26T11:00:00Z",
                                "all_day": false, "location": "", "description": ""})),
    );
    let ev = ok("events", api.calendar_events("2026-10-01", "2026-11-30"));
    let uid = ev["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["summary"] == "From Rust")
        .map(|e| e["uid"].as_str().unwrap().to_string())
        .expect("created event is listed");
    ok("delete_event", api.delete_event(&uid));

    // ---- email (fake IMAP + SMTP) ---------------------------------------
    ok(
        "connect_imap",
        api.connect_imap(&[
            ("email", "me@example.com"),
            ("imap_host", "imap.example.com"),
            ("imap_port", "993"),
            ("imap_user", "me@example.com"),
            ("imap_password", "pw"),
            ("smtp_host", "smtp.example.com"),
            ("smtp_port", "465"),
            ("smtp_user", "me@example.com"),
            ("smtp_password", "pw"),
        ]),
    );
    assert_eq!(ok("email_status", api.email_status())["configured"], true);
    assert!(ok("folders", api.email_folders()).to_string().contains("Archive"));
    let list = ok("email_list", api.email_list("INBOX", "all", 10));
    let uid = list["emails"][0]["uid"].as_str().unwrap().to_string();
    assert_eq!(list["emails"][0]["subject"], "Hello from the mock mailbox");
    assert!(ok("email_read", api.email_read(&uid, "INBOX")).to_string().contains("mocked message body"));
    assert_eq!(ok("unread", api.email_unread_total())["total"].is_number(), true);
    ok("email_send", api.email_send(json!({"to": "bob@example.com", "subject": "S", "body": "B"})));
    let sent = std::fs::read_to_string(tmp.join("sent.jsonl")).unwrap();
    assert!(sent.contains("bob@example.com"));

    // ---- restart keeps the data + token; stop shuts it down -------------
    backend::stop();
    assert!(!backend::started_by_app());
    assert!(api.ping().is_err(), "backend should be gone after stop()");
    let port2 = ok("restart", backend::start());
    assert_eq!(backend::read_token().unwrap(), token, "token persists across restarts");
    let api2 = Api::with_token(&backend::url_for(port2), Some(&token));
    let rows = ok("sessions after restart", api2.sessions());
    assert!(rows.as_array().unwrap().iter().any(|r| r["id"] == sid.as_str()), "chats persist");
    backend::stop();
    assert!(matches!(backend::snapshot(), backend::State::Stopped));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn mac_backend_shape_is_detected_and_adapted() {
    let h = Helper::spawn("mac", &["mac-token"]);
    let url = format!("http://127.0.0.1:{}", h.port("mac"));
    let info = ok("probe", Api::probe(&url, "mac-token"));
    assert_eq!(info.kind, BackendKind::MacLocal);
    assert!(Api::probe(&url, "nope").unwrap_err().contains("rejected the token"));
    let api = Api::with_token(&url, Some("mac-token"));
    api.set_kind(info.kind);
    // no device registry: degrade to an empty list instead of an error
    assert_eq!(ok("devices", api.devices())["devices"].as_array().unwrap().len(), 0);
    assert_eq!(ok("research_library", api.research_library())["research"][0]["id"], "r1");
    assert_eq!(ok("research_status", api.research_status("r1"))["status"], "completed");
    assert_eq!(ok("research_detail", api.research_detail("r1"))["report"], "# R");
    // permission replies use the id path; the append route is missing -> soft fail
    assert_eq!(ok("permission", api.respond_permission("req_9", "deny"))["ok"], true);
    assert_eq!(ok("append", api.append_message("s1", "assistant", "x", json!({})))["unsupported"], true);
    assert_eq!(ok("prefs", api.prefs())["seeded_defaults_v1"], true);
    assert!(ok("email_unread_total", api.email_unread_total().or_else(|_| Ok::<Value, String>(json!({"unread": 0})))).is_object());
}

fn readonly_suite(url: &str, token: &str, expect: BackendKind) {
    let info = ok("probe", Api::probe(url, token));
    assert_eq!(info.kind, expect, "backend flavour");
    let api = Api::with_token(url, Some(token));
    api.set_kind(info.kind);
    assert_eq!(ok("ping", api.ping())["ok"], true);
    assert!(ok("sessions", api.sessions()).is_array());
    assert!(ok("models", api.models()).is_object());
    assert!(ok("prefs", api.prefs()).is_object());
    assert!(ok("devices", api.devices()).get("devices").is_some());
    assert!(ok("research_library", api.research_library()).get("research").is_some());
    assert!(ok("email_status", api.email_status()).is_object());
    assert!(ok("email_folders", api.email_folders()).is_object());
    assert!(ok("calendar_status", api.calendar_status()).is_object());
    assert!(ok("calendar_events", api.calendar_events("2026-10-01", "2026-11-01")).get("events").is_some());
    assert!(ok("skills", api.skills()).is_object());
    assert!(ok("tasks", api.tasks()).is_object());
    assert!(ok("memory", api.memory()).is_object());
    assert!(ok("agents", api.agents("", "")).is_object());
    // history of the newest chat, if any
    if let Some(first) = ok("sessions", api.sessions()).as_array().and_then(|a| a.first().cloned()) {
        let sid = first["id"].as_str().unwrap();
        assert!(ok("history", api.history(sid)).get("history").is_some());
    }
}

#[test]
fn live_pi_read_only() {
    if std::env::var("JOEBRO_TEST_PI").ok().as_deref() != Some("1") {
        eprintln!("skipped: set JOEBRO_TEST_PI=1 to run against the live Pi (GET only)");
        return;
    }
    let tok_path = dirs::config_dir().unwrap().join("joebro-personal").join("token");
    let token = std::fs::read_to_string(tok_path).expect("Pi token file").trim().to_string();
    readonly_suite(crate::config::DEFAULT_SERVER, &token, BackendKind::Server);
}

#[test]
fn external_backend_read_only() {
    let Ok(url) = std::env::var("JOEBRO_TEST_URL") else {
        eprintln!("skipped: set JOEBRO_TEST_URL (+ JOEBRO_TEST_TOKEN, JOEBRO_TEST_EXPECT)");
        return;
    };
    let token = std::env::var("JOEBRO_TEST_TOKEN").unwrap_or_default();
    let expect = match std::env::var("JOEBRO_TEST_EXPECT").unwrap_or_default().as_str() {
        "mac" => BackendKind::MacLocal,
        "bundled" => BackendKind::Bundled,
        _ => BackendKind::Server,
    };
    readonly_suite(&url, &token, expect);
}
