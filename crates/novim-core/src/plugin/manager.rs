//! Plugin lifecycle management and event dispatch.

use std::collections::HashMap;
use std::path::PathBuf;

use super::lua_bridge::LuaPlugin;
use super::registry::CommandRegistry;
use super::{EditorEvent, Plugin, PluginContext, PluginError};

/// Manages all loaded plugins, dispatches events, and owns the command registry.
pub struct PluginManager {
    /// Loaded plugins, keyed by id.
    plugins: Vec<Box<dyn Plugin>>,
    /// Per-plugin enabled state (disabled plugins skip event dispatch).
    enabled: HashMap<String, bool>,
    /// Per-plugin consecutive error count (for auto-disable).
    error_counts: HashMap<String, usize>,
    /// Shared command registry for plugin-defined `:` commands.
    pub registry: CommandRegistry,
    /// Whether the frontend is GUI.
    is_gui: bool,
    /// Per-plugin config from `[plugins.<id>]` in config.toml.
    plugin_configs: HashMap<String, HashMap<String, toml::Value>>,
}

impl PluginManager {
    pub fn new(
        is_gui: bool,
        plugin_configs: HashMap<String, HashMap<String, toml::Value>>,
    ) -> Self {
        Self {
            plugins: Vec::new(),
            enabled: HashMap::new(),
            error_counts: HashMap::new(),
            registry: CommandRegistry::new(),
            is_gui,
            plugin_configs,
        }
    }

    /// Register and initialize a plugin.
    pub fn add(&mut self, mut plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        let id = plugin.id().to_string();

        // Check if explicitly disabled in config
        if let Some(cfg) = self.plugin_configs.get(&id) {
            if let Some(toml::Value::Boolean(false)) = cfg.get("enabled") {
                log::info!("Plugin '{}' is disabled in config, skipping", id);
                return Ok(());
            }
        }

        let config = self.plugin_configs.get(&id).cloned().unwrap_or_default();
        let mut ctx = PluginContext::new(self.is_gui);
        ctx.config = config;

        plugin.init(&mut ctx);
        log::info!("Initialized plugin: {} ({})", plugin.name(), id);

        self.enabled.insert(id.clone(), true);
        self.error_counts.insert(id, 0);
        self.plugins.push(plugin);
        Ok(())
    }

    /// Dispatch an event to all enabled plugins. Returns commands to execute.
    pub fn dispatch(&mut self, event: &EditorEvent) -> Vec<String> {
        let mut all_commands = Vec::new();

        for plugin in &mut self.plugins {
            let id = plugin.id().to_string();

            if !self.enabled.get(&id).copied().unwrap_or(false) {
                continue;
            }

            let config = self.plugin_configs.get(&id).cloned().unwrap_or_default();
            let mut ctx = PluginContext::new(self.is_gui);
            ctx.config = config;

            let commands = plugin.on_event(event, &ctx);

            if commands.iter().any(|c| c.starts_with("__error:")) {
                let count = self.error_counts.entry(id.clone()).or_insert(0);
                *count += 1;
                if *count >= 5 {
                    log::warn!("Plugin '{}' hit 5 consecutive errors, disabling", id);
                    self.enabled.insert(id, false);
                }
            } else {
                self.error_counts.insert(id, 0);
            }

            all_commands.extend(commands);
        }

        all_commands
    }

    /// Look up which plugin owns a command name.
    pub fn command_owner(&self, command: &str) -> Option<&str> {
        self.registry.lookup(command).map(|r| r.plugin_id.as_str())
    }

    /// Shut down all plugins.
    pub fn shutdown_all(&mut self) {
        for plugin in &mut self.plugins {
            plugin.shutdown();
            log::info!("Shut down plugin: {}", plugin.id());
        }
        self.plugins.clear();
        self.enabled.clear();
        self.error_counts.clear();
    }

    /// List all loaded plugins with their enabled state.
    pub fn list(&self) -> Vec<(&str, &str, bool, bool)> {
        self.plugins
            .iter()
            .map(|p| {
                let id = p.id();
                let name = p.name();
                let enabled = self.enabled.get(id).copied().unwrap_or(false);
                let builtin = p.is_builtin();
                (id, name, enabled, builtin)
            })
            .collect()
    }

    /// Check if any plugin handles this command name.
    pub fn has_command(&self, name: &str) -> bool {
        self.registry.lookup(name).is_some()
    }

    /// Get the novim config directory (~/.config/novim/).
    fn config_dir() -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        Some(PathBuf::from(home).join(".config").join("novim"))
    }

    /// Discover and load all Lua plugins from:
    /// 1. `~/.config/novim/init.lua` (if it exists)
    /// 2. `~/.config/novim/plugins/*.lua` (all files)
    pub fn load_lua_plugins(&mut self) {
        let Some(config_dir) = Self::config_dir() else {
            return;
        };

        // 1. Load init.lua
        let init_lua = config_dir.join("init.lua");
        if init_lua.is_file() {
            self.load_one_lua(&init_lua);
        }

        // 2. Load plugins/*.lua
        let plugins_dir = config_dir.join("plugins");
        if plugins_dir.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(&plugins_dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path().extension().is_some_and(|ext| ext == "lua")
                })
                .collect();
            // Sort for deterministic load order
            entries.sort_by_key(|e| e.file_name());

            for entry in entries {
                self.load_one_lua(&entry.path());
            }
        }
    }

    /// Load a single Lua plugin file and register its commands.
    fn load_one_lua(&mut self, path: &std::path::Path) {
        match LuaPlugin::from_file(path) {
            Ok(mut plugin) => {
                let id = plugin.id().to_string();

                // Init the plugin so it registers commands
                let config = self.plugin_configs.get(&id).cloned().unwrap_or_default();
                let mut ctx = PluginContext::new(self.is_gui);
                ctx.config = config;
                plugin.init(&mut ctx);

                // Register any commands the Lua script declared
                for cmd_name in plugin.registered_commands() {
                    if let Err(e) = self.registry.register(&cmd_name, &id, "Lua command") {
                        log::warn!("{}", e);
                    }
                }

                log::info!("Loaded Lua plugin: {} ({})", plugin.name(), path.display());
                self.enabled.insert(id.clone(), true);
                self.error_counts.insert(id, 0);
                self.plugins.push(Box::new(plugin));
            }
            Err(e) => {
                log::error!("Failed to load Lua plugin {}: {}", path.display(), e);
            }
        }
    }
}
