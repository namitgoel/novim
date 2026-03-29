//! Code folding support — indent-based fold detection and state management.

/// A foldable region in the buffer.
#[derive(Debug, Clone)]
pub struct FoldRegion {
    /// First line of the fold (the header line, always visible).
    pub start_line: usize,
    /// Last line of the fold (inclusive).
    pub end_line: usize,
    /// Whether this fold is collapsed.
    pub collapsed: bool,
}

/// Manages fold state for a buffer.
#[derive(Debug, Clone, Default)]
pub struct FoldState {
    regions: Vec<FoldRegion>,
}

impl FoldState {
    pub fn new() -> Self {
        Self { regions: Vec::new() }
    }

    /// Detect foldable regions based on indentation.
    /// A fold starts at a line with indentation N and ends at the last
    /// consecutive line with indentation > N.
    pub fn detect_indent_folds(lines: &[String], tab_width: usize) -> Self {
        let mut regions = Vec::new();
        let indents: Vec<usize> = lines.iter().map(|l| indent_level(l, tab_width)).collect();
        let len = lines.len();

        let mut i = 0;
        while i < len {
            // Skip blank lines
            if lines[i].trim().is_empty() {
                i += 1;
                continue;
            }
            let base_indent = indents[i];
            // Look for a block of lines with deeper indentation following this line
            let mut end = i;
            let mut j = i + 1;
            while j < len {
                if lines[j].trim().is_empty() {
                    // Blank lines don't end a fold
                    j += 1;
                    continue;
                }
                if indents[j] > base_indent {
                    end = j;
                    j += 1;
                } else {
                    break;
                }
            }
            if end > i {
                regions.push(FoldRegion {
                    start_line: i,
                    end_line: end,
                    collapsed: false,
                });
            }
            i += 1;
        }

        Self { regions }
    }

    /// Toggle fold at the given line. Matches the fold whose start_line equals
    /// the given line, or the innermost fold containing the line.
    pub fn toggle_fold(&mut self, line: usize) -> bool {
        // First try exact start_line match
        for region in &mut self.regions {
            if region.start_line == line {
                region.collapsed = !region.collapsed;
                return true;
            }
        }
        // Fall back: find the innermost fold containing this line
        let mut best: Option<usize> = None;
        for (i, region) in self.regions.iter().enumerate() {
            if line >= region.start_line && line <= region.end_line {
                if best.map_or(true, |b| {
                    let prev = &self.regions[b];
                    (region.end_line - region.start_line) < (prev.end_line - prev.start_line)
                }) {
                    best = Some(i);
                }
            }
        }
        if let Some(i) = best {
            self.regions[i].collapsed = !self.regions[i].collapsed;
            return true;
        }
        false
    }

    /// Collapse all folds.
    pub fn fold_all(&mut self) {
        for region in &mut self.regions {
            region.collapsed = true;
        }
    }

    /// Expand all folds.
    pub fn unfold_all(&mut self) {
        for region in &mut self.regions {
            region.collapsed = false;
        }
    }

    /// Check if a line is hidden by a collapsed fold.
    pub fn is_line_hidden(&self, line: usize) -> bool {
        self.regions.iter().any(|r| r.collapsed && line > r.start_line && line <= r.end_line)
    }

    /// Get the fold region starting at a given line, if any.
    pub fn fold_at(&self, line: usize) -> Option<&FoldRegion> {
        self.regions.iter().find(|r| r.start_line == line)
    }

    /// Get the next visible line after `line` (skipping folded regions).
    pub fn next_visible_line(&self, line: usize, total_lines: usize) -> usize {
        let mut next = line + 1;
        while next < total_lines && self.is_line_hidden(next) {
            next += 1;
        }
        next
    }

    /// Get the previous visible line before `line` (skipping folded regions).
    pub fn prev_visible_line(&self, line: usize) -> usize {
        if line == 0 { return 0; }
        let mut prev = line - 1;
        while prev > 0 && self.is_line_hidden(prev) {
            prev -= 1;
        }
        prev
    }

    pub fn regions(&self) -> &[FoldRegion] {
        &self.regions
    }

    pub fn has_folds(&self) -> bool {
        !self.regions.is_empty()
    }
}

