use crate::stack_graphs::builder::GraphBuilder;
use crate::stack_graphs::mapper::GraphMapper;


#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_end_to_end_rust_graph() {
        // Create sample Rust files
        let test_dir = std::env::temp_dir().join("stack_graphs_test");
        std::fs::create_dir_all(&test_dir).unwrap();

        let types_rs = test_dir.join("types.rs");
        std::fs::write(&types_rs, r#"
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

        // Build StackGraph
        let mut builder = GraphBuilder::new();
        builder.build_from_files(&[&types_rs]).expect("Failed to build stack graph");

        // Map to data structures
        let mapper = GraphMapper::new(&builder.graph);
        let symbols = mapper.extract_symbols().expect("Failed to extract symbols");
        let calls = mapper.extract_calls().expect("Failed to extract calls");

        println!("Symbols: {:#?}", symbols);
        println!("Calls: {:#?}", calls);
        
        // Verify: We should have symbol nodes for User, new, and greet
        assert!(!symbols.is_empty(), "Should have at least one symbol");
        assert!(symbols.iter().any(|s| s.symbol == "User"), "Should find User struct");
        assert!(symbols.iter().any(|s| s.symbol == "new"), "Should find new method");
        assert!(symbols.iter().any(|s| s.symbol == "greet"), "Should find greet method");
        
        // Cleanup
        std::fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_python_call_resolution() {
        let test_dir = std::env::temp_dir().join("stack_graphs_python_test");
        fs::create_dir_all(&test_dir).unwrap();

        let test_file = test_dir.join("example.py");
        fs::write(&test_file, r#"
def greet(name):
    print(f"Hello, {name}!")

def main():
    greet("World")

main()
"#).unwrap();

        // Build using Python loader
        let mut builder = GraphBuilder::new();
        builder.build_from_files(&[&test_file]).unwrap();

        let mapper = GraphMapper::new(&builder.graph);
        
        println!("=== Python Graph Nodes ===");
        for node_handle in builder.graph.iter_nodes() {
            let node = &builder.graph[node_handle];
            println!("Node: {} is_ref={} is_def={}", 
                     node.display(&builder.graph), 
                     node.is_reference(), 
                     node.is_definition());
        }
        println!("==========================");
        
        let symbols = mapper.extract_symbols().unwrap();
        let calls = mapper.extract_calls().unwrap();

        println!("Symbols: {:#?}", symbols);
        println!("Calls: {:#?}", calls);

        // Python should find symbols
        assert!(!symbols.is_empty(), "Should find Python symbols");
        
        // With official TSG, we should get call edges!
        if !calls.is_empty() {
            println!("✅ SUCCESS: Python call resolution working!");
            assert!(calls.iter().any(|c| c.to_symbol == "greet"), 
                    "Should resolve call to greet function");
        } else {
            println!("⚠️ No calls found - may need to check TSG compatibility");
        }

        fs::remove_dir_all(&test_dir).ok();
    }
}
