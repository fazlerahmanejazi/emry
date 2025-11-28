use std::path::Path;
use std::collections::HashMap;
use anyhow::{Result, Context};
use stack_graphs::graph::StackGraph;
use tree_sitter_stack_graphs::NoCancellation;
use tree_sitter_graph::Variables;
use crate::stack_graphs::loader::{StackGraphLoader, Language};

// FILE_PATH global variable name (standard across stack-graphs)
const FILE_PATH_VAR: &str = "FILE_PATH";

pub struct GraphBuilder {
    pub graph: StackGraph,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self {
            graph: StackGraph::new(),
        }
    }

    pub fn build_from_files(&mut self, files: &[&Path]) -> Result<()> {
        // Group files by language to load each config only once
        let mut files_by_lang: HashMap<Language, Vec<&Path>> = HashMap::new();
        
        for file_path in files {
            if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
                if let Some(language) = Language::from_extension(ext) {
                    files_by_lang.entry(language).or_default().push(file_path);
                }
            }
        }

        // Process each language group
        for (language, paths) in files_by_lang {
            // Load language configuration ONCE per language
            let config = StackGraphLoader::load_language_config(language)
                .with_context(|| format!("Failed to load configuration for {:?}", language))?;
            
            // Merge builtins into main graph (includes stdlib symbols!)
            self.graph.add_from_graph(&config.builtins).ok();
            
            // Process all files of this language
            for file_path in paths {
                let source = std::fs::read_to_string(file_path)
                    .with_context(|| format!("Failed to read file: {:?}", file_path))?;
                
                let file_handle = self.graph.get_or_create_file(file_path.to_string_lossy().as_ref());
                
                // Create globals with FILE_PATH set
                let mut globals = Variables::new();
                globals.add(FILE_PATH_VAR.into(), file_path.to_string_lossy().as_ref().into())
                    .unwrap_or_default();
                
                // Build stack graph with proper globals
                config.sgl.build_stack_graph_into(
                    &mut self.graph,
                    file_handle,
                    &source,
                    &globals,
                    &NoCancellation,
                ).with_context(|| format!("Failed to build stack graph for {:?}", file_path))?;
            }
        }
        
        Ok(())
    }
}
