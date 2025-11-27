use crate::models::{Language, Symbol};
use crate::tags_extractor::TagsExtractor;
use anyhow::Result;
use std::path::Path;

/// Extract symbols for a given language using tree-sitter-tags.
/// Now supports ALL symbol types: structs, enums, traits, interfaces, classes, etc.
pub fn extract_symbols(content: &str, path: &Path, language: &Language) -> Result<Vec<Symbol>> {
    // Create extractor per-call since TagsConfiguration isn't Send
    // This is fine - extraction only happens during indexing, not on hot path
    let mut extractor = TagsExtractor::new()?;
    extractor.extract_symbols(content, path, language)
}

// ===== DEPRECATED: Old manual extraction code below =====
// Kept temporarily for reference, will be removed after verification
// TODO: Remove all code below after Phase 2 testing is complete

#[allow(dead_code)]
fn extract_symbols_legacy(content: &str, path: &Path, language: &Language) -> Result<Vec<Symbol>> {
    match language {
        Language::Rust => extract_rust_symbols(content, path),
        Language::Go => extract_go_symbols(content, path),
        Language::Python => extract_python_symbols(content, path),
        Language::JavaScript => extract_js_symbols(content, path),
        Language::TypeScript => extract_ts_symbols(content, path),
        Language::Java => extract_java_symbols(content, path),
        Language::C => extract_c_symbols(content, path),
        Language::Cpp => extract_cpp_symbols(content, path),
        Language::CSharp => extract_csharp_symbols(content, path),
        _ => Ok(Vec::new()),
    }
}

#[allow(dead_code)]
use std::path::PathBuf;

#[allow(dead_code)]
fn leading_doc_comment(content: &str, start_line: usize, language: &Language) -> Option<String> {
    if start_line == 0 {
        return None;
    }
    let lines: Vec<&str> = content.lines().collect();
    let mut collected = Vec::new();
    let prefixes: &[&str] = match language {
        Language::Rust => &["///", "//!", "//"],
        Language::Go => &["//"],
        Language::Python => &["#"],
        Language::JavaScript
        | Language::TypeScript
        | Language::Java
        | Language::C
        | Language::Cpp
        | Language::CSharp => &["//"],
        _ => &[],
    };
    if prefixes.is_empty() {
        return None;
    }
    let mut line_idx = start_line.saturating_sub(2); // 0-based
    while line_idx < lines.len() {
        let trimmed = lines[line_idx].trim();
        if trimmed.is_empty() {
            if collected.is_empty() {
                line_idx = line_idx.saturating_sub(1);
                continue;
            } else {
                break;
            }
        }
        if let Some(prefix) = prefixes.iter().find(|p| trimmed.starts_with(**p)) {
            let stripped = trimmed.strip_prefix(prefix).unwrap_or(trimmed).trim_start();
            collected.push(stripped.to_string());
            if line_idx == 0 {
                break;
            }
            line_idx -= 1;
        } else {
            break;
        }
    }
    if collected.is_empty() {
        None
    } else {
        collected.reverse();
        Some(collected.join("\n"))
    }
}

#[allow(dead_code)]

fn extract_rust_symbols(content: &str, path: &Path) -> Result<Vec<Symbol>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("Failed to load Rust grammar: {}", e))?;

    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };

    let mut symbols = Vec::new();
    let root = tree.root_node();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        // Collect children first for DFS
        let mut child_cursor = node.walk();
        for child in node.children(&mut child_cursor) {
            stack.push(child);
        }

        // First, handle top-level function items as before
        if node.kind() == "function_item" {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node
                    .utf8_text(content.as_bytes())
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let start = node.start_position();
                let end = node.end_position();
                let symbol = Symbol {
                    id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                    name: name.clone(),
                    kind: "function".to_string(),
                    file_path: PathBuf::from(path),
                    start_line: start.row + 1,
                    end_line: end.row + 1,
                    fqn: name.clone(), // FQN is just name for top-level functions
                    language: Language::Rust,
                    doc_comment: leading_doc_comment(content, start.row + 1, &Language::Rust),
                };
                symbols.push(symbol);
            }
        } else if node.kind() == "impl_item" {
            let mut type_name = "_".to_string(); // Default if type cannot be determined
            if let Some(type_node) = node.child_by_field_name("type") {
                type_name = type_node
                    .utf8_text(content.as_bytes())
                    .unwrap_or("")
                    .to_string();

                // Check for 'impl Trait for Type' pattern
                // Iterate through siblings to find the 'for' keyword
                let mut current_sibling = type_node.next_sibling();
                while let Some(sibling) = current_sibling {
                    if sibling.kind() == "for" {
                        if let Some(actual_type_node) = sibling.next_sibling() {
                            type_name = actual_type_node
                                .utf8_text(content.as_bytes())
                                .unwrap_or("")
                                .to_string();
                        }
                        break; // Found the 'for' and extracted the type, so break
                    }
                    current_sibling = sibling.next_sibling();
                }
            }

            let mut impl_cursor = node.walk();
            for item in node.children(&mut impl_cursor) {
                if item.kind() == "function_item" {
                    if let Some(name_node) = item.child_by_field_name("name") {
                        let name = name_node
                            .utf8_text(content.as_bytes())
                            .unwrap_or("")
                            .to_string();
                        if name.is_empty() {
                            continue;
                        }
                        let start = item.start_position();
                        let end = item.end_position();
                        let fqn = format!("{}::{}", type_name, name);
                        let symbol = Symbol {
                            id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                            name: name.clone(),
                            kind: "method".to_string(), // Mark as method
                            file_path: PathBuf::from(path),
                            start_line: start.row + 1,
                            end_line: end.row + 1,
                            fqn,
                            language: Language::Rust,
                            doc_comment: leading_doc_comment(
                                content,
                                start.row + 1,
                                &Language::Rust,
                            ),
                        };
                        symbols.push(symbol);
                    }
                }
            }
        }
    }

    Ok(symbols)
}

