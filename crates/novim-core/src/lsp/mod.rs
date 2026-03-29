//! LSP client — spawns a language server, communicates over JSON-RPC.
//!
//! Uses the same async pattern as the terminal emulator:
//! background reader thread + mpsc channel + main loop polling.

pub mod provider;
mod transport;

use std::collections::{HashMap, HashSet};
use std::io::{self, BufReader, BufWriter};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use serde_json::json;

use provider::ResolvedServer;

/// Events received from the language server.
#[derive(Debug)]
pub enum LspEvent {
    /// Server finished initializing
    Initialized,
    /// Diagnostics for a file
    Diagnostics {
        uri: String,
        diagnostics: Vec<Diagnostic>,
    },
    /// Response to go-to-definition request
    GotoDefinitionResponse {
        uri: String,
        line: u32,
        col: u32,
    },
    /// Completion items response
    CompletionResponse {
        items: Vec<CompletionItem>,
    },
    /// Server sent an error
    ServerError(String),
    /// Hover info response (type signature, docs)
    HoverResponse {
        contents: String,
    },
    /// Server process exited
    ServerExited,
    /// Progress notification (server is indexing, loading, etc.)
    Progress { message: String },
}

/// A simplified completion item.
#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: Option<String>,
    pub kind: CompletionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Function,
    Variable,
    Field,
    Type,
    Keyword,
    Module,
    Property,
    Other,
}

/// A simplified diagnostic (from LSP publishDiagnostics).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub line: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// LSP client that communicates with a language server process.
pub struct LspClient {
    _child: Child,
    writer: BufWriter<std::process::ChildStdin>,
    receiver: Receiver<LspEvent>,
    _reader_handle: thread::JoinHandle<()>,
    next_request_id: i64,
    pending_requests: HashMap<i64, String>,
    initialized: bool,
    pub language_id: String,
    opened_uris: HashSet<String>,
    queued_opens: Vec<(String, String, i32, String)>, // (uri, lang_id, version, text)
}

impl LspClient {
    /// Spawn a language server and start the background reader.
    pub fn spawn(server: &ResolvedServer, root_path: &Path) -> io::Result<Self> {
        let mut child = Command::new(&server.command)
            .args(&server.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Failed to spawn '{}': {}", server.command, e),
                )
            })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "Failed to get server stdin")
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "Failed to get server stdout")
        })?;

        let writer = BufWriter::new(stdin);
        let (tx, rx): (Sender<LspEvent>, Receiver<LspEvent>) = mpsc::channel();

        // Background reader thread
        let handle = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match transport::read_message(&mut reader) {
                    Some(msg) => {
                        let events = parse_server_message(&msg);
                        for event in events {
                            if tx.send(event).is_err() {
                                return;
                            }
                        }
                    }
                    None => {
                        let _ = tx.send(LspEvent::ServerExited);
                        return;
                    }
                }
            }
        });

        let mut client = Self {
            _child: child,
            writer,
            receiver: rx,
            _reader_handle: handle,
            next_request_id: 1,
            pending_requests: HashMap::new(),
            initialized: false,
            language_id: server.language_id.clone(),
            opened_uris: HashSet::new(),
            queued_opens: Vec::new(),
        };

        // Send initialize request
        let root_uri = format!("file://{}", root_path.display());
        client.send_initialize(&root_uri)?;

        Ok(client)
    }

    /// Poll for events from the server (non-blocking).
    pub fn poll(&mut self) -> Vec<LspEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.receiver.try_recv() {
            match &event {
                LspEvent::Initialized => {
                    self.initialized = true;
                    // Flush queued didOpen notifications
                    let queued = std::mem::take(&mut self.queued_opens);
                    for (uri, lang_id, version, text) in queued {
                        let _ = self.did_open(&uri, &lang_id, version, &text);
                    }
                }
                _ => {}
            }
            events.push(event);
        }
        events
    }

    /// Notify the server that a file was opened.
    pub fn did_open(&mut self, uri: &str, language_id: &str, version: i32, text: &str) -> io::Result<()> {
        if !self.initialized {
            self.queued_opens.push((
                uri.to_string(),
                language_id.to_string(),
                version,
                text.to_string(),
            ));
            return Ok(());
        }

        if self.opened_uris.contains(uri) {
            return Ok(());
        }

        let msg = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": version,
                    "text": text
                }
            }
        });

        transport::write_message(&mut self.writer, &msg)?;
        self.opened_uris.insert(uri.to_string());
        Ok(())
    }

    /// Notify the server of document changes (full sync).
    pub fn did_change(&mut self, uri: &str, version: i32, text: &str) -> io::Result<()> {
        if !self.initialized || !self.opened_uris.contains(uri) {
            return Ok(());
        }

        let msg = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "version": version
                },
                "contentChanges": [{ "text": text }]
            }
        });

        transport::write_message(&mut self.writer, &msg)
    }

    /// Notify the server that a file was saved.
    pub fn did_save(&mut self, uri: &str) -> io::Result<()> {
        if !self.initialized || !self.opened_uris.contains(uri) {
            return Ok(());
        }

        let msg = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didSave",
            "params": {
                "textDocument": { "uri": uri }
            }
        });

        transport::write_message(&mut self.writer, &msg)
    }

    /// Request go-to-definition at the given position.
    pub fn goto_definition(&mut self, uri: &str, line: u32, character: u32) -> io::Result<()> {
        if !self.initialized {
            return Ok(());
        }

        let id = self.next_request_id;
        self.next_request_id += 1;

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        });

        self.pending_requests.insert(id, "textDocument/definition".to_string());
        transport::write_message(&mut self.writer, &msg)
    }

    /// Request hover info at the given position.
    pub fn hover(&mut self, uri: &str, line: u32, character: u32) -> io::Result<()> {
        if !self.initialized {
            return Ok(());
        }

        let id = self.next_request_id;
        self.next_request_id += 1;

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        });

        self.pending_requests.insert(id, "textDocument/hover".to_string());
        transport::write_message(&mut self.writer, &msg)
    }

    /// Request completion at the given position.
    pub fn completion(&mut self, uri: &str, line: u32, character: u32) -> io::Result<()> {
        if !self.initialized {
            return Ok(());
        }

        let id = self.next_request_id;
        self.next_request_id += 1;

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        });

        self.pending_requests.insert(id, "textDocument/completion".to_string());
        transport::write_message(&mut self.writer, &msg)
    }

    /// Send the LSP initialize request.
    fn send_initialize(&mut self, root_uri: &str) -> io::Result<()> {
        let id = self.next_request_id;
        self.next_request_id += 1;

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "processId": std::process::id(),
                "rootUri": root_uri,
                "capabilities": {
                    "textDocument": {
                        "synchronization": {
                            "didSave": true
                        },
                        "definition": {
                            "dynamicRegistration": false
                        },
                        "hover": {
                            "contentFormat": ["plaintext"]
                        },
                        "completion": {
                            "completionItem": {
                                "snippetSupport": false
                            }
                        },
                        "publishDiagnostics": {
                            "relatedInformation": false
                        }
                    }
                }
            }
        });

        self.pending_requests.insert(id, "initialize".to_string());
        transport::write_message(&mut self.writer, &msg)
    }

}

