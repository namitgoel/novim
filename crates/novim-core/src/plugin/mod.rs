//! Plugin system for Novim.
//!
//! Provides the `Plugin` trait that all plugins (built-in Rust and user Lua)
//! implement, plus error types and re-exports.

pub mod lua_bridge;
pub mod manager;
pub mod registry;

use std::collections::HashMap;
use std::fmt;

/// Errors that can occur during plugin operations.
#[derive(Debug)]
pub enum PluginError {
    /// Plugin initialization failed.
    Init(String),
    /// Error during event handling.
    Event(String),
    /// Plugin not found by id.
    NotFound(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::Init(msg) => write!(f, "Plugin init error: {}", msg),
            PluginError::Event(msg) => write!(f, "Plugin event error: {}", msg),
            PluginError::NotFound(id) => write!(f, "Plugin not found: {}", id),
        }
    }
}

impl std::error::Error for PluginError {}

// ── Snapshot: read-only buffer state passed to plugins ──

/// Read-only snapshot of the focused buffer, populated before event dispatch.
#[derive(Debug, Clone, Default)]
pub struct BufferSnapshot {
    pub lines: Vec<String>,
    pub line_count: usize,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub path: Option<String>,
    pub is_dirty: bool,
    pub mode: String,
    pub selection: Option<(usize, usize, usize, usize)>,
    pub selected_text: Option<String>,
    // Editor options
    pub tab_width: usize,
    pub expand_tab: bool,
    pub auto_indent: bool,
    pub word_wrap: bool,
    pub line_numbers: String,
    // Pane info
    pub pane_count: usize,
}

// ── PluginAction: structured mutations plugins can request ──

/// Actions plugins can return. Replaces the old `Vec<String>` return type.
#[derive(Debug, Clone)]
pub enum PluginAction {
    /// Execute an ex-command string (e.g. "set wrap", "echo hello").
    ExecCommand(String),
    /// Replace lines[start..end] with new lines.
    SetLines { start: usize, end: usize, lines: Vec<String> },
    /// Insert text at a position.
    InsertText { line: usize, col: usize, text: String },
    /// Move the cursor.
    SetCursor { line: usize, col: usize },
    /// Set the status bar message.
    SetStatus(String),
    /// Register a keybinding at runtime.
    RegisterKeymap { mode: String, key: String, action: KeymapAction },
    /// Set visual selection.
    SetSelection { start_line: usize, start_col: usize, end_line: usize, end_col: usize },
    /// Clear visual selection.
    ClearSelection,
}

/// What happens when a plugin-registered key is pressed.
#[derive(Debug, Clone)]
pub enum KeymapAction {
    /// Execute an ex-command string.
    Command(String),
    /// Call a Lua function (plugin_id + callback key for lookup).
    LuaCallback { plugin_id: String, callback_key: String },
}

// ── Keymap Registry ──

/// Stores keymaps registered by plugins at runtime.
pub struct KeymapRegistry {
    /// (mode, key_string) → action
    keymaps: HashMap<(String, String), KeymapEntry>,
}

pub struct KeymapEntry {
    pub plugin_id: String,
    pub action: KeymapAction,
}

impl KeymapRegistry {
    pub fn new() -> Self {
        Self { keymaps: HashMap::new() }
    }

    pub fn register(&mut self, mode: &str, key: &str, plugin_id: &str, action: KeymapAction) {
        self.keymaps.insert(
            (mode.to_string(), key.to_string()),
            KeymapEntry { plugin_id: plugin_id.to_string(), action },
        );
    }

    /// Look up a keymap for the given mode and key string.
    pub fn lookup(&self, mode: &str, key: &str) -> Option<&KeymapEntry> {
        self.keymaps.get(&(mode.to_string(), key.to_string()))
    }
}

impl Default for KeymapRegistry {
    fn default() -> Self { Self::new() }
}

// ── Plugin trait ──

