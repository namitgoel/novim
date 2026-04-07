//! Simple background task runner for non-blocking operations.
//!
//! Spawn a closure on a background thread, poll for results from the main loop.
//! Used for git operations, shell commands, plugin installs, etc.

use std::sync::mpsc;

/// A result from a background task, ready to be applied on the main thread.
pub enum TaskResult {
    /// Set a status message.
    StatusMessage(String),
    /// Set blame data on the focused buffer.
    BlameData(std::collections::HashMap<usize, crate::buffer::BlameInfo>),
    /// Quickfix entries from :make.
    QuickfixEntries(Vec<crate::editor::QuickfixEntry>),
    /// Git branch name detected in background.
    GitBranch(Option<String>),
}

/// Manages background tasks with a channel for results.
pub struct TaskRunner {
    receiver: mpsc::Receiver<TaskResult>,
    sender: mpsc::Sender<TaskResult>,
}

impl TaskRunner {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { receiver, sender }
    }

    /// Spawn a blocking operation on a background thread.
    pub fn spawn<F>(&self, f: F)
    where
        F: FnOnce() -> Vec<TaskResult> + Send + 'static,
    {
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let results = f();
            for result in results {
                let _ = sender.send(result);
            }
        });
    }

    /// Poll for completed task results (non-blocking). Returns all available results.
    pub fn poll(&self) -> Vec<TaskResult> {
        let mut results = Vec::new();
        while let Ok(result) = self.receiver.try_recv() {
            results.push(result);
        }
        results
    }
}

impl Default for TaskRunner {
    fn default() -> Self {
        Self::new()
    }
}
