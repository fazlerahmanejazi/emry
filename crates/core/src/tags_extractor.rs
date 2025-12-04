use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use tree_sitter_tags::{TagsConfiguration, TagsContext};

use crate::models::{Language, Symbol};
use std::path::PathBuf;

pub struct TagsExtractor {
    context: TagsContext,
    configs: HashMap<Language, TagsConfiguration>,
}

impl TagsExtractor {
    pub fn new() -> Result<Self> {
        let mut configs = HashMap::new();
        
        let rust_config = TagsConfiguration::new(
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::TAGS_QUERY,
            "",
        )?;
        configs.insert(Language::Rust, rust_config);
        
        let python_config = TagsConfiguration::new(
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::TAGS_QUERY,
            "",
        )?;
        configs.insert(Language::Python, python_config);
        
        let go_config = TagsConfiguration::new(
            tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::TAGS_QUERY,
            "",
        )?;
        configs.insert(Language::Go, go_config);
        
        let js_config = TagsConfiguration::new(
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::TAGS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        )?;
        configs.insert(Language::JavaScript, js_config);
        
        let ts_query = r#"
(function_declaration name: (identifier) @name) @definition.function
(class_declaration name: (type_identifier) @name) @definition.class
(interface_declaration name: (type_identifier) @name) @definition.interface
(type_alias_declaration name: (type_identifier) @name) @definition.type
(enum_declaration name: (identifier) @name) @definition.enum
(module name: (identifier) @name) @definition.module
(variable_declarator name: (identifier) @name) @definition.variable
(method_definition name: (property_identifier) @name) @definition.method
        "#;
        let ts_config = TagsConfiguration::new(
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            ts_query,
            "", 
        )?;
        configs.insert(Language::TypeScript, ts_config);
        
        let java_config = TagsConfiguration::new(
            tree_sitter_java::LANGUAGE.into(),
            tree_sitter_java::TAGS_QUERY,
            "",
        )?;
        configs.insert(Language::Java, java_config);
        
        let c_config = TagsConfiguration::new(
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::TAGS_QUERY,
            "",
        )?;
        configs.insert(Language::C, c_config);
        
        let cpp_config = TagsConfiguration::new(
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::TAGS_QUERY,
            "",
        )?;
        configs.insert(Language::Cpp, cpp_config);
        
        Ok(Self {
            context: TagsContext::new(),
            configs,
        })
    }
    
    pub fn extract_symbols(
        &mut self,
        content: &str,
        path: &Path,
        language: &Language,
    ) -> Result<Vec<Symbol>> {
        let config = self.configs.get(language)
            .ok_or_else(|| anyhow::anyhow!("No tags config for {:?}", language))?;
        
        let mut parser = tree_sitter::Parser::new();
        let lang_ts = match language {
            Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
            Language::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
            Language::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
            Language::Java => Some(tree_sitter_java::LANGUAGE.into()),
            Language::C => Some(tree_sitter_c::LANGUAGE.into()),
            Language::Cpp => Some(tree_sitter_cpp::LANGUAGE.into()),
            Language::CSharp => Some(tree_sitter_c_sharp::LANGUAGE.into()),
            _ => None,
        };
        
        let tree = if let Some(lang) = lang_ts {
            if parser.set_language(&lang).is_ok() {
                parser.parse(content, None)
            } else {
                None
            }
        } else {
            None
        };

        let (tags, _) = self.context.generate_tags(
            config,
            content.as_bytes(),
            None,
        )?;

        let mut symbols = Vec::new();

        for tag in tags {
            let tag = tag?;
            
            if !tag.is_definition {
                continue;
            }
            
            let kind = config.syntax_type_name(tag.syntax_type_id).to_string();
            
            let name = std::str::from_utf8(
                &content.as_bytes()[tag.name_range.start..tag.name_range.end]
            )?
            .to_string();
            
            let mut start_byte = tag.line_range.start;
            let mut end_byte = tag.line_range.end;
            let mut parent_scope = None;
            
            if let Some(tree) = &tree {
                if let Some(node) = tree.root_node().descendant_for_byte_range(tag.name_range.start, tag.name_range.end) {
                    let mut curr = node;
                    while let Some(parent) = curr.parent() {
                        if is_definition_node(parent.kind(), language) {
                            start_byte = parent.start_byte();
                            end_byte = parent.end_byte();
                            break;
                        }
                        curr = parent;
                    }
                    
                    parent_scope = find_parent_scope(node, language, content);
                }
            }
            
            let start_line = byte_to_line(content, start_byte);
            let end_line = byte_to_line(content, end_byte);

            symbols.push(Symbol {
                id: format!("{}:{}-{}", path.display(), start_line, end_line),
                name: name.clone(),
                kind,
                file_path: PathBuf::from(path),
                start_line,
                end_line,
                fqn: name.clone(), 
                language: *language,
                doc_comment: tag.docs,
                parent_scope,
            });
        }
        
        Ok(symbols)
    }
    pub fn generate_outline(
        &mut self,
        code: &str,
        path: &Path,
        language: &Language,
    ) -> Result<String> {
        let symbols = self.extract_symbols(code, path, language)?;
        
        let mut sorted_symbols = symbols;
        sorted_symbols.sort_by_key(|s| s.start_line);
        
        let mut outline = String::new();
        
        for sym in sorted_symbols {
            let indent = if sym.parent_scope.is_some() { "    " } else { "" };
            let kind_marker = match sym.kind.as_str() {
                "function" | "method" => "fn",
                "class" | "struct" => "class",
                "interface" | "trait" => "interface",
                "module" => "mod",
                k => k,
            };
            
            let line = format!("{}{}: {} (L{}-L{})\n", indent, kind_marker, sym.name, sym.start_line, sym.end_line);
            outline.push_str(&line);
        }
        
        Ok(outline)
    }