/// Calculate indentation level of a line (number of leading spaces, with tabs expanded).
fn indent_level(line: &str, tab_width: usize) -> usize {
    let mut level = 0;
    for c in line.chars() {
        match c {
            ' ' => level += 1,
            '\t' => level += tab_width - (level % tab_width),
            _ => break,
        }
    }
    level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indent_folds() {
        let lines: Vec<String> = vec![
            "fn main() {".into(),
            "    let x = 1;".into(),
            "    if x > 0 {".into(),
            "        println!(\"hi\");".into(),
            "    }".into(),
            "}".into(),
        ];
        let folds = FoldState::detect_indent_folds(&lines, 4);
        assert_eq!(folds.regions().len(), 2); // main block and if block
        assert_eq!(folds.regions()[0].start_line, 0);
        assert_eq!(folds.regions()[0].end_line, 4);
    }

    #[test]
    fn test_toggle_fold() {
        let lines: Vec<String> = vec![
            "fn main() {".into(),
            "    let x = 1;".into(),
            "}".into(),
        ];
        let mut folds = FoldState::detect_indent_folds(&lines, 4);
        assert!(!folds.is_line_hidden(1));
        folds.toggle_fold(0);
        assert!(folds.is_line_hidden(1));
        assert!(!folds.is_line_hidden(0)); // header line is visible
    }

    #[test]
    fn test_typescript_interfaces() {
        let lines: Vec<String> = vec![
            "export interface ProjectSettings {".into(),
            "  theme?: 'light' | 'dark';".into(),
            "  autoSave?: boolean;".into(),
            "}".into(),
            "".into(),
            "export interface Project {".into(),
            "  id: string;".into(),
            "  name: string;".into(),
            "}".into(),
        ];
        let folds = FoldState::detect_indent_folds(&lines, 2);
        // Should find folds for both interfaces
        assert!(folds.regions().len() >= 2, "Expected at least 2 folds, got {}", folds.regions().len());
        // First fold starts at line 0 (ProjectSettings)
        assert_eq!(folds.regions()[0].start_line, 0);
        assert_eq!(folds.regions()[0].end_line, 2);
        // Second fold starts at line 5 (Project)
        assert_eq!(folds.regions()[1].start_line, 5);
        assert_eq!(folds.regions()[1].end_line, 7);

        // Toggle fold at line 0
        let mut folds = folds;
        assert!(folds.toggle_fold(0));
        assert!(folds.is_line_hidden(1));
        assert!(folds.is_line_hidden(2));
        assert!(!folds.is_line_hidden(3)); // closing brace visible
    }

    #[test]
    fn test_typescript_with_jsdoc() {
        let lines: Vec<String> = vec![
            "/**".into(),                                    // 0
            " * Project Entity".into(),                      // 1
            " */".into(),                                    // 2
            "".into(),                                       // 3
            "export interface ProjectSettings {".into(),     // 4
            "  theme?: 'light' | 'dark';".into(),            // 5
            "  autoSave?: boolean;".into(),                  // 6
            "}".into(),                                      // 7
            "".into(),                                       // 8
            "export interface Project {".into(),             // 9
            "  id: string;".into(),                          // 10
            "  name: string;".into(),                        // 11
            "}".into(),                                      // 12
        ];
        let folds = FoldState::detect_indent_folds(&lines, 2);
        // Print regions for debugging
        for (i, r) in folds.regions().iter().enumerate() {
            eprintln!("Region {}: start={}, end={}", i, r.start_line, r.end_line);
        }
        // Should have fold at line 4 (ProjectSettings) and line 9 (Project)
        let has_fold_at_4 = folds.regions().iter().any(|r| r.start_line == 4);
        let has_fold_at_9 = folds.regions().iter().any(|r| r.start_line == 9);
        assert!(has_fold_at_4, "Expected fold at line 4 (ProjectSettings)");
        assert!(has_fold_at_9, "Expected fold at line 9 (Project)");
    }

    #[test]
    fn test_fold_all_unfold_all() {
        let lines: Vec<String> = vec![
            "a".into(),
            "  b".into(),
            "  c".into(),
            "d".into(),
            "  e".into(),
        ];
        let mut folds = FoldState::detect_indent_folds(&lines, 2);
        folds.fold_all();
        assert!(folds.is_line_hidden(1));
        assert!(folds.is_line_hidden(4));
        folds.unfold_all();
        assert!(!folds.is_line_hidden(1));
    }
}
