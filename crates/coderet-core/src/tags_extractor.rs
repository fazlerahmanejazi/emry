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
            
            // Use line_range directly (it's already in line numbers, 0-indexed)
            let start_line = tag.line_range.start + 1;  // Convert to 1-indexed
            let end_line = tag.line_range.end + 1;
            
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
class CodeRetriever:
    def search(self, query):
        pass
        "#;
        
        let mut extractor = TagsExtractor::new().unwrap();
        let symbols = extractor.extract_symbols(
            code,
            Path::new("test.py"),
            &Language::Python,
        ).unwrap();
        
        assert!(symbols.iter().any(|s| s.name == "CodeRetriever"));
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
}
