//! Lua plugin bridge — wraps a Lua script as a `Plugin` impl.
//!
//! Each Lua file gets its own `LuaPlugin` with an isolated `mlua::Lua` VM.
//! The Lua script registers event handlers via the `novim` API table,
//! and the bridge dispatches `EditorEvent`s to those handlers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mlua::{Lua, Function, Value};

use super::{EditorEvent, Plugin, PluginAction, PluginContext};

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

        // ── novim.buf — buffer read/write API ──
        let buf = self.lua.create_table()
            .map_err(|e| format!("Failed to create buf table: {}", e))?;

        // novim.buf.get_lines(start, end) — read lines from snapshot
        buf.set("get_lines", self.lua.create_function(|lua, (start, end): (usize, usize)| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let snap: mlua::Table = novim.get("_snapshot")?;
            let lines: mlua::Table = snap.get("lines")?;
            let result = lua.create_table()?;
            let mut idx = 1;
            for i in start..end {
                if let Ok(line) = lines.get::<String>(i + 1) {
                    result.set(idx, line)?;
                    idx += 1;
                }
            }
            Ok(result)
        }).map_err(|e| format!("buf.get_lines: {}", e))?)
            .map_err(|e| format!("set buf.get_lines: {}", e))?;

        // novim.buf.get_text() — full buffer text
        buf.set("get_text", self.lua.create_function(|lua, ()| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let snap: mlua::Table = novim.get("_snapshot")?;
            let lines: mlua::Table = snap.get("lines")?;
            let mut text = String::new();
            for pair in lines.pairs::<i64, String>() {
                if let Ok((_, line)) = pair {
                    text.push_str(&line);
                    text.push('\n');
                }
            }
            Ok(text)
        }).map_err(|e| format!("buf.get_text: {}", e))?)
            .map_err(|e| format!("set buf.get_text: {}", e))?;

        // novim.buf.line_count()
        buf.set("line_count", self.lua.create_function(|lua, ()| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let snap: mlua::Table = novim.get("_snapshot")?;
            Ok(snap.get::<usize>("line_count")?)
        }).map_err(|e| format!("buf.line_count: {}", e))?)
            .map_err(|e| format!("set buf.line_count: {}", e))?;

        // novim.buf.path()
        buf.set("path", self.lua.create_function(|lua, ()| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let snap: mlua::Table = novim.get("_snapshot")?;
            Ok(snap.get::<Option<String>>("path")?)
        }).map_err(|e| format!("buf.path: {}", e))?)
            .map_err(|e| format!("set buf.path: {}", e))?;

        // novim.buf.is_dirty()
        buf.set("is_dirty", self.lua.create_function(|lua, ()| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let snap: mlua::Table = novim.get("_snapshot")?;
            Ok(snap.get::<bool>("is_dirty")?)
        }).map_err(|e| format!("buf.is_dirty: {}", e))?)
            .map_err(|e| format!("set buf.is_dirty: {}", e))?;

        // novim.buf.cursor() — returns {line=N, col=N}
        buf.set("cursor", self.lua.create_function(|lua, ()| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let snap: mlua::Table = novim.get("_snapshot")?;
            let result = lua.create_table()?;
            result.set("line", snap.get::<usize>("cursor_line")?)?;
            result.set("col", snap.get::<usize>("cursor_col")?)?;
            Ok(result)
        }).map_err(|e| format!("buf.cursor: {}", e))?)
            .map_err(|e| format!("set buf.cursor: {}", e))?;

        // novim.buf.set_lines(start, end, lines) — replace lines
        buf.set("set_lines", self.lua.create_function(|lua, (start, end, lines): (usize, usize, mlua::Table)| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let push: Function = novim.get("_push_action")?;
            let action = lua.create_table()?;
            action.set("type", "set_lines")?;
            action.set("start", start)?;
            action.set("end_", end)?;
            action.set("lines", lines)?;
            push.call::<()>(action)?;
            Ok(())
        }).map_err(|e| format!("buf.set_lines: {}", e))?)
            .map_err(|e| format!("set buf.set_lines: {}", e))?;

        // novim.buf.insert(line, col, text) — insert text at position
        buf.set("insert", self.lua.create_function(|lua, (line, col, text): (usize, usize, String)| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let push: Function = novim.get("_push_action")?;
            let action = lua.create_table()?;
            action.set("type", "insert_text")?;
            action.set("line", line)?;
            action.set("col", col)?;
            action.set("text", text)?;
            push.call::<()>(action)?;
            Ok(())
        }).map_err(|e| format!("buf.insert: {}", e))?)
            .map_err(|e| format!("set buf.insert: {}", e))?;

        // novim.buf.set_cursor(line, col) — move cursor
        buf.set("set_cursor", self.lua.create_function(|lua, (line, col): (usize, usize)| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let push: Function = novim.get("_push_action")?;
            let action = lua.create_table()?;
            action.set("type", "set_cursor")?;
            action.set("line", line)?;
            action.set("col", col)?;
            push.call::<()>(action)?;
            Ok(())
        }).map_err(|e| format!("buf.set_cursor: {}", e))?)
            .map_err(|e| format!("set buf.set_cursor: {}", e))?;

        novim.set("buf", buf)
            .map_err(|e| format!("Failed to set novim.buf: {}", e))?;

        // ── novim.fn — shell, filesystem utilities ──
        let fns = self.lua.create_table()
            .map_err(|e| format!("Failed to create fn table: {}", e))?;

        // novim.fn.shell(cmd) — run shell command, return stdout
        fns.set("shell", self.lua.create_function(|_lua, cmd: String| {
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .output()
                .map_err(|e| mlua::Error::runtime(format!("shell error: {}", e)))?;
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        }).map_err(|e| format!("fn.shell: {}", e))?)
            .map_err(|e| format!("set fn.shell: {}", e))?;

        // novim.fn.readfile(path) — read file, return lines as table
        fns.set("readfile", self.lua.create_function(|lua, path: String| {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| mlua::Error::runtime(format!("readfile error: {}", e)))?;
            let result = lua.create_table()?;
            for (i, line) in content.lines().enumerate() {
                result.set(i + 1, line)?;
            }
            Ok(result)
        }).map_err(|e| format!("fn.readfile: {}", e))?)
            .map_err(|e| format!("set fn.readfile: {}", e))?;

        // novim.fn.writefile(path, lines) — write lines to file
        fns.set("writefile", self.lua.create_function(|_lua, (path, lines): (String, mlua::Table)| {
            let mut content = String::new();
            for pair in lines.pairs::<i64, String>() {
                if let Ok((_, line)) = pair {
                    content.push_str(&line);
                    content.push('\n');
                }
            }
            std::fs::write(&path, &content)
                .map_err(|e| mlua::Error::runtime(format!("writefile error: {}", e)))?;
            Ok(())
        }).map_err(|e| format!("fn.writefile: {}", e))?)
            .map_err(|e| format!("set fn.writefile: {}", e))?;

        // novim.fn.glob(pattern) — return matching file paths
        fns.set("glob", self.lua.create_function(|lua, pattern: String| {
            let result = lua.create_table()?;
            let mut idx = 1;
            if let Ok(entries) = glob::glob(&pattern) {
                for entry in entries.flatten() {
                    result.set(idx, entry.to_string_lossy().to_string())?;
                    idx += 1;
                }
            }
            Ok(result)
        }).map_err(|e| format!("fn.glob: {}", e))?)
            .map_err(|e| format!("set fn.glob: {}", e))?;

        novim.set("fn", fns)
            .map_err(|e| format!("Failed to set novim.fn: {}", e))?;

        // ── novim.ui — status/notification API ──
        let ui = self.lua.create_table()
            .map_err(|e| format!("Failed to create ui table: {}", e))?;

        // novim.ui.status(msg) — set status bar message
        ui.set("status", self.lua.create_function(|lua, msg: String| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let push: Function = novim.get("_push_action")?;
            let action = lua.create_table()?;
            action.set("type", "set_status")?;
            action.set("msg", msg)?;
            push.call::<()>(action)?;
            Ok(())
        }).map_err(|e| format!("ui.status: {}", e))?)
            .map_err(|e| format!("set ui.status: {}", e))?;

        // novim.ui.log(msg) — write to debug log
        ui.set("log", self.lua.create_function(|_lua, msg: String| {
            log::info!("[lua] {}", msg);
            Ok(())
        }).map_err(|e| format!("ui.log: {}", e))?)
            .map_err(|e| format!("set ui.log: {}", e))?;

        novim.set("ui", ui)
            .map_err(|e| format!("Failed to set novim.ui: {}", e))?;

        // ── novim.keymap(mode, key, action) — register a keybinding ──
        // action can be a string (ex-command) or a function (Lua callback)
        let keymap_callbacks = self.lua.create_table()
            .map_err(|e| format!("Failed to create keymap_callbacks: {}", e))?;
        novim.set("_keymap_callbacks", keymap_callbacks)
            .map_err(|e| format!("Failed to set _keymap_callbacks: {}", e))?;

        let keymap_fn = self.lua.create_function(|lua, (mode, key, action): (String, String, Value)| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let push: Function = novim.get("_push_action")?;
            let tbl = lua.create_table()?;
            tbl.set("type", "register_keymap")?;
            tbl.set("mode", mode)?;
            tbl.set("key", key.clone())?;

            match action {
                Value::String(cmd) => {
                    tbl.set("action_type", "command")?;
                    tbl.set("command", cmd.to_str()?.to_string())?;
                }
                Value::Function(f) => {
                    // Store the callback in _keymap_callbacks[key]
                    let callbacks: mlua::Table = novim.get("_keymap_callbacks")?;
                    callbacks.set(key.as_str(), f)?;
                    tbl.set("action_type", "lua_callback")?;
                    tbl.set("callback_key", key)?;
                }
                _ => return Err(mlua::Error::runtime("keymap action must be a string or function")),
            }
            push.call::<()>(tbl)?;
            Ok(())
        }).map_err(|e| format!("Failed to create novim.keymap: {}", e))?;
        novim.set("keymap", keymap_fn)
            .map_err(|e| format!("Failed to set novim.keymap: {}", e))?;

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

    /// Drain all queued actions (exec commands + structured actions).
    fn drain_actions(&self) -> Vec<PluginAction> {
        let mut actions = Vec::new();
        let Ok(novim) = self.lua.globals().get::<mlua::Table>("novim") else { return actions };

        // Drain exec command queue
        if let Ok(Value::Table(queue)) = novim.get::<Value>("_cmd_queue") {
            for pair in queue.clone().pairs::<i64, String>() {
                if let Ok((_, cmd)) = pair {
                    actions.push(PluginAction::ExecCommand(cmd));
                }
            }
            if let Ok(fresh) = self.lua.create_table() {
                let _ = novim.set("_cmd_queue", fresh);
            }
        }

        // Drain structured action queue
        if let Ok(Value::Table(queue)) = novim.get::<Value>("_action_queue") {
            for pair in queue.clone().pairs::<i64, mlua::Table>() {
                if let Ok((_, tbl)) = pair {
                    if let Some(action) = Self::table_to_action(&tbl) {
                        actions.push(action);
                    }
                }
            }
            if let Ok(fresh) = self.lua.create_table() {
                let _ = novim.set("_action_queue", fresh);
            }
        }

        actions
    }

    /// Convert a Lua table from the action queue into a PluginAction.
    fn table_to_action(tbl: &mlua::Table) -> Option<PluginAction> {
        let action_type: String = tbl.get("type").ok()?;
        match action_type.as_str() {
            "set_lines" => {
                let start: usize = tbl.get("start").ok()?;
                let end: usize = tbl.get("end_").ok()?;
                let lines_tbl: mlua::Table = tbl.get("lines").ok()?;
                let lines: Vec<String> = lines_tbl.pairs::<i64, String>()
                    .filter_map(|r| r.ok().map(|(_, v)| v))
                    .collect();
                Some(PluginAction::SetLines { start, end, lines })
            }
            "insert_text" => {
                let line: usize = tbl.get("line").ok()?;
                let col: usize = tbl.get("col").ok()?;
                let text: String = tbl.get("text").ok()?;
                Some(PluginAction::InsertText { line, col, text })
            }
            "set_cursor" => {
                let line: usize = tbl.get("line").ok()?;
                let col: usize = tbl.get("col").ok()?;
                Some(PluginAction::SetCursor { line, col })
            }
            "set_status" => {
                let msg: String = tbl.get("msg").ok()?;
                Some(PluginAction::SetStatus(msg))
            }
            "register_keymap" => {
                let mode: String = tbl.get("mode").ok()?;
                let key: String = tbl.get("key").ok()?;
                let action_type: String = tbl.get("action_type").ok()?;
                let action = match action_type.as_str() {
                    "command" => {
                        let cmd: String = tbl.get("command").ok()?;
                        super::KeymapAction::Command(cmd)
                    }
                    "lua_callback" => {
                        let callback_key: String = tbl.get("callback_key").ok()?;
                        super::KeymapAction::LuaCallback {
                            plugin_id: String::new(), // filled by manager
                            callback_key,
                        }
                    }
                    _ => return None,
                };
                Some(PluginAction::RegisterKeymap { mode, key, action })
            }
            _ => None,
        }
    }

    /// Inject the buffer snapshot into the Lua `novim._snapshot` table.
    fn inject_snapshot(&self, ctx: &PluginContext) {
        let Ok(novim) = self.lua.globals().get::<mlua::Table>("novim") else { return };
        let Ok(snap) = self.lua.create_table() else { return };

        // Lines as a Lua table
        let Ok(lines_tbl) = self.lua.create_table() else { return };
        for (i, line) in ctx.buf.lines.iter().enumerate() {
            let _ = lines_tbl.set(i + 1, line.as_str());
        }
        let _ = snap.set("lines", lines_tbl);
        let _ = snap.set("line_count", ctx.buf.line_count);
        let _ = snap.set("cursor_line", ctx.buf.cursor_line);
        let _ = snap.set("cursor_col", ctx.buf.cursor_col);
        let _ = snap.set("is_dirty", ctx.buf.is_dirty);
        let _ = snap.set("mode", ctx.buf.mode.as_str());
        if let Some(path) = &ctx.buf.path {
            let _ = snap.set("path", path.as_str());
        }
        if let Some((sl, sc, el, ec)) = ctx.buf.selection {
            let Ok(sel) = self.lua.create_table() else { return };
            let _ = sel.set("start_line", sl);
            let _ = sel.set("start_col", sc);
            let _ = sel.set("end_line", el);
            let _ = sel.set("end_col", ec);
            let _ = snap.set("selection", sel);
        }

        let _ = novim.set("_snapshot", snap);
    }

    /// Call all Lua handlers registered for `event_name`, passing event data as args.
    fn call_handlers(&self, event_name: &str, args: &HashMap<String, String>, ctx: &PluginContext) -> Vec<PluginAction> {
        // Inject snapshot before calling handlers
        self.inject_snapshot(ctx);
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

        self.drain_actions()
    }

    /// Drain actions queued during init (e.g. keymap registrations).
    pub fn drain_init_actions(&self) -> Vec<PluginAction> {
        self.drain_actions()
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

                // Handle keymap callbacks: __keymap:<key>
                if let Some(callback_key) = command.strip_prefix("__keymap:") {
                    let Ok(callbacks) = novim.get::<mlua::Table>("_keymap_callbacks") else {
                        return vec![];
                    };
                    if let Ok(handler) = callbacks.get::<Function>(callback_key) {
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
                if let Ok(handler) = commands.get::<Function>(cmd_name) {
                    self.inject_snapshot(ctx);
                    if let Err(e) = handler.call::<()>(cmd_args.to_string()) {
                        log::warn!("Lua command error in {} for :{}: {}", self.id, cmd_name, e);
                    }
                    return self.drain_actions();
                }
                ("CommandExecuted", HashMap::from([("command".into(), command.clone())]))
            }
        };

        self.call_handlers(event_name, &args, ctx)
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

    fn assert_exec_commands(actions: &[PluginAction], expected: &[&str]) {
        let cmds: Vec<&str> = actions.iter().filter_map(|a| {
            if let PluginAction::ExecCommand(s) = a { Some(s.as_str()) } else { None }
        }).collect();
        assert_eq!(cmds, expected);
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

        let actions = plugin.on_event(
            &EditorEvent::BufWrite { path: "main.rs".into() },
            &PluginContext::new(false),
        );
        assert_exec_commands(&actions, &["set wrap"]);

        let actions = plugin.on_event(
            &EditorEvent::BufOpen { path: "main.rs".into() },
            &PluginContext::new(false),
        );
        assert!(actions.is_empty());
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

        let actions = plugin.on_event(
            &EditorEvent::CommandExecuted { command: "Hello".into() },
            &PluginContext::new(false),
        );
        assert_exec_commands(&actions, &["set nowrap"]);
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

        let actions = plugin.on_event(
            &EditorEvent::BufOpen { path: "test.rs".into() },
            &PluginContext::new(false),
        );
        assert_exec_commands(&actions, &["set wrap", "set rnu"]);
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

    fn ctx_with_snapshot() -> PluginContext {
        let mut ctx = PluginContext::new(false);
        ctx.buf = super::super::BufferSnapshot {
            lines: vec!["hello world".into(), "second line".into(), "third line".into()],
            line_count: 3,
            cursor_line: 0,
            cursor_col: 5,
            path: Some("test.rs".into()),
            is_dirty: false,
            mode: "NORMAL".into(),
            selection: None,
        };
        ctx
    }

    #[test]
    fn lua_buf_read_api() {
        let mut plugin = lua_plugin_from_source("test_buf_read", r#"
            novim.on("BufOpen", function(args)
                local count = novim.buf.line_count()
                local path = novim.buf.path()
                local dirty = novim.buf.is_dirty()
                local cur = novim.buf.cursor()
                novim.exec("echo lines=" .. count .. " path=" .. path .. " dirty=" .. tostring(dirty) .. " cur=" .. cur.line .. "," .. cur.col)
            end)
        "#);

        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(
            &EditorEvent::BufOpen { path: "test.rs".into() },
            &ctx,
        );
        assert_exec_commands(&actions, &["echo lines=3 path=test.rs dirty=false cur=0,5"]);
    }

    #[test]
    fn lua_buf_get_lines() {
        let mut plugin = lua_plugin_from_source("test_get_lines", r#"
            novim.on("BufOpen", function(args)
                local lines = novim.buf.get_lines(0, 2)
                novim.exec("echo " .. lines[1] .. "|" .. lines[2])
            end)
        "#);

        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(
            &EditorEvent::BufOpen { path: "test.rs".into() },
            &ctx,
        );
        assert_exec_commands(&actions, &["echo hello world|second line"]);
    }

    #[test]
    fn lua_buf_write_api() {
        let mut plugin = lua_plugin_from_source("test_buf_write", r#"
            novim.on("BufWrite", function(args)
                novim.buf.set_cursor(2, 0)
                novim.buf.insert(1, 0, "inserted")
            end)
        "#);

        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(
            &EditorEvent::BufWrite { path: "test.rs".into() },
            &ctx,
        );
        assert_eq!(actions.len(), 2);
        assert!(matches!(&actions[0], PluginAction::SetCursor { line: 2, col: 0 }));
        assert!(matches!(&actions[1], PluginAction::InsertText { line: 1, col: 0, text } if text == "inserted"));
    }

    #[test]
    fn lua_ui_status() {
        let mut plugin = lua_plugin_from_source("test_ui", r#"
            novim.on("BufOpen", function(args)
                novim.ui.status("Hello from UI!")
            end)
        "#);

        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(
            &EditorEvent::BufOpen { path: "test.rs".into() },
            &ctx,
        );
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], PluginAction::SetStatus(msg) if msg == "Hello from UI!"));
    }

    #[test]
    fn lua_shell_api() {
        let mut plugin = lua_plugin_from_source("test_shell", r#"
            novim.on("BufOpen", function(args)
                local result = novim.fn.shell("echo hello")
                novim.exec("echo " .. result:gsub("\n", ""))
            end)
        "#);

        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(
            &EditorEvent::BufOpen { path: "test.rs".into() },
            &ctx,
        );
        assert_exec_commands(&actions, &["echo hello"]);
    }

    #[test]
    fn lua_file_api() {
        let tmp = std::env::temp_dir().join("novim_test_file_api.txt");
        let mut plugin = lua_plugin_from_source("test_file", &format!(r#"
            novim.on("BufOpen", function(args)
                novim.fn.writefile("{path}", {{"line one", "line two"}})
                local lines = novim.fn.readfile("{path}")
                novim.exec("echo " .. lines[1] .. "|" .. lines[2])
            end)
        "#, path = tmp.display()));

        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(
            &EditorEvent::BufOpen { path: "test.rs".into() },
            &ctx,
        );
        assert_exec_commands(&actions, &["echo line one|line two"]);
        let _ = std::fs::remove_file(&tmp);
    }
}
