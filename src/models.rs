//! Typed views over the backend JSON for the 1.3+ features (Night Shift,
//! Shared Memory, Notifications inbox, coding agents, plugins, phone access).
//! Port of `Models.swift` / `FeatureModels.swift` / `MobileAPI.swift`.
//!
//! The API layer returns raw `serde_json::Value`; each type here has a
//! tolerant `from_value` (every field optional/defaulted, ints-as-strings and
//! 0/1-as-bool accepted) so a backend that adds or drops a field never breaks
//! a screen. `Api::*_typed` helpers return these directly.
#![allow(dead_code)]
use serde_json::Value;

pub fn s(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}
pub fn opt_s(v: &Value, k: &str) -> Option<String> {
    let x = s(v, k);
    (!x.is_empty()).then_some(x)
}
pub fn i(v: &Value, k: &str) -> i64 {
    match v.get(k) {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Some(Value::String(x)) => x.parse().unwrap_or(0),
        Some(Value::Bool(b)) => *b as i64,
        _ => 0,
    }
}
pub fn f(v: &Value, k: &str) -> f64 {
    match v.get(k) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(x)) => x.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}
pub fn b(v: &Value, k: &str) -> bool {
    match v.get(k) {
        Some(Value::Bool(x)) => *x,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        Some(Value::String(x)) => x == "true" || x == "1",
        _ => false,
    }
}
pub fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
fn strs(v: &Value, k: &str) -> Vec<String> {
    arr(v, k).iter().filter_map(|x| x.as_str().map(String::from)).collect()
}

