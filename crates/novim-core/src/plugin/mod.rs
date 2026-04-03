//! Plugin system for Novim.
//!
//! Provides the `Plugin` trait that all plugins (built-in Rust and user Lua)
//! implement, plus error types and re-exports.

pub mod lua_bridge;
pub mod manager;
pub mod registry;

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

/// The core trait that all Novim plugins implement.
///
/// Built-in plugins (git_signs, lsp, syntax, etc.) implement this in Rust.
/// User plugins implement it via the Lua bridge.
pub trait Plugin: Send {
    /// Unique identifier (e.g. "git_signs", "user.auto_save").
    fn id(&self) -> &str;

    /// Human-readable name (e.g. "Git Signs").
    fn name(&self) -> &str;

    /// Called once when the plugin is loaded.
    fn init(&mut self, ctx: &mut PluginContext);

    /// Called when the plugin is unloaded.
    fn shutdown(&mut self) {}

    /// Called when an editor event fires. Returns commands to execute.
    fn on_event(&mut self, event: &EditorEvent, ctx: &PluginContext) -> Vec<String>;

    /// Whether this is a built-in plugin (cannot be uninstalled).
    fn is_builtin(&self) -> bool {
        false
    }
}

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

/// Restricted API surface that plugins receive to interact with the editor.
///
/// Plugins cannot hold &mut EditorState directly — they go through this context
/// which queues commands for the host to apply between frames.
pub struct PluginContext {
    /// Commands queued by plugins, applied by the host after event dispatch.
    pub queued_commands: Vec<String>,
    /// Plugin-specific configuration from config.toml `[plugins.<id>]`.
    pub config: std::collections::HashMap<String, toml::Value>,
    /// Whether the current frontend is a GUI.
    pub is_gui: bool,
}

impl PluginContext {
    pub fn new(is_gui: bool) -> Self {
        Self {
            queued_commands: Vec::new(),
            config: std::collections::HashMap::new(),
            is_gui,
        }
    }

    /// Queue a command for execution after the current event dispatch.
    pub fn exec(&mut self, command: impl Into<String>) {
        self.queued_commands.push(command.into());
    }

    /// Drain all queued commands.
    pub fn take_commands(&mut self) -> Vec<String> {
        std::mem::take(&mut self.queued_commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::manager::PluginManager;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// A test plugin that logs events it receives and responds to BufWrite
    /// by returning a status command.
    struct TestPlugin {
        events: Arc<Mutex<Vec<String>>>,
        initialized: bool,
        was_shutdown: Arc<Mutex<bool>>,
    }

    impl TestPlugin {
        fn new(events: Arc<Mutex<Vec<String>>>, was_shutdown: Arc<Mutex<bool>>) -> Self {
            Self { events, initialized: false, was_shutdown }
        }
    }

    impl Plugin for TestPlugin {
        fn id(&self) -> &str { "test_plugin" }
        fn name(&self) -> &str { "Test Plugin" }

        fn init(&mut self, _ctx: &mut PluginContext) {
            self.initialized = true;
        }

        fn shutdown(&mut self) {
            *self.was_shutdown.lock().unwrap() = true;
        }

        fn on_event(&mut self, event: &EditorEvent, _ctx: &PluginContext) -> Vec<String> {
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

    #[test]
    fn plugin_lifecycle() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(Mutex::new(false));
        let plugin = TestPlugin::new(Arc::clone(&events), Arc::clone(&shutdown));

        let mut manager = PluginManager::new(false, HashMap::new());
        manager.add(Box::new(plugin)).unwrap();

        // Should be listed as enabled
        let list = manager.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0], ("test_plugin", "Test Plugin", true, false));

        // Dispatch events
        manager.dispatch(&EditorEvent::BufOpen { path: "main.rs".into() });
        manager.dispatch(&EditorEvent::BufWrite { path: "main.rs".into() });
        manager.dispatch(&EditorEvent::TextChanged { path: "main.rs".into() });
        manager.dispatch(&EditorEvent::ModeChanged { from: "NORMAL".into(), to: "INSERT".into() });

        let log = events.lock().unwrap();
        assert_eq!(log.len(), 4);
        assert_eq!(log[0], "BufOpen:main.rs");
        assert_eq!(log[1], "BufWrite:main.rs");
        assert_eq!(log[2], "TextChanged:main.rs");
        assert_eq!(log[3], "ModeChanged:NORMAL>INSERT");
        drop(log);

        // Shutdown
        assert!(!*shutdown.lock().unwrap());
        manager.shutdown_all();
        assert!(*shutdown.lock().unwrap());
        assert_eq!(manager.list().len(), 0);
    }

    #[test]
    fn plugin_returns_commands() {
        struct CmdPlugin;
        impl Plugin for CmdPlugin {
            fn id(&self) -> &str { "cmd_plugin" }
            fn name(&self) -> &str { "Command Plugin" }
            fn init(&mut self, _ctx: &mut PluginContext) {}
            fn on_event(&mut self, event: &EditorEvent, _ctx: &PluginContext) -> Vec<String> {
                if let EditorEvent::BufWrite { .. } = event {
                    vec!["set wrap".to_string()]
                } else {
                    vec![]
                }
            }
        }

        let mut manager = PluginManager::new(false, HashMap::new());
        manager.add(Box::new(CmdPlugin)).unwrap();

        let cmds = manager.dispatch(&EditorEvent::BufWrite { path: "test.rs".into() });
        assert_eq!(cmds, vec!["set wrap"]);

        let cmds = manager.dispatch(&EditorEvent::BufOpen { path: "test.rs".into() });
        assert!(cmds.is_empty());
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

        // Plugin should not be loaded at all
        assert_eq!(manager.list().len(), 0);
    }

    #[test]
    fn command_registry_routing() {
        let mut manager = PluginManager::new(false, HashMap::new());

        // Register a command
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

        // Known command → not a PluginCommand
        assert!(!matches!(parse_ex_command("w"), EditorCommand::PluginCommand(..)));
        assert!(!matches!(parse_ex_command("q"), EditorCommand::PluginCommand(..)));

        // Unknown command → PluginCommand
        match parse_ex_command("AutoSave") {
            EditorCommand::PluginCommand(name, args) => {
                assert_eq!(name, "AutoSave");
                assert_eq!(args, "");
            }
            other => panic!("Expected PluginCommand, got {:?}", other),
        }

        // Unknown command with args
        match parse_ex_command("Format rust") {
            EditorCommand::PluginCommand(name, args) => {
                assert_eq!(name, "Format");
                assert_eq!(args, "rust");
            }
            other => panic!("Expected PluginCommand, got {:?}", other),
        }
    }
}
