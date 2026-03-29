//! Pane management using a binary space partitioning (BSP) tree.
//!
//! Each pane is a leaf containing either a text Buffer or a TerminalPane.
//! Splits create internal nodes with two children.

use novim_types::{Direction, Rect};
use std::io;

use crate::buffer::{Buffer, BufferLike};
use crate::emulator::TerminalPane;

pub type PaneId = usize;

/// Split direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// What a pane contains — either a text editor or a terminal.
pub enum PaneContent {
    Editor(Buffer),
    Terminal(TerminalPane),
}

impl PaneContent {
    /// Access the content through the BufferLike trait.
    pub fn as_buffer_like(&self) -> &dyn BufferLike {
        match self {
            PaneContent::Editor(buf) => buf,
            PaneContent::Terminal(term) => term,
        }
    }

    pub fn as_buffer_like_mut(&mut self) -> &mut dyn BufferLike {
        match self {
            PaneContent::Editor(buf) => buf,
            PaneContent::Terminal(term) => term,
        }
    }
}

/// A single pane (leaf node in the BSP tree).
pub struct Pane {
    pub id: PaneId,
    pub content: PaneContent,
    pub viewport_offset: usize,
}

impl Pane {
    pub fn new_editor(id: PaneId, buffer: Buffer) -> Self {
        Self {
            id,
            content: PaneContent::Editor(buffer),
            viewport_offset: 0,
        }
    }

    pub fn new_terminal(id: PaneId, term: TerminalPane) -> Self {
        Self {
            id,
            content: PaneContent::Terminal(term),
            viewport_offset: 0,
        }
    }
}

/// BSP tree node
pub enum PaneNode {
    Leaf(Pane),
    Split {
        direction: SplitDirection,
        ratio: f64,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

impl PaneNode {
    fn get_pane(&self, id: PaneId) -> Option<&Pane> {
        match self {
            PaneNode::Leaf(pane) if pane.id == id => Some(pane),
            PaneNode::Leaf(_) => None,
            PaneNode::Split { first, second, .. } => {
                first.get_pane(id).or_else(|| second.get_pane(id))
            }
        }
    }

    fn get_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        match self {
            PaneNode::Leaf(pane) if pane.id == id => Some(pane),
            PaneNode::Leaf(_) => None,
            PaneNode::Split { first, second, .. } => {
                first.get_pane_mut(id).or_else(|| second.get_pane_mut(id))
            }
        }
    }

    fn layout(&self, area: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            PaneNode::Leaf(pane) => out.push((pane.id, area)),
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (a, b) = split_rect(area, *direction, *ratio);
                first.layout(a, out);
                second.layout(b, out);
            }
        }
    }

    fn split_pane(
        &mut self,
        target_id: PaneId,
        direction: SplitDirection,
        new_pane: Pane,
    ) -> Result<(), Pane> {
        match self {
            PaneNode::Leaf(pane) if pane.id == target_id => {
                let old = std::mem::replace(
                    self,
                    PaneNode::Leaf(Pane::new_editor(0, Buffer::new())),
                );
                *self = PaneNode::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(old),
                    second: Box::new(PaneNode::Leaf(new_pane)),
                };
                Ok(())
            }
            PaneNode::Leaf(_) => Err(new_pane),
            PaneNode::Split { first, second, .. } => {
                let new_pane = match first.split_pane(target_id, direction, new_pane) {
                    Ok(()) => return Ok(()),
                    Err(pane) => pane, // Not found in first, try second
                };
                second.split_pane(target_id, direction, new_pane)
            }
        }
    }

    fn remove_pane(self, target_id: PaneId) -> RemoveResult {
        match self {
            PaneNode::Leaf(pane) if pane.id == target_id => RemoveResult::Removed,
            PaneNode::Leaf(pane) => RemoveResult::NotFound(PaneNode::Leaf(pane)),
            PaneNode::Split {
                first,
                second,
                direction,
                ratio,
            } => match first.remove_pane(target_id) {
                RemoveResult::Removed => RemoveResult::Promoted(*second),
                RemoveResult::Promoted(r) => RemoveResult::Promoted(PaneNode::Split {
                    direction,
                    ratio,
                    first: Box::new(r),
                    second,
                }),
                RemoveResult::NotFound(f) => match second.remove_pane(target_id) {
                    RemoveResult::Removed => RemoveResult::Promoted(f),
                    RemoveResult::Promoted(r) => RemoveResult::Promoted(PaneNode::Split {
                        direction,
                        ratio,
                        first: Box::new(f),
                        second: Box::new(r),
                    }),
                    RemoveResult::NotFound(s) => RemoveResult::NotFound(PaneNode::Split {
                        direction,
                        ratio,
                        first: Box::new(f),
                        second: Box::new(s),
                    }),
                },
            },
        }
    }

    fn first_pane_id(&self) -> PaneId {
        match self {
            PaneNode::Leaf(pane) => pane.id,
            PaneNode::Split { first, .. } => first.first_pane_id(),
        }
    }

    fn collect_ids(&self, out: &mut Vec<PaneId>) {
        match self {
            PaneNode::Leaf(pane) => out.push(pane.id),
            PaneNode::Split { first, second, .. } => {
                first.collect_ids(out);
                second.collect_ids(out);
            }
        }
    }

    /// Visit the tree to build a serializable layout.
    fn visit(
        &self,
        visitor: &mut dyn FnMut(&Pane) -> crate::session::PaneState,
    ) -> crate::session::PaneLayout {
        match self {
            PaneNode::Leaf(pane) => crate::session::PaneLayout::Leaf(visitor(pane)),
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => crate::session::PaneLayout::Split {
                direction: (*direction).into(),
                ratio: *ratio,
                first: Box::new(first.visit(visitor)),
                second: Box::new(second.visit(visitor)),
            },
        }
    }

    /// Poll all terminal panes for PTY output.
    fn poll_terminals(&mut self) {
        match self {
            PaneNode::Leaf(pane) => {
                pane.content.as_buffer_like_mut().poll_pty();
            }
            PaneNode::Split { first, second, .. } => {
                first.poll_terminals();
                second.poll_terminals();
            }
        }
    }
}

