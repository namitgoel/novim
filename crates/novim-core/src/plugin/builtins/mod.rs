//! Built-in plugins — Rust implementations that own real logic.

pub mod git_signs;

use super::manager::PluginManager;

/// Register all built-in plugins.
pub fn register_builtins(manager: &mut PluginManager) {
    let _ = manager.add(Box::new(git_signs::GitSignsPlugin::new()));
}