fn extract_go_symbols(content: &str, path: &Path) -> Result<Vec<Symbol>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("Failed to load Go grammar: {}", e))?;
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };
    let mut symbols = Vec::new();
    let root = tree.root_node();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        for child in node.children(&mut node.walk()) {
            stack.push(child);
        }
        if node.kind() == "function_declaration" {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node
                    .utf8_text(content.as_bytes())
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let start = node.start_position();
                let end = node.end_position();
                symbols.push(Symbol {
                    id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                    name: name.clone(),
                    kind: "function".to_string(),
                    file_path: PathBuf::from(path),
                    start_line: start.row + 1,
                    end_line: end.row + 1,
                    fqn: name,
                    language: Language::Go,
                    doc_comment: leading_doc_comment(content, start.row + 1, &Language::Go),
                });
            }
        }
    }
    Ok(symbols)
}

fn extract_python_symbols(content: &str, path: &Path) -> Result<Vec<Symbol>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("Failed to load Python grammar: {}", e))?;
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };
    let mut symbols = Vec::new();
    let root = tree.root_node();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        for child in node.children(&mut node.walk()) {
            stack.push(child);
        }
        match node.kind() {
            "function_definition" | "class_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let start = node.start_position();
                    let end = node.end_position();
                    let kind = if node.kind() == "class_definition" {
                        "class"
                    } else {
                        "function"
                    };
                    symbols.push(Symbol {
                        id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                        name: name.clone(),
                        kind: kind.to_string(),
                        file_path: PathBuf::from(path),
                        start_line: start.row + 1,
                        end_line: end.row + 1,
                        fqn: name,
                        language: Language::Python,
                        doc_comment: leading_doc_comment(content, start.row + 1, &Language::Python),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(symbols)
}

fn extract_js_symbols(content: &str, path: &Path) -> Result<Vec<Symbol>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("Failed to load JavaScript grammar: {}", e))?;
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };
    let mut symbols = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        for child in node.children(&mut node.walk()) {
            stack.push(child);
        }
        match node.kind() {
            "function_declaration" | "method_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let start = node.start_position();
                    let end = node.end_position();
                    symbols.push(Symbol {
                        id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                        name: name.clone(),
                        kind: "function".to_string(),
                        file_path: PathBuf::from(path),
                        start_line: start.row + 1,
                        end_line: end.row + 1,
                        fqn: name,
                        language: Language::JavaScript,
                        doc_comment: leading_doc_comment(
                            content,
                            start.row + 1,
                            &Language::JavaScript,
                        ),
                    });
                }
            }
            "class_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let start = node.start_position();
                    let end = node.end_position();
                    symbols.push(Symbol {
                        id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                        name: name.clone(),
                        kind: "class".to_string(),
                        file_path: PathBuf::from(path),
                        start_line: start.row + 1,
                        end_line: end.row + 1,
                        fqn: name,
                        language: Language::JavaScript,
                        doc_comment: leading_doc_comment(
                            content,
                            start.row + 1,
                            &Language::JavaScript,
                        ),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(symbols)
}

fn extract_ts_symbols(content: &str, path: &Path) -> Result<Vec<Symbol>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .map_err(|e| anyhow::anyhow!("Failed to load TypeScript grammar: {}", e))?;
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };
    let mut symbols = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        for child in node.children(&mut node.walk()) {
            stack.push(child);
        }
        match node.kind() {
            "function_declaration" | "method_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let start = node.start_position();
                    let end = node.end_position();
                    symbols.push(Symbol {
                        id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                        name: name.clone(),
                        kind: "function".to_string(),
                        file_path: PathBuf::from(path),
                        start_line: start.row + 1,
                        end_line: end.row + 1,
                        fqn: name,
                        language: Language::TypeScript,
                        doc_comment: leading_doc_comment(
                            content,
                            start.row + 1,
                            &Language::TypeScript,
                        ),
                    });
                }
            }
            "class_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let start = node.start_position();
                    let end = node.end_position();
                    symbols.push(Symbol {
                        id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                        name: name.clone(),
                        kind: "class".to_string(),
                        file_path: PathBuf::from(path),
                        start_line: start.row + 1,
                        end_line: end.row + 1,
                        fqn: name,
                        language: Language::TypeScript,
                        doc_comment: leading_doc_comment(
                            content,
                            start.row + 1,
                            &Language::TypeScript,
                        ),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(symbols)
}

fn extract_java_symbols(content: &str, path: &Path) -> Result<Vec<Symbol>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("Failed to load Java grammar: {}", e))?;
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };
    let mut symbols = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        for child in node.children(&mut node.walk()) {
            stack.push(child);
        }
        match node.kind() {
            "method_declaration" | "constructor_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let start = node.start_position();
                    let end = node.end_position();
                    symbols.push(Symbol {
                        id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                        name: name.clone(),
                        kind: "function".to_string(),
                        file_path: PathBuf::from(path),
                        start_line: start.row + 1,
                        end_line: end.row + 1,
                        fqn: name,
                        language: Language::Java,
                        doc_comment: leading_doc_comment(content, start.row + 1, &Language::Java),
                    });
                }
            }
            "class_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let start = node.start_position();
                    let end = node.end_position();
                    symbols.push(Symbol {
                        id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                        name: name.clone(),
                        kind: "class".to_string(),
                        file_path: PathBuf::from(path),
                        start_line: start.row + 1,
                        end_line: end.row + 1,
                        fqn: name,
                        language: Language::Java,
                        doc_comment: leading_doc_comment(content, start.row + 1, &Language::Java),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(symbols)
}

fn extract_c_symbols(content: &str, path: &Path) -> Result<Vec<Symbol>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("Failed to load C grammar: {}", e))?;
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };
    let mut symbols = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        for child in node.children(&mut node.walk()) {
            stack.push(child);
        }
        if node.kind() == "function_definition" {
            if let Some(decl) = node.child_by_field_name("declarator") {
                let name = decl
                    .utf8_text(content.as_bytes())
                    .unwrap_or("")
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let start = node.start_position();
                let end = node.end_position();
                symbols.push(Symbol {
                    id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                    name: name.clone(),
                    kind: "function".to_string(),
                    file_path: PathBuf::from(path),
                    start_line: start.row + 1,
                    end_line: end.row + 1,
                    fqn: name,
                    language: Language::C,
                    doc_comment: leading_doc_comment(content, start.row + 1, &Language::C),
                });
            }
        }
    }
    Ok(symbols)
}