    pub fn extract_code_item(
        &mut self,
        code: &str,
        path: &Path,
        language: &Language,
        node_path: &str,
    ) -> Result<Option<String>> {
        let symbols = self.extract_symbols(code, path, language)?;
        
        for sym in symbols {
            let match_found = if let Some(parent) = &sym.parent_scope {
                let full_name = format!("{}.{}", parent, sym.name);
                full_name == node_path || sym.name == node_path 
            } else {
                sym.name == node_path
            };
            
            if match_found {
                let lines: Vec<&str> = code.lines().collect();
                if sym.start_line > 0 && sym.end_line <= lines.len() {
                    let snippet = lines[sym.start_line - 1..sym.end_line].join("\n");
                    return Ok(Some(snippet));
                }
            }
        }
        
        Ok(None)
    }
}

fn is_definition_node(kind: &str, lang: &Language) -> bool {
    match lang {
        Language::Rust => matches!(kind, "function_item" | "struct_item" | "enum_item" | "trait_item" | "impl_item" | "mod_item" | "const_item" | "static_item"),
        Language::Python => matches!(kind, "function_definition" | "class_definition"),
        Language::JavaScript | Language::TypeScript => matches!(kind, "function_declaration" | "class_declaration" | "method_definition" | "arrow_function" | "variable_declarator"),
        Language::Go => matches!(kind, "function_declaration" | "type_declaration" | "method_declaration"),
        Language::Java => matches!(kind, "method_declaration" | "class_declaration" | "interface_declaration"),
        Language::C | Language::Cpp => matches!(kind, "function_definition" | "struct_specifier" | "class_specifier"),
        _ => false,
    }
}

fn byte_to_line(content: &str, byte_offset: usize) -> usize {
    if byte_offset >= content.len() {
        return content.lines().count();
    }
    content[..byte_offset].chars().filter(|&c| c == '\n').count() + 1
}

