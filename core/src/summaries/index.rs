use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SummaryLevel {
    Function,
    Class,
    File,
    Module,
    Repo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub level: SummaryLevel,
    pub target_id: String, // Symbol ID or File Path
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SummaryIndex {
    pub summaries: HashMap<String, Summary>, // Map ID -> Summary
    #[serde(skip)]
    path: PathBuf,
}

impl SummaryIndex {
    pub fn new(path: &Path) -> Self {
        if path.exists() {
            if let Ok(index) = Self::load(path) {
                return index;
            }
        }
        Self {
            summaries: HashMap::new(),
            path: path.to_path_buf(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut index: SummaryIndex = serde_json::from_reader(reader)?;
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

    pub fn add_summary(&mut self, summary: Summary) {
        self.summaries.insert(summary.id.clone(), summary);
    }

    pub fn get_summary(&self, id: &str) -> Option<&Summary> {
        self.summaries.get(id)
    }
    
    pub fn clear(&mut self) {
        self.summaries.clear();
    }
}
