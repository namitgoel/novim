//! Lua plugin bridge — wraps a Lua script as a `Plugin` impl.
//!
//! Each Lua file gets its own `LuaPlugin` with an isolated `mlua::Lua` VM.
//! The Lua script registers event handlers via the `novim` API table,
//! and the bridge dispatches `EditorEvent`s to those handlers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mlua::{Lua, Function, Value};

use super::{EditorEvent, Plugin, PluginContext};

/// A single Lua plugin loaded from a `.lua` file.
pub struct LuaPlugin {
    id: String,
    name: String,
    path: PathBuf,
    lua: Lua,
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

        // novim.event_handlers: { event_name => [handler_fn, ...] }
        let handlers = self.lua.create_table()
            .map_err(|e| format!("Failed to create handlers table: {}", e))?;
        novim.set("_handlers", handlers)
            .map_err(|e| format!("Failed to set _handlers: {}", e))?;

        // novim.commands: { cmd_name => handler_fn }
        let commands = self.lua.create_table()
            .map_err(|e| format!("Failed to create commands table: {}", e))?;
        novim.set("_commands", commands)
            .map_err(|e| format!("Failed to set _commands: {}", e))?;

        // novim.on(event, fn) — subscribe to an event
        let on_fn = self.lua.create_function(|lua, (event, handler): (String, Function)| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let handlers: mlua::Table = novim.get("_handlers")?;
            let list: mlua::Table = match handlers.get::<Value>(event.as_str())? {
                Value::Table(t) => t,
                _ => {
                    let t = lua.create_table()?;
                    handlers.set(event.as_str(), t.clone())?;
                    t
                }
            };
            let len = list.len()?;
            list.set(len + 1, handler)?;
            Ok(())
        }).map_err(|e| format!("Failed to create novim.on: {}", e))?;
        novim.set("on", on_fn)
            .map_err(|e| format!("Failed to set novim.on: {}", e))?;

        // novim.register_command(name, fn) — register an ex-command
        let cmd_fn = self.lua.create_function(|lua, (name, handler): (String, Function)| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let commands: mlua::Table = novim.get("_commands")?;
            commands.set(name, handler)?;
            Ok(())
        }).map_err(|e| format!("Failed to create novim.register_command: {}", e))?;
        novim.set("register_command", cmd_fn)
            .map_err(|e| format!("Failed to set novim.register_command: {}", e))?;

        // novim.exec(cmd) — queue a command for the host to execute
        let exec_fn = self.lua.create_function(|lua, cmd: String| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let queue: mlua::Table = match novim.get::<Value>("_cmd_queue")? {
                Value::Table(t) => t,
                _ => {
                    let t = lua.create_table()?;
                    novim.set("_cmd_queue", t.clone())?;
                    t
                }
            };
            let len = queue.len()?;
            queue.set(len + 1, cmd)?;
            Ok(())
        }).map_err(|e| format!("Failed to create novim.exec: {}", e))?;
        novim.set("exec", exec_fn)
            .map_err(|e| format!("Failed to set novim.exec: {}", e))?;

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

    /// Drain the command queue that Lua scripts populate via `novim.exec()`.
    fn drain_cmd_queue(&self) -> Vec<String> {
        let mut cmds = Vec::new();
        let Ok(novim) = self.lua.globals().get::<mlua::Table>("novim") else { return cmds };
        let Ok(Value::Table(queue)) = novim.get::<Value>("_cmd_queue") else { return cmds };

        for pair in queue.clone().pairs::<i64, String>() {
            if let Ok((_, cmd)) = pair {
                cmds.push(cmd);
            }
        }

        // Clear the queue
        if let Ok(fresh) = self.lua.create_table() {
            let _ = novim.set("_cmd_queue", fresh);
        }

        cmds
    }

    /// Call all Lua handlers registered for `event_name`, passing event data as args.
    fn call_handlers(&self, event_name: &str, args: &HashMap<String, String>) -> Vec<String> {
        let Ok(novim) = self.lua.globals().get::<mlua::Table>("novim") else { return vec![] };
        let Ok(handlers) = novim.get::<mlua::Table>("_handlers") else { return vec![] };
        let Ok(Value::Table(list)) = handlers.get::<Value>(event_name) else { return vec![] };

        // Build args table
        let Ok(args_table) = self.lua.create_table() else { return vec![] };
        for (k, v) in args {
            let _ = args_table.set(k.as_str(), v.as_str());
        }

        for pair in list.pairs::<i64, Function>() {
            if let Ok((_, handler)) = pair {
                if let Err(e) = handler.call::<()>(args_table.clone()) {
                    log::warn!("Lua handler error in {} for {}: {}", self.id, event_name, e);
                }
            }
        }

        self.drain_cmd_queue()
    }

    /// Get the list of command names this plugin registered.
    pub fn registered_commands(&self) -> Vec<String> {
        let mut names = Vec::new();
        let Ok(novim) = self.lua.globals().get::<mlua::Table>("novim") else { return names };
        let Ok(commands) = novim.get::<mlua::Table>("_commands") else { return names };

        for pair in commands.pairs::<String, Function>() {
            if let Ok((name, _)) = pair {
                names.push(name);
            }
        }
        names
    }
}

