use anyhow::Result;
use emry_core::stack_graphs::loader::Language;
use emry_core::stack_graphs::manager::StackGraphManager;
use tempfile::TempDir;

#[test]
fn test_cross_file_resolution_python() -> Result<()> {
    // 1. Setup
    let temp_dir = TempDir::new()?;
    let root = temp_dir.path();
    let storage_path = root.join("stack_graph.bin");
    let mut manager = StackGraphManager::new(storage_path)?;

    // 2. Create files
    // lib.py: Defines 'greet'
    let lib_path = root.join("lib.py");
    let lib_content = "
def greet(name):
    return 'Hello ' + name
";
    let lib_hash = "hash_lib_v1";

    // main.py: Imports 'greet' and calls it
    let main_path = root.join("main.py");
    let main_content = "
from lib import greet

def main():
    greet('World')
";
    let main_hash = "hash_main_v1";

    // 3. Sync
    let files = vec![
        (lib_path.clone(), lib_content.to_string(), Language::Python, lib_hash.to_string()),
        (main_path.clone(), main_content.to_string(), Language::Python, main_hash.to_string()),
    ];
    
    manager.sync(&files, root)?;

    // 4. Verify Edges
    let edges = manager.extract_call_edges()?;
    
    // We expect an edge from main.py to 'greet' in lib.py
    // Note: stack-graphs resolution might be tricky.
    // The call `greet('World')` in main.py refers to `greet` imported from `lib`.
    // The definition is in `lib.py`.
    
    // Let's print edges for debugging if this fails
    println!("Found edges: {:?}", edges);

    let found_edge = edges.iter().find(|e| {
        e.from_file.ends_with("main.py") && e.to_symbol == "greet" && e.to_file.ends_with("lib.py")
    });

    assert!(found_edge.is_some(), "Failed to find cross-file call edge from main.py to lib.py/greet");

    // 5. Incremental Update: Modify lib.py
    // Rename 'greet' to 'welcome' (breaking change, but let's see if graph updates)
    // Actually, let's keep it compatible but change content to force re-parse.
    // If we rename, the call in main.py becomes invalid unless we update main.py too.
    
    // Scenario: Update both to rename function.
    let lib_content_v2 = "
def welcome(name):
    return 'Welcome ' + name
";
    let lib_hash_v2 = "hash_lib_v2";

    let main_content_v2 = "
from lib import welcome

def main():
    welcome('World')
";
    let main_hash_v2 = "hash_main_v2";

    let files_v2 = vec![
        (lib_path.clone(), lib_content_v2.to_string(), Language::Python, lib_hash_v2.to_string()),
        (main_path.clone(), main_content_v2.to_string(), Language::Python, main_hash_v2.to_string()),
    ];

    manager.sync(&files_v2, root)?;

    let edges_v2 = manager.extract_call_edges()?;
    println!("Found edges v2: {:?}", edges_v2);

    let found_edge_v2 = edges_v2.iter().find(|e| {
        e.from_file.ends_with("main.py") && e.to_symbol == "welcome" && e.to_file.ends_with("lib.py")
    });
    
    assert!(found_edge_v2.is_some(), "Failed to find updated call edge (welcome)");
    
    // Ensure old edge is gone
    let old_edge = edges_v2.iter().find(|e| {
        e.to_symbol == "greet"
    });
    assert!(old_edge.is_none(), "Old symbol 'greet' should not be referenced");

    Ok(())
}
