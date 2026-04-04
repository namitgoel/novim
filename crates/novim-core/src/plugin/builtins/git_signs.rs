//! Built-in git signs plugin — computes gutter signs from git diff.
//!
//! Subscribes to BufOpen and BufWrite events, reads the file path from
//! the buffer snapshot, runs git diff, and returns a SetGutterSigns action.
//!
//! Can be disabled via config: `[plugins.git_signs] enabled = false`

use std::collections::HashMap;
use std::path::Path;

use crate::plugin::{EditorEvent, GutterSign, Plugin, PluginAction, PluginContext};

pub struct GitSignsPlugin;

impl GitSignsPlugin {
    pub fn new() -> Self {
        Self
    }
}

/// Compute git diff signs for a file.
/// Returns a map of line_number → GutterSign.
fn diff_signs(file_path: &Path) -> HashMap<usize, GutterSign> {
    let mut signs = HashMap::new();

    let repo = match git2::Repository::discover(file_path.parent().unwrap_or(Path::new("."))) {
        Ok(r) => r,
        Err(_) => return signs,
    };

    let workdir = match repo.workdir() {
        Some(w) => w,
        None => return signs,
    };
    let rel_path = match file_path.strip_prefix(workdir) {
        Ok(p) => p,
        Err(_) => return signs,
    };

    let head = repo.head().and_then(|h| h.peel_to_tree()).ok();

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.pathspec(rel_path);

    let diff = match repo.diff_tree_to_workdir(head.as_ref(), Some(&mut diff_opts)) {
        Ok(d) => d,
        Err(_) => return signs,
    };

    let _ = diff.foreach(
        &mut |_, _| true,
        None,
        Some(&mut |_delta, hunk| {
            let new_start = hunk.new_start() as usize;
            let new_lines = hunk.new_lines() as usize;
            let old_lines = hunk.old_lines() as usize;

            if old_lines == 0 {
                for i in 0..new_lines {
                    signs.insert(new_start - 1 + i, GutterSign::Added);
                }
            } else if new_lines == 0 {
                if new_start > 0 {
                    signs.insert(new_start - 1, GutterSign::Deleted);
                }
            } else {
                for i in 0..new_lines {
                    signs.insert(new_start - 1 + i, GutterSign::Modified);
                }
            }
            true
        }),
        None,
    );

    signs
}

impl Plugin for GitSignsPlugin {
    fn id(&self) -> &str { "git_signs" }
    fn name(&self) -> &str { "Git Signs" }
    fn is_builtin(&self) -> bool { true }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }

    fn init(&mut self, _ctx: &mut PluginContext) {}

    fn on_event(&mut self, event: &EditorEvent, ctx: &PluginContext) -> Vec<PluginAction> {
        match event {
            EditorEvent::BufOpen { .. } | EditorEvent::BufWrite { .. } => {
                if let Some(path) = &ctx.buf.path {
                    if path != "[No Name]" {
                        let signs = diff_signs(Path::new(path));
                        return vec![PluginAction::SetGutterSigns(signs)];
                    }
                }
                vec![]
            }
            _ => vec![],
        }
    }
}
