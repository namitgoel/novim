//! Session persistence — save/restore editor state.
//!
//! Sessions are stored as JSON in ~/.novim/sessions/<name>.json.
//! Terminal panes are recorded as placeholders (reopened as empty editors on restore).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::buffer::Buffer;
use crate::error::{NovimError, Result};
use crate::pane::{Pane, PaneId, PaneManager, PaneNode, SplitDirection};

/// Restored workspace data: (name, pane_manager, launch_dir).
pub type RestoredWorkspaces = (Vec<(String, PaneManager, String)>, usize);

/// Serializable session state (supports multiple workspaces).
#[derive(Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub layout: PaneLayout,
    pub focused_pane: PaneId,
    /// Multiple workspaces (v2 format). If present, `layout` and `focused_pane` are ignored.
    #[serde(default)]
    pub workspaces: Vec<WorkspaceState>,
    #[serde(default)]
    pub active_workspace: usize,
}

/// Serializable workspace state.
#[derive(Serialize, Deserialize)]
pub struct WorkspaceState {
    pub name: String,
    pub layout: PaneLayout,
    pub focused_pane: PaneId,
    pub launch_dir: String,
}

/// Serializable pane tree layout.
#[derive(Clone, Serialize, Deserialize)]
pub enum PaneLayout {
    Leaf(PaneState),
    Split {
        direction: SerSplitDirection,
        ratio: f64,
        first: Box<PaneLayout>,
        second: Box<PaneLayout>,
    },
}

/// Serializable pane content state.
#[derive(Clone, Serialize, Deserialize)]
pub struct PaneState {
    pub id: PaneId,
    pub kind: PaneKind,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub viewport_offset: usize,
}

/// What kind of content the pane had.
#[derive(Clone, Serialize, Deserialize)]
pub enum PaneKind {
    /// Editor with a file path (None = unnamed buffer)
    Editor(Option<String>),
    /// Terminal pane (can't be restored — becomes empty editor)
    Terminal,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum SerSplitDirection {
    Horizontal,
    Vertical,
}

impl From<SplitDirection> for SerSplitDirection {
    fn from(d: SplitDirection) -> Self {
        match d {
            SplitDirection::Horizontal => SerSplitDirection::Horizontal,
            SplitDirection::Vertical => SerSplitDirection::Vertical,
        }
    }
}

impl From<SerSplitDirection> for SplitDirection {
    fn from(d: SerSplitDirection) -> Self {
        match d {
            SerSplitDirection::Horizontal => SplitDirection::Horizontal,
            SerSplitDirection::Vertical => SplitDirection::Vertical,
        }
    }
}

/// Get the sessions directory (~/.novim/sessions/).
fn sessions_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|_| NovimError::Session("HOME not set".to_string()))?;
    let dir = PathBuf::from(home).join(".novim").join("sessions");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Save a session to disk.
pub fn save_session(session: &Session) -> Result<String> {
    let dir = sessions_dir()?;
    let path = dir.join(format!("{}.json", session.name));
    let json = serde_json::to_string_pretty(session)?;
    fs::write(&path, json)?;
    Ok(format!("Session '{}' saved to {}", session.name, path.display()))
}

/// Load a session from disk.
pub fn load_session(name: &str) -> Result<Session> {
    let dir = sessions_dir()?;
    let path = dir.join(format!("{}.json", name));
    let json = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&json)?)
}

/// List all saved sessions.
pub fn list_sessions() -> Result<Vec<String>> {
    let dir = sessions_dir()?;
    let mut sessions = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                sessions.push(name.to_string());
            }
        }
    }
    sessions.sort();
    Ok(sessions)
}

/// Delete a saved session.
pub fn delete_session(name: &str) -> Result<()> {
    let dir = sessions_dir()?;
    let path = dir.join(format!("{}.json", name));
    fs::remove_file(path)?;
    Ok(())
}

/// Capture current editor state into a serializable Session.
pub fn capture_session(name: &str, panes: &PaneManager) -> Session {
    Session {
        name: name.to_string(),
        layout: capture_layout(panes),
        focused_pane: panes.focused_id(),
        workspaces: Vec::new(),
        active_workspace: 0,
    }
}

