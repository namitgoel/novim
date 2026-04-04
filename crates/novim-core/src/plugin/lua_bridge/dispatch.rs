//! Dispatch methods for LuaPlugin — draining actions, injecting snapshots, calling handlers.

use std::collections::HashMap;

use mlua::{Function, Value};

use super::LuaPlugin;
use super::super::{PluginAction, PluginContext};

impl LuaPlugin {
    /// Drain all queued actions (exec commands + structured actions).
    pub(super) fn drain_actions(&self) -> Vec<PluginAction> {
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
            "open_float" => {
                let title: String = tbl.get("title").ok()?;
                let lines_tbl: mlua::Table = tbl.get("lines").ok()?;
                let lines: Vec<String> = lines_tbl.pairs::<i64, String>()
                    .filter_map(|r| r.ok().map(|(_, v)| v))
                    .collect();
                let width: u16 = tbl.get("width").unwrap_or(60);
                let height: u16 = tbl.get("height").unwrap_or(20);
                Some(PluginAction::OpenFloat { title, lines, width, height })
            }
            "close_float" => {
                Some(PluginAction::CloseFloat)
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
                        super::super::KeymapAction::Command(cmd)
                    }
                    "lua_callback" => {
                        let callback_key: String = tbl.get("callback_key").ok()?;
                        super::super::KeymapAction::LuaCallback {
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
    pub(super) fn inject_snapshot(&self, ctx: &PluginContext) {
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
    pub(super) fn call_handlers(&mut self, event_name: &str, args: &HashMap<String, String>, ctx: &PluginContext) -> Vec<PluginAction> {
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
