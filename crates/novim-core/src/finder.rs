//! File finder — fuzzy search for files in a directory tree.

use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

/// A file search result.
#[derive(Debug, Clone)]
pub struct FileMatch {
    pub path: PathBuf,
    pub display: String, // relative path for display
    pub score: i32,
}

/// Find files matching a query in the given directory.
pub fn find_files(root: &Path, query: &str, max_results: usize) -> Vec<FileMatch> {
    let mut all_files = Vec::new();
    let max_collect = 5000; // Stop collecting after this many files
    collect_files(root, root, &mut all_files, 0, max_collect);

    if query.is_empty() {
        all_files.truncate(max_results);
        return all_files;
    }

    // Try regex first, fall back to fuzzy matching
    let regex_match = Regex::new(query).ok();

    let query_lower = query.to_lowercase();
    let mut matches: Vec<FileMatch> = all_files
        .into_iter()
        .filter_map(|mut f| {
            // Try regex match
            if let Some(ref re) = regex_match {
                if re.is_match(&f.display) {
                    f.score = 100 + (1000 / (f.display.len() as i32 + 1));
                    return Some(f);
                }
            }

            // Fall back to fuzzy match
            let score = fuzzy_score(&f.display.to_lowercase(), &query_lower);
            if score > 0 {
                f.score = score;
                Some(f)
            } else {
                None
            }
        })
        .collect();

    matches.sort_by(|a, b| {
        b.score.cmp(&a.score)
            .then(a.display.len().cmp(&b.display.len()))
    });

    matches.truncate(max_results);
    matches
}

/// Simple fuzzy matching score.
/// Returns 0 if no match, higher = better match.
fn fuzzy_score(haystack: &str, needle: &str) -> i32 {
    if needle.is_empty() {
        return 1;
    }

    // Exact substring match gets highest score
    if haystack.contains(needle) {
        return 100 + (1000 / (haystack.len() as i32 + 1));
    }

    // Check if filename (last component) contains the query
    if let Some(filename) = haystack.rsplit('/').next() {
        if filename.contains(needle) {
            return 80 + (1000 / (filename.len() as i32 + 1));
        }
    }

    // Fuzzy: all chars of needle appear in order in haystack
    let mut needle_chars = needle.chars();
    let mut current = needle_chars.next();
    let mut matched = 0;

    for c in haystack.chars() {
        if let Some(nc) = current {
            if c == nc {
                matched += 1;
                current = needle_chars.next();
            }
        } else {
            break;
        }
    }

    if current.is_some() {
        0 // Not all chars matched
    } else {
        matched * 10 // Partial score based on matches
    }
}

/// Recursively collect files, skipping hidden dirs and common ignores.
fn collect_files(root: &Path, current: &Path, out: &mut Vec<FileMatch>, depth: usize, max: usize) {
    if depth > 6 || out.len() >= max {
        return;
    }

    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if out.len() >= max {
            return;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files/dirs and common ignores
        if name.starts_with('.')
            || name == "node_modules"
            || name == "target"
            || name == "dist"
            || name == "__pycache__"
            || name == "build"
            || name == ".git"
            || name == "vendor"
            || name == "Pods"
            || name == "DerivedData"
        {
            continue;
        }

        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out, depth + 1, max);
        } else {
            let display = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            out.push(FileMatch {
                path,
                display,
                score: 0,
            });
        }
    }
}