// ---- Night Shift -----------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct NightDiffFile {
    pub path: String,
    pub added: i64,
    pub removed: i64,
}
#[derive(Clone, Debug, Default)]
pub struct NightDiff {
    pub files: i64,
    pub insertions: i64,
    pub deletions: i64,
    pub file_list: Vec<NightDiffFile>,
}
#[derive(Clone, Debug, Default)]
pub struct NightTests {
    pub command: String,
    pub passed: Option<bool>,
    pub duration_s: f64,
    pub output_tail: String,
}
#[derive(Clone, Debug, Default)]
pub struct NightResult {
    pub summary: String,
    pub learned: Vec<String>,
    pub commits: i64,
    pub duration_s: f64,
    pub cost_usd: f64,
    pub screenshot: bool,
    pub diff: Option<NightDiff>,
    pub tests: Option<NightTests>,
}
#[derive(Clone, Debug, Default)]
pub struct NightTask {
    pub id: String,
    pub title: String,
    pub repo_name: String,
    pub repo: String,
    pub prompt: String,
    pub agent: String,
    pub model: String,
    pub host: String,
    pub status: String,
    pub branch: String,
    pub error: String,
    pub window_start: String,
    pub window_end: String,
    pub test_command: String,
    pub on_approve: String,
    pub max_minutes: i64,
    pub position: i64,
    pub task_ref: String,
    pub started_at: String,
    pub followups: Vec<String>,
    pub result: Option<NightResult>,
}
impl NightTask {
    pub fn from_value(v: &Value) -> NightTask {
        let result = v.get("result").filter(|r| r.is_object()).map(|r| NightResult {
            summary: s(r, "summary"),
            learned: strs(r, "learned"),
            commits: i(r, "commits"),
            duration_s: f(r, "duration_s"),
            cost_usd: f(r, "cost_usd"),
            screenshot: b(r, "screenshot"),
            diff: r.get("diff").filter(|d| d.is_object()).map(|d| NightDiff {
                files: i(d, "files"),
                insertions: i(d, "insertions"),
                deletions: i(d, "deletions"),
                file_list: arr(d, "file_list")
                    .iter()
                    .map(|x| NightDiffFile { path: s(x, "path"), added: i(x, "added"), removed: i(x, "removed") })
                    .collect(),
            }),
            tests: r.get("tests").filter(|t| t.is_object()).map(|t| NightTests {
                command: s(t, "command"),
                passed: t.get("passed").and_then(Value::as_bool),
                duration_s: f(t, "duration_s"),
                output_tail: s(t, "output_tail"),
            }),
        });
        NightTask {
            id: s(v, "id"),
            title: s(v, "title"),
            repo_name: s(v, "repo_name"),
            repo: s(v, "repo"),
            prompt: s(v, "prompt"),
            agent: s(v, "agent"),
            model: s(v, "model"),
            host: s(v, "host"),
            status: s(v, "status"),
            branch: s(v, "branch"),
            error: s(v, "error"),
            window_start: s(v, "window_start"),
            window_end: s(v, "window_end"),
            test_command: s(v, "test_command"),
            on_approve: s(v, "on_approve"),
            max_minutes: i(v, "max_minutes"),
            position: i(v, "position"),
            task_ref: s(v, "task_ref"),
            started_at: s(v, "started_at"),
            followups: arr(v, "followups").iter().map(|x| s(x, "text")).collect(),
            result,
        }
    }
    pub fn name(&self) -> String {
        if !self.title.is_empty() {
            self.title.clone()
        } else if self.prompt.is_empty() {
            "Task".into()
        } else {
            self.prompt.chars().take(48).collect()
        }
    }
    pub fn state(&self) -> &str {
        if self.status.is_empty() { "queued" } else { &self.status }
    }
    pub fn needs_review(&self) -> bool {
        self.state() == "review" || (self.state() == "failed" && self.result.is_some())
    }
    pub fn is_open(&self) -> bool {
        matches!(self.state(), "queued" | "running")
    }
    pub fn agent_label(&self) -> &'static str {
        match self.agent.as_str() {
            "claude" => "Claude Code",
            "codex" => "Codex",
            "opencode" => "OpenCode",
            "agy" => "Antigravity",
            _ => "Any agent",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NightHost {
    pub id: String,
    pub name: String,
    pub local: bool,
    pub up: bool,
}
impl NightHost {
    pub fn from_value(v: &Value) -> NightHost {
        NightHost { id: s(v, "id"), name: s(v, "name"), local: b(v, "local"), up: v.get("up").map_or(true, |_| b(v, "up")) }
    }
    pub fn label(&self) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else if self.id.is_empty() {
            "This computer".into()
        } else {
            self.id.clone()
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NightConfig {
    pub enabled: bool,
    pub window_start: String,
    pub window_end: String,
    pub default_agent: String,
    pub default_host: String,
}

#[derive(Clone, Debug, Default)]
pub struct NightShiftState {
    pub tasks: Vec<NightTask>,
    pub config: NightConfig,
    pub hosts: Vec<NightHost>,
}
impl NightShiftState {
    pub fn from_value(v: &Value) -> NightShiftState {
        let c = v.get("config").cloned().unwrap_or(Value::Null);
        NightShiftState {
            tasks: arr(v, "tasks").iter().map(NightTask::from_value).collect(),
            config: NightConfig {
                enabled: b(&c, "enabled"),
                window_start: s(&c, "window_start"),
                window_end: s(&c, "window_end"),
                default_agent: s(&c, "default_agent"),
                default_host: s(&c, "default_host"),
            },
            hosts: arr(v, "hosts").iter().map(NightHost::from_value).collect(),
        }
    }
}

// ---- Shared memory -----------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct MemFact {
    pub id: String,
    pub project: String,
    pub project_name: String,
    pub scope: String,
    pub topic: String,
    pub kind: String,
    pub text: String,
    pub status: String,
    pub pinned: bool,
    pub strength: f64,
    pub recalls: i64,
    pub updated_at: String,
    pub created_at: String,
    pub prov_agent: String,
    pub prov_model: String,
    pub prov_host: String,
    pub prov_source: String,
}
impl MemFact {
    pub fn from_value(v: &Value) -> MemFact {
        let p = v.get("provenance").cloned().unwrap_or(Value::Null);
        MemFact {
            id: s(v, "id"),
            project: s(v, "project"),
            project_name: s(v, "project_name"),
            scope: s(v, "scope"),
            topic: s(v, "topic"),
            kind: s(v, "kind"),
            text: s(v, "text"),
            status: s(v, "status"),
            pinned: b(v, "pinned"),
            strength: if v.get("strength").is_some() { f(v, "strength") } else { 0.5 },
            recalls: i(v, "recalls"),
            updated_at: s(v, "updated_at"),
            created_at: s(v, "created_at"),
            prov_agent: s(&p, "agent"),
            prov_model: s(&p, "model"),
            prov_host: s(&p, "host"),
            prov_source: s(&p, "source"),
        }
    }
    pub fn is_archived(&self) -> bool {
        self.status == "archived"
    }
    pub fn strength_value(&self) -> f64 {
        self.strength.clamp(0.0, 1.0)
    }
    pub fn kind_label(&self) -> String {
        let k = if self.kind.is_empty() { "fact" } else { &self.kind };
        let mut c = k.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }
    /// SF symbol name for the kind (use with `icons::sf`).
    pub fn kind_symbol(&self) -> &'static str {
        match self.kind.as_str() {
            "decision" => "checkmark.seal",
            "gotcha" => "exclamationmark.triangle",
            "preference" => "heart",
            "handoff" => "arrow.right.circle",
            "learned" => "lightbulb",
            _ => "text.quote",
        }
    }
    /// "Claude Code · opus · mac · MCP"
    pub fn provenance_line(&self) -> String {
        let agent = match self.prov_agent.as_str() {
            "claude" => "Claude Code".to_string(),
            "codex" => "Codex".into(),
            "opencode" => "OpenCode".into(),
            "agy" => "Antigravity".into(),
            "app" => "JoeBro app".into(),
            "joebro" => "JoeBro".into(),
            o => o.to_string(),
        };
        let source = match self.prov_source.as_str() {
            "mcp" => "MCP".to_string(),
            "night_shift" => "Night Shift".into(),
            "consolidation" => "consolidated".into(),
            "app" | "" => String::new(),
            o => o.to_string(),
        };
        [agent, self.prov_model.clone(), self.prov_host.clone(), source]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(" \u{b7} ")
    }
}

#[derive(Clone, Debug, Default)]
pub struct MemProject {
    pub key: String,
    pub name: String,
    pub active: i64,
    pub archived: i64,
}
impl MemProject {
    pub fn from_value(v: &Value) -> MemProject {
        MemProject { key: s(v, "key"), name: s(v, "name"), active: i(v, "active"), archived: i(v, "archived") }
    }
    pub fn label(&self) -> String {
        if self.key == "global" {
            "All projects (global)".into()
        } else if self.name.is_empty() {
            self.key.clone()
        } else {
            self.name.clone()
        }
    }
}

// ---- Inbox / notifications ------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct InboxItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub read: bool,
    pub created_at: String,
    pub source: String,
}
impl InboxItem {
    pub fn from_value(v: &Value) -> Option<InboxItem> {
        let id = s(v, "id");
        if id.is_empty() {
            return None;
        }
        Some(InboxItem {
            id,
            kind: s(v, "kind"),
            title: s(v, "title"),
            body: s(v, "body"),
            read: b(v, "read"),
            created_at: s(v, "created_at"),
            source: s(v, "source"),
        })
    }
    /// SF symbol name for the kind.
    pub fn icon(&self) -> &'static str {
        match self.kind.as_str() {
            "heartbeat" | "pulse" => "waveform.path.ecg",
            "task" => "clock.badge.checkmark",
            "agent" => "sparkles",
            "test" => "bell",
            _ => "tray",
        }
    }
}

