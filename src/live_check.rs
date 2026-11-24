//! Read-only live check of the typed models against the real backend (GET only).
//! Run: `JOEBRO_TEST_PI=1 cargo test --features personal live_models -- --nocapture`
use crate::api::Api;

#[test]
fn live_models() {
    if std::env::var("JOEBRO_TEST_PI").is_err() {
        return;
    }
    let api = Api::with_token(&crate::config::server_url(), crate::config::load_token().as_deref());
    let filled = |n: &str, total: usize, ok: usize| println!("{n}: {ok}/{total} with id/name");
    let a = api.agents_typed("", "").unwrap();
    filled("agents", a.agents.len(), a.agents.iter().filter(|x| !x.id.is_empty() && !x.status.is_empty()).count());
    let c = api.agents_typed("coding", "").unwrap();
    filled("coding agents", c.agents.len(), c.agents.iter().filter(|x| !x.id.is_empty() && !x.source.is_empty()).count());
    let n = api.night_shift().unwrap();
    filled("night tasks", n.tasks.len(), n.tasks.iter().filter(|x| !x.id.is_empty() && !x.status.is_empty()).count());
    println!("night hosts {} cfg {:?}", n.hosts.len(), n.config.enabled);
    match api.memory_projects() { Ok(p) => filled("mem projects", p.len(), p.iter().filter(|x| !x.key.is_empty()).count()), Err(e) => println!("mem projects ERR {e}") }
    match api.memory_facts("", "active", "") { Ok(p) => filled("mem facts", p.len(), p.iter().filter(|x| !x.id.is_empty() && !x.text.is_empty()).count()), Err(e) => println!("mem facts ERR {e}") }
    match api.resumable_sessions(20) { Ok(p) => filled("resumable", p.len(), p.iter().filter(|x| !x.id.is_empty() && !x.agent.is_empty()).count()), Err(e) => println!("resumable ERR {e}") }
    match api.inbox() { Ok(p) => filled("inbox", p.len(), p.iter().filter(|x| !x.title.is_empty()).count()), Err(e) => println!("inbox ERR {e}") }
    match api.plugins_typed() { Ok(p) => filled("plugins", p.len(), p.iter().filter(|x| !x.name.is_empty()).count()), Err(e) => println!("plugins ERR {e}") }
    match api.mobile_devices() { Ok(p) => filled("mobile", p.len(), p.iter().filter(|x| !x.name.is_empty()).count()), Err(e) => println!("mobile ERR {e}") }
    match api.muse_files() { Ok(p) => println!("muse files {}", p.len()), Err(e) => println!("muse ERR {e}") }
    for (name, r) in [
        ("sessions", api.sessions()), ("memory", api.memory()), ("skills", api.skills()), ("tasks", api.tasks()),
        ("tools", api.api_tools()), ("plugins", api.plugins()), ("mcp", api.mcp_servers()), ("research", api.research_library()),
        ("models", api.models()), ("endpoints", api.model_endpoints()), ("prefs", api.prefs()), ("email status", api.email_status()),
        ("email folders", api.email_folders()), ("calendar status", api.calendar_status()), ("devices", api.devices()),
        ("personal tasks", api.personal_tasks()), ("documents", api.documents_library("")), ("research active", api.research_active()),
        ("mobile", api.mobile_status()), ("push", api.push_status()), ("heartbeat", api.heartbeat()), ("pulse", api.pulse()),
        ("client settings", api.client_settings()),
    ] {
        match r {
            Ok(v) => {
                let keys: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_else(|| vec![format!("array[{}]", v.as_array().map(|a| a.len()).unwrap_or(0))]);
                println!("{name}: ok {}", keys.join(","));
            }
            Err(e) => println!("{name}: ERR {e}"),
        }
    }
}