fn find_parent_scope(node: tree_sitter::Node, lang: &Language, source: &str) -> Option<String> {
    let mut curr = node;
    while let Some(parent) = curr.parent() {
        let kind = parent.kind();
        
        let is_self = parent.child_by_field_name("name")
            .or_else(|| parent.child_by_field_name("type"))
            .map_or(false, |n| n.start_byte() == node.start_byte() && n.end_byte() == node.end_byte());

        if is_self {
            curr = parent;
            continue;
        }

        match lang {
            Language::Rust => {
                if kind == "impl_item" {
                    if let Some(type_node) = parent.child_by_field_name("type") {
                        return type_node.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
                    }
                } else if matches!(kind, "mod_item" | "trait_item") {
                    if let Some(name_node) = parent.child_by_field_name("name") {
                        return name_node.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
                    }
                }
            },
            Language::Java | Language::JavaScript | Language::TypeScript | Language::Cpp | Language::C | Language::CSharp => {
                if matches!(kind, "class_declaration" | "interface_declaration" | "enum_declaration" | "struct_specifier" | "class_specifier") {
                    if let Some(name_node) = parent.child_by_field_name("name") {
                        return name_node.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
                    }
                }
            },
            Language::Python => {
                if kind == "class_definition" {
                    if let Some(name_node) = parent.child_by_field_name("name") {
                        return name_node.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
                    }
                }
            },
            _ => {}
        }
        curr = parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rust_struct_extraction() {
        let code = r#"
            pub struct ChunkingConfig {
                pub max_tokens: usize,
            }
        "#;
        
        let mut extractor = TagsExtractor::new().unwrap();
        let symbols = extractor.extract_symbols(
            code,
            Path::new("test.rs"),
            &Language::Rust,
        ).unwrap();
        
        assert!(!symbols.is_empty(), "Should extract at least one symbol");
        assert!(
            symbols.iter().any(|s| s.name == "ChunkingConfig"),
            "Should extract ChunkingConfig struct"
        );
        let config_sym = symbols.iter().find(|s| s.name == "ChunkingConfig").unwrap();
        assert_eq!(config_sym.kind, "class"); // struct -> class mapping
    }
    
    #[test]
    fn test_rust_enum_extraction() {
        let code = r#"
            enum SplitStrategy {
                Truncate,
                Split,
            }
        "#;
        
        let mut extractor = TagsExtractor::new().unwrap();
        let symbols = extractor.extract_symbols(
            code,
            Path::new("test.rs"),
            &Language::Rust,
        ).unwrap();
        
        assert!(symbols.iter().any(|s| s.name == "SplitStrategy"));
    }
    
    #[test]
    fn test_rust_trait_extraction() {
        let code = r#"
            pub trait Embedder {
                fn embed(&self, text: &str) -> Vec<f32>;
            }
        "#;
        
        let mut extractor = TagsExtractor::new().unwrap();
        let symbols = extractor.extract_symbols(
            code,
            Path::new("test.rs"),
            &Language::Rust,
        ).unwrap();
        
        assert!(symbols.iter().any(|s| s.name == "Embedder"), "Should extract Embedder trait");
    }
    
    #[test]
    fn test_python_class_extraction() {
        let code = r#"
class Emry:
    def search(self, query):
        pass
        "#;
        
        let mut extractor = TagsExtractor::new().unwrap();
        let symbols = extractor.extract_symbols(
            code,
            Path::new("test.py"),
            &Language::Python,
        ).unwrap();
        
        assert!(symbols.iter().any(|s| s.name == "Emry"));
        assert!(symbols.iter().any(|s| s.name == "search"));
    }
    
    #[test]
    fn test_go_struct_extraction() {
        let code = r#"
type Config struct {
    MaxTokens int
}
        "#;
        
        let mut extractor = TagsExtractor::new().unwrap();
        let symbols = extractor.extract_symbols(
            code,
            Path::new("test.go"),
            &Language::Go,
        ).unwrap();
        
        assert!(symbols.iter().any(|s| s.name == "Config"));
    }
    #[test]
    fn test_rust_multiline_range() {
        let code = r#"
fn my_func() {
    println!("line 1");
    println!("line 2");
}
"#;
        let mut extractor = TagsExtractor::new().unwrap();
        let symbols = extractor.extract_symbols(
            code,
            Path::new("test.rs"),
            &Language::Rust,
        ).unwrap();
        
        let sym = symbols.iter().find(|s| s.name == "my_func").unwrap();
        assert_eq!(sym.start_line, 2, "Start line should be 2");
        assert_eq!(sym.end_line, 5, "End line should be 5");
    }

    #[test]
    fn test_ts_extraction() {
        let code = r#"
            export function hello() {
                console.log("Hello");
            }
        "#;
        
        let mut extractor = TagsExtractor::new().unwrap();
        let symbols = extractor.extract_symbols(
            code,
            Path::new("test.ts"),
            &Language::TypeScript,
        ).unwrap();
        
        assert!(symbols.iter().any(|s| s.name == "hello"), "Should extract hello function");
    }

    #[test]
    fn test_generate_outline() {
        let code = r#"
            class MyClass {
                myMethod() {
                    console.log("hello");
                }
            }
            function globalFunc() {}
        "#;
        
        let mut extractor = TagsExtractor::new().unwrap();
        let outline = extractor.generate_outline(
            code, 
            Path::new("test.js"), 
            &Language::JavaScript
        ).unwrap();
        
        assert!(outline.contains("class: MyClass"));
        assert!(outline.contains("fn: myMethod"));
        assert!(outline.contains("fn: globalFunc"));
    }

    #[test]
    fn test_extract_code_item() {
        let code = r#"
            class MyClass {
                myMethod() {
                    console.log("hello");
                }
            }
        "#;
        
        let mut extractor = TagsExtractor::new().unwrap();
        let item = extractor.extract_code_item(
            code,
            Path::new("test.js"),
            &Language::JavaScript,
            "MyClass.myMethod"
        ).unwrap();
        
        assert!(item.is_some());
        let content = item.unwrap();
        assert!(content.contains("myMethod()"));
        assert!(content.contains("console.log"));
    }
}