impl Drop for LspClient {
    fn drop(&mut self) {
        // Try graceful shutdown
        let id = self.next_request_id;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "shutdown",
            "params": null
        });
        let _ = transport::write_message(&mut self.writer, &msg);

        let exit_msg = json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        });
        let _ = transport::write_message(&mut self.writer, &exit_msg);
    }
}

/// Parse a server message into LspEvents.
fn parse_server_message(msg: &serde_json::Value) -> Vec<LspEvent> {
    let mut events = Vec::new();

    // Check if it's a response (has "id" and "result")
    if let Some(_id) = msg.get("id").and_then(|v| v.as_i64()) {
        if let Some(result) = msg.get("result") {
            // Initialize response (has capabilities)
            if result.get("capabilities").is_some() {
                events.push(LspEvent::Initialized);
            }
            // Hover response
            else if let Some(hover) = parse_hover_response(result) {
                events.push(hover);
            }
            // Completion response (array of items or CompletionList)
            else if let Some(completions) = parse_completion_response(result) {
                events.push(completions);
            }
            // Definition response
            else if let Some(def) = extract_definition_location(result) {
                events.push(def);
            }
        }
        if let Some(error) = msg.get("error") {
            let message = error.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown LSP error")
                .to_string();
            events.push(LspEvent::ServerError(message));
        }
        return events;
    }

    // Check if it's a notification (has "method" but no "id")
    if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
        match method {
            "textDocument/publishDiagnostics" => {
                if let Some(params) = msg.get("params") {
                    if let Some(event) = parse_diagnostics(params) {
                        events.push(event);
                    }
                }
            }
            "$/progress" => {
                if let Some(params) = msg.get("params") {
                    if let Some(value) = params.get("value") {
                        let kind = value.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                        let title = value.get("title").and_then(|t| t.as_str()).unwrap_or("");
                        let message = value.get("message").and_then(|m| m.as_str()).unwrap_or("");

                        let status = match kind {
                            "begin" => format!("{}: {}", title, message).trim().to_string(),
                            "report" => {
                                if let Some(pct) = value.get("percentage").and_then(|p| p.as_u64()) {
                                    format!("{} {}%", message, pct)
                                } else {
                                    message.to_string()
                                }
                            }
                            "end" => "Ready".to_string(),
                            _ => String::new(),
                        };

                        if !status.is_empty() {
                            events.push(LspEvent::Progress { message: status });
                        }
                    }
                }
            }
            "window/logMessage" | "window/showMessage" => {
                if let Some(params) = msg.get("params") {
                    if let Some(msg_text) = params.get("message").and_then(|m| m.as_str()) {
                        // Only show important messages (type 1=error, 2=warning)
                        let msg_type = params.get("type").and_then(|t| t.as_u64()).unwrap_or(4);
                        if msg_type <= 2 {
                            events.push(LspEvent::Progress { message: msg_text.to_string() });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    events
}

fn extract_definition_location(result: &serde_json::Value) -> Option<LspEvent> {
    // Definition can be a single Location, array of Locations, or array of LocationLinks
    let location = if result.is_array() {
        result.get(0)?
    } else if result.is_object() {
        result
    } else {
        return None;
    };

    // Handle LocationLink (has targetUri)
    let uri = location.get("targetUri")
        .or_else(|| location.get("uri"))
        .and_then(|u| u.as_str())?
        .to_string();

    let range = location.get("targetRange")
        .or_else(|| location.get("range"))?;
    let start = range.get("start")?;
    let line = start.get("line")?.as_u64()? as u32;
    let col = start.get("character")?.as_u64()? as u32;

    Some(LspEvent::GotoDefinitionResponse { uri, line, col })
}

fn parse_diagnostics(params: &serde_json::Value) -> Option<LspEvent> {
    let uri = params.get("uri")?.as_str()?.to_string();
    let diag_array = params.get("diagnostics")?.as_array()?;

    let diagnostics: Vec<Diagnostic> = diag_array
        .iter()
        .filter_map(|d| {
            let range = d.get("range")?;
            let start = range.get("start")?;
            let end = range.get("end")?;

            let severity = match d.get("severity")?.as_u64()? {
                1 => DiagnosticSeverity::Error,
                2 => DiagnosticSeverity::Warning,
                3 => DiagnosticSeverity::Info,
                _ => DiagnosticSeverity::Hint,
            };

            let message = d.get("message")?.as_str()?.to_string();

            Some(Diagnostic {
                line: start.get("line")?.as_u64()? as usize,
                col_start: start.get("character")?.as_u64()? as usize,
                col_end: end.get("character")?.as_u64()? as usize,
                severity,
                message,
            })
        })
        .collect();

    Some(LspEvent::Diagnostics { uri, diagnostics })
}

fn parse_hover_response(result: &serde_json::Value) -> Option<LspEvent> {
    // Hover result has "contents" which can be:
    // - string
    // - { kind, value } (MarkupContent)
    // - [MarkedString, ...]
    let contents = result.get("contents")?;

    let raw_text = if let Some(s) = contents.as_str() {
        s.to_string()
    } else if let Some(value) = contents.get("value").and_then(|v| v.as_str()) {
        value.to_string()
    } else if let Some(arr) = contents.as_array() {
        arr.iter()
            .filter_map(|item| {
                if let Some(s) = item.as_str() {
                    Some(s.to_string())
                } else {
                    item.get("value").and_then(|v| v.as_str()).map(|s| s.to_string())
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        return None;
    };

    if raw_text.is_empty() {
        return None;
    }

    // Strip markdown code fences and clean up
    let text = raw_text
        .lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    if text.is_empty() {
        return None;
    }

    Some(LspEvent::HoverResponse { contents: text })
}

fn parse_completion_response(result: &serde_json::Value) -> Option<LspEvent> {
    // Completion can be: array of CompletionItems, or CompletionList { items: [...] }
    let items_array = if result.is_array() {
        result.as_array()?
    } else if let Some(items) = result.get("items").and_then(|v| v.as_array()) {
        items
    } else {
        return None;
    };

    // Must have at least one item that looks like a completion
    if items_array.is_empty() || items_array.first().and_then(|i| i.get("label")).is_none() {
        return None;
    }

    let items: Vec<CompletionItem> = items_array
        .iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_string();
            let detail = item.get("detail").and_then(|d| d.as_str()).map(|s| s.to_string());
            let insert_text = item.get("insertText")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());
            let kind = match item.get("kind").and_then(|k| k.as_u64()) {
                Some(3) => CompletionKind::Function,
                Some(6) => CompletionKind::Variable,
                Some(5) => CompletionKind::Field,
                Some(7) | Some(22) | Some(23) => CompletionKind::Type,
                Some(14) => CompletionKind::Keyword,
                Some(9) => CompletionKind::Module,
                Some(10) => CompletionKind::Property,
                _ => CompletionKind::Other,
            };

            Some(CompletionItem { label, detail, insert_text, kind })
        })
        .take(20) // Limit to 20 items for display
        .collect();

    if items.is_empty() {
        None
    } else {
        Some(LspEvent::CompletionResponse { items })
    }
}
