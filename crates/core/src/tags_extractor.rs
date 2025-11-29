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
        
        // Rust - use built-in TAGS_QUERY from tree-sitter-rust  
        let rust_config = TagsConfiguration::new(
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::TAGS_QUERY,
            "", // no locals query needed
        )?;
        configs.insert(Language::Rust, rust_config);
        
        // Python - use built-in TAGS_QUERY
        let python_config = TagsConfiguration::new(
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::TAGS_QUERY,
            "",
        )?;
        configs.insert(Language::Python, python_config);
        
        // Go - use built-in TAGS_QUERY
        let go_config = TagsConfiguration::new(
            tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::TAGS_QUERY,
            "",
        )?;
        configs.insert(Language::Go, go_config);
        
        // JavaScript - use built-in TAGS_QUERY
        let js_config = TagsConfiguration::new(
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::TAGS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,  // JS has locals query
        )?;
        configs.insert(Language::JavaScript, js_config);
        
        // TypeScript - use built-in TAGS_QUERY
        let ts_config = TagsConfiguration::new(
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tree_sitter_typescript::TAGS_QUERY,
            tree_sitter_typescript::LOCALS_QUERY,  // TS has locals query
        )?;
        configs.insert(Language::TypeScript, ts_config);
        
        // Java - use built-in TAGS_QUERY
        let java_config = TagsConfiguration::new(
            tree_sitter_java::LANGUAGE.into(),
            tree_sitter_java::TAGS_QUERY,
            "",
        )?;
        configs.insert(Language::Java, java_config);
        
        // C - use built-in TAGS_QUERY
        let c_config = TagsConfiguration::new(
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::TAGS_QUERY,
            "",
        )?;
        configs.insert(Language::C, c_config);
        
        // C++ - use built-in TAGS_QUERY
        let cpp_config = TagsConfiguration::new(
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::TAGS_QUERY,
            "",
        )?;
        configs.insert(Language::Cpp, cpp_config);
        
        // Note: C# and some other parsers may not have TAGS_QUERY constants
        // For those, we'd need to provide custom queries or skip them
        
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
        
        // Parse with tree-sitter to get full ranges
        // Parse with tree-sitter to get full ranges
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
            
            // In tree-sitter-tags, we only have is_definition and syntax_type_id
            // Skip references (only keep definitions)
            if !tag.is_definition {
                continue;
            }
            
            // Get the actual kind string using the config's syntax_type_name method
            // The syntax_type_id is dynamically assigned based on the captures in the query file
            let kind = config.syntax_type_name(tag.syntax_type_id).to_string();
            
            // Extract name from content
            let name = std::str::from_utf8(
                &content.as_bytes()[tag.name_range.start..tag.name_range.end]
            )?
            .to_string();
            
            // Default to tag range (byte offsets converted to lines)
            let mut start_byte = tag.line_range.start;
            let mut end_byte = tag.line_range.end;
            
            // Try to refine range using AST
            if let Some(tree) = &tree {
                if let Some(node) = tree.root_node().descendant_for_byte_range(tag.name_range.start, tag.name_range.end) {
                    // Walk up to find definition
                    let mut curr = node;
                    while let Some(parent) = curr.parent() {
                        if is_definition_node(parent.kind(), language) {
                            start_byte = parent.start_byte();
                            end_byte = parent.end_byte();
                            break;
                        }
                        curr = parent;
                    }
                }
            }
            
            let start_line = byte_to_line(content, start_byte);
            let end_line = byte_to_line(content, end_byte);
            
            // In 0.23, docs is Option<String> not a Range
            let doc_comment = tag.docs;
            
            symbols.push(Symbol {
                id: format!("{}:{}-{}", path.display(), start_line, end_line),
                name: name.clone(),
                kind,
                file_path: PathBuf::from(path),
                start_line,
                end_line,
                fqn: name.clone(), // For now, use simple name; enhance FQN logic later
                language: language.clone(),
                doc_comment,
            });
        }
        
        Ok(symbols)
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
    // 1-indexed line number
    if byte_offset >= content.len() {
        return content.lines().count();
    }
    content[..byte_offset].chars().filter(|&c| c == '\n').count() + 1
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
        
        // Debug: print what we actually got
        eprintln!("Extracted symbols: {:?}", symbols.iter().map(|s| (&s.name, &s.kind)).collect::<Vec<_>>());
        
        // Should extract trait
        // Note: The method signature inside the trait is not extracted as a separate symbol
        // which is correct - it's just a signature, not an implementation
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
        // Lines:
        // 1: empty
        // 2: fn my_func() {
        // 3:     println!("line 1");
        // 4:     println!("line 2");
        // 5: }
        
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
}
