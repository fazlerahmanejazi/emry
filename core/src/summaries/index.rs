use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

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
    #[serde(default)]
    pub file_path: Option<std::path::PathBuf>,
    #[serde(default)]
    pub start_line: Option<usize>,
    #[serde(default)]
    pub end_line: Option<usize>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt_version: Option<String>,
    #[serde(default)]
    pub generated_at: Option<u64>,
    #[serde(default)]
    pub source_hash: Option<String>,
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
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

    pub fn semantic_search(
        &self,
        query: &str,
        embedder: &dyn crate::embeddings::Embedder,
        top: usize,
    ) -> anyhow::Result<Vec<(f32, &Summary)>> {
        let query_emb = embedder.embed(&[query.to_string()])?.pop().unwrap_or_default();
        if query_emb.is_empty() {
            return Ok(Vec::new());
        }
        let mut scored = Vec::new();
        for summary in self.summaries.values() {
            if let Some(emb) = &summary.embedding {
                let score = dot(&query_emb, emb);
                scored.push((score, summary));
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top);
        Ok(scored)
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    a.iter().zip(b.iter()).take(len).map(|(x, y)| x * y).sum()
}
