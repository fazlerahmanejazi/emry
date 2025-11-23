use crate::structure::symbols::Symbol;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SymbolIndex {
    // Map from symbol name to list of symbols (since names can be duplicated across files/classes)
    pub symbols: HashMap<String, Vec<Symbol>>,
    #[serde(skip)]
    path: PathBuf,
}

impl SymbolIndex {
    pub fn new(path: &Path) -> Self {
        // Try to load existing index
        if path.exists() {
            if let Ok(index) = Self::load(path) {
                return index;
            }
        }

        Self {
            symbols: HashMap::new(),
            path: path.to_path_buf(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut index: SymbolIndex = serde_json::from_reader(reader)?;
        index.path = path.to_path_buf();
        Ok(index)
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(&self.path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer(writer, &self)?;
        Ok(())
    }

    pub fn add_symbols(&mut self, new_symbols: Vec<Symbol>) {
        for symbol in new_symbols {
            self.symbols
                .entry(symbol.name.clone())
                .or_default()
                .push(symbol);
        }
    }

    pub fn search(&self, query: &str) -> Vec<&Symbol> {
        // Exact match first
        if let Some(matches) = self.symbols.get(query) {
            return matches.iter().collect();
        }

        // TODO: Fuzzy match or partial match
        // For now, just return empty if no exact match
        Vec::new()
    }

    // Clear index (for full rebuild)
    pub fn clear(&mut self) {
        self.symbols.clear();
    }
}
