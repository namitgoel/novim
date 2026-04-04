//! Plugin lifecycle management and event dispatch.

use std::collections::HashMap;
use std::path::PathBuf;

use super::lua_bridge::LuaPlugin;
use super::registry::CommandRegistry;
use super::{BufferSnapshot, EditorEvent, KeymapRegistry, Plugin, PluginAction, PluginContext, PluginError};

/// Manages all loaded plugins, dispatches events, and owns the command registry.
pub struct PluginManager {
    plugins: Vec<Box<dyn Plugin>>,
    enabled: HashMap<String, bool>,
    error_counts: HashMap<String, usize>,
    pub registry: CommandRegistry,
    pub keymaps: KeymapRegistry,
    is_gui: bool,
    plugin_configs: HashMap<String, HashMap<String, toml::Value>>,
    load_errors: Vec<String>,
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
            keymaps: KeymapRegistry::new(),
            is_gui,
            plugin_configs,
            load_errors: Vec::new(),
        }
    }

    /// Register and initialize a plugin.
    pub fn add(&mut self, mut plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        let id = plugin.id().to_string();

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

    /// Dispatch an event to all enabled plugins. Returns actions to execute.
    pub fn dispatch(&mut self, event: &EditorEvent, snapshot: &BufferSnapshot) -> Vec<PluginAction> {
        let mut all_actions = Vec::new();

        for plugin in &mut self.plugins {
            let id = plugin.id().to_string();

            if !self.enabled.get(&id).copied().unwrap_or(false) {
                continue;
            }

            let config = self.plugin_configs.get(&id).cloned().unwrap_or_default();
            let mut ctx = PluginContext::new(self.is_gui);
            ctx.config = config;
            ctx.buf = snapshot.clone();

            let actions = plugin.on_event(event, &ctx);
            if plugin.had_error() {
                let count = self.error_counts.entry(id.clone()).or_insert(0);
                *count += 1;
                if *count >= 5 {
                    log::warn!("Plugin '{}' auto-disabled after {} consecutive errors", id, count);
                    self.enabled.insert(id.clone(), false);
                    all_actions.push(PluginAction::SetStatus(
                        format!("Plugin '{}' disabled (too many errors)", id),
                    ));
                }
            } else {
                self.error_counts.insert(id, 0);
            }
            all_actions.extend(actions);
        }

        all_actions
    }

    pub fn command_owner(&self, command: &str) -> Option<&str> {
        self.registry.lookup(command).map(|r| r.plugin_id.as_str())
    }

    pub fn shutdown_all(&mut self) {
        for plugin in &mut self.plugins {
            plugin.shutdown();
            log::info!("Shut down plugin: {}", plugin.id());
        }
        self.plugins.clear();
        self.enabled.clear();
        self.error_counts.clear();
    }

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

    pub fn has_command(&self, name: &str) -> bool {
        self.registry.lookup(name).is_some()
    }

    /// Return all registered plugin command names (for tab completion).
    pub fn command_names(&self) -> Vec<String> {
        self.registry.names()
    }

    /// Get a mutable reference to the LSP plugin (for direct command dispatch).
    pub fn lsp_plugin(&mut self) -> Option<&mut super::builtins::lsp_plugin::LspPlugin> {
        for plugin in &mut self.plugins {
            if plugin.id() == "lsp" {
                if let Some(lsp) = plugin.as_any_mut().downcast_mut::<super::builtins::lsp_plugin::LspPlugin>() {
                    return Some(lsp);
                }
            }
        }
        None
    }

    /// Take accumulated load errors (clears the list).
    pub fn take_load_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.load_errors)
    }

    /// Reload a Lua plugin file. Unregisters old plugin, loads fresh.
    pub fn reload_file(&mut self, path: &std::path::Path) {
        // Find and remove any plugin loaded from this path
        let mut removed_id = None;
        self.plugins.retain(|p| {
            let id = p.id().to_string();
            let expected_id = path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| format!("user.{}", s))
                .unwrap_or_default();
            if id == expected_id {
                removed_id = Some(id);
                false
            } else {
                true
            }
        });
        if let Some(id) = &removed_id {
            self.enabled.remove(id);
            self.error_counts.remove(id);
            self.registry.unregister_plugin(id);
            self.keymaps.unregister_plugin(id);
        }
        // Re-load
        self.load_one_lua(path, None);
    }

    /// Poll all plugins for scheduled/deferred callbacks. Returns actions.
    pub fn poll_timers(&mut self) -> Vec<PluginAction> {
        let mut all_actions = Vec::new();
        for plugin in &mut self.plugins {
            let id = plugin.id().to_string();
            if !self.enabled.get(&id).copied().unwrap_or(false) {
                continue;
            }
            all_actions.extend(plugin.poll_timers());
        }
        all_actions
    }

    fn config_dir() -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        Some(PathBuf::from(home).join(".config").join("novim"))
    }

    pub fn load_lua_plugins(&mut self) {
        let Some(config_dir) = Self::config_dir() else {
            return;
        };

        let init_lua = config_dir.join("init.lua");
        if init_lua.is_file() {
            self.load_one_lua(&init_lua, None);
        }

        let plugins_dir = config_dir.join("plugins");
        if plugins_dir.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(&plugins_dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .collect();
            entries.sort_by_key(|e| e.file_name());

            for entry in entries {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "lua") {
                    // Single .lua file plugin (legacy)
                    self.load_one_lua(&path, None);
                } else if path.is_dir() {
                    // Directory-based plugin: look for plugin.toml + init.lua
                    let manifest_path = path.join("plugin.toml");
                    let manifest = super::PluginManifest::from_file(&manifest_path);
                    let entry_file = manifest.as_ref()
                        .and_then(|m| m.entry.as_deref())
                        .unwrap_or("init.lua");
                    let lua_path = path.join(entry_file);
                    if lua_path.is_file() {
                        self.load_one_lua(&lua_path, manifest);
                    }
                }
            }
        }
    }

    fn load_one_lua(&mut self, path: &std::path::Path, manifest: Option<super::PluginManifest>) {
        match LuaPlugin::from_file(path) {
            Ok(mut plugin) => {
                if let Some(ref m) = manifest {
                    if !m.name.is_empty() {
                        log::info!("Plugin manifest: {} v{}", m.name, m.version);
                    }
                    // Check dependencies
                    for dep in &m.dependencies {
                        if !self.enabled.contains_key(dep) {
                            let msg = format!("Plugin {} requires dependency '{}' which is not loaded", m.name, dep);
                            log::warn!("{}", msg);
                            self.load_errors.push(msg);
                        }
                    }
                }
                let id = plugin.id().to_string();

                let config = self.plugin_configs.get(&id).cloned().unwrap_or_default();
                let mut ctx = PluginContext::new(self.is_gui);
                ctx.config = config;
                plugin.init(&mut ctx);

                // Drain init-time actions (keymaps registered at load)
                let init_actions = plugin.drain_init_actions();
                for action in init_actions {
                    if let PluginAction::RegisterKeymap { mode, key, action: km_action } = action {
                        self.keymaps.register(&mode, &key, &id, km_action);
                    }
                }

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
                let msg = format!("Failed to load {}: {}", path.display(), e);
                log::error!("{}", msg);
                self.load_errors.push(msg);
            }
        }
    }
}
