//! Novim Neon — Node.js FFI bindings via Neon.
//!
//! This is the cdylib entry point that TypeScript calls.
//! It delegates to novim-tui for terminal operations
//! and novim-core for session listing.

use neon::prelude::*;

fn hello(mut cx: FunctionContext) -> JsResult<JsString> {
    Ok(cx.string("Hello from Rust via Neon! 🦀"))
}

fn echo(mut cx: FunctionContext) -> JsResult<JsString> {
    let input = cx.argument::<JsString>(0)?.value(&mut cx);
    Ok(cx.string(format!("Rust echoes: {}", input)))
}

/// DRY macro for running a terminal session with error conversion.
macro_rules! run_session {
    ($cx:expr, $body:expr) => {{
        let result: std::io::Result<()> = (|| $body)();
        if let Err(e) = result {
            return $cx.throw_error(format!("Terminal error: {}", e));
        }
        Ok($cx.undefined())
    }};
}

fn run_terminal(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    run_session!(cx, {
        let mut tm = novim_tui::TerminalManager::new()?;
        tm.run()?;
        tm.shutdown()
    })
}

fn run_terminal_with_file(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let file_path = cx.argument::<JsString>(0)?.value(&mut cx);
    run_session!(cx, {
        let mut tm = novim_tui::TerminalManager::with_file(&file_path)?;
        tm.run()?;
        tm.shutdown()
    })
}

fn run_attach_session(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let name = cx.argument::<JsString>(0)?.value(&mut cx);
    run_session!(cx, {
        let mut tm = novim_tui::TerminalManager::with_session(&name)?;
        tm.run()?;
        tm.shutdown()
    })
}

fn list_sessions(mut cx: FunctionContext) -> JsResult<JsArray> {
    let sessions = novim_core::session::list_sessions()
        .map_err(|e| e.to_string())
        .or_else(|e| cx.throw_error::<_, Vec<String>>(e))
        .unwrap_or_default();

    let arr = cx.empty_array();
    for (i, name) in sessions.iter().enumerate() {
        let val = cx.string(name);
        arr.set(&mut cx, i as u32, val)?;
    }
    Ok(arr)
}

fn run_terminal_mode(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    run_session!(cx, {
        let mut tm = novim_tui::TerminalManager::with_terminal()?;
        tm.run()?;
        tm.shutdown()
    })
}

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("hello", hello)?;
    cx.export_function("echo", echo)?;
    cx.export_function("runTerminal", run_terminal)?;
    cx.export_function("runTerminalWithFile", run_terminal_with_file)?;
    cx.export_function("runTerminalMode", run_terminal_mode)?;
    cx.export_function("runAttachSession", run_attach_session)?;
    cx.export_function("listSessions", list_sessions)?;
    Ok(())
}
