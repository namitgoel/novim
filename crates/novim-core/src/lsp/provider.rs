//! LSP server provider trait and registry.
//!
//! Extensible design: implement LspServerProvider for new languages,
//! or add servers via config.toml. Config takes priority over built-in.

use std::collections::HashMap;

use crate::config::NovimConfig;

/// Trait for providing LSP server configuration.
/// Implement this to add support for a new language.
pub trait LspServerProvider: Send + Sync {
    /// LSP language identifier (e.g., "rust", "typescript")
    fn language_id(&self) -> &str;
    /// File extensions this server handles
    fn file_extensions(&self) -> &[&str];
    /// Server binary command
    fn server_command(&self) -> &str;
    /// Server command arguments
    fn server_args(&self) -> Vec<String>;
    /// Files that indicate the project root (e.g., "Cargo.toml")
    fn root_markers(&self) -> &[&str];
    /// Custom initialization options (sent during LSP initialize)
    fn initialization_options(&self) -> Option<serde_json::Value> {
        None
    }
}

/// Resolved server configuration (from either trait or config).
#[derive(Debug, Clone)]
pub struct ResolvedServer {
    pub language_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub root_markers: Vec<String>,
}

// --- Built-in providers ---

pub struct RustAnalyzerProvider;
impl LspServerProvider for RustAnalyzerProvider {
    fn language_id(&self) -> &str { "rust" }
    fn file_extensions(&self) -> &[&str] { &["rs"] }
    fn server_command(&self) -> &str { "rust-analyzer" }
    fn server_args(&self) -> Vec<String> { vec![] }
    fn root_markers(&self) -> &[&str] { &["Cargo.toml"] }
}

pub struct TsServerProvider;
impl LspServerProvider for TsServerProvider {
    fn language_id(&self) -> &str { "typescript" }
    fn file_extensions(&self) -> &[&str] { &["ts", "tsx", "js", "jsx", "mjs", "cjs"] }
    fn server_command(&self) -> &str { "typescript-language-server" }
    fn server_args(&self) -> Vec<String> { vec!["--stdio".to_string()] }
    fn root_markers(&self) -> &[&str] { &["tsconfig.json", "package.json"] }
}

pub struct PyrightProvider;
impl LspServerProvider for PyrightProvider {
    fn language_id(&self) -> &str { "python" }
    fn file_extensions(&self) -> &[&str] { &["py", "pyw"] }
    fn server_command(&self) -> &str { "pyright-langserver" }
    fn server_args(&self) -> Vec<String> { vec!["--stdio".to_string()] }
    fn root_markers(&self) -> &[&str] { &["pyproject.toml", "setup.py", "requirements.txt"] }
}

/// Registry that resolves file extensions to server configurations.
/// Config overrides take priority over built-in providers.
pub struct LspRegistry {
    providers: Vec<Box<dyn LspServerProvider>>,
    config_servers: HashMap<String, ResolvedServer>, // keyed by extension
}

impl Default for LspRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LspRegistry {
    /// Create a registry with built-in providers.
    pub fn new() -> Self {
        let mut registry = Self {
            providers: Vec::new(),
            config_servers: HashMap::new(),
        };
        registry.register(Box::new(RustAnalyzerProvider));
        registry.register(Box::new(TsServerProvider));
        registry.register(Box::new(PyrightProvider));
        registry
    }

    /// Create from config (loads config overrides on top of built-ins).
    pub fn from_config(config: &NovimConfig) -> Self {
        let mut registry = Self::new();

        // Load config-based servers
        for (name, server_config) in &config.lsp.servers {
            let resolved = ResolvedServer {
                language_id: name.clone(),
                command: server_config.command.clone(),
                args: server_config.args.clone(),
                root_markers: server_config.root_markers.clone(),
            };
            // Register for each extension
            for ext in &server_config.extensions {
                registry.config_servers.insert(ext.clone(), resolved.clone());
            }
        }

        registry
    }

    /// Register a new provider.
    pub fn register(&mut self, provider: Box<dyn LspServerProvider>) {
        self.providers.push(provider);
    }

    /// Resolve a file extension to a server configuration.
    /// Config overrides take priority over built-in providers.
    pub fn resolve(&self, extension: &str) -> Option<ResolvedServer> {
        // Check config first
        if let Some(server) = self.config_servers.get(extension) {
            return Some(server.clone());
        }

        // Fall back to built-in providers
        for provider in &self.providers {
            if provider.file_extensions().contains(&extension) {
                return Some(ResolvedServer {
                    language_id: provider.language_id().to_string(),
                    command: provider.server_command().to_string(),
                    args: provider.server_args(),
                    root_markers: provider.root_markers().iter().map(|s| s.to_string()).collect(),
                });
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_rust() {
        let registry = LspRegistry::new();
        let server = registry.resolve("rs").unwrap();
        assert_eq!(server.language_id, "rust");
        assert_eq!(server.command, "rust-analyzer");
    }

    #[test]
    fn test_resolve_typescript() {
        let registry = LspRegistry::new();
        let server = registry.resolve("ts").unwrap();
        assert_eq!(server.language_id, "typescript");
    }

    #[test]
    fn test_resolve_unknown() {
        let registry = LspRegistry::new();
        assert!(registry.resolve("xyz").is_none());
    }
}
