use crate::models::Language;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tree_sitter::{Parser, Query, QueryCursor};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    Module,
    Unknown,
}

impl From<&str> for SymbolKind {
    fn from(s: &str) -> Self {
        match s {
            "function" => SymbolKind::Function,
            "method" => SymbolKind::Method,
            "class" => SymbolKind::Class,
            "interface" => SymbolKind::Interface,
            "module" => SymbolKind::Module,
            _ => SymbolKind::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub language: Language,
    pub file_path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(default)]
    pub fqn: String,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub chunk_ids: Vec<String>, // one or more chunks covering this symbol
    pub chunk_id: Option<String>, // Link to the main chunk if available (legacy)
}

pub struct SymbolExtractor;

impl SymbolExtractor {
    pub fn extract(
        content: &str,
        file_path: &std::path::Path,
        language: Language,
    ) -> Result<Vec<Symbol>> {
        match language {
            Language::Python => Self::extract_python(content, file_path),
            Language::TypeScript | Language::JavaScript => {
                Self::extract_typescript(content, file_path, language)
            }
            Language::Java => Self::extract_java(content, file_path),
            Language::Cpp => Self::extract_cpp(content, file_path),
            _ => Ok(Vec::new()),
        }
    }

    fn extract_python(content: &str, file_path: &std::path::Path) -> Result<Vec<Symbol>> {
        let mut parser = Parser::new();
        let language = tree_sitter_python::LANGUAGE;
        parser.set_language(&language.into())?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Python"))?;

        let query_str = "
            (function_definition name: (identifier) @name) @function
            (class_definition name: (identifier) @name) @class
        ";

        Self::run_query(
            content,
            file_path,
            Language::Python,
            tree.root_node(),
            &language.into(),
            query_str,
        )
    }

    fn extract_typescript(
        content: &str,
        file_path: &std::path::Path,
        language_enum: Language,
    ) -> Result<Vec<Symbol>> {
        let mut parser = Parser::new();
        let is_tsx = file_path
            .extension()
            .map_or(false, |ext| ext == "tsx" || ext == "jsx");
        let language = if is_tsx {
            tree_sitter_typescript::LANGUAGE_TSX
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT
        };
        parser.set_language(&language.into())?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse TS/JS"))?;

        let query_str = "
            (function_declaration name: (identifier) @name) @function
            (class_declaration name: (type_identifier) @name) @class
            (interface_declaration name: (type_identifier) @name) @interface
            (method_definition name: (property_identifier) @name) @method
            (public_field_definition name: (property_identifier) @name) @method
        ";

        Self::run_query(
            content,
            file_path,
            language_enum,
            tree.root_node(),
            &language.into(),
            query_str,
        )
    }

    fn extract_java(content: &str, file_path: &std::path::Path) -> Result<Vec<Symbol>> {
        let mut parser = Parser::new();
        let language = tree_sitter_java::LANGUAGE;
        parser.set_language(&language.into())?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Java"))?;

        let query_str = "
            (class_declaration name: (identifier) @name) @class
            (interface_declaration name: (identifier) @name) @interface
            (method_declaration name: (identifier) @name) @method
            (constructor_declaration name: (identifier) @name) @method
        ";

        Self::run_query(
            content,
            file_path,
            Language::Java,
            tree.root_node(),
            &language.into(),
            query_str,
        )
    }

    fn extract_cpp(content: &str, file_path: &std::path::Path) -> Result<Vec<Symbol>> {
        let mut parser = Parser::new();
        let language = tree_sitter_cpp::LANGUAGE;
        parser.set_language(&language.into())?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse C++"))?;

        // C++ is tricky with names (qualified names etc).
        // Simplified query for now.
        let query_str = "
            (class_specifier name: (type_identifier) @name) @class
            (struct_specifier name: (type_identifier) @name) @class
            (function_definition declarator: (function_declarator declarator: (identifier) @name)) @function
        ";

        Self::run_query(
            content,
            file_path,
            Language::Cpp,
            tree.root_node(),
            &language.into(),
            query_str,
        )
    }

    fn run_query(
        content: &str,
        file_path: &std::path::Path,
        language: Language,
        root_node: tree_sitter::Node,
        ts_language: &tree_sitter::Language,
        query_str: &str,
    ) -> Result<Vec<Symbol>> {
        let query = Query::new(ts_language, query_str)?;
        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, root_node, content.as_bytes());

        let mut symbols = Vec::new();

        for m in matches {
            let mut name_node = None;
            let mut kind = SymbolKind::Unknown;
            let mut definition_node = None;

            for capture in m.captures {
                let capture_name = query.capture_names()[capture.index as usize];
                match capture_name {
                    "name" => name_node = Some(capture.node),
                    "function" => {
                        kind = SymbolKind::Function;
                        definition_node = Some(capture.node);
                    }
                    "method" => {
                        kind = SymbolKind::Method;
                        definition_node = Some(capture.node);
                    }
                    "class" => {
                        kind = SymbolKind::Class;
                        definition_node = Some(capture.node);
                    }
                    "interface" => {
                        kind = SymbolKind::Interface;
                        definition_node = Some(capture.node);
                    }
                    "module" => {
                        kind = SymbolKind::Module;
                        definition_node = Some(capture.node);
                    }
                    _ => {}
                }
            }

            if let (Some(name_n), Some(def_n)) = (name_node, definition_node) {
                let name = name_n.utf8_text(content.as_bytes())?.to_string();
                let start_line = def_n.start_position().row + 1;
                let end_line = def_n.end_position().row + 1;

                let id = format!("{}:{}:{}", file_path.to_string_lossy(), name, start_line);
                let fqn = build_fqn(file_path, &name);

                symbols.push(Symbol {
                    id,
                    name,
                    kind,
                    language: language.clone(),
                    file_path: file_path.to_path_buf(),
                    start_line,
                    end_line,
                    fqn,
                    visibility: None,
                    chunk_ids: Vec::new(),
                    chunk_id: None,
                });
            }
        }

        Ok(symbols)
    }
}

fn build_fqn(file_path: &std::path::Path, name: &str) -> String {
    let mut parts = Vec::new();
    if let Some(parent) = file_path.parent() {
        if let Some(parent_str) = parent.to_str() {
            if !parent_str.is_empty() {
                parts.push(parent_str.replace(std::path::MAIN_SEPARATOR, "::"));
            }
        }
    }
    if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
        parts.push(stem.to_string());
    }
    parts.push(name.to_string());
    parts
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("::")
}
