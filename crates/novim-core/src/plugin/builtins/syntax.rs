//! Syntax highlighting using Tree-sitter.
//!
//! Detects language from file extension, parses with Tree-sitter,
//! and provides highlight spans for each line.

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter as TsHighlighter};

/// Highlight groups — indices into this array map to HighlightEvent::Source
const HIGHLIGHT_NAMES: &[&str] = &[
    "keyword",
    "function",
    "function.builtin",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "constant",
    "constant.builtin",
    "string",
    "number",
    "comment",
    "operator",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "property",
    "attribute",
    "tag",
    "escape",
];

/// A highlight group identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightGroup {
    Keyword,
    Function,
    FunctionBuiltin,
    Type,
    TypeBuiltin,
    Variable,
    VariableBuiltin,
    Constant,
    ConstantBuiltin,
    String,
    Number,
    Comment,
    Operator,
    Punctuation,
    PunctuationBracket,
    PunctuationDelimiter,
    Property,
    Attribute,
    Tag,
    Escape,
    None,
}

impl HighlightGroup {
    /// Return the color string from the syntax theme for this highlight group.
    /// Returns `None` for `HighlightGroup::None` (use default foreground).
    pub fn theme_color<'a>(&self, theme: &'a crate::config::SyntaxTheme) -> Option<&'a str> {
        let s = match self {
            Self::Keyword => &theme.keyword,
            Self::Function | Self::FunctionBuiltin => &theme.function,
            Self::Type | Self::TypeBuiltin => &theme.r#type,
            Self::Variable | Self::VariableBuiltin => &theme.variable,
            Self::Constant | Self::ConstantBuiltin => &theme.constant,
            Self::String => &theme.string,
            Self::Number => &theme.number,
            Self::Comment => &theme.comment,
            Self::Operator => &theme.operator,
            Self::Punctuation | Self::PunctuationBracket | Self::PunctuationDelimiter => &theme.punctuation,
            Self::Property => &theme.property,
            Self::Attribute => &theme.attribute,
            Self::Tag => &theme.property,
            Self::Escape => &theme.constant,
            Self::None => return None,
        };
        Some(s)
    }

    /// Whether this group should be rendered bold.
    pub fn is_bold(&self) -> bool {
        matches!(self, Self::Keyword)
    }

    fn from_index(idx: usize) -> Self {
        match idx {
            0 => Self::Keyword,
            1 => Self::Function,
            2 => Self::FunctionBuiltin,
            3 => Self::Type,
            4 => Self::TypeBuiltin,
            5 => Self::Variable,
            6 => Self::VariableBuiltin,
            7 => Self::Constant,
            8 => Self::ConstantBuiltin,
            9 => Self::String,
            10 => Self::Number,
            11 => Self::Comment,
            12 => Self::Operator,
            13 => Self::Punctuation,
            14 => Self::PunctuationBracket,
            15 => Self::PunctuationDelimiter,
            16 => Self::Property,
            17 => Self::Attribute,
            18 => Self::Tag,
            19 => Self::Escape,
            _ => Self::None,
        }
    }
}

/// Supported languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    JavaScript,
    TypeScript,
    Json,
    Toml,
    Python,
    Markdown,
}

impl Language {
    /// Detect language from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "rs" => Some(Self::Rust),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" | "tsx" | "mts" => Some(Self::TypeScript),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "py" | "pyw" => Some(Self::Python),
            "md" | "markdown" => Some(Self::Markdown),
            _ => None,
        }
    }
}

/// A span of highlighted text within a line.
#[derive(Debug, Clone)]
pub struct HighlightSpan {
    pub start: usize,
    pub end: usize,
    pub group: HighlightGroup,
}

/// Syntax highlighter for a buffer.
pub struct SyntaxHighlighter {
    config: HighlightConfiguration,
    language: Language,
}

impl SyntaxHighlighter {
    /// Create a highlighter for the given language.
    pub fn new(lang: Language) -> Option<Self> {
        let config = make_config(lang)?;
        Some(Self {
            config,
            language: lang,
        })
    }

    /// Create from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        let lang = Language::from_extension(ext)?;
        Self::new(lang)
    }

    pub fn language(&self) -> Language {
        self.language
    }

    /// Highlight the given source code and return spans per line.
    pub fn highlight(&self, source: &str) -> Vec<Vec<HighlightSpan>> {
        let mut highlighter = TsHighlighter::new();
        let highlights = highlighter.highlight(&self.config, source.as_bytes(), None, |_| None);

        let Ok(highlights) = highlights else {
            return default_spans(source);
        };

        let mut result: Vec<Vec<HighlightSpan>> = source.lines().map(|_| Vec::new()).collect();
        if result.is_empty() {
            result.push(Vec::new());
        }

        let mut current_group = HighlightGroup::None;

        // Build a map of byte offset → (line, col) for quick lookup
        let line_starts: Vec<usize> = std::iter::once(0)
            .chain(source.bytes().enumerate().filter_map(|(i, b)| {
                if b == b'\n' { Some(i + 1) } else { None }
            }))
            .collect();

        for event in highlights {
            let Ok(event) = event else { continue };
            match event {
                HighlightEvent::Source { start, end } => {
                    if current_group != HighlightGroup::None {
                        // Find which lines this span covers
                        let start_line = line_starts.partition_point(|&s| s <= start).saturating_sub(1);
                        let end_line = line_starts.partition_point(|&s| s <= end).saturating_sub(1);

                        for line in start_line..=end_line {
                            if line >= result.len() { break; }
                            let line_start = line_starts.get(line).copied().unwrap_or(0);
                            let line_end = line_starts.get(line + 1).copied().unwrap_or(source.len());

                            let span_start = start.max(line_start) - line_start;
                            let span_end = end.min(line_end) - line_start;
                            // Don't include newline
                            let span_end = span_end.min(line_end - line_start);

                            if span_start < span_end {
                                result[line].push(HighlightSpan {
                                    start: span_start,
                                    end: span_end,
                                    group: current_group,
                                });
                            }
                        }
                    }
                }
                HighlightEvent::HighlightStart(h) => {
                    current_group = HighlightGroup::from_index(h.0);
                }
                HighlightEvent::HighlightEnd => {
                    current_group = HighlightGroup::None;
                }
            }
        }

        result
    }
}

