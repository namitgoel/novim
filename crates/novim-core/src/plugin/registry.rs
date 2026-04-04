//! Command registry for plugin-defined ex-commands.
//!
//! Plugins register string commands (e.g., `:PluginList`, `:AutoSave`).
//! Unknown `:` commands are looked up here before returning Noop.

use std::collections::HashMap;

/// A registered command handler.
pub struct RegisteredCommand {
    /// The plugin that registered this command.
    pub plugin_id: String,
    /// Human-readable description (for `:help` / `:PluginList`).
    pub description: String,
}

/// Maps command names to the plugin that handles them.
///
/// When `parse_ex_command` encounters an unknown command, it can check
/// the registry. The plugin manager then dispatches to the owning plugin.
pub struct CommandRegistry {
    commands: HashMap<String, RegisteredCommand>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Register a command. Returns `Err` if the name is already taken.
    pub fn register(
        &mut self,
        name: impl Into<String>,
        plugin_id: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<(), String> {
        let name = name.into();
        if self.commands.contains_key(&name) {
            return Err(format!("Command '{}' is already registered", name));
        }
        self.commands.insert(
            name,
            RegisteredCommand {
                plugin_id: plugin_id.into(),
                description: description.into(),
            },
        );
        Ok(())
    }

    /// Unregister all commands owned by a plugin.
    pub fn unregister_plugin(&mut self, plugin_id: &str) {
        self.commands.retain(|_, cmd| cmd.plugin_id != plugin_id);
    }

    /// Look up which plugin owns a command. Returns `None` if not registered.
    pub fn lookup(&self, name: &str) -> Option<&RegisteredCommand> {
        self.commands.get(name)
    }

    /// List all registered commands (sorted by name).
    pub fn list(&self) -> Vec<(&str, &RegisteredCommand)> {
        let mut entries: Vec<_> = self.commands.iter().map(|(k, v)| (k.as_str(), v)).collect();
        entries.sort_by_key(|(name, _)| *name);
        entries
    }

    /// Return all registered command names (for tab completion).
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.commands.keys().cloned().collect();
        names.sort();
        names
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}
