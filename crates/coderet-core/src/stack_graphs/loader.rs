use tree_sitter_stack_graphs::NoCancellation;
use tree_sitter_stack_graphs::loader::LanguageConfiguration;


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Java,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "rs" => Some(Language::Rust),
            "py" => Some(Language::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            "ts" | "tsx" => Some(Language::TypeScript),
            "java" => Some(Language::Java),
            _ => None,
        }
    }
}

pub struct StackGraphLoader;

impl StackGraphLoader {
    /// Load a language configuration (returns StackGraphLanguage for Rust, LanguageConfiguration for others)
    pub fn load_language_config(language: Language) -> Result<LanguageConfiguration, anyhow::Error> {
        match language {
            Language::Rust => Self::load_rust_as_config(),
            Language::Python => Ok(tree_sitter_stack_graphs_python::language_configuration(&NoCancellation)),
            Language::JavaScript => Ok(tree_sitter_stack_graphs_javascript::language_configuration(&NoCancellation)),
            Language::TypeScript => Ok(tree_sitter_stack_graphs_typescript::language_configuration_tsx(&NoCancellation)),
            Language::Java => Ok(tree_sitter_stack_graphs_java::language_configuration(&NoCancellation)),
        }
    }

    /// Load Rust config (custom since no official crate exists)
    fn load_rust_as_config() -> Result<LanguageConfiguration, anyhow::Error> {
        let tsg_source = include_str!("rules/rust.tsg");
        let language = tree_sitter::Language::new(tree_sitter_rust::LANGUAGE);
        
        Ok(LanguageConfiguration::from_sources(
            language,
            Some(String::from("source.rs")),
            None,
            vec![String::from("rs")],
            "rules/rust.tsg".into(),
            tsg_source,
            None, // No builtins for Rust
            None, // No builtins config
            &NoCancellation,
        )?)
    }


}

#[cfg(test)]
mod tests {
    use super::*;
    use stack_graphs::graph::StackGraph;
    use tree_sitter_graph::Variables;

    #[test]
    fn test_load_rust_rules() {
        let config = StackGraphLoader::load_language_config(Language::Rust).expect("Failed to load Rust rules");
        
        let source = "fn main() {}";
        let mut graph = StackGraph::new();
        let file = graph.get_or_create_file("test.rs");
        let globals = Variables::new();
        
        config.sgl.build_stack_graph_into(&mut graph, file, source, &globals, &NoCancellation)
           .expect("Failed to build graph");
           
        assert!(graph.iter_nodes().count() > 0);
    }

    #[test]
    fn test_load_python_rules() {
        let config = StackGraphLoader::load_language_config(Language::Python).expect("Failed to load Python rules");
        
        let source = "def foo():\n    pass";
        let mut graph = StackGraph::new();
        let file = graph.get_or_create_file("test.py");
        let globals = Variables::new();
        
        config.sgl.build_stack_graph_into(&mut graph, file, source, &globals, &NoCancellation)
           .expect("Failed to build graph");
           
        assert!(graph.iter_nodes().count() > 0);
    }

    #[test]
    fn test_language_from_extension() {
        assert!(matches!(Language::from_extension("rs"), Some(Language::Rust)));
        assert!(matches!(Language::from_extension("py"), Some(Language::Python)));
        assert!(matches!(Language::from_extension("js"), Some(Language::JavaScript)));
        assert!(matches!(Language::from_extension("ts"), Some(Language::TypeScript)));
        assert!(matches!(Language::from_extension("java"), Some(Language::Java)));
        assert!(Language::from_extension("unknown").is_none());
    }
}