/// Capture a multi-workspace session.
pub fn capture_multi_session(
    name: &str,
    workspaces: &[(String, &PaneManager, String)], // (name, panes, launch_dir)
    active_workspace: usize,
) -> Session {
    let ws_states: Vec<WorkspaceState> = workspaces
        .iter()
        .map(|(ws_name, panes, dir)| WorkspaceState {
            name: ws_name.clone(),
            layout: capture_layout(panes),
            focused_pane: panes.focused_id(),
            launch_dir: dir.clone(),
        })
        .collect();

    // Use first workspace's layout as fallback for legacy format
    let (layout, focused) = if let Some(first) = ws_states.first() {
        (first.layout.clone(), first.focused_pane)
    } else {
        (PaneLayout::Leaf(PaneState {
            id: 0,
            kind: PaneKind::Editor(None),
            cursor_line: 0,
            cursor_col: 0,
            viewport_offset: 0,
        }), 0)
    };

    Session {
        name: name.to_string(),
        layout,
        focused_pane: focused,
        workspaces: ws_states,
        active_workspace,
    }
}

/// Capture the pane layout tree. Uses the pane manager's internal traversal.
fn capture_layout(panes: &PaneManager) -> PaneLayout {
    // We need to walk the tree — expose via a visitor method on PaneManager
    panes.visit_tree(&mut |pane| {
        let cursor = pane.content.as_buffer_like().cursor();
        let kind = if pane.content.as_buffer_like().is_terminal() {
            PaneKind::Terminal
        } else {
            let path = pane.content.as_buffer_like().display_name();
            let path = if path == "[No Name]" {
                None
            } else {
                // Try to get the full path from the buffer
                match &pane.content {
                    crate::pane::PaneContent::Editor(buf) => {
                        buf.file_path_str().map(|s| s.to_string())
                    }
                    _ => None,
                }
            };
            PaneKind::Editor(path)
        };
        PaneState {
            id: pane.id,
            kind,
            cursor_line: cursor.line,
            cursor_col: cursor.column,
            viewport_offset: pane.viewport_offset,
        }
    })
}

/// Restore a session — returns a PaneManager built from the session layout.
pub fn restore_session(session: &Session) -> Result<PaneManager> {
    let root = build_pane_tree(&session.layout)?;
    Ok(PaneManager::from_tree(root, session.focused_pane))
}

/// Restore multi-workspace session. Returns Vec of (name, PaneManager, launch_dir) + active index.
pub fn restore_multi_session(session: &Session) -> Result<RestoredWorkspaces> {
    if session.workspaces.is_empty() {
        // Legacy single-workspace session
        let mgr = restore_session(session)?;
        return Ok((vec![(session.name.clone(), mgr, ".".to_string())], 0));
    }

    let mut workspaces = Vec::new();
    for ws in &session.workspaces {
        let root = build_pane_tree(&ws.layout)?;
        let mgr = PaneManager::from_tree(root, ws.focused_pane);
        workspaces.push((ws.name.clone(), mgr, ws.launch_dir.clone()));
    }

    let active = session.active_workspace.min(workspaces.len().saturating_sub(1));
    Ok((workspaces, active))
}

/// Recursively build PaneNode tree from serialized layout.
fn build_pane_tree(layout: &PaneLayout) -> Result<PaneNode> {
    match layout {
        PaneLayout::Leaf(state) => {
            let pane = match &state.kind {
                PaneKind::Editor(Some(path)) => {
                    let mut buffer = Buffer::from_file(path)?;
                    buffer.set_cursor_position(state.cursor_line, state.cursor_col);
                    let mut p = Pane::new_editor(state.id, buffer);
                    p.viewport_offset = state.viewport_offset;
                    p
                }
                PaneKind::Editor(None) => {
                    Pane::new_editor(state.id, Buffer::new())
                }
                PaneKind::Terminal => {
                    // Restore terminal panes as new terminal sessions
                    match crate::emulator::TerminalPane::new(24, 80) {
                        Ok(term) => Pane::new_terminal(state.id, term),
                        Err(_) => Pane::new_editor(state.id, Buffer::new()), // fallback
                    }
                }
            };
            Ok(PaneNode::Leaf(pane))
        }
        PaneLayout::Split {
            direction,
            ratio,
            first,
            second,
        } => Ok(PaneNode::Split {
            direction: (*direction).into(),
            ratio: *ratio,
            first: Box::new(build_pane_tree(first)?),
            second: Box::new(build_pane_tree(second)?),
        }),
    }
}