/// The core trait that all Novim plugins implement.
pub trait Plugin: Send {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn init(&mut self, ctx: &mut PluginContext);
    fn shutdown(&mut self) {}
    /// Called when an editor event fires. Returns actions to execute.
    fn on_event(&mut self, event: &EditorEvent, ctx: &PluginContext) -> Vec<PluginAction>;
    fn is_builtin(&self) -> bool { false }
}

// ── Events ──

/// Events emitted by the editor that plugins can subscribe to.
#[derive(Debug, Clone)]
pub enum EditorEvent {
    BufOpen { path: String },
    BufEnter { path: String },
    BufLeave { path: String },
    BufWrite { path: String },
    BufClose { path: String },
    TextChanged { path: String },
    CursorMoved { line: usize, column: usize },
    ModeChanged { from: String, to: String },
    CommandExecuted { command: String },
}

// ── PluginContext ──

/// Restricted API surface that plugins receive to interact with the editor.
pub struct PluginContext {
    /// Plugin-specific configuration from config.toml `[plugins.<id>]`.
    pub config: HashMap<String, toml::Value>,
    /// Whether the current frontend is a GUI.
    pub is_gui: bool,
    /// Snapshot of the focused buffer (populated before dispatch).
    pub buf: BufferSnapshot,
}

impl PluginContext {
    pub fn new(is_gui: bool) -> Self {
        Self {
            config: HashMap::new(),
            is_gui,
            buf: BufferSnapshot::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::manager::PluginManager;
    use std::sync::{Arc, Mutex};

    struct TestPlugin {
        events: Arc<Mutex<Vec<String>>>,
        was_shutdown: Arc<Mutex<bool>>,
    }

    impl TestPlugin {
        fn new(events: Arc<Mutex<Vec<String>>>, was_shutdown: Arc<Mutex<bool>>) -> Self {
            Self { events, was_shutdown }
        }
    }

    impl Plugin for TestPlugin {
        fn id(&self) -> &str { "test_plugin" }
        fn name(&self) -> &str { "Test Plugin" }
        fn init(&mut self, _ctx: &mut PluginContext) {}

        fn shutdown(&mut self) {
            *self.was_shutdown.lock().unwrap() = true;
        }

        fn on_event(&mut self, event: &EditorEvent, _ctx: &PluginContext) -> Vec<PluginAction> {
            let label = match event {
                EditorEvent::BufOpen { path } => format!("BufOpen:{}", path),
                EditorEvent::BufWrite { path } => format!("BufWrite:{}", path),
                EditorEvent::TextChanged { path } => format!("TextChanged:{}", path),
                EditorEvent::ModeChanged { from, to } => format!("ModeChanged:{}>{}", from, to),
                EditorEvent::CommandExecuted { command } => format!("Cmd:{}", command),
                _ => format!("{:?}", event),
            };
            self.events.lock().unwrap().push(label);
            vec![]
        }
    }

    fn snap() -> BufferSnapshot { BufferSnapshot::default() }

    #[test]
    fn plugin_lifecycle() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(Mutex::new(false));
        let plugin = TestPlugin::new(Arc::clone(&events), Arc::clone(&shutdown));

        let mut manager = PluginManager::new(false, HashMap::new());
        manager.add(Box::new(plugin)).unwrap();

        let list = manager.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0], ("test_plugin", "Test Plugin", true, false));

        manager.dispatch(&EditorEvent::BufOpen { path: "main.rs".into() }, &snap());
        manager.dispatch(&EditorEvent::BufWrite { path: "main.rs".into() }, &snap());
        manager.dispatch(&EditorEvent::TextChanged { path: "main.rs".into() }, &snap());
        manager.dispatch(&EditorEvent::ModeChanged { from: "NORMAL".into(), to: "INSERT".into() }, &snap());

        let log = events.lock().unwrap();
        assert_eq!(log.len(), 4);
        assert_eq!(log[0], "BufOpen:main.rs");
        assert_eq!(log[1], "BufWrite:main.rs");
        assert_eq!(log[2], "TextChanged:main.rs");
        assert_eq!(log[3], "ModeChanged:NORMAL>INSERT");
        drop(log);

