use core::config::IndexConfig;
use core::index::manager::IndexManager;
use core::scanner::scan_repo;
use core::structure::graph::{CodeGraph, GraphBuilder, EdgeType, NodeId};
use core::structure::symbols::{Symbol, SymbolKind};
use core::models::{Language, IndexMetadata};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn scan_respects_include_exclude() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src = root.join("src");
    let vendor = root.join("vendor");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&vendor).unwrap();
    fs::write(src.join("main.py"), "print('ok')").unwrap();
    fs::write(vendor.join("skip.py"), "print('skip')").unwrap();
    fs::write(src.join("note.txt"), "ignore").unwrap();

    let mut cfg = IndexConfig::default();
    cfg.include_paths = vec!["**/*.py".to_string()];
    cfg.exclude_paths = vec!["vendor/**".to_string()];

    let files = scan_repo(root, &cfg);
    let paths: HashSet<_> = files.into_iter().map(|f| f.path).collect();
    assert!(paths.contains(&src.join("main.py")));
    assert!(!paths.contains(&vendor.join("skip.py")));
    assert!(!paths.contains(&src.join("note.txt")));
}

#[test]
fn index_manager_save_load_roundtrip() {
    let dir = tempdir().unwrap();
    let meta_path = dir.path().join("meta.json");
    let mut meta = IndexMetadata::default();
    let file_path = PathBuf::from("src/foo.py");
    IndexManager::update_file_entry(&mut meta, &file_path, "hash123".into(), vec!["c1".into()]);
    IndexManager::save(&meta_path, &meta).unwrap();

    let loaded = IndexManager::load(&meta_path);
    assert_eq!(loaded.version, "1");
    assert_eq!(loaded.files.len(), 1);
    assert_eq!(loaded.files[0].path, file_path);
    assert_eq!(loaded.files[0].content_hash, "hash123");
    assert_eq!(loaded.files[0].chunk_ids, vec!["c1".to_string()]);
}

#[test]
fn graph_builder_adds_calls_and_imports() {
    let dir = tempdir().unwrap();
    let file_a = dir.path().join("a.py");
    let file_b = dir.path().join("other.py");

    let content_a = r#"
import other
def foo():
    bar()
"#;
    let content_b = r#"
def bar():
    pass
"#;
    fs::write(&file_a, content_a).unwrap();
    fs::write(&file_b, content_b).unwrap();

    let sym_foo = Symbol {
        id: format!("{}:foo:2", file_a.to_string_lossy()),
        name: "foo".into(),
        kind: SymbolKind::Function,
        language: Language::Python,
        file_path: file_a.clone(),
        start_line: 2,
        end_line: 4,
        chunk_id: None,
    };
    let sym_bar = Symbol {
        id: format!("{}:bar:1", file_b.to_string_lossy()),
        name: "bar".into(),
        kind: SymbolKind::Function,
        language: Language::Python,
        file_path: file_b.clone(),
        start_line: 1,
        end_line: 2,
        chunk_id: None,
    };
    let symbols = vec![sym_foo.clone(), sym_bar.clone()];

    let mut graph = CodeGraph::new(dir.path().join("graph.json").as_path());
    GraphBuilder::build(&mut graph, &symbols);
    GraphBuilder::build_calls_and_imports(
        &mut graph,
        &symbols,
        &vec![
            (file_a.clone(), Language::Python, content_a.to_string()),
            (file_b.clone(), Language::Python, content_b.to_string()),
        ],
    );

    // DefinedIn edges and call/import edges
    let mut calls = 0;
    let mut imports = 0;
    for e in &graph.edges {
        if e.kind == EdgeType::Calls {
            calls += 1;
            assert_eq!(e.source, NodeId(sym_foo.id.clone()));
            assert_eq!(e.target, NodeId(sym_bar.id.clone()));
        }
        if e.kind == EdgeType::Imports {
            imports += 1;
        }
    }
    assert!(calls >= 1);
    assert!(imports >= 1);
}
