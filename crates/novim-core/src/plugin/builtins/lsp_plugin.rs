//! Built-in LSP plugin — manages language server connections.
//!
//! Replaces the hardwired LSP code in Workspace/EditorState.
//! Starts servers on BufOpen, sends changes on TextChanged,
//! polls for responses in poll_timers().

use std::collections::HashMap;
use std::sync::Arc;

use std::path::PathBuf;

use crate::lsp::{LspClient, LspEvent};
use crate::lsp::provider::LspRegistry;
use crate::plugin::{EditorEvent, Plugin, PluginAction, PluginContext};

pub struct LspPlugin {
    registry: Arc<LspRegistry>,
    clients: HashMap<String, LspClient>, // keyed by language_id
}

impl LspPlugin {
    pub fn new(registry: Arc<LspRegistry>) -> Self {
        Self {
            registry,
            clients: HashMap::new(),
        }
    }

    /// Ensure a client exists for the given file extension. Returns the language_id if started.
    fn ensure_client(&mut self, ext: &str, uri: &str, text: &str, version: i32) -> Option<String> {
        let server = self.registry.resolve(ext)?;
        let lang_id = server.language_id.clone();

        if !self.clients.contains_key(&lang_id) {
            let root = std::env::current_dir().unwrap_or_default();
            match LspClient::spawn(&server, &root) {
                Ok(client) => {
                    self.clients.insert(lang_id.clone(), client);
                }
                Err(e) => {
                    log::error!("Failed to spawn LSP for {}: {}", lang_id, e);
                    return None;
                }
            }
        }

        if let Some(client) = self.clients.get_mut(&lang_id) {
            let _ = client.did_open(uri, &lang_id, version, text);
        }

        Some(lang_id)
    }

    fn client_for_ext(&mut self, ext: &str) -> Option<&mut LspClient> {
        let server = self.registry.resolve(ext)?;
        self.clients.get_mut(&server.language_id)
    }

    fn collect_events(&mut self) -> Vec<PluginAction> {
        let mut actions = Vec::new();

        for (_lang, client) in &mut self.clients {
            let events = client.poll();
            for event in events {
                match event {
                    LspEvent::Diagnostics { uri, diagnostics } => {
                        actions.push(PluginAction::SetDiagnostics { uri, diagnostics });
                    }
                    LspEvent::HoverResponse { contents } => {
                        actions.push(PluginAction::ShowHoverText { text: contents });
                    }
                    LspEvent::CompletionResponse { items } => {
                        actions.push(PluginAction::ShowCompletions { items });
                    }
                    LspEvent::GotoDefinitionResponse { uri, line, col } => {
                        // Convert file:// URI to path
                        let file = if let Some(path) = uri.strip_prefix("file://") {
                            path.to_string()
                        } else {
                            uri
                        };
                        actions.push(PluginAction::GotoLocation { file, line, col });
                    }
                    LspEvent::Progress { message } => {
                        actions.push(PluginAction::SetLspStatus {
                            lang: String::new(),
                            message: Some(message),
                        });
                    }
                    LspEvent::ServerError(msg) => {
                        actions.push(PluginAction::SetStatus(format!("LSP error: {}", msg)));
                    }
                    LspEvent::ServerExited => {
                        actions.push(PluginAction::SetStatus("LSP server exited".to_string()));
                    }
                    LspEvent::Initialized => {
                        // Already handled internally by LspClient
                    }
                }
            }
        }

        actions
    }
}

impl Plugin for LspPlugin {
    fn id(&self) -> &str { "lsp" }
    fn name(&self) -> &str { "Language Server Protocol" }
    fn is_builtin(&self) -> bool { true }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }

    fn init(&mut self, _ctx: &mut PluginContext) {}

    fn on_event(&mut self, event: &EditorEvent, ctx: &PluginContext) -> Vec<PluginAction> {
        match event {
            EditorEvent::BufOpen { path: _ } => {
                let full_path = match &ctx.buf.path {
                    Some(p) => p.clone(),
                    None => return vec![],
                };
                let ext = std::path::Path::new(&full_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                if ext.is_empty() { return vec![]; }

                let uri = path_to_uri(&full_path);
                let text = ctx.buf.text.clone().unwrap_or_default();
                let version = ctx.buf.version.unwrap_or(0);

                if let Some(lang) = self.ensure_client(ext, &uri, &text, version) {
                    vec![PluginAction::SetLspStatus {
                        lang: lang.clone(),
                        message: Some(format!("LSP:{}", lang)),
                    }]
                } else {
                    vec![]
                }
            }
            EditorEvent::TextChanged { .. } => {
                // Send didChange to the appropriate client
                if let Some(path) = &ctx.buf.path {
                    let ext = std::path::Path::new(path)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    let uri = path_to_uri(path);
                    let text = ctx.buf.text.clone().unwrap_or_default();
                    let version = ctx.buf.version.unwrap_or(0);

                    if let Some(client) = self.client_for_ext(ext) {
                        let _ = client.did_change(&uri, version, &text);
                    }
                }
                vec![]
            }
            EditorEvent::BufWrite { path: _ } => {
                if let Some(p) = &ctx.buf.path {
                    let ext = std::path::Path::new(p)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    let uri = path_to_uri(p);
                    if let Some(client) = self.client_for_ext(ext) {
                        let _ = client.did_save(&uri);
                    }
                }
                vec![]
            }
            _ => vec![],
        }
    }

    fn poll_timers(&mut self) -> Vec<PluginAction> {
        self.collect_events()
    }
}

// ── LSP command dispatch (called from EditorState) ──

impl LspPlugin {
    /// Request hover info for a file at a position.
    pub fn request_hover(&mut self, ext: &str, uri: &str, line: usize, col: usize) {
        if let Some(client) = self.client_for_ext(ext) {
            let _ = client.hover(uri, line as u32, col as u32);
        }
    }

    /// Request goto-definition.
    pub fn request_goto_definition(&mut self, ext: &str, uri: &str, line: usize, col: usize) {
        if let Some(client) = self.client_for_ext(ext) {
            let _ = client.goto_definition(uri, line as u32, col as u32);
        }
    }

    /// Request completions.
    pub fn request_completion(&mut self, ext: &str, uri: &str, line: usize, col: usize) {
        if let Some(client) = self.client_for_ext(ext) {
            let _ = client.completion(uri, line as u32, col as u32);
        }
    }

    /// Get active language IDs (for status bar display).
    pub fn active_languages(&self) -> Vec<&str> {
        self.clients.keys().map(|s| s.as_str()).collect()
    }
}

/// Build a proper file:// URI from a path, canonicalizing if possible.
fn path_to_uri(path: &str) -> String {
    let p = PathBuf::from(path);
    let abs = p.canonicalize().unwrap_or_else(|_| {
        if p.is_absolute() { p } else {
            std::env::current_dir().unwrap_or_default().join(p)
        }
    });
    format!("file://{}", abs.display())
}