        assert!(!*shutdown.lock().unwrap());
        manager.shutdown_all();
        assert!(*shutdown.lock().unwrap());
        assert_eq!(manager.list().len(), 0);
    }

    #[test]
    fn plugin_returns_actions() {
        struct CmdPlugin;
        impl Plugin for CmdPlugin {
            fn id(&self) -> &str { "cmd_plugin" }
            fn name(&self) -> &str { "Command Plugin" }
            fn init(&mut self, _ctx: &mut PluginContext) {}
            fn on_event(&mut self, event: &EditorEvent, _ctx: &PluginContext) -> Vec<PluginAction> {
                if let EditorEvent::BufWrite { .. } = event {
                    vec![PluginAction::ExecCommand("set wrap".into())]
                } else {
                    vec![]
                }
            }
        }

        let mut manager = PluginManager::new(false, HashMap::new());
        manager.add(Box::new(CmdPlugin)).unwrap();

        let actions = manager.dispatch(&EditorEvent::BufWrite { path: "test.rs".into() }, &snap());
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], PluginAction::ExecCommand(s) if s == "set wrap"));

        let actions = manager.dispatch(&EditorEvent::BufOpen { path: "test.rs".into() }, &snap());
        assert!(actions.is_empty());
    }

    #[test]
    fn disabled_plugin_skipped() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(Mutex::new(false));
        let plugin = TestPlugin::new(Arc::clone(&events), Arc::clone(&shutdown));

        let mut configs = HashMap::new();
        let mut plugin_cfg = HashMap::new();
        plugin_cfg.insert("enabled".to_string(), toml::Value::Boolean(false));
        configs.insert("test_plugin".to_string(), plugin_cfg);

        let mut manager = PluginManager::new(false, configs);
        manager.add(Box::new(plugin)).unwrap();
        assert_eq!(manager.list().len(), 0);
    }

    #[test]
    fn command_registry_routing() {
        let mut manager = PluginManager::new(false, HashMap::new());
        manager.registry.register("AutoSave", "auto_save", "Toggle auto-save").unwrap();
        assert!(manager.has_command("AutoSave"));
        assert!(!manager.has_command("DoesNotExist"));
        assert_eq!(manager.command_owner("AutoSave"), Some("auto_save"));
    }

    #[test]
    fn command_registry_duplicate_rejected() {
        use super::registry::CommandRegistry;
        let mut reg = CommandRegistry::new();
        reg.register("Foo", "p1", "desc").unwrap();
        assert!(reg.register("Foo", "p2", "other").is_err());
    }

    #[test]
    fn command_registry_unregister_plugin() {
        use super::registry::CommandRegistry;
        let mut reg = CommandRegistry::new();
        reg.register("Cmd1", "p1", "desc").unwrap();
        reg.register("Cmd2", "p1", "desc").unwrap();
        reg.register("Cmd3", "p2", "desc").unwrap();

        reg.unregister_plugin("p1");
        assert!(reg.lookup("Cmd1").is_none());
        assert!(reg.lookup("Cmd2").is_none());
        assert!(reg.lookup("Cmd3").is_some());
    }

    #[test]
    fn plugin_command_parsed_from_ex() {
        use crate::input::{parse_ex_command, EditorCommand};

        assert!(!matches!(parse_ex_command("w"), EditorCommand::PluginCommand(..)));
        assert!(!matches!(parse_ex_command("q"), EditorCommand::PluginCommand(..)));

        match parse_ex_command("AutoSave") {
            EditorCommand::PluginCommand(name, args) => {
                assert_eq!(name, "AutoSave");
                assert_eq!(args, "");
            }
            other => panic!("Expected PluginCommand, got {:?}", other),
        }

        match parse_ex_command("Format rust") {
            EditorCommand::PluginCommand(name, args) => {
                assert_eq!(name, "Format");
                assert_eq!(args, "rust");
            }
            other => panic!("Expected PluginCommand, got {:?}", other),
        }
    }
}
