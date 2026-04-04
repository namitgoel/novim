//! Lua plugin bridge — wraps a Lua script as a `Plugin` impl.
//!
//! Each Lua file gets its own `LuaPlugin` with an isolated `mlua::Lua` VM.
//! The Lua script registers event handlers via the `novim` API table,
//! and the bridge dispatches `EditorEvent`s to those handlers.

mod api;
mod dispatch;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mlua::{Lua, Value};

use super::{EditorEvent, Plugin, PluginAction, PluginContext};

/// A single Lua plugin loaded from a `.lua` file.
pub struct LuaPlugin {
    id: String,
    name: String,
    path: PathBuf,
    lua: Lua,
    last_had_error: bool,
}

impl LuaPlugin {
    /// Load a Lua plugin from a file path.
    /// The plugin id is derived from the filename (e.g. `auto_save.lua` → `user.auto_save`).
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let stem = path.file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("Invalid plugin filename: {:?}", path))?;

        let id = format!("user.{}", stem);
        let name = stem.replace('_', " ");

        let lua = Lua::new();

        // Read and load the script (but don't execute yet — that happens in init)
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

        // Pre-compile to catch syntax errors early
        lua.load(&source).set_name(stem)
            .into_function()
            .map_err(|e| format!("Lua syntax error in {}: {}", path.display(), e))?;

        Ok(Self {
            id,
            name,
            path: path.to_path_buf(),
            lua,
            last_had_error: false,
        })
    }

    /// Set up the `novim` API table and execute the plugin script.
    fn setup_api_and_run(&self) -> Result<(), String> {
        self.lua.scope(|_scope| {
            // We set up the API table outside the scope since it needs to persist
            Ok(())
        }).map_err(|e| format!("Lua scope error: {}", e))?;

        // Create the novim API table
        let novim = self.lua.create_table()
            .map_err(|e| format!("Failed to create novim table: {}", e))?;

        // ── Helper: push a structured action into _action_queue ──
        let push_action_fn = self.lua.create_function(|lua, action: mlua::Table| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let queue: mlua::Table = match novim.get::<Value>("_action_queue")? {
                Value::Table(t) => t,
                _ => {
                    let t = lua.create_table()?;
                    novim.set("_action_queue", t.clone())?;
                    t
                }
            };
            let len = queue.len()?;
            queue.set(len + 1, action)?;
            Ok(())
        }).map_err(|e| format!("Failed to create _push_action: {}", e))?;
        novim.set("_push_action", push_action_fn)
            .map_err(|e| format!("Failed to set _push_action: {}", e))?;

        // Set up each API subsystem
        self.setup_event_system(&novim)?;
        self.setup_buf_api(&novim)?;
        self.setup_fn_api(&novim)?;
        self.setup_ui_api(&novim)?;
        self.setup_opt_api(&novim)?;
        self.setup_win_api(&novim)?;
        self.setup_keymap_api(&novim)?;
        self.setup_misc_api(&novim)?;

        // Set global
        self.lua.globals().set("novim", novim)
            .map_err(|e| format!("Failed to set global novim: {}", e))?;

        // Execute the plugin script
        let source = std::fs::read_to_string(&self.path)
            .map_err(|e| format!("Failed to re-read {}: {}", self.path.display(), e))?;
        self.lua.load(&source)
            .set_name(self.id.as_str())
            .exec()
            .map_err(|e| format!("Lua runtime error in {}: {}", self.path.display(), e))?;

        Ok(())
    }
}

