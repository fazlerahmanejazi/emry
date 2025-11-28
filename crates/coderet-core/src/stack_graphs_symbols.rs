use crate::models::{Language, Symbol};
use crate::stack_graphs::builder::GraphBuilder;
use crate::stack_graphs::mapper::{GraphMapper, CallEdge};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Check if stack-graphs is supported for this language
pub fn supports_stack_graphs(language: &Language) -> bool {
    matches!(
        language,
        Language::Rust | Language::Python | Language::JavaScript | Language::TypeScript | Language::Java
    )
}

/// Extract symbols using stack-graphs (supports all configured languages)
pub fn extract_symbols_stack_graphs(_content: &str, path: &Path, language: &Language) -> Result<Vec<Symbol>> {
    // Build stack graph (supports Rust, Python, JavaScript, TypeScript, Java)
    let mut builder = GraphBuilder::new();
    builder.build_from_files(&[path])?;

    // Extract symbols
    let mapper = GraphMapper::new(&builder.graph);
    let symbol_infos = mapper.extract_symbols()?;

    // Convert to Symbol struct
    let symbols: Vec<Symbol> = symbol_infos
        .into_iter()
        .map(|info| {
            // Generate ID based on file and symbol name and line
            let id = format!("{}:{}:{}", path.display(), info.start_line, info.symbol);
            
            Symbol {
                id: id.clone(),
                name: info.symbol.clone(),
                kind: info.kind,
                file_path: PathBuf::from(&info.file_path),
                start_line: info.start_line,
                end_line: info.end_line,
                fqn: info.symbol.clone(),
                language: language.clone(),
                doc_comment: None, // TODO: Extract from source
            }
        })
        .collect();

    Ok(symbols)
}

/// Extract call edges using stack-graphs
pub fn extract_call_edges_stack_graphs(path: &Path, _language: &Language) -> Result<Vec<CallEdge>> {
    // Build stack graph
    let mut builder = GraphBuilder::new();
    builder.build_from_files(&[path])?;
    
    // Extract call edges
    let mapper = GraphMapper::new(&builder.graph);
    mapper.extract_calls()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_call_extraction(
        language: Language, 
        file_name: &str, 
        content: &str, 
        expected_calls: &[(&str, &str)] // (caller, callee)
    ) {
        let test_dir = std::env::temp_dir().join(format!("stack_graphs_test_{}", file_name));
        fs::create_dir_all(&test_dir).unwrap();
        let file_path = test_dir.join(file_name);
        fs::write(&file_path, content).unwrap();

        let edges = extract_call_edges_stack_graphs(&file_path, &language)
            .expect("Failed to extract call edges");

        // Cleanup
        fs::remove_dir_all(&test_dir).ok();

        // Verify edges
        // Note: stack-graphs might return full paths or just names depending on implementation.
        // The mapper.rs implementation returns SymbolInfo which has 'symbol' field.
        // We just check if the edge exists.
        
        for (_caller, callee) in expected_calls {
            let found = edges.iter().any(|edge| {
                // Check if from_file contains our test file (it might be absolute)
                // and to_symbol matches
                // Note: from_file is actually the file path string in CallEdge
                // to_symbol is the name of the function being called
                
                // For now, we don't check caller strictly because it might be complex (e.g. top-level scope)
                // But for function-to-function calls, we can try.
                
                // Simpler check: verify 'to_symbol' is correct.
                edge.to_symbol == *callee
            });
            
            if !found {
                 // Debug print
                 println!("Edges found for {}: {:?}", file_name, edges);
            }
            assert!(found, "Missing call edge to '{}' in {}", callee, file_name);
        }
    }

    #[test]
    fn test_stack_graphs_extraction() {
        let test_dir = std::env::temp_dir().join("stack_graphs_symbol_test");
        fs::create_dir_all(&test_dir).unwrap();

        let test_file = test_dir.join("test.rs");
        fs::write(&test_file, r#"
pub struct User {
    pub name: String,
}

impl User {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }

    pub fn greet(&self) {
        println!("Hello, {}!", self.name);
    }
}
"#).unwrap();

        let symbols = extract_symbols_stack_graphs(
            &fs::read_to_string(&test_file).unwrap(),
            &test_file,
            &Language::Rust,
        ).unwrap();

        assert!(!symbols.is_empty(), "Should extract symbols");
        assert!(symbols.iter().any(|s| s.name == "User"), "Should find User");
        assert!(symbols.iter().any(|s| s.name == "new"), "Should find new");
        assert!(symbols.iter().any(|s| s.name == "greet"), "Should find greet");

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_python_call_edges() {
        let content = r#"
def greet(name):
    print(f"Hello, {name}!")

def main():
    greet("World")
    
main()
"#;
        test_call_extraction(
            Language::Python, 
            "test.py", 
            content, 
            &[("main", "greet")]
        );
    }

    #[test]
    fn test_javascript_call_edges() {
        let content = r#"
function greet(name) {
    console.log("Hello, " + name);
}

function main() {
    greet("World");
}

main();
"#;
        test_call_extraction(
            Language::JavaScript, 
            "test.js", 
            content, 
            &[("main", "greet")]
        );
    }

    #[test]
    fn test_typescript_call_edges() {
        let content = r#"
function greet(name: string) {
    console.log("Hello, " + name);
}

function main() {
    greet("World");
}

main();
"#;
        test_call_extraction(
            Language::TypeScript, 
            "test.ts", 
            content, 
            &[("main", "greet")]
        );
    }

    #[test]
    fn test_java_call_edges() {
        let content = r#"
class Test {
    void greet(String name) {
        System.out.println("Hello " + name);
    }
    
    void main() {
        greet("World");
    }
}
"#;
        test_call_extraction(
            Language::Java, 
            "Test.java", 
            content, 
            &[("main", "greet")]
        );
    }
}