// ---- Agents registry / coding agents ----------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct PromptOption {
    pub key: String,
    pub label: String,
    pub description: String,
    pub selected: bool,
    pub freeform: bool,
}
/// The question a coding agent is asking (answered with `Api::coding_answer`).
#[derive(Clone, Debug, Default)]
pub struct AgentPrompt {
    pub question: String,
    pub header: String,
    pub options: Vec<PromptOption>,
}
impl AgentPrompt {
    pub fn from_value(v: &Value) -> Option<AgentPrompt> {
        if !v.is_object() {
            return None;
        }
        Some(AgentPrompt {
            question: s(v, "question"),
            header: s(v, "header"),
            options: arr(v, "options")
                .iter()
                .map(|o| PromptOption {
                    key: s(o, "key"),
                    label: s(o, "label"),
                    description: s(o, "description"),
                    selected: b(o, "selected"),
                    freeform: b(o, "freeform"),
                })
                .collect(),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct CodingModel {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct AgentMessage {
    pub role: String,
    pub content: String,
    pub created_at: String,
}

/// One run in the unified agents registry (chat, research, task, subagent, briefing, coding).
#[derive(Clone, Debug, Default)]
pub struct AgentRun {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub status: String,
    pub live: bool,
    /// coding: still a conversation in its terminal.
    pub open: bool,
    /// coding: claude | codex | opencode | agy.
    pub source: String,
    pub host: String,
    pub cwd: String,
    pub progress: String,
    pub session_id: String,
    pub model: String,
    pub available_models: Vec<CodingModel>,
    pub ref_id: String,
    pub parent_id: String,
    pub created_at: String,
    pub started_at: String,
    pub finished_at: String,
    pub error: String,
    pub result: String,
    pub result_chars: i64,
    pub result_truncated: bool,
    pub messages: Vec<AgentMessage>,
    pub prompt: Option<AgentPrompt>,
}
impl AgentRun {
    pub fn from_value(v: &Value) -> Option<AgentRun> {
        let id = s(v, "id");
        if id.is_empty() {
            return None;
        }
        let kind = s(v, "kind");
        let status = s(v, "status");
        Some(AgentRun {
            kind: if kind.is_empty() { "chat".into() } else { kind },
            title: s(v, "title"),
            status: if status.is_empty() { "queued".into() } else { status },
            live: b(v, "live"),
            open: b(v, "open"),
            source: s(v, "source"),
            host: s(v, "host"),
            cwd: s(v, "cwd"),
            progress: s(v, "progress"),
            session_id: s(v, "session_id"),
            model: s(v, "model"),
            available_models: arr(v, "available_models")
                .iter()
                .map(|m| {
                    let (id, label) = match m {
                        Value::String(x) => (x.clone(), String::new()),
                        o => (s(o, "id"), s(o, "label")),
                    };
                    let label = if label.is_empty() { id.rsplit('/').next().unwrap_or(&id).to_string() } else { label };
                    CodingModel { id, label }
                })
                .collect(),
            ref_id: s(v, "ref_id"),
            parent_id: s(v, "parent_id"),
            created_at: s(v, "created_at"),
            started_at: s(v, "started_at"),
            finished_at: s(v, "finished_at"),
            error: s(v, "error"),
            result: s(v, "result"),
            result_chars: i(v, "result_chars"),
            result_truncated: b(v, "result_truncated"),
            messages: arr(v, "messages")
                .iter()
                .map(|m| AgentMessage { role: s(m, "role"), content: s(m, "content"), created_at: s(m, "created_at") })
                .collect(),
            prompt: v.get("prompt").and_then(AgentPrompt::from_value),
            id,
        })
    }
    /// Harness slug for a coding agent ("claude", "codex", "opencode", "agy").
    pub fn harness(&self) -> String {
        if !self.source.is_empty() {
            return self.source.to_lowercase();
        }
        let parts: Vec<&str> = self.id.split(':').collect();
        if parts.first() == Some(&"code") && parts.len() >= 3 {
            return parts[2].to_lowercase();
        }
        let t = self.title.to_lowercase();
        for (p, h) in [("claude", "claude"), ("codex", "codex"), ("opencode", "opencode"), ("agy", "agy"), ("antigravity", "agy")] {
            if t.starts_with(p) {
                return h.into();
            }
        }
        String::new()
    }
    pub fn kind_label(&self) -> String {
        match self.kind.as_str() {
            "research" => "Research".into(),
            "task" => "Task".into(),
            "subagent" => "Subagent".into(),
            "briefing" => "Briefing".into(),
            "coding" => "Coding".into(),
            "chat" | "" => "Chat".into(),
            o => {
                let mut c = o.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }
        }
    }
    /// SF symbol name for the kind.
    pub fn kind_icon(&self) -> &'static str {
        match self.kind.as_str() {
            "research" => "antenna.radiowaves.left.and.right",
            "task" => "clock.badge.checkmark",
            "subagent" => "arrow.triangle.branch",
            "briefing" => "newspaper",
            "coding" => "chevron.left.forwardslash.chevron.right",
            _ => "bubble.left.and.bubble.right",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AgentsEnvelope {
    pub agents: Vec<AgentRun>,
    pub count: i64,
    pub live: i64,
    pub kinds: Vec<String>,
}
impl AgentsEnvelope {
    pub fn from_value(v: &Value) -> AgentsEnvelope {
        AgentsEnvelope {
            agents: arr(v, "agents").iter().filter_map(AgentRun::from_value).collect(),
            count: i(v, "count"),
            live: i(v, "live"),
            kinds: strs(v, "kinds"),
        }
    }
}

// ---- Plugins -----------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct PluginPanel {
    pub id: String,
    pub title: String,
    pub icon: String,
    /// Backend path, e.g. /api/plugins/plant-care/ui/index.html
    pub url: String,
    pub surfaces: Vec<String>,
}
#[derive(Clone, Debug, Default)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub repo_path: String,
    pub description: String,
    pub is_enabled: bool,
    pub source: String,
    pub permission_mode: String,
    pub slug: String,
    pub version: String,
    pub managed: bool,
    pub error: String,
    pub tools: Vec<String>,
    pub guardrail: bool,
    pub panels: Vec<PluginPanel>,
}
impl Plugin {
    pub fn from_value(v: &Value) -> Option<Plugin> {
        let id = s(v, "id");
        if id.is_empty() {
            return None;
        }
        let c = v.get("contributes").cloned().unwrap_or(Value::Null);
        Some(Plugin {
            name: s(v, "name"),
            kind: s(v, "kind"),
            repo_path: s(v, "repo_path"),
            description: s(v, "description"),
            is_enabled: v.get("is_enabled").map_or(true, |_| b(v, "is_enabled")),
            source: s(v, "source"),
            permission_mode: s(v, "permission_mode"),
            slug: s(v, "slug"),
            version: s(v, "version"),
            managed: b(v, "managed"),
            error: s(v, "error"),
            tools: strs(&c, "tools"),
            guardrail: b(&c, "guardrail"),
            panels: arr(&c, "panels")
                .iter()
                .map(|p| PluginPanel {
                    id: s(p, "id"),
                    title: s(p, "title"),
                    icon: s(p, "icon"),
                    url: s(p, "url"),
                    surfaces: strs(p, "surfaces"),
                })
                .collect(),
            id,
        })
    }
    /// "v1.0.0 · 2 tools · 1 panel · guardrail"
    pub fn summary(&self) -> String {
        let mut bits: Vec<String> = vec![];
        if !self.version.is_empty() {
            bits.push(format!("v{}", self.version));
        }
        let plural = |n: usize, w: &str| if n == 1 { format!("1 {w}") } else { format!("{n} {w}s") };
        if !self.tools.is_empty() {
            bits.push(plural(self.tools.len(), "tool"));
        }
        if !self.panels.is_empty() {
            bits.push(plural(self.panels.len(), "panel"));
        }
        if self.guardrail {
            bits.push("guardrail".into());
        }
        bits.join(" \u{b7} ")
    }
}

// ---- Phone access (mobile) -------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct MobileDevice {
    pub id: String,
    pub install_id: String,
    pub name: String,
    pub environment: String,
    pub bundle_id: String,
    pub enabled: bool,
    pub push: bool,
    pub hide_previews: bool,
    pub last_seen: String,
    pub last_success: String,
}
impl MobileDevice {
    pub fn from_value(v: &Value) -> MobileDevice {
        MobileDevice {
            id: s(v, "id"),
            install_id: s(v, "install_id"),
            name: s(v, "name"),
            environment: s(v, "environment"),
            bundle_id: s(v, "bundle_id"),
            enabled: b(v, "enabled"),
            push: b(v, "push"),
            hide_previews: b(v, "hide_previews"),
            last_seen: s(v, "last_seen"),
            last_success: s(v, "last_success"),
        }
    }
}

/// A file in the phone agent's "Muse" memory (`api/muse`).
pub fn muse_name(v: &Value) -> Option<String> {
    v.get("name").and_then(Value::as_str).or_else(|| v.as_str()).map(String::from)
}

/// A resumable coding session offered by the New Coding Agent sheet (`api/shared-memory/sessions`).
#[derive(Clone, Debug, Default)]
pub struct ResumableSession {
    pub id: String,
    pub repo: String,
    pub host: String,
    pub agent: String,
    pub summary: String,
    pub branch: String,
    pub at: String,
    pub resume_id: String,
}
impl ResumableSession {
    pub fn from_value(v: &Value) -> ResumableSession {
        ResumableSession {
            id: s(v, "id"),
            repo: s(v, "repo"),
            host: s(v, "host"),
            agent: s(v, "agent"),
            summary: s(v, "summary"),
            branch: s(v, "branch"),
            at: s(v, "at"),
            resume_id: s(v, "resume_id"),
        }
    }
}

/// "3d ago" style relative time from an ISO timestamp; empty when it does not parse.
pub fn relative_iso(iso: &str) -> String {
    match crate::widgets::iso_to_secs(iso) {
        Some(t) => crate::widgets::relative_from_now(t),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn night_task_defaults_and_state() {
        let t = NightTask::from_value(&json!({"id":"ns_1","status":"failed","result":{"summary":"x","diff":{"files":2}},"prompt":"do the thing"}));
        assert_eq!(t.name(), "do the thing");
        assert!(t.needs_review());
        assert_eq!(NightTask::from_value(&json!({"id":"a"})).state(), "queued");
    }

    #[test]
    fn inbox_read_accepts_ints() {
        let i = InboxItem::from_value(&json!({"id":"i","read":1,"kind":"pulse"})).unwrap();
        assert!(i.read);
        assert_eq!(i.icon(), "waveform.path.ecg");
    }

    #[test]
    fn agent_run_harness_from_id() {
        let a = AgentRun::from_value(&json!({"id":"code:pc:claude:abc","kind":"coding"})).unwrap();
        assert_eq!(a.harness(), "claude");
        assert_eq!(a.kind_label(), "Coding");
    }

    #[test]
    fn mem_provenance_line() {
        let f = MemFact::from_value(&json!({"id":"1","provenance":{"agent":"claude","model":"opus","host":"mac","source":"mcp"}}));
        assert_eq!(f.provenance_line(), "Claude Code \u{b7} opus \u{b7} mac \u{b7} MCP");
    }

    #[test]
    fn plugin_summary() {
        let p = Plugin::from_value(&json!({"id":"p","version":"1.0.0","contributes":{"tools":["a","b"],"guardrail":true,"panels":[{"id":"x","title":"X","url":"/u"}]}})).unwrap();
        assert_eq!(p.summary(), "v1.0.0 \u{b7} 2 tools \u{b7} 1 panel \u{b7} guardrail");
    }
}
