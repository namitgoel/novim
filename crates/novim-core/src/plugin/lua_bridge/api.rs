//! API setup methods for LuaPlugin — each `setup_*` populates a section of the `novim` table.

use mlua::{Function, Value};

use super::LuaPlugin;

impl LuaPlugin {
    /// Set up event handlers table, commands table, `novim.on()`, `novim.register_command()`, and `novim.exec()`.
    pub(super) fn setup_event_system(&self, novim: &mlua::Table) -> Result<(), String> {
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
    pub(super) fn setup_buf_api(&self, novim: &mlua::Table) -> Result<(), String> {
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
    pub(super) fn setup_fn_api(&self, novim: &mlua::Table) -> Result<(), String> {
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
    pub(super) fn setup_ui_api(&self, novim: &mlua::Table) -> Result<(), String> {
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
    pub(super) fn setup_opt_api(&self, novim: &mlua::Table) -> Result<(), String> {
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
    pub(super) fn setup_win_api(&self, novim: &mlua::Table) -> Result<(), String> {
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
    pub(super) fn setup_keymap_api(&self, novim: &mlua::Table) -> Result<(), String> {
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
    pub(super) fn setup_misc_api(&self, novim: &mlua::Table) -> Result<(), String> {
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
}