fn extract_cpp_symbols(content: &str, path: &Path) -> Result<Vec<Symbol>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_cpp::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("Failed to load C++ grammar: {}", e))?;
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };
    let mut symbols = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        for child in node.children(&mut node.walk()) {
            stack.push(child);
        }
        if node.kind() == "function_definition" {
            if let Some(decl) = node.child_by_field_name("declarator") {
                let name = decl
                    .utf8_text(content.as_bytes())
                    .unwrap_or("")
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let start = node.start_position();
                let end = node.end_position();
                symbols.push(Symbol {
                    id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                    name: name.clone(),
                    kind: "function".to_string(),
                    file_path: PathBuf::from(path),
                    start_line: start.row + 1,
                    end_line: end.row + 1,
                    fqn: name,
                    language: Language::Cpp,
                    doc_comment: leading_doc_comment(content, start.row + 1, &Language::Cpp),
                });
            }
        }
        if node.kind() == "class_specifier" {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node
                    .utf8_text(content.as_bytes())
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let start = node.start_position();
                let end = node.end_position();
                symbols.push(Symbol {
                    id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                    name: name.clone(),
                    kind: "class".to_string(),
                    file_path: PathBuf::from(path),
                    start_line: start.row + 1,
                    end_line: end.row + 1,
                    fqn: name,
                    language: Language::Cpp,
                    doc_comment: leading_doc_comment(content, start.row + 1, &Language::Cpp),
                });
            }
        }
    }
    Ok(symbols)
}

fn extract_csharp_symbols(content: &str, path: &Path) -> Result<Vec<Symbol>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("Failed to load C# grammar: {}", e))?;
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };
    let mut symbols = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        for child in node.children(&mut node.walk()) {
            stack.push(child);
        }
        match node.kind() {
            "method_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let start = node.start_position();
                    let end = node.end_position();
                    symbols.push(Symbol {
                        id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                        name: name.clone(),
                        kind: "function".to_string(),
                        file_path: PathBuf::from(path),
                        start_line: start.row + 1,
                        end_line: end.row + 1,
                        fqn: name,
                        language: Language::CSharp,
                        doc_comment: leading_doc_comment(content, start.row + 1, &Language::CSharp),
                    });
                }
            }
            "class_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let start = node.start_position();
                    let end = node.end_position();
                    symbols.push(Symbol {
                        id: format!("{}:{}-{}", path.display(), start.row + 1, end.row + 1),
                        name: name.clone(),
                        kind: "class".to_string(),
                        file_path: PathBuf::from(path),
                        start_line: start.row + 1,
                        end_line: end.row + 1,
                        fqn: name,
                        language: Language::CSharp,
                        doc_comment: leading_doc_comment(content, start.row + 1, &Language::CSharp),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(symbols)
}
