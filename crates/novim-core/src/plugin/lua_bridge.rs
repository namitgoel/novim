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

    /// Set up event handlers table, commands table, `novim.on()`, `novim.register_command()`, and `novim.exec()`.
    fn setup_event_system(&self, novim: &mlua::Table) -> Result<(), String> {
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

        // novim.on(event, [opts], fn) — subscribe to an event
        // opts is optional: { pattern = "*.rs" } to filter by file path glob
        let on_fn = self.lua.create_function(|lua, args: mlua::MultiValue| {
            let mut args_vec: Vec<Value> = args.into_iter().collect();
            if args_vec.len() < 2 {
                return Err(mlua::Error::runtime("novim.on requires at least 2 arguments: event, handler"));
            }
            let event = match args_vec.remove(0) {
                Value::String(s) => s.to_str()?.to_string(),
                _ => return Err(mlua::Error::runtime("first argument must be event name string")),
            };
            // If 3 args: (event, opts, handler). If 2: (event, handler)
            let (pattern, handler) = if args_vec.len() >= 2 {
                let opts = match args_vec.remove(0) {
                    Value::Table(t) => t,
                    _ => return Err(mlua::Error::runtime("second argument must be opts table or handler function")),
                };
                let pattern: Option<String> = opts.get("pattern").ok();
                let handler = match args_vec.remove(0) {
                    Value::Function(f) => f,
                    _ => return Err(mlua::Error::runtime("last argument must be handler function")),
                };
                (pattern, handler)
            } else {
                let handler = match args_vec.remove(0) {
                    Value::Function(f) => f,
                    _ => return Err(mlua::Error::runtime("last argument must be handler function")),
                };
                (None, handler)
            };

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
            // Store as {fn=handler, pattern=pattern_or_nil}
            let entry = lua.create_table()?;
            entry.set("fn", handler)?;
            if let Some(p) = pattern {
                entry.set("pattern", p)?;
            }
            list.set(len + 1, entry)?;
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

        Ok(())
    }

    /// Set up `novim.buf.*` including the Selection API additions.
    fn setup_buf_api(&self, novim: &mlua::Table) -> Result<(), String> {
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

        // ── Selection API additions to novim.buf ──
        let buf_table: mlua::Table = novim.get("buf")
            .map_err(|e| format!("Failed to get buf table: {}", e))?;

        buf_table.set("get_selection", self.lua.create_function(|lua, ()| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let snap: mlua::Table = novim.get("_snapshot")?;
            match snap.get::<Value>("selection")? {
                Value::Table(sel) => {
                    let result = lua.create_table()?;
                    result.set("start_line", sel.get::<usize>("start_line")?)?;
                    result.set("start_col", sel.get::<usize>("start_col")?)?;
                    result.set("end_line", sel.get::<usize>("end_line")?)?;
                    result.set("end_col", sel.get::<usize>("end_col")?)?;
                    Ok(Value::Table(result))
                }
                _ => Ok(Value::Nil),
            }
        }).map_err(|e| format!("buf.get_selection: {}", e))?)
            .map_err(|e| format!("set buf.get_selection: {}", e))?;

        buf_table.set("selected_text", self.lua.create_function(|lua, ()| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let snap: mlua::Table = novim.get("_snapshot")?;
            match snap.get::<Value>("selected_text")? {
                Value::String(s) => Ok(Value::String(s)),
                _ => Ok(Value::Nil),
            }
        }).map_err(|e| format!("buf.selected_text: {}", e))?)
            .map_err(|e| format!("set buf.selected_text: {}", e))?;

        buf_table.set("set_selection", self.lua.create_function(|lua, (sl, sc, el, ec): (usize, usize, usize, usize)| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let push: Function = novim.get("_push_action")?;
            let action = lua.create_table()?;
            action.set("type", "set_selection")?;
            action.set("start_line", sl)?;
            action.set("start_col", sc)?;
            action.set("end_line", el)?;
            action.set("end_col", ec)?;
            push.call::<()>(action)?;
            Ok(())
        }).map_err(|e| format!("buf.set_selection: {}", e))?)
            .map_err(|e| format!("set buf.set_selection: {}", e))?;

        buf_table.set("clear_selection", self.lua.create_function(|lua, ()| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let push: Function = novim.get("_push_action")?;
            let action = lua.create_table()?;
            action.set("type", "clear_selection")?;
            push.call::<()>(action)?;
            Ok(())
        }).map_err(|e| format!("buf.clear_selection: {}", e))?)
            .map_err(|e| format!("set buf.clear_selection: {}", e))?;

        Ok(())
    }

    /// Set up `novim.fn.*` (shell, readfile, writefile, glob).
    fn setup_fn_api(&self, novim: &mlua::Table) -> Result<(), String> {
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

        Ok(())
    }

    /// Set up `novim.ui.*` (status, log, popup).
    fn setup_ui_api(&self, novim: &mlua::Table) -> Result<(), String> {
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

        // novim.ui.popup(title, lines, [opts]) — show a popup overlay
        // opts: { width = 60, height = 20, on_select = function(index, text) end }
        ui.set("popup", self.lua.create_function(|lua, args: mlua::MultiValue| {
            let mut args_vec: Vec<Value> = args.into_iter().collect();
            if args_vec.len() < 2 {
                return Err(mlua::Error::runtime("ui.popup requires at least 2 arguments: title, lines"));
            }
            let title = match args_vec.remove(0) {
                Value::String(s) => s.to_str()?.to_string(),
                _ => return Err(mlua::Error::runtime("first argument must be title string")),
            };
            let lines = match args_vec.remove(0) {
                Value::Table(t) => t,
                _ => return Err(mlua::Error::runtime("second argument must be lines table")),
            };

            let mut width: Option<u16> = None;
            let mut height: Option<u16> = None;
            let mut on_select_key: Option<String> = None;

            if !args_vec.is_empty() {
                if let Value::Table(opts) = args_vec.remove(0) {
                    width = opts.get("width").ok();
                    height = opts.get("height").ok();
                    // Store on_select callback if provided
                    if let Ok(callback) = opts.get::<Function>("on_select") {
                        let novim: mlua::Table = lua.globals().get("novim")?;
                        let callbacks: mlua::Table = match novim.get::<Value>("_popup_callbacks")? {
                            Value::Table(t) => t,
                            _ => {
                                let t = lua.create_table()?;
                                novim.set("_popup_callbacks", t.clone())?;
                                t
                            }
                        };
                        let key = format!("popup_{}", callbacks.len()? + 1);
                        callbacks.set(key.as_str(), callback)?;
                        on_select_key = Some(key);
                    }
                }
            }

            let novim: mlua::Table = lua.globals().get("novim")?;
            let push: Function = novim.get("_push_action")?;
            let action = lua.create_table()?;
            action.set("type", "show_popup")?;
            action.set("title", title)?;
            action.set("lines", lines)?;
            if let Some(w) = width { action.set("width", w)?; }
            if let Some(h) = height { action.set("height", h)?; }
            if let Some(key) = on_select_key { action.set("on_select_key", key)?; }
            push.call::<()>(action)?;
            Ok(())
        }).map_err(|e| format!("ui.popup: {}", e))?)
            .map_err(|e| format!("set ui.popup: {}", e))?;

        novim.set("ui", ui)
            .map_err(|e| format!("Failed to set novim.ui: {}", e))?;

        Ok(())
    }

    /// Set up `novim.opt.*` (get, set).
    fn setup_opt_api(&self, novim: &mlua::Table) -> Result<(), String> {
        // ── novim.opt — editor options read/write ──
        let opt = self.lua.create_table()
            .map_err(|e| format!("Failed to create opt table: {}", e))?;

        // novim.opt.get(name) — read an editor option
        opt.set("get", self.lua.create_function(|lua, name: String| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let snap: mlua::Table = novim.get("_snapshot")?;
            match name.as_str() {
                "tab_width" | "tabstop" | "ts" => Ok(Value::Integer(snap.get::<i64>("tab_width")?)),
                "expand_tab" | "et" => Ok(Value::Boolean(snap.get::<bool>("expand_tab")?)),
                "auto_indent" | "ai" => Ok(Value::Boolean(snap.get::<bool>("auto_indent")?)),
                "word_wrap" | "wrap" => Ok(Value::Boolean(snap.get::<bool>("word_wrap")?)),
                "line_numbers" => Ok(Value::String(lua.create_string(snap.get::<String>("line_numbers")?)?)),
                "pane_count" => Ok(Value::Integer(snap.get::<i64>("pane_count")?)),
                _ => Ok(Value::Nil),
            }
        }).map_err(|e| format!("opt.get: {}", e))?)
            .map_err(|e| format!("set opt.get: {}", e))?;

        // novim.opt.set(name, value) — write an editor option via :set
        opt.set("set", self.lua.create_function(|lua, (name, value): (String, Value)| {
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
            let cmd = match name.as_str() {
                "tab_width" | "tabstop" | "ts" => {
                    if let Value::Integer(n) = value { format!("set ts={}", n) }
                    else { return Err(mlua::Error::runtime("tab_width must be integer")) }
                }
                "expand_tab" | "et" => {
                    if let Value::Boolean(v) = value { if v { "set et".into() } else { "set noet".into() } }
                    else { return Err(mlua::Error::runtime("expand_tab must be boolean")) }
                }
                "auto_indent" | "ai" => {
                    if let Value::Boolean(v) = value { if v { "set ai".into() } else { "set noai".into() } }
                    else { return Err(mlua::Error::runtime("auto_indent must be boolean")) }
                }
                "word_wrap" | "wrap" => {
                    if let Value::Boolean(v) = value { if v { "set wrap".into() } else { "set nowrap".into() } }
                    else { return Err(mlua::Error::runtime("word_wrap must be boolean")) }
                }
                "line_numbers" => {
                    if let Value::String(s) = value {
                        let s = s.to_str()?.to_string();
                        match s.as_str() {
                            "absolute" | "nu" => "set nu".into(),
                            "relative" | "rnu" => "set rnu".into(),
                            "off" | "nonu" => "set nonu".into(),
                            _ => return Err(mlua::Error::runtime(format!("Unknown line_numbers mode: {}", s))),
                        }
                    } else { return Err(mlua::Error::runtime("line_numbers must be string")) }
                }
                _ => return Err(mlua::Error::runtime(format!("Unknown option: {}", name))),
            };
            queue.set(len + 1, cmd)?;
            Ok(())
        }).map_err(|e| format!("opt.set: {}", e))?)
            .map_err(|e| format!("set opt.set: {}", e))?;

        novim.set("opt", opt)
            .map_err(|e| format!("Failed to set novim.opt: {}", e))?;

        Ok(())
    }

    /// Set up `novim.win.*` (split, close, count).
    fn setup_win_api(&self, novim: &mlua::Table) -> Result<(), String> {
        // ── novim.win — window/pane API ──
        let win = self.lua.create_table()
            .map_err(|e| format!("Failed to create win table: {}", e))?;

        win.set("split", self.lua.create_function(|lua, dir: String| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let queue: mlua::Table = match novim.get::<Value>("_cmd_queue")? {
                Value::Table(t) => t,
                _ => { let t = lua.create_table()?; novim.set("_cmd_queue", t.clone())?; t }
            };
            let len = queue.len()?;
            let cmd = match dir.as_str() {
                "vertical" | "v" => "vsplit",
                "horizontal" | "h" => "split",
                _ => return Err(mlua::Error::runtime(format!("Unknown split direction: {}", dir))),
            };
            queue.set(len + 1, cmd)?;
            Ok(())
        }).map_err(|e| format!("win.split: {}", e))?)
            .map_err(|e| format!("set win.split: {}", e))?;

        win.set("close", self.lua.create_function(|lua, ()| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let queue: mlua::Table = match novim.get::<Value>("_cmd_queue")? {
                Value::Table(t) => t,
                _ => { let t = lua.create_table()?; novim.set("_cmd_queue", t.clone())?; t }
            };
            let len = queue.len()?;
            queue.set(len + 1, "close")?;
            Ok(())
        }).map_err(|e| format!("win.close: {}", e))?)
            .map_err(|e| format!("set win.close: {}", e))?;

        win.set("count", self.lua.create_function(|lua, ()| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let snap: mlua::Table = novim.get("_snapshot")?;
            Ok(snap.get::<usize>("pane_count")?)
        }).map_err(|e| format!("win.count: {}", e))?)
            .map_err(|e| format!("set win.count: {}", e))?;

        novim.set("win", win)
            .map_err(|e| format!("Failed to set novim.win: {}", e))?;

        Ok(())
    }

    /// Set up `novim.keymap(mode, key, action)`.
    fn setup_keymap_api(&self, novim: &mlua::Table) -> Result<(), String> {
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

        Ok(())
    }

    /// Set up `novim.emit`, `novim.schedule`, `novim.defer`, and `novim.lsp`.
    fn setup_misc_api(&self, novim: &mlua::Table) -> Result<(), String> {
        // ── novim.emit(event_name, data) — emit custom event to all plugins ──
        let emit_fn = self.lua.create_function(|lua, (name, data): (String, Option<mlua::Table>)| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let push: Function = novim.get("_push_action")?;
            let action = lua.create_table()?;
            action.set("type", "emit_event")?;
            action.set("name", name)?;
            if let Some(data) = data {
                action.set("data", data)?;
            }
            push.call::<()>(action)?;
            Ok(())
        }).map_err(|e| format!("Failed to create novim.emit: {}", e))?;
        novim.set("emit", emit_fn)
            .map_err(|e| format!("Failed to set novim.emit: {}", e))?;

        // ── novim.schedule(fn) — run callback on next event cycle ──
        let scheduled = self.lua.create_table()
            .map_err(|e| format!("Failed to create _scheduled: {}", e))?;
        novim.set("_scheduled", scheduled)
            .map_err(|e| format!("Failed to set _scheduled: {}", e))?;

        let schedule_fn = self.lua.create_function(|lua, callback: Function| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let scheduled: mlua::Table = novim.get("_scheduled")?;
            let len = scheduled.len()?;
            scheduled.set(len + 1, callback)?;
            Ok(())
        }).map_err(|e| format!("Failed to create novim.schedule: {}", e))?;
        novim.set("schedule", schedule_fn)
            .map_err(|e| format!("Failed to set novim.schedule: {}", e))?;

        // ── novim.defer(ms, fn) — run callback after a delay ──
        // Note: stores in _deferred table; the host polls and runs them
        let deferred = self.lua.create_table()
            .map_err(|e| format!("Failed to create _deferred: {}", e))?;
        novim.set("_deferred", deferred)
            .map_err(|e| format!("Failed to set _deferred: {}", e))?;

        let defer_fn = self.lua.create_function(|lua, (ms, callback): (u64, Function)| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let deferred: mlua::Table = novim.get("_deferred")?;
            let len = deferred.len()?;
            let entry = lua.create_table()?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            entry.set("run_at", now + ms)?;
            entry.set("fn", callback)?;
            deferred.set(len + 1, entry)?;
            Ok(())
        }).map_err(|e| format!("Failed to create novim.defer: {}", e))?;
        novim.set("defer", defer_fn)
            .map_err(|e| format!("Failed to set novim.defer: {}", e))?;

        // ── novim.lsp — LSP info API ──
        let lsp = self.lua.create_table()
            .map_err(|e| format!("Failed to create lsp table: {}", e))?;

        // novim.lsp.on_attach(fn) — sugar for novim.on("LspAttach", fn)
        lsp.set("on_attach", self.lua.create_function(|lua, handler: Function| {
            let novim: mlua::Table = lua.globals().get("novim")?;
            let handlers: mlua::Table = novim.get("_handlers")?;
            let list: mlua::Table = match handlers.get::<Value>("LspAttach")? {
                Value::Table(t) => t,
                _ => {
                    let t = lua.create_table()?;
                    handlers.set("LspAttach", t.clone())?;
                    t
                }
            };
            let len = list.len()?;
            let entry = lua.create_table()?;
            entry.set("fn", handler)?;
            list.set(len + 1, entry)?;
            Ok(())
        }).map_err(|e| format!("lsp.on_attach: {}", e))?)
            .map_err(|e| format!("set lsp.on_attach: {}", e))?;

        novim.set("lsp", lsp)
            .map_err(|e| format!("Failed to set novim.lsp: {}", e))?;

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
            "set_selection" => {
                let start_line: usize = tbl.get("start_line").ok()?;
                let start_col: usize = tbl.get("start_col").ok()?;
                let end_line: usize = tbl.get("end_line").ok()?;
                let end_col: usize = tbl.get("end_col").ok()?;
                Some(PluginAction::SetSelection { start_line, start_col, end_line, end_col })
            }
            "clear_selection" => {
                Some(PluginAction::ClearSelection)
            }
            "show_popup" => {
                let title: String = tbl.get("title").ok()?;
                let lines_tbl: mlua::Table = tbl.get("lines").ok()?;
                let lines: Vec<String> = lines_tbl.pairs::<i64, String>()
                    .filter_map(|r| r.ok().map(|(_, v)| v))
                    .collect();
                let width: Option<u16> = tbl.get("width").ok();
                let height: Option<u16> = tbl.get("height").ok();
                let on_select: Option<(String, String)> = tbl.get::<String>("on_select_key").ok()
                    .map(|key| (String::new(), key));
                Some(PluginAction::ShowPopup { title, lines, width, height, on_select })
            }
            "emit_event" => {
                let name: String = tbl.get("name").ok()?;
                let mut data = std::collections::HashMap::new();
                if let Ok(data_tbl) = tbl.get::<mlua::Table>("data") {
                    for pair in data_tbl.pairs::<String, String>() {
                        if let Ok((k, v)) = pair {
                            data.insert(k, v);
                        }
                    }
                }
                Some(PluginAction::EmitEvent { name, data })
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
        if let Some(text) = &ctx.buf.selected_text {
            let _ = snap.set("selected_text", text.as_str());
        }
        // Options
        let _ = snap.set("tab_width", ctx.buf.tab_width);
        let _ = snap.set("expand_tab", ctx.buf.expand_tab);
        let _ = snap.set("auto_indent", ctx.buf.auto_indent);
        let _ = snap.set("word_wrap", ctx.buf.word_wrap);
        let _ = snap.set("line_numbers", ctx.buf.line_numbers.as_str());
        let _ = snap.set("pane_count", ctx.buf.pane_count);

        let _ = novim.set("_snapshot", snap);
    }

    /// Call all Lua handlers registered for `event_name`, passing event data as args.
    fn call_handlers(&mut self, event_name: &str, args: &HashMap<String, String>, ctx: &PluginContext) -> Vec<PluginAction> {
        self.last_had_error = false;
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

        // Get the current file path for pattern matching
        let current_path = args.get("path").cloned()
            .or_else(|| ctx.buf.path.clone())
            .unwrap_or_default();

        for pair in list.pairs::<i64, Value>() {
            let Ok((_, entry)) = pair else { continue };
            match entry {
                // New format: { fn=handler, pattern="*.rs" }
                Value::Table(tbl) => {
                    // Check pattern filter
                    if let Ok(pattern) = tbl.get::<String>("pattern") {
                        let pat = glob::Pattern::new(&pattern).ok();
                        if let Some(pat) = pat {
                            if !pat.matches(&current_path) {
                                continue; // Skip — pattern doesn't match
                            }
                        }
                    }
                    if let Ok(handler) = tbl.get::<Function>("fn") {
                        if let Err(e) = handler.call::<()>(args_table.clone()) {
                            log::warn!("Lua handler error in {} for {}: {}", self.id, event_name, e);
                            self.last_had_error = true;
                        }
                    }
                }
                Value::Function(handler) => {
                    if let Err(e) = handler.call::<()>(args_table.clone()) {
                        log::warn!("Lua handler error in {} for {}: {}", self.id, event_name, e);
                        self.last_had_error = true;
                    }
                }
                _ => {}
            }
        }

        self.drain_actions()
    }

    /// Drain actions queued during init (e.g. keymap registrations).
    pub fn drain_init_actions(&self) -> Vec<PluginAction> {
        self.drain_actions()
    }

    /// Run scheduled callbacks and check deferred timers. Returns any actions they produce.
    pub fn poll_timers(&self) -> Vec<PluginAction> {
        let Ok(novim) = self.lua.globals().get::<mlua::Table>("novim") else { return vec![] };

        // Run scheduled callbacks (one-shot, cleared after execution)
        if let Ok(Value::Table(scheduled)) = novim.get::<Value>("_scheduled") {
            for pair in scheduled.clone().pairs::<i64, Function>() {
                if let Ok((_, callback)) = pair {
                    if let Err(e) = callback.call::<()>(()) {
                        log::warn!("Lua scheduled callback error in {}: {}", self.id, e);
                    }
                }
            }
            if let Ok(fresh) = self.lua.create_table() {
                let _ = novim.set("_scheduled", fresh);
            }
        }

        // Check deferred timers
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if let Ok(Value::Table(deferred)) = novim.get::<Value>("_deferred") {
            let Ok(remaining) = self.lua.create_table() else { return self.drain_actions() };
            let mut remaining_idx = 1;

            for pair in deferred.clone().pairs::<i64, mlua::Table>() {
                if let Ok((_, entry)) = pair {
                    let run_at: u64 = entry.get("run_at").unwrap_or(0);
                    if now >= run_at {
                        if let Ok(callback) = entry.get::<Function>("fn") {
                            if let Err(e) = callback.call::<()>(()) {
                                log::warn!("Lua deferred callback error in {}: {}", self.id, e);
                            }
                        }
                    } else {
                        let _ = remaining.set(remaining_idx, entry);
                        remaining_idx += 1;
                    }
                }
            }
            let _ = novim.set("_deferred", remaining);
        }

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
                        if let Ok(handler) = callbacks.get::<Function>(callback_key) {
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

    #[test]
    fn lua_opt_get() {
        let mut plugin = lua_plugin_from_source("test_opt", r#"
            novim.on("BufOpen", function(args)
                local tw = novim.opt.get("tab_width")
                local wrap = novim.opt.get("word_wrap")
                novim.exec("echo tw=" .. tw .. " wrap=" .. tostring(wrap))
            end)
        "#);
        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(&EditorEvent::BufOpen { path: "test.rs".into() }, &ctx);
        assert_exec_commands(&actions, &["echo tw=4 wrap=false"]);
    }

    #[test]
    fn lua_opt_set() {
        let mut plugin = lua_plugin_from_source("test_opt_set", r#"
            novim.on("BufOpen", function(args)
                novim.opt.set("word_wrap", true)
                novim.opt.set("tab_width", 2)
            end)
        "#);
        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(&EditorEvent::BufOpen { path: "test.rs".into() }, &ctx);
        assert_exec_commands(&actions, &["set wrap", "set ts=2"]);
    }

    #[test]
    fn lua_win_api() {
        let mut plugin = lua_plugin_from_source("test_win", r#"
            novim.on("BufOpen", function(args)
                local n = novim.win.count()
                novim.exec("echo panes=" .. n)
                novim.win.split("vertical")
            end)
        "#);
        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(&EditorEvent::BufOpen { path: "test.rs".into() }, &ctx);
        // Should have echo + vsplit
        assert_eq!(actions.len(), 2);
        assert!(matches!(&actions[0], PluginAction::ExecCommand(s) if s == "echo panes=1"));
        assert!(matches!(&actions[1], PluginAction::ExecCommand(s) if s == "vsplit"));
    }

    #[test]
    fn lua_autocmd_filter() {
        let mut plugin = lua_plugin_from_source("test_filter", r#"
            novim.on("BufWrite", { pattern = "*.rs" }, function(args)
                novim.exec("echo rust file saved")
            end)
            novim.on("BufWrite", { pattern = "*.lua" }, function(args)
                novim.exec("echo lua file saved")
            end)
        "#);
        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        // .rs file should trigger first handler only
        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(&EditorEvent::BufWrite { path: "main.rs".into() }, &ctx);
        assert_exec_commands(&actions, &["echo rust file saved"]);

        // .lua file should trigger second handler only
        let actions = plugin.on_event(&EditorEvent::BufWrite { path: "init.lua".into() }, &ctx);
        assert_exec_commands(&actions, &["echo lua file saved"]);

        // .py file should trigger neither
        let actions = plugin.on_event(&EditorEvent::BufWrite { path: "test.py".into() }, &ctx);
        assert!(actions.is_empty());
    }

    #[test]
    fn lua_selection_api() {
        let mut plugin = lua_plugin_from_source("test_sel", r#"
            novim.on("BufOpen", function(args)
                novim.buf.set_selection(0, 0, 1, 5)
            end)
        "#);
        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(&EditorEvent::BufOpen { path: "test.rs".into() }, &ctx);
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], PluginAction::SetSelection {
            start_line: 0, start_col: 0, end_line: 1, end_col: 5
        }));
    }

    #[test]
    fn lua_emit_custom_event() {
        let mut plugin = lua_plugin_from_source("test_emit", r#"
            -- Listen for custom event
            novim.on("my_event", function(args)
                novim.exec("echo got " .. (args.key or "nil"))
            end)
            -- Emit it on BufOpen
            novim.on("BufOpen", function(args)
                novim.emit("my_event", { key = "hello" })
            end)
        "#);
        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(&EditorEvent::BufOpen { path: "test.rs".into() }, &ctx);
        // Should have EmitEvent action
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], PluginAction::EmitEvent { name, .. } if name == "my_event"));
    }

    #[test]
    fn lua_schedule() {
        let mut plugin = lua_plugin_from_source("test_schedule", r#"
            novim.on("BufOpen", function(args)
                novim.schedule(function()
                    novim.exec("echo scheduled!")
                end)
            end)
        "#);
        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        // BufOpen should schedule (not execute yet)
        let actions = plugin.on_event(&EditorEvent::BufOpen { path: "test.rs".into() }, &ctx);
        assert!(actions.is_empty()); // schedule doesn't produce immediate actions

        // poll_timers should run the scheduled callback
        let actions = plugin.poll_timers();
        assert_exec_commands(&actions, &["echo scheduled!"]);

        // Second poll should have nothing
        let actions = plugin.poll_timers();
        assert!(actions.is_empty());
    }

    #[test]
    fn lua_defer() {
        let mut plugin = lua_plugin_from_source("test_defer", r#"
            novim.on("BufOpen", function(args)
                novim.defer(0, function()
                    novim.exec("echo deferred!")
                end)
            end)
        "#);
        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        plugin.on_event(&EditorEvent::BufOpen { path: "test.rs".into() }, &ctx);

        // 0ms delay means it should fire immediately on next poll
        let actions = plugin.poll_timers();
        assert_exec_commands(&actions, &["echo deferred!"]);
    }

    #[test]
    fn lua_lsp_on_attach() {
        let mut plugin = lua_plugin_from_source("test_lsp", r#"
            novim.lsp.on_attach(function(args)
                novim.exec("echo LSP attached: " .. args.language)
            end)
        "#);
        let mut init_ctx = PluginContext::new(false);
        plugin.init(&mut init_ctx);

        let ctx = ctx_with_snapshot();
        let actions = plugin.on_event(
            &EditorEvent::LspAttach { path: "main.rs".into(), language: "rust".into() },
            &ctx,
        );
        assert_exec_commands(&actions, &["echo LSP attached: rust"]);
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
            selected_text: None,
            tab_width: 4,
            expand_tab: true,
            auto_indent: true,
            word_wrap: false,
            line_numbers: "hybrid".into(),
            pane_count: 1,
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