enum RemoveResult {
    Removed,
    Promoted(PaneNode),
    NotFound(PaneNode),
}

/// Manages the pane tree and tracks focus.
pub struct PaneManager {
    root: PaneNode,
    focused_id: PaneId,
    next_id: PaneId,
    count: usize,
}

impl PaneManager {
    /// Create with a single editor pane.
    pub fn new(buffer: Buffer) -> Self {
        Self {
            root: PaneNode::Leaf(Pane::new_editor(0, buffer)),
            focused_id: 0,
            next_id: 1,
            count: 1,
        }
    }

    /// Create from a pre-built pane tree (for session restore).
    pub fn from_tree(root: PaneNode, focused_id: PaneId) -> Self {
        let mut ids = Vec::new();
        root.collect_ids(&mut ids);
        let count = ids.len();
        let max_id = ids.iter().copied().max().unwrap_or(0);
        // Validate focused_id exists, fall back to first pane
        let focused = if ids.contains(&focused_id) {
            focused_id
        } else {
            root.first_pane_id()
        };
        Self {
            root,
            focused_id: focused,
            next_id: max_id + 1,
            count,
        }
    }

    /// Create with a single terminal pane.
    pub fn new_terminal(rows: u16, cols: u16) -> io::Result<Self> {
        let term = TerminalPane::new(rows, cols)?;
        Ok(Self {
            root: PaneNode::Leaf(Pane::new_terminal(0, term)),
            focused_id: 0,
            next_id: 1,
            count: 1,
        })
    }

    pub fn focused_pane(&self) -> &Pane {
        self.root
            .get_pane(self.focused_id)
            .expect("focused pane must exist")
    }

    pub fn focused_pane_mut(&mut self) -> &mut Pane {
        self.root
            .get_pane_mut(self.focused_id)
            .expect("focused pane must exist")
    }

    pub fn focused_id(&self) -> PaneId {
        self.focused_id
    }

    pub fn get_pane(&self, id: PaneId) -> Option<&Pane> {
        self.root.get_pane(id)
    }

