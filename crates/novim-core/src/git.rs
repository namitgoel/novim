//! Git integration utilities: blame, diff.

use std::collections::HashMap;
use std::path::Path;

use crate::buffer::BlameInfo;

/// Compute git blame for a file. Returns line_number → BlameInfo.
pub fn compute_blame(file_path: &Path) -> HashMap<usize, BlameInfo> {
    let mut result = HashMap::new();

    let repo = match git2::Repository::discover(file_path.parent().unwrap_or(Path::new("."))) {
        Ok(r) => r,
        Err(_) => return result,
    };

    let workdir = match repo.workdir() {
        Some(w) => w,
        None => return result,
    };
    let rel_path = match file_path.strip_prefix(workdir) {
        Ok(p) => p,
        Err(_) => return result,
    };

    let blame = match repo.blame_file(rel_path, None) {
        Ok(b) => b,
        Err(_) => return result,
    };

    for i in 0..blame.len() {
        let Some(hunk) = blame.get_index(i) else { continue };
        let sig = hunk.final_signature();
        let author = sig.name().unwrap_or("unknown").to_string();
        let time = sig.when();
        let epoch = time.seconds();
        // Format as YYYY-MM-DD
        let date = format_epoch(epoch);
        let commit_id = hunk.final_commit_id();

        // Get commit message summary
        let summary = repo.find_commit(commit_id)
            .ok()
            .and_then(|c| c.summary().map(|s| s.to_string()))
            .unwrap_or_default();

        let start = hunk.final_start_line().saturating_sub(1);
        let lines = hunk.lines_in_hunk();
        for line in start..start + lines {
            result.insert(line, BlameInfo {
                author: author.clone(),
                date: date.clone(),
                summary: summary.clone(),
            });
        }
    }

    result
}

/// Get the HEAD version of a file as a string.
pub fn head_file_content(file_path: &Path) -> Option<String> {
    let repo = git2::Repository::discover(file_path.parent()?).ok()?;
    let workdir = repo.workdir()?;
    let rel_path = file_path.strip_prefix(workdir).ok()?;

    let head = repo.head().ok()?.peel_to_tree().ok()?;
    let entry = head.get_path(rel_path).ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;

    std::str::from_utf8(blob.content()).ok().map(|s| s.to_string())
}

/// Compute line-level diff between two strings (old vs new).
/// Returns (old_diff, new_diff) maps of line → DiffLineKind.
pub fn compute_line_diff(old: &str, new: &str) -> (HashMap<usize, crate::buffer::DiffLineKind>, HashMap<usize, crate::buffer::DiffLineKind>) {
    use crate::buffer::DiffLineKind;
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut old_diff = HashMap::new();
    let mut new_diff = HashMap::new();

    // Simple LCS-based diff
    let m = old_lines.len();
    let n = new_lines.len();

    // Build a set of matching lines using a simple approach:
    // Walk both sides, mark lines that differ.
    let mut oi = 0;
    let mut ni = 0;

    while oi < m && ni < n {
        if old_lines[oi] == new_lines[ni] {
            // Same line, no diff
            oi += 1;
            ni += 1;
        } else {
            // Check if it's a change (both differ) or add/remove
            // Look ahead to find a match
            let old_ahead = old_lines[oi..].iter().position(|&l| ni < n && l == new_lines[ni]);
            let new_ahead = new_lines[ni..].iter().position(|&l| oi < m && l == old_lines[oi]);

            match (old_ahead, new_ahead) {
                (Some(0), _) | (None, Some(_)) => {
                    // Line added in new
                    new_diff.insert(ni, DiffLineKind::Added);
                    ni += 1;
                }
                (Some(_), Some(0)) | (Some(_), None) => {
                    // Line removed from old
                    old_diff.insert(oi, DiffLineKind::Removed);
                    oi += 1;
                }
                _ => {
                    // Changed line
                    old_diff.insert(oi, DiffLineKind::Changed);
                    new_diff.insert(ni, DiffLineKind::Changed);
                    oi += 1;
                    ni += 1;
                }
            }
        }
    }

    // Remaining lines
    while oi < m {
        old_diff.insert(oi, DiffLineKind::Removed);
        oi += 1;
    }
    while ni < n {
        new_diff.insert(ni, DiffLineKind::Added);
        ni += 1;
    }

    (old_diff, new_diff)
}

fn format_epoch(epoch: i64) -> String {
    // Simple date formatting without chrono dependency
    let days = epoch / 86400;
    // Unix epoch = 1970-01-01
    let mut y = 1970i64;
    let mut remaining = days;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }

    let months = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 1;
    for &dm in &months {
        if remaining < dm { break; }
        remaining -= dm;
        m += 1;
    }
    let d = remaining + 1;

    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
