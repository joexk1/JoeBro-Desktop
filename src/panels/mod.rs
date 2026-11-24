//! One module per workspace tab. Each panel owns its local state and async
//! jobs; it renders via `ui(ui, shared)` and reaches the backend through
//! `shared.api` / `shared.spawn(...)`.
pub mod agents;
pub mod attachments;
pub mod aicheck;
pub mod brain;
pub mod calendar;
pub mod chat;
pub mod coding_agents;
pub mod editor;
pub mod editor_chrome;
pub mod email;
pub mod inbox;
pub mod mdview;
pub mod message_bot;
pub mod mobile_settings;
pub mod night_shift;
pub mod notes;
pub mod palette;
pub mod phone_memory;
pub mod plugins;
pub mod research;
pub mod settings;
pub mod shared_memory;
pub mod sidebar;
pub mod skills;
pub mod tasks;
pub mod tools;
pub mod workkit;

use crate::state::Tab;

/// Tabs whose panel draws its own `widgets::panel_chrome` surface. For every other tab the shell
/// paints the flat panel surface behind the panel. When you migrate a panel to `panel_chrome`
/// (or the Chats header-card layout), add its tab here.
pub const SELF_CHROME: &[Tab] = &[
    Tab::Chats, Tab::NightShift, Tab::Inbox, Tab::Plugins, Tab::Memory, Tab::Brain, Tab::Notes, Tab::Calendar, Tab::Email, Tab::Agents, Tab::Tasks, Tab::Skills, Tab::Tools, Tab::Research, Tab::AICheck,
];
