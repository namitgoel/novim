//! Built-in plugins — Rust implementations that own real logic.

pub mod git_signs;
pub mod lsp_plugin;
pub mod syntax;

use std::sync::Arc;
use super::manager::PluginManager;

/// Register all built-in plugins.
pub fn register_builtins(manager: &mut PluginManager) {
    let _ = manager.add(Box::new(git_signs::GitSignsPlugin::new()));
}

/// Register the LSP plugin (needs the registry, so called separately).
pub fn register_lsp(manager: &mut PluginManager, registry: Arc<crate::lsp::provider::LspRegistry>) {
    let _ = manager.add(Box::new(lsp_plugin::LspPlugin::new(registry)));
}
