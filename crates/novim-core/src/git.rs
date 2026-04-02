//! Git integration — diff signs for the gutter.

use std::collections::HashMap;
use std::path::Path;

/// Git gutter sign for a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitSign {
    Added,
    Modified,
    Deleted,
}

/// Compute git diff signs for a file.
/// Returns a map of line_number → GitSign.
pub fn diff_signs(file_path: &Path) -> HashMap<usize, GitSign> {
    let mut signs = HashMap::new();

    let repo = match git2::Repository::discover(file_path.parent().unwrap_or(Path::new("."))) {
        Ok(r) => r,
        Err(_) => return signs,
    };

    // Get the file path relative to the repo root
    let workdir = match repo.workdir() {
        Some(w) => w,
        None => return signs,
    };
    let rel_path = match file_path.strip_prefix(workdir) {
        Ok(p) => p,
        Err(_) => return signs,
    };

    // Diff HEAD against working directory
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
                // Pure addition
                for i in 0..new_lines {
                    signs.insert(new_start - 1 + i, GitSign::Added);
                }
            } else if new_lines == 0 {
                // Pure deletion — mark the line before
                if new_start > 0 {
                    signs.insert(new_start - 1, GitSign::Deleted);
                }
            } else {
                // Modification
                for i in 0..new_lines {
                    signs.insert(new_start - 1 + i, GitSign::Modified);
                }
            }
            true
        }),
        None,
    );

    signs
}