impl Plugin for LuaPlugin {
    fn id(&self) -> &str { &self.id }
    fn name(&self) -> &str { &self.name }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }

    fn init(&mut self, _ctx: &mut PluginContext) {
        if let Err(e) = self.setup_api_and_run() {
            log::error!("Failed to initialize Lua plugin {}: {}", self.id, e);
        }
    }

    fn on_event(&mut self, event: &EditorEvent, ctx: &PluginContext) -> Vec<PluginAction> {
        let (event_name, args) = match event {
            EditorEvent::BufOpen { path } => ("BufOpen", HashMap::from([("path".into(), path.clone())])),
            EditorEvent::BufEnter { path } => ("BufEnter", HashMap::from([("path".into(), path.clone())])),
            EditorEvent::BufLeave { path } => ("BufLeave", HashMap::from([("path".into(), path.clone())])),
            EditorEvent::BufWrite { path } => ("BufWrite", HashMap::from([("path".into(), path.clone())])),
            EditorEvent::BufClose { path } => ("BufClose", HashMap::from([("path".into(), path.clone())])),
            EditorEvent::TextChanged { path } => ("TextChanged", HashMap::from([("path".into(), path.clone())])),
            EditorEvent::CursorMoved { line, column } => ("CursorMoved", HashMap::from([
                ("line".into(), line.to_string()),
                ("column".into(), column.to_string()),
            ])),
            EditorEvent::ModeChanged { from, to } => ("ModeChanged", HashMap::from([
                ("from".into(), from.clone()),
                ("to".into(), to.clone()),
            ])),
            EditorEvent::CommandExecuted { command } => {
                let Ok(novim) = self.lua.globals().get::<mlua::Table>("novim") else {
                    return vec![];
                };

                // Handle popup select callbacks: __popup_select:<key>:<index>:<text>
                if let Some(rest) = command.strip_prefix("__popup_select:") {
                    // Parse: key:index:text
                    let parts: Vec<&str> = rest.splitn(3, ':').collect();
                    if parts.len() >= 2 {
                        let callback_key = parts[0];
                        let index: i64 = parts[1].parse().unwrap_or(0);
                        let text = if parts.len() >= 3 { parts[2] } else { "" };

                        let Ok(callbacks) = novim.get::<mlua::Table>("_popup_callbacks") else {
                            return vec![];
                        };
                        if let Ok(handler) = callbacks.get::<mlua::Function>(callback_key) {
                            self.inject_snapshot(ctx);
                            if let Err(e) = handler.call::<()>((index + 1, text.to_string())) {
                                log::warn!("Lua popup select error in {}: {}", self.id, e);
                            }
                            return self.drain_actions();
                        }
                    }
                    return vec![];
                }

                // Handle keymap callbacks: __keymap:<key>
                if let Some(callback_key) = command.strip_prefix("__keymap:") {
                    let Ok(callbacks) = novim.get::<mlua::Table>("_keymap_callbacks") else {
                        return vec![];
                    };
                    if let Ok(handler) = callbacks.get::<mlua::Function>(callback_key) {
                        self.inject_snapshot(ctx);
                        if let Err(e) = handler.call::<()>(()) {
                            log::warn!("Lua keymap callback error in {} for {}: {}", self.id, callback_key, e);
                        }
                        return self.drain_actions();
                    }
                    return vec![];
                }

                // Handle registered commands
                let (cmd_name, cmd_args) = command.split_once(' ').unwrap_or((command, ""));
                let Ok(commands) = novim.get::<mlua::Table>("_commands") else {
                    return vec![];
                };
                if let Ok(handler) = commands.get::<mlua::Function>(cmd_name) {
                    self.inject_snapshot(ctx);
                    if let Err(e) = handler.call::<()>(cmd_args.to_string()) {
                        log::warn!("Lua command error in {} for :{}: {}", self.id, cmd_name, e);
                    }
                    return self.drain_actions();
                }
                ("CommandExecuted", HashMap::from([("command".into(), command.clone())]))
            }
            EditorEvent::Custom { name, data } => {
                return self.call_handlers(name, data, ctx);
            }
            EditorEvent::LspAttach { path, language } => {
                ("LspAttach", HashMap::from([
                    ("path".into(), path.clone()),
                    ("language".into(), language.clone()),
                ]))
            }
        };

        self.call_handlers(event_name, &args, ctx)
    }

    fn poll_timers(&mut self) -> Vec<PluginAction> {
        LuaPlugin::poll_timers(self)
    }

    fn had_error(&self) -> bool {
        self.last_had_error
    }
}
