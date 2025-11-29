use crate::models::Language;
use tree_sitter::Node;

#[derive(Debug, Clone)]
pub struct RelationRef {
    pub name: String,
    pub line: usize,
}

impl RelationRef {
    pub fn to_symbol_name(self) -> String {
        self.name
    }
}

/// Extract calls/imports using Tree-sitter (where supported) to avoid fragile regexes.
/// Returns (calls, imports) with best-effort line numbers for mapping to chunks.
pub fn extract_calls_imports(
    language: &Language,
    content: &str,
) -> (Vec<RelationRef>, Vec<RelationRef>) {
    match language {
        Language::JavaScript | Language::TypeScript => {
            extract_js_ts_calls_imports(language, content)
        }
        Language::Java => extract_java_calls_imports(content),
        Language::C | Language::Cpp => extract_c_cpp_calls_imports(language, content),
        Language::CSharp => extract_csharp_calls_imports(content),
        Language::Python => extract_python_calls_imports(content),
        Language::Rust => extract_rust_calls_imports(content),
        Language::Go => extract_go_calls_imports(content),
        _ => (Vec::new(), Vec::new()),
    }
}

fn walk_tree(root: Node) -> Vec<Node> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        for child in node.children(&mut node.walk()) {
            stack.push(child);
        }
        out.push(node);
    }
    out
}