    pub fn get_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.root.get_pane_mut(id)
    }

    pub fn layout(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        let mut result = Vec::new();
        self.root.layout(area, &mut result);
        result
    }

    /// Split focused pane, new pane gets an empty editor buffer.
    pub fn split(&mut self, direction: SplitDirection) {
        let new_id = self.next_id;
        self.next_id += 1;
        let new_pane = Pane::new_editor(new_id, Buffer::new());
        if self.root.split_pane(self.focused_id, direction, new_pane).is_ok() {
            self.count += 1;
        }
    }

    /// Split focused pane, new pane gets a terminal.
    pub fn split_terminal(
        &mut self,
        direction: SplitDirection,
        rows: u16,
        cols: u16,
    ) -> io::Result<()> {
        let new_id = self.next_id;
        self.next_id += 1;
        let term = TerminalPane::new(rows, cols)?;
        let new_pane = Pane::new_terminal(new_id, term);
        if self.root.split_pane(self.focused_id, direction, new_pane).is_ok() {
            self.count += 1;
        }
        self.focused_id = new_id;
        Ok(())
    }

    pub fn split_horizontal(&mut self) {
        self.split(SplitDirection::Horizontal);
    }

    pub fn split_vertical(&mut self) {
        self.split(SplitDirection::Vertical);
    }

    /// Close the focused pane. Returns false if it's the last pane.
    pub fn close_focused(&mut self) -> bool {
        if self.count <= 1 {
            return false;
        }

        // Take root out temporarily (placeholder is cheap — Rope::new() is zero-alloc)
        let root = std::mem::replace(
            &mut self.root,
            PaneNode::Leaf(Pane::new_editor(0, Buffer::new())),
        );

        match root.remove_pane(self.focused_id) {
            RemoveResult::Promoted(new_root) => {
                self.root = new_root;
                self.focused_id = self.root.first_pane_id();
                self.count -= 1;
                true
            }
            RemoveResult::NotFound(original) => {
                self.root = original;
                false
            }
            RemoveResult::Removed => false,
        }
    }

    pub fn focus_direction(&mut self, direction: Direction, screen_area: Rect) {
        let layouts = self.layout(screen_area);
        let current_rect = layouts
            .iter()
            .find(|(id, _)| *id == self.focused_id)
            .map(|(_, r)| *r);

        let Some(current) = current_rect else {
            return;
        };

        let candidate = layouts
            .iter()
            .filter(|(id, _)| *id != self.focused_id)
            .filter(|(_, rect)| match direction {
                Direction::Left => rect.x + rect.width <= current.x,
                Direction::Right => rect.x >= current.x + current.width,
                Direction::Up => rect.y + rect.height <= current.y,
                Direction::Down => rect.y >= current.y + current.height,
            })
            .min_by_key(|(_, rect)| match direction {
                Direction::Left => current.x.saturating_sub(rect.x + rect.width),
                Direction::Right => rect.x.saturating_sub(current.x + current.width),
                Direction::Up => current.y.saturating_sub(rect.y + rect.height),
                Direction::Down => rect.y.saturating_sub(current.y + current.height),
            });

        if let Some((id, _)) = candidate {
            self.focused_id = *id;
        }
    }

    pub fn focus_next(&mut self) {
        let mut ids = Vec::new();
        self.root.collect_ids(&mut ids);
        if let Some(pos) = ids.iter().position(|id| *id == self.focused_id) {
            self.focused_id = ids[(pos + 1) % ids.len()];
        }
    }

    pub fn pane_count(&self) -> usize {
        self.count
    }

    /// Poll all terminal panes for new PTY output.
    pub fn poll_terminals(&mut self) {
        self.root.poll_terminals();
    }

    /// Visit the pane tree, calling the visitor on each leaf to build a layout.
    pub fn visit_tree(
        &self,
        visitor: &mut dyn FnMut(&Pane) -> crate::session::PaneState,
    ) -> crate::session::PaneLayout {
        self.root.visit(visitor)
    }

    /// Get the last pane (most recently added) mutably.
    pub fn last_pane_mut(&mut self) -> Option<&mut Pane> {
        let mut ids = Vec::new();
        self.root.collect_ids(&mut ids);
        if let Some(&last_id) = ids.last() {
            self.root.get_pane_mut(last_id)
        } else {
            None
        }
    }

    /// Try to set focus to a specific pane ID. No-op if ID doesn't exist.
    pub fn try_set_focus(&mut self, id: PaneId) {
        let mut ids = Vec::new();
        self.root.collect_ids(&mut ids);
        if ids.contains(&id) {
            self.focused_id = id;
        }
    }
}

fn split_rect(area: Rect, direction: SplitDirection, ratio: f64) -> (Rect, Rect) {
    match direction {
        SplitDirection::Horizontal => {
            let h1 = (area.height as f64 * ratio) as u16;
            let h2 = area.height.saturating_sub(h1);
            (
                Rect::new(area.x, area.y, area.width, h1),
                Rect::new(area.x, area.y + h1, area.width, h2),
            )
        }
        SplitDirection::Vertical => {
            let w1 = (area.width as f64 * ratio) as u16;
            let w2 = area.width.saturating_sub(w1);
            (
                Rect::new(area.x, area.y, w1, area.height),
                Rect::new(area.x + w1, area.y, w2, area.height),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_pane() {
        let mgr = PaneManager::new(Buffer::new());
        assert_eq!(mgr.pane_count(), 1);
        assert_eq!(mgr.focused_id(), 0);
    }

    #[test]
    fn test_vertical_split() {
        let mut mgr = PaneManager::new(Buffer::new());
        mgr.split_vertical();
        assert_eq!(mgr.pane_count(), 2);
        assert_eq!(mgr.focused_id(), 0);
    }

    #[test]
    fn test_horizontal_split() {
        let mut mgr = PaneManager::new(Buffer::new());
        mgr.split_horizontal();
        assert_eq!(mgr.pane_count(), 2);
    }

    #[test]
    fn test_close_pane() {
        let mut mgr = PaneManager::new(Buffer::new());
        mgr.split_vertical();
        mgr.focus_next();
        assert!(mgr.close_focused());
        assert_eq!(mgr.pane_count(), 1);
    }

    #[test]
    fn test_cannot_close_last_pane() {
        let mut mgr = PaneManager::new(Buffer::new());
        assert!(!mgr.close_focused());
    }

    #[test]
    fn test_focus_next_cycles() {
        let mut mgr = PaneManager::new(Buffer::new());
        mgr.split_vertical();
        assert_eq!(mgr.focused_id(), 0);
        mgr.focus_next();
        assert_eq!(mgr.focused_id(), 1);
        mgr.focus_next();
        assert_eq!(mgr.focused_id(), 0);
    }

    #[test]
    fn test_layout_vertical_split() {
        let mut mgr = PaneManager::new(Buffer::new());
        mgr.split_vertical();
        let area = Rect::new(0, 0, 80, 24);
        let layouts = mgr.layout(area);
        assert_eq!(layouts.len(), 2);
        assert_eq!(layouts[0].1.width, 40);
        assert_eq!(layouts[1].1.width, 40);
    }
}
