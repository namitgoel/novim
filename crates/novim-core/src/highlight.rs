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