fn extract_js_ts_calls_imports(
    language: &Language,
    content: &str,
) -> (Vec<RelationRef>, Vec<RelationRef>) {
    let mut parser = tree_sitter::Parser::new();
    let lang = match language {
        Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    };
    if parser.set_language(&lang).is_err() {
        return (Vec::new(), Vec::new());
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return (Vec::new(), Vec::new()),
    };

    let mut calls = Vec::new();
    let mut imports = Vec::new();
    for node in walk_tree(tree.root_node()) {
        match node.kind() {
            "call_expression" => {
                if let Some(function) = node.child_by_field_name("function") {
                    if let Ok(name) = function.utf8_text(content.as_bytes()) {
                        if !name.is_empty() {
                            calls.push(RelationRef {
                                name: name.to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            "import_clause" | "import_statement" => {
                if let Some(spec) = node.child_by_field_name("name") {
                    if let Ok(name) = spec.utf8_text(content.as_bytes()) {
                        if !name.is_empty() {
                            imports.push(RelationRef {
                                name: name.to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
                if let Some(module) = node.child_by_field_name("source") {
                    if let Ok(name) = module.utf8_text(content.as_bytes()) {
                        let trimmed = name.trim_matches(['\"', '\'']);
                        if !trimmed.is_empty() {
                            imports.push(RelationRef {
                                name: trimmed.to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (calls, imports)
}

fn extract_java_calls_imports(content: &str) -> (Vec<RelationRef>, Vec<RelationRef>) {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .is_err()
    {
        return (Vec::new(), Vec::new());
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return (Vec::new(), Vec::new()),
    };
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    for node in walk_tree(tree.root_node()) {
        match node.kind() {
            "method_invocation" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
                        if !name.is_empty() {
                            calls.push(RelationRef {
                                name: name.to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            "import_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
                        if !name.is_empty() {
                            imports.push(RelationRef {
                                name: name.to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (calls, imports)
}

fn extract_c_cpp_calls_imports(
    language: &Language,
    content: &str,
) -> (Vec<RelationRef>, Vec<RelationRef>) {
    let mut parser = tree_sitter::Parser::new();
    let lang = if matches!(language, Language::C) {
        tree_sitter_c::LANGUAGE.into()
    } else {
        tree_sitter_cpp::LANGUAGE.into()
    };
    if parser.set_language(&lang).is_err() {
        return (Vec::new(), Vec::new());
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return (Vec::new(), Vec::new()),
    };

    let mut calls = Vec::new();
    let mut imports = Vec::new();
    for node in walk_tree(tree.root_node()) {
        match node.kind() {
            "call_expression" => {
                if let Some(fn_node) = node.child_by_field_name("function") {
                    if let Ok(name) = fn_node.utf8_text(content.as_bytes()) {
                        if !name.is_empty() {
                            calls.push(RelationRef {
                                name: name.to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            "preproc_include" => {
                if let Some(path_node) = node.child_by_field_name("path") {
                    if let Ok(name) = path_node.utf8_text(content.as_bytes()) {
                        if !name.is_empty() {
                            imports.push(RelationRef {
                                name: name.to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (calls, imports)
}

fn extract_csharp_calls_imports(content: &str) -> (Vec<RelationRef>, Vec<RelationRef>) {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .is_err()
    {
        return (Vec::new(), Vec::new());
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return (Vec::new(), Vec::new()),
    };

    let mut calls = Vec::new();
    let mut imports = Vec::new();
    for node in walk_tree(tree.root_node()) {
        match node.kind() {
            "invocation_expression" => {
                if let Some(exp) = node.child_by_field_name("expression") {
                    if let Ok(name) = exp.utf8_text(content.as_bytes()) {
                        if !name.is_empty() {
                            calls.push(RelationRef {
                                name: name.to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            "using_directive" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
                        if !name.is_empty() {
                            imports.push(RelationRef {
                                name: name.to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (calls, imports)
}

fn extract_python_calls_imports(content: &str) -> (Vec<RelationRef>, Vec<RelationRef>) {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .is_err()
    {
        return (Vec::new(), Vec::new());
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return (Vec::new(), Vec::new()),
    };
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    for node in walk_tree(tree.root_node()) {
        match node.kind() {
            "call" => {
                if let Some(func) = node.child_by_field_name("function") {
                    if let Ok(name) = func.utf8_text(content.as_bytes()) {
                        let trimmed = name.split('.').last().unwrap_or(name);
                        if !trimmed.is_empty() {
                            calls.push(RelationRef {
                                name: trimmed.to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            "import_name" | "import_from_statement" => {
                if let Ok(name) = node.utf8_text(content.as_bytes()) {
                    let cleaned = name
                        .replace("import", "")
                        .replace("from", "")
                        .replace(" as ", " ")
                        .trim()
                        .to_string();
                    if !cleaned.is_empty() {
                        imports.push(RelationRef {
                            name: cleaned,
                            line: node.start_position().row + 1,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    (calls, imports)
}

fn extract_rust_calls_imports(content: &str) -> (Vec<RelationRef>, Vec<RelationRef>) {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        return (Vec::new(), Vec::new());
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return (Vec::new(), Vec::new()),
    };
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    for node in walk_tree(tree.root_node()) {
        match node.kind() {
            "call_expression" => {
                if let Some(func) = node.child_by_field_name("function") {
                    if let Ok(full_name) = func.utf8_text(content.as_bytes()) {
                        let name = full_name.to_string(); // Keep full qualified name
                        if !name.is_empty() {
                            calls.push(RelationRef {
                                name,
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            "use_declaration" => {
                if let Ok(name) = node.utf8_text(content.as_bytes()) {
                    let cleaned = name.trim().trim_start_matches("use").trim_end_matches(';');
                    let last = cleaned
                        .rsplit("::")
                        .next()
                        .unwrap_or(cleaned)
                        .trim()
                        .to_string();
                    if !last.is_empty() {
                        imports.push(RelationRef {
                            name: last,
                            line: node.start_position().row + 1,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    (calls, imports)
}

fn extract_go_calls_imports(content: &str) -> (Vec<RelationRef>, Vec<RelationRef>) {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .is_err()
    {
        return (Vec::new(), Vec::new());
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return (Vec::new(), Vec::new()),
    };
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    for node in walk_tree(tree.root_node()) {
        match node.kind() {
            "call_expression" => {
                if let Some(func) = node.child_by_field_name("function") {
                    if let Ok(name) = func.utf8_text(content.as_bytes()) {
                        let trimmed = name.split('.').last().unwrap_or(name);
                        calls.push(RelationRef {
                            name: trimmed.to_string(),
                            line: node.start_position().row + 1,
                        });
                    }
                }
            }
            "import_spec" => {
                if let Some(path_node) = node.child_by_field_name("path") {
                    if let Ok(name) = path_node.utf8_text(content.as_bytes()) {
                        let trimmed = name.trim_matches('"');
                        if !trimmed.is_empty() {
                            imports.push(RelationRef {
                                name: trimmed.to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (calls, imports)
}
