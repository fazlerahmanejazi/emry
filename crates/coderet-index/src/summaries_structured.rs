use crate::summaries::SummaryIndex;
use anyhow::{anyhow, Result};
use coderet_core::models::{Language, Summary};
use coderet_core::traits::Embedder;
use futures::stream::TryStreamExt;
use std::path::Path;

/// Structured search helpers for summaries. This wraps the existing SummaryIndex to return
/// richer typed results (still using the same underlying dataset).
pub struct StructuredSummaryIndex {
    inner: SummaryIndex,
}

impl StructuredSummaryIndex {
    pub async fn new(path: &Path) -> Result<Self> {
        Ok(Self {
            inner: SummaryIndex::new(path).await?,
        })
    }

    pub async fn semantic_search(
        &self,
        query: &str,
        embedder: &dyn Embedder,
        limit: usize,
    ) -> Result<Vec<(f32, SummaryRecord)>> {
        let results = self.inner.semantic_search(query, embedder, limit).await?;
        Ok(results
            .into_iter()
            .map(|(score, summary)| (score, SummaryRecord::from(summary)))
            .collect())
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SummaryRecord>> {
        let results = self.inner.search(query, limit).await?;
        Ok(results.into_iter().map(SummaryRecord::from).collect())
    }

    pub async fn get_repo_and_module_summaries(&self, limit: usize) -> Result<Vec<SummaryRecord>> {
        // naive: full scan; acceptable for small datasets in current scope
        let mut out = Vec::new();
        if let Some(ds) = self.inner.public_dataset() {
            let mut stream = ds.scan().try_into_stream().await?;
            while let Some(batch) = stream.try_next().await? {
                let levels = batch
                    .column_by_name("level")
                    .ok_or_else(|| anyhow!("missing level column"))?
                    .as_any()
                    .downcast_ref::<arrow::array::StringArray>()
                    .ok_or_else(|| anyhow!("failed to cast level column"))?;
                for i in 0..batch.num_rows() {
                    let level = levels.value(i).to_lowercase();
                    if level != "repo" && level != "module" {
                        continue;
                    }
                    let summary = summary_from_batch_row(&batch, i)?;
                    out.push(SummaryRecord::from(summary));
                    if out.len() >= limit {
                        return Ok(out);
                    }
                }
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct SummaryRecord {
    pub kind: SummaryKind,
    pub target_id: String,
    pub file_path: Option<std::path::PathBuf>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub language: Option<Language>,
    pub module: Option<String>,
    pub text: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryKind {
    Repo,
    Module,
    File,
    Symbol,
    Unknown,
}

impl From<Summary> for SummaryRecord {
    fn from(s: Summary) -> Self {
        let kind = match s.level {
            coderet_config::SummaryLevel::Repo => SummaryKind::Repo,
            coderet_config::SummaryLevel::Module => SummaryKind::Module,
            coderet_config::SummaryLevel::File => SummaryKind::File,
            coderet_config::SummaryLevel::Function | coderet_config::SummaryLevel::Class => {
                SummaryKind::Symbol
            }
        };
        Self {
            kind,
            target_id: s.target_id.clone(),
            file_path: s.file_path.clone(),
            start_line: s.start_line,
            end_line: s.end_line,
            language: s.language.as_deref().map(Language::from_name),
            module: s.module.clone(),
            text: s.text.clone(),
            name: s.name.clone(),
        }
    }
}

fn summary_from_batch_row(batch: &arrow::array::RecordBatch, i: usize) -> Result<Summary> {
    let ids = batch
        .column_by_name("id")
        .ok_or_else(|| anyhow!("missing id"))?
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .ok_or_else(|| anyhow!("failed to cast id column"))?;
    let target_ids = batch
        .column_by_name("target_id")
        .ok_or_else(|| anyhow!("missing target_id"))?
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .ok_or_else(|| anyhow!("failed to cast target_id column"))?;
    let file_paths = batch
        .column_by_name("file_path")
        .ok_or_else(|| anyhow!("missing file_path"))?
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .ok_or_else(|| anyhow!("failed to cast file_path column"))?;
    let start_lines = batch
        .column_by_name("start_line")
        .ok_or_else(|| anyhow!("missing start_line"))?
        .as_any()
        .downcast_ref::<arrow::array::UInt64Array>()
        .ok_or_else(|| anyhow!("failed to cast start_line column"))?;
    let end_lines = batch
        .column_by_name("end_line")
        .ok_or_else(|| anyhow!("missing end_line"))?
        .as_any()
        .downcast_ref::<arrow::array::UInt64Array>()
        .ok_or_else(|| anyhow!("failed to cast end_line column"))?;
    let levels = batch
        .column_by_name("level")
        .ok_or_else(|| anyhow!("missing level"))?
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .ok_or_else(|| anyhow!("failed to cast level column"))?;
    let texts = batch
        .column_by_name("text")
        .ok_or_else(|| anyhow!("missing text"))?
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .ok_or_else(|| anyhow!("failed to cast text column"))?;
    let language = optional_column_string(batch, "language", i);
    let module = optional_column_string(batch, "module", i);

    let level_str = levels.value(i).to_string();
    let level = match level_str.to_lowercase().as_str() {
        "function" => coderet_config::SummaryLevel::Function,
        "class" => coderet_config::SummaryLevel::Class,
        "file" => coderet_config::SummaryLevel::File,
        "module" => coderet_config::SummaryLevel::Module,
        "repo" => coderet_config::SummaryLevel::Repo,
        _ => coderet_config::SummaryLevel::File,
    };

    let file_path = file_paths
        .value(i)
        .is_empty()
        .then(|| None)
        .unwrap_or_else(|| Some(std::path::PathBuf::from(file_paths.value(i))));

    Ok(Summary {
        id: ids.value(i).to_string(),
        target_id: target_ids.value(i).to_string(),
        level,
        text: texts.value(i).to_string(),
        file_path,
        start_line: Some(start_lines.value(i) as usize),
        end_line: Some(end_lines.value(i) as usize),
        name: None,
        language,
        module,
        model: None,
        prompt_version: None,
        generated_at: None,
        source_hash: None,
        embedding: None,
        canonical_target_id: None,
    })
}

fn optional_column_string(
    batch: &arrow::array::RecordBatch,
    name: &str,
    i: usize,
) -> Option<String> {
    let col = batch.column_by_name(name)?;
    let arr = col.as_any().downcast_ref::<arrow::array::StringArray>()?;
    let value = arr.value(i);
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
