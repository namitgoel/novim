//! File explorer — tree view of the filesystem.

use std::fs;
use std::path::{Path, PathBuf};

/// A node in the file tree.
#[derive(Debug)]
pub struct FileNode {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub children: Vec<FileNode>,
    pub expanded: bool,
    pub depth: usize,
}

impl FileNode {
    /// Create a file node (leaf).
    pub fn file(name: String, path: PathBuf, depth: usize) -> Self {
        Self {
            name,
            path,
            is_dir: false,
            children: Vec::new(),
            expanded: false,
            depth,
        }
    }

    /// Create a directory node.
    pub fn dir(name: String, path: PathBuf, depth: usize) -> Self {
        Self {
            name,
            path,
            is_dir: true,
            children: Vec::new(),
            expanded: false,
            depth,
        }
    }
}

/// File explorer state.
pub struct Explorer {
    pub root: FileNode,
    pub visible: Vec<usize>, // indices into flattened tree
    pub cursor: usize,
    flat: Vec<FlatEntry>,
}

#[derive(Debug, Clone)]
pub struct FlatEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    pub depth: usize,
}

impl Explorer {
    /// Create an explorer rooted at the given directory.
    pub fn new(root_path: &Path) -> std::io::Result<Self> {
        let name = root_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(".")
            .to_string();

        let mut root = FileNode::dir(name, root_path.to_path_buf(), 0);
        root.expanded = true;
        load_children(&mut root)?;

        let mut explorer = Self {
            root,
            visible: Vec::new(),
            cursor: 0,
            flat: Vec::new(),
        };
        explorer.rebuild_flat();
        Ok(explorer)
    }

    /// Rebuild the flat list from the tree.
    fn rebuild_flat(&mut self) {
        self.flat.clear();
        flatten_tree(&self.root, &mut self.flat);
        // Clamp cursor
        if self.cursor >= self.flat.len() {
            self.cursor = self.flat.len().saturating_sub(1);
        }
    }

    /// Get the flattened entries for rendering.
    pub fn entries(&self) -> &[FlatEntry] {
        &self.flat
    }

    /// Get the number of visible entries.
    pub fn len(&self) -> usize {
        self.flat.len()
    }

    /// Check if there are no visible entries.
    pub fn is_empty(&self) -> bool {
        self.flat.is_empty()
    }

    /// Move cursor up.
    pub fn cursor_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor down.
    pub fn cursor_down(&mut self) {
        if self.cursor + 1 < self.flat.len() {
            self.cursor += 1;
        }
    }

    /// Get the path at the cursor.
    pub fn cursor_path(&self) -> Option<&Path> {
        self.flat.get(self.cursor).map(|e| e.path.as_path())
    }

    /// Get the entry at the cursor.
    pub fn cursor_entry(&self) -> Option<&FlatEntry> {
        self.flat.get(self.cursor)
    }

    /// Is the cursor on a directory?
    pub fn cursor_is_dir(&self) -> bool {
        self.flat.get(self.cursor).map(|e| e.is_dir).unwrap_or(false)
    }

    /// Toggle expand/collapse on the cursor entry (if directory).
    pub fn toggle_expand(&mut self) {
        if let Some(entry) = self.flat.get(self.cursor) {
            if entry.is_dir {
                let path = entry.path.clone();
                toggle_dir(&mut self.root, &path);
                self.rebuild_flat();
            }
        }
    }

    /// Open the entry at cursor. Returns file path if it's a file, or toggles dir.
    pub fn open_at_cursor(&mut self) -> Option<PathBuf> {
        if let Some(entry) = self.flat.get(self.cursor) {
            if entry.is_dir {
                let path = entry.path.clone();
                toggle_dir(&mut self.root, &path);
                self.rebuild_flat();
                None
            } else {
                Some(entry.path.clone())
            }
        } else {
            None
        }
    }

    /// Get the display name for an entry.
    pub fn entry_display(&self, idx: usize) -> Option<(String, bool, bool, usize)> {
        self.flat.get(idx).map(|e| {
            (e.name.clone(), e.is_dir, e.expanded, e.depth)
        })
    }
}

/// Load children of a directory node.
fn load_children(node: &mut FileNode) -> std::io::Result<()> {
    if !node.is_dir {
        return Ok(());
    }

    node.children.clear();
    let mut entries: Vec<_> = fs::read_dir(&node.path)?
        .filter_map(|e| e.ok())
        .collect();

    // Sort: directories first, then alphabetically
    entries.sort_by(|a, b| {
        let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    let depth = node.depth + 1;
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files/dirs
        if name.starts_with('.') {
            continue;
        }

        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

        if is_dir {
            node.children.push(FileNode::dir(name, path, depth));
        } else {
            node.children.push(FileNode::file(name, path, depth));
        }
    }

    Ok(())
}

/// Toggle a directory's expanded state in the tree.
fn toggle_dir(node: &mut FileNode, target: &Path) {
    if node.path == target && node.is_dir {
        node.expanded = !node.expanded;
        if node.expanded && node.children.is_empty() {
            let _ = load_children(node);
        }
        return;
    }
    for child in &mut node.children {
        toggle_dir(child, target);
    }
}

/// Flatten the tree into a list for rendering.
fn flatten_tree(node: &FileNode, out: &mut Vec<FlatEntry>) {
    out.push(FlatEntry {
        path: node.path.clone(),
        name: node.name.clone(),
        is_dir: node.is_dir,
        expanded: node.expanded,
        depth: node.depth,
    });

    if node.expanded {
        for child in &node.children {
            flatten_tree(child, out);
        }
    }
}
