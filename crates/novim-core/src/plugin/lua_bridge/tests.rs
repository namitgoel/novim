//! Tests for LuaPlugin.

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
        text: Some("hello world\nsecond line\nthird line\n".into()),
        version: Some(0),
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