impl Plugin for LuaPlugin {
    fn id(&self) -> &str { &self.id }
    fn name(&self) -> &str { &self.name }

    fn init(&mut self, _ctx: &mut PluginContext) {
        if let Err(e) = self.setup_api_and_run() {
            log::error!("Failed to initialize Lua plugin {}: {}", self.id, e);
        }
    }

    fn on_event(&mut self, event: &EditorEvent, _ctx: &PluginContext) -> Vec<String> {
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
                // Check if this is a plugin-registered command
                let (cmd_name, cmd_args) = command.split_once(' ').unwrap_or((command, ""));
                let Ok(novim) = self.lua.globals().get::<mlua::Table>("novim") else {
                    return vec![];
                };
                let Ok(commands) = novim.get::<mlua::Table>("_commands") else {
                    return vec![];
                };
                if let Ok(handler) = commands.get::<Function>(cmd_name) {
                    if let Err(e) = handler.call::<()>(cmd_args.to_string()) {
                        log::warn!("Lua command error in {} for :{}: {}", self.id, cmd_name, e);
                    }
                    return self.drain_cmd_queue();
                }
                ("CommandExecuted", HashMap::from([("command".into(), command.clone())]))
            }
        };

        self.call_handlers(event_name, &args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::PluginContext;
    use std::io::Write;

    fn lua_plugin_from_source(name: &str, source: &str) -> LuaPlugin {
        let dir = std::env::temp_dir().join("novim_test_plugins");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}.lua", name));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(source.as_bytes()).unwrap();
        LuaPlugin::from_file(&path).unwrap()
    }

    #[test]
    fn lua_plugin_event_handler() {
        let mut plugin = lua_plugin_from_source("test_on", r#"
            novim.on("BufWrite", function(args)
                novim.exec("set wrap")
            end)
        "#);

        let mut ctx = PluginContext::new(false);
        plugin.init(&mut ctx);

        let cmds = plugin.on_event(
            &EditorEvent::BufWrite { path: "main.rs".into() },
            &PluginContext::new(false),
        );
        assert_eq!(cmds, vec!["set wrap"]);

        // Other events should produce no commands
        let cmds = plugin.on_event(
            &EditorEvent::BufOpen { path: "main.rs".into() },
            &PluginContext::new(false),
        );
        assert!(cmds.is_empty());
    }

    #[test]
    fn lua_plugin_register_command() {
        let mut plugin = lua_plugin_from_source("test_cmd", r#"
            novim.register_command("Hello", function(args)
                novim.exec("set nowrap")
            end)
        "#);

        let mut ctx = PluginContext::new(false);
        plugin.init(&mut ctx);

        let names = plugin.registered_commands();
        assert_eq!(names, vec!["Hello"]);

        // Trigger via CommandExecuted with the command name
        let cmds = plugin.on_event(
            &EditorEvent::CommandExecuted { command: "Hello".into() },
            &PluginContext::new(false),
        );
        assert_eq!(cmds, vec!["set nowrap"]);
    }

    #[test]
    fn lua_plugin_multiple_handlers() {
        let mut plugin = lua_plugin_from_source("test_multi", r#"
            novim.on("BufOpen", function(args)
                novim.exec("set wrap")
            end)
            novim.on("BufOpen", function(args)
                novim.exec("set rnu")
            end)
        "#);

        let mut ctx = PluginContext::new(false);
        plugin.init(&mut ctx);

        let cmds = plugin.on_event(
            &EditorEvent::BufOpen { path: "test.rs".into() },
            &PluginContext::new(false),
        );
        assert_eq!(cmds, vec!["set wrap", "set rnu"]);
    }

    #[test]
    fn lua_plugin_syntax_error_rejected() {
        let dir = std::env::temp_dir().join("novim_test_plugins");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad_syntax.lua");
        std::fs::write(&path, "this is not valid lua !!!@@@").unwrap();

        let result = LuaPlugin::from_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn lua_plugin_id_from_filename() {
        let plugin = lua_plugin_from_source("auto_save", "-- empty plugin");
        assert_eq!(plugin.id(), "user.auto_save");
        assert_eq!(plugin.name(), "auto save");
    }
}