fn default_spans(source: &str) -> Vec<Vec<HighlightSpan>> {
    source.lines().map(|_| Vec::new()).collect()
}

/// A symbol extracted from the AST (function, struct, class, etc.).
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub line: usize,
    /// End line of the symbol's scope (for containment/breadcrumb checks).
    pub end_line: usize,
    /// Nesting depth (0 = top-level, 1 = inside impl/class, etc.).
    pub depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    Class,
    Interface,
    Constant,
    Module,
}

impl SymbolKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Function => "fn",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Constant => "const",
            Self::Module => "mod",
        }
    }
}

/// Extract symbols (functions, structs, classes) from source code.
pub fn extract_symbols(source: &str, lang: Language) -> Vec<SymbolInfo> {
    let Some(ts_lang) = ts_language(lang) else { return Vec::new() };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&ts_lang).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else { return Vec::new() };

    let mut symbols = Vec::new();
    let node_kinds = symbol_node_kinds(lang);
    walk_for_symbols(&tree.root_node(), source, &node_kinds, lang, &mut symbols, 0);
    symbols
}

/// Find the breadcrumb trail for a given cursor line.
/// Returns the chain of containing symbols from outermost to innermost.
pub fn breadcrumb_at(symbols: &[SymbolInfo], cursor_line: usize) -> Vec<&SymbolInfo> {
    let mut trail: Vec<&SymbolInfo> = symbols.iter()
        .filter(|s| cursor_line >= s.line && cursor_line <= s.end_line)
        .collect();
    // Sort by depth (outermost first), then by line (earlier first)
    trail.sort_by_key(|s| (s.depth, s.line));
    trail
}

fn ts_language(lang: Language) -> Option<tree_sitter::Language> {
    match lang {
        Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        Language::JavaScript | Language::TypeScript => Some(tree_sitter_javascript::LANGUAGE.into()),
        Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Language::Json | Language::Toml | Language::Markdown => None, // no meaningful symbols
    }
}

/// Map of AST node kinds → SymbolKind for each language.
fn symbol_node_kinds(lang: Language) -> Vec<(&'static str, &'static str, SymbolKind)> {
    // (node_kind, name_field, symbol_kind)
    match lang {
        Language::Rust => vec![
            ("function_item", "name", SymbolKind::Function),
            ("struct_item", "name", SymbolKind::Struct),
            ("enum_item", "name", SymbolKind::Enum),
            ("impl_item", "type", SymbolKind::Struct),
            ("mod_item", "name", SymbolKind::Module),
            ("const_item", "name", SymbolKind::Constant),
            ("static_item", "name", SymbolKind::Constant),
            ("trait_item", "name", SymbolKind::Interface),
        ],
        Language::JavaScript | Language::TypeScript => vec![
            ("function_declaration", "name", SymbolKind::Function),
            ("method_definition", "name", SymbolKind::Method),
            ("class_declaration", "name", SymbolKind::Class),
            ("variable_declarator", "name", SymbolKind::Constant),
        ],
        Language::Python => vec![
            ("function_definition", "name", SymbolKind::Function),
            ("class_definition", "name", SymbolKind::Class),
        ],
        _ => Vec::new(),
    }
}

fn walk_for_symbols(
    node: &tree_sitter::Node,
    source: &str,
    kinds: &[(&str, &str, SymbolKind)],
    lang: Language,
    out: &mut Vec<SymbolInfo>,
    depth: usize,
) {
    let node_kind = node.kind();
    let mut matched = false;
    for (kind_str, name_field, sym_kind) in kinds {
        if node_kind == *kind_str {
            if let Some(name_node) = node.child_by_field_name(name_field) {
                let name = &source[name_node.start_byte()..name_node.end_byte()];
                out.push(SymbolInfo {
                    name: name.to_string(),
                    kind: *sym_kind,
                    line: node.start_position().row,
                    end_line: node.end_position().row,
                    depth,
                });
                matched = true;
            }
            break;
        }
    }
    // Recurse into children (increase depth if this node was a symbol)
    let child_depth = if matched { depth + 1 } else { depth };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_for_symbols(&child, source, kinds, lang, out, child_depth);
    }
}

fn make_config(lang: Language) -> Option<HighlightConfiguration> {
    let (ts_lang, highlights_query) = match lang {
        Language::Rust => (
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY,
        ),
        Language::JavaScript | Language::TypeScript => (
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY,
        ),
        Language::Json => (
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY,
        ),
        Language::Toml => (
            tree_sitter_toml_ng::language(),
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
        ),
        Language::Python => (
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY,
        ),
        Language::Markdown => (
            tree_sitter_md::LANGUAGE.into(),
            "", // md grammar doesn't bundle highlights
        ),
    };

    let mut config = HighlightConfiguration::new(ts_lang, "source", highlights_query, "", "").ok()?;
    config.configure(HIGHLIGHT_NAMES);
    Some(config)
}
