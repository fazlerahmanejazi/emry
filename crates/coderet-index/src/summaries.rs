use anyhow::{anyhow, Result};
use arrow::array::{ArrayRef, Float32Array, RecordBatch, StringArray, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};
use coderet_config::SummaryLevel;
use coderet_core::models::{Language, Summary};
use coderet_core::traits::Embedder;
use futures::stream::TryStreamExt;
use lance::dataset::{Dataset, WriteMode, WriteParams};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize}; // Added import

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] // Added Serialize, Deserialize
pub enum SummaryKind {
    Repo,
    Module,
    File,
    Symbol,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)] // Added Serialize, Deserialize
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

/// LanceDB-backed summary index for scalable semantic search.
pub struct SummaryIndex {
    dataset: Option<Dataset>,
    index_path: std::path::PathBuf,
}

impl SummaryIndex {
    pub async fn new(path: &Path) -> Result<Self> {
        let dataset = if path.exists() {
            match Dataset::open(path.to_str().unwrap()).await {
                Ok(ds) => Some(ds),
                Err(_) => None,
            }
        } else {
            None
        };

        Ok(Self {
            dataset,
            index_path: path.to_path_buf(),
        })
    }

    /// Expose the dataset for read-only consumers (e.g., structured wrappers).
    pub fn public_dataset(&self) -> Option<&Dataset> {
        self.dataset.as_ref()
    }

    pub async fn add_summaries(&mut self, summaries: &[Summary]) -> Result<()> {
        let with_emb: Vec<&Summary> = summaries
            .iter()
            .filter(|s| s.embedding.as_ref().map(|e| !e.is_empty()).unwrap_or(false))
            .collect();

        if with_emb.is_empty() {
            return Ok(());
        }

        let embedding_dim = with_emb[0].embedding.as_ref().unwrap().len();
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("target_id", DataType::Utf8, false),
            Field::new("file_path", DataType::Utf8, true),
            Field::new("start_line", DataType::UInt64, true),
            Field::new("end_line", DataType::UInt64, true),
            Field::new("module", DataType::Utf8, true),
            Field::new("language", DataType::Utf8, true),
            Field::new("level", DataType::Utf8, true),
            Field::new("text", DataType::Utf8, false),
            Field::new(
                "embedding",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    embedding_dim as i32,
                ),
                false,
            ),
        ]));

        let ids: Vec<String> = with_emb.iter().map(|s| s.id.clone()).collect();
        let target_ids: Vec<String> = with_emb.iter().map(|s| s.target_id.clone()).collect();
        let file_paths: Vec<Option<String>> = with_emb
            .iter()
            .map(|s| {
                s.file_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
            })
            .collect();
        let start_lines: Vec<Option<u64>> = with_emb
            .iter()
            .map(|s| s.start_line.map(|v| v as u64))
            .collect();
        let end_lines: Vec<Option<u64>> = with_emb
            .iter()
            .map(|s| s.end_line.map(|v| v as u64))
            .collect();
        let modules: Vec<Option<String>> = with_emb.iter().map(|s| s.module.clone()).collect();
        let languages: Vec<Option<String>> = with_emb.iter().map(|s| s.language.clone()).collect();
        let levels: Vec<Option<String>> = with_emb
            .iter()
            .map(|s| Some(format!("{:?}", s.level)))
            .collect();
        let texts: Vec<String> = with_emb.iter().map(|s| s.text.clone()).collect();
        let embeddings: Vec<f32> = with_emb
            .iter()
            .flat_map(|s| s.embedding.as_ref().unwrap().clone())
            .collect();

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(ids)) as ArrayRef,
                Arc::new(StringArray::from(target_ids)),
                Arc::new(StringArray::from(file_paths)) as ArrayRef,
                Arc::new(UInt64Array::from(start_lines)) as ArrayRef,
                Arc::new(UInt64Array::from(end_lines)) as ArrayRef,
                Arc::new(StringArray::from(modules)) as ArrayRef,
                Arc::new(StringArray::from(languages)) as ArrayRef,
                Arc::new(StringArray::from(levels)) as ArrayRef,
                Arc::new(StringArray::from(texts)) as ArrayRef,
                {
                    let values = Float32Array::from(embeddings);
                    let field = Arc::new(Field::new("item", DataType::Float32, true));
                    Arc::new(arrow::array::FixedSizeListArray::new(
                        field,
                        embedding_dim as i32,
                        Arc::new(values),
                        None,
                    )) as ArrayRef
                },
            ],
        )?;

        let mode = if self.dataset.is_some() {
            WriteMode::Append
        } else {
            WriteMode::Create
        };

        let reader = arrow::array::RecordBatchIterator::new(vec![Ok(batch)].into_iter(), schema);
        let dataset = Dataset::write(
            reader,
            self.index_path.to_str().unwrap(),
            Some(WriteParams {
                mode,
                ..Default::default()
            }),
        )
        .await?;

        self.dataset = Some(dataset);
        Ok(())
    }

    pub async fn remove_targets(&mut self, targets: &[String]) -> Result<()> {
        if targets.is_empty() {
            return Ok(());
        }
        let dataset = match self.dataset.as_mut() {
            Some(ds) => ds,
            None => return Ok(()),
        };

        let mut parts = Vec::new();
        for t in targets {
            let escaped = t.replace('\'', "''");
            parts.push(format!("'{}'", escaped));
        }
        let predicate = format!("target_id IN ({})", parts.join(","));
        dataset.delete(&predicate).await?;
        Ok(())
    }

    pub async fn remove_by_files(&mut self, files: &[std::path::PathBuf]) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let dataset = match self.dataset.as_mut() {
            Some(ds) => ds,
            None => return Ok(()),
        };

        let mut parts = Vec::new();
        for f in files {
            let escaped = f.to_string_lossy().replace('\'', "''");
            parts.push(format!("'{}'", escaped));
        }
        let predicate = format!("file_path IN ({})", parts.join(","));
        dataset.delete(&predicate).await?;
        Ok(())
    }

    pub async fn semantic_search(
        &self,
        query: &str,
        embedder: &dyn Embedder,
        limit: usize,
    ) -> Result<Vec<(f32, Summary)>> {
        let dataset = match self.dataset.as_ref() {
            Some(ds) => ds,
            None => return Ok(Vec::new()),
        };

        let query_vec = embedder.embed(query).await?;
        let query_array = Float32Array::from(query_vec.clone());

        let results = dataset
            .scan()
            .nearest("embedding", &query_array, limit)?
            .try_into_stream()
            .await?;

        let mut batches = Vec::new();
        let mut stream = results;
        while let Some(batch) = stream.try_next().await? {
            batches.push(batch);
        }

        let mut out = Vec::new();
        for batch in batches {
            let ids = batch
                .column_by_name("id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let target_ids = batch
                .column_by_name("target_id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let file_paths = batch
                .column_by_name("file_path")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let start_lines = batch
                .column_by_name("start_line")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();
            let end_lines = batch
                .column_by_name("end_line")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();
            let levels = batch
                .column_by_name("level")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let texts = batch
                .column_by_name("text")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let distances = batch
                .column_by_name("_distance")
                .unwrap()
                .as_any()
                .downcast_ref::<Float32Array>()
                .unwrap();

            for i in 0..batch.num_rows() {
                let distance = distances.value(i);
                let similarity = 1.0 / (1.0 + distance);
                let level_str = levels.value(i).to_string();
                let level = match level_str.to_lowercase().as_str() {
                    "function" => SummaryLevel::Function,
                    "class" => SummaryLevel::Class,
                    "file" => SummaryLevel::File,
                    "module" => SummaryLevel::Module,
                    "repo" => SummaryLevel::Repo,
                    _ => SummaryLevel::File,
                };

                let summary = Summary {
                    id: ids.value(i).to_string(),
                    target_id: target_ids.value(i).to_string(),
                    level,
                    text: texts.value(i).to_string(),
                    file_path: optional_string(&file_paths, i).map(std::path::PathBuf::from),
                    start_line: Some(start_lines.value(i) as usize),
                    end_line: Some(end_lines.value(i) as usize),
                    name: None,
                    language: optional_column_string(&batch, "language", i),
                    module: optional_column_string(&batch, "module", i),
                    model: None,
                    prompt_version: None,
                    generated_at: None,
                    source_hash: None,
                    embedding: None,
                    canonical_target_id: None,
                };
                out.push((similarity, summary));
            }
        }

        out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(limit);
        Ok(out)
    }

    /// List summaries filtered by level (e.g., repo/module) up to `limit`.
    pub async fn list_by_level(
        &self,
        levels: &[SummaryLevel],
        limit: usize,
    ) -> Result<Vec<Summary>> {
        let mut out = Vec::new();
        let dataset = match self.dataset.as_ref() {
            Some(ds) => ds,
            None => return Ok(out),
        };
        let mut stream = dataset.scan().try_into_stream().await?;
        let level_set: std::collections::HashSet<String> =
            levels.iter().map(|l| format!("{:?}", l)).collect();

        while let Some(batch) = stream.try_next().await? {
            let ids = batch
                .column_by_name("id")
                .ok_or_else(|| anyhow!("missing id"))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow!("failed to cast id column"))?;
            let target_ids = batch
                .column_by_name("target_id")
                .ok_or_else(|| anyhow!("missing target_id"))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow!("failed to cast target_id column"))?;
            let file_paths = batch
                .column_by_name("file_path")
                .ok_or_else(|| anyhow!("missing file_path"))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow!("failed to cast file_path column"))?;
            let start_lines = batch
                .column_by_name("start_line")
                .ok_or_else(|| anyhow!("missing start_line"))?
                .as_any()
                .downcast_ref::<UInt64Array>()
                .ok_or_else(|| anyhow!("failed to cast start_line column"))?;
            let end_lines = batch
                .column_by_name("end_line")
                .ok_or_else(|| anyhow!("missing end_line"))?
                .as_any()
                .downcast_ref::<UInt64Array>()
                .ok_or_else(|| anyhow!("failed to cast end_line column"))?;
            let levels_col = batch
                .column_by_name("level")
                .ok_or_else(|| anyhow!("missing level"))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow!("failed to cast level column"))?;
            let texts = batch
                .column_by_name("text")
                .ok_or_else(|| anyhow!("missing text"))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow!("failed to cast text column"))?;

            for i in 0..batch.num_rows() {
                let level_str = levels_col.value(i);
                if !level_set.contains(level_str) {
                    continue;
                }
                let level = match level_str.to_lowercase().as_str() {
                    "function" => SummaryLevel::Function,
                    "class" => SummaryLevel::Class,
                    "file" => SummaryLevel::File,
                    "module" => SummaryLevel::Module,
                    "repo" => SummaryLevel::Repo,
                    _ => SummaryLevel::File,
                };
                let file_path = file_paths
                    .value(i)
                    .is_empty()
                    .then(|| None)
                    .unwrap_or_else(|| Some(std::path::PathBuf::from(file_paths.value(i))));
                out.push(Summary {
                    id: ids.value(i).to_string(),
                    target_id: target_ids.value(i).to_string(),
                    level,
                    text: texts.value(i).to_string(),
                    file_path,
                    start_line: Some(start_lines.value(i) as usize),
                    end_line: Some(end_lines.value(i) as usize),
                    name: None,
                    language: optional_column_string(&batch, "language", i),
                    module: optional_column_string(&batch, "module", i),
                    model: None,
                    prompt_version: None,
                    generated_at: None,
                    source_hash: None,
                    embedding: None,
                    canonical_target_id: None,
                });
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<Summary>> {
        let dataset = match self.dataset.as_ref() {
            Some(ds) => ds,
            None => return Ok(Vec::new()),
        };
        let mut out = Vec::new();
        let mut stream = dataset.scan().try_into_stream().await?;
        let needle = query.to_lowercase();
        while let Some(batch) = stream.try_next().await? {
            let texts = batch
                .column_by_name("text")
                .ok_or_else(|| anyhow!("missing text column"))?
                .as_any()
                .downcast_ref::<arrow::array::StringArray>()
                .ok_or_else(|| anyhow!("failed to cast text column"))?;
            for i in 0..batch.num_rows() {
                let text = texts.value(i).to_lowercase();
                if !text.contains(&needle) {
                    continue;
                }
                out.push(summary_from_batch_row(&batch, i)?);
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }

    pub async fn search_structured(
        &self,
        query: &str,
        limit: usize,
        embedder: Option<&dyn Embedder>,
    ) -> Result<Vec<(f32, SummaryRecord)>> {
        if let Some(e) = embedder {
            let semantic = self.semantic_search(query, e, limit).await?;
            return Ok(semantic
                .into_iter()
                .map(|(score, summary)| (score, SummaryRecord::from(summary)))
                .collect());
        }
        let fallback = self.search(query, limit).await?;
        Ok(fallback
            .into_iter()
            .map(|summary| (0.0, SummaryRecord::from(summary)))
            .collect())
    }

    pub async fn get_repo_and_module_summaries(&self, limit: usize) -> Result<Vec<SummaryRecord>> {
        let dataset = match self.dataset.as_ref() {
            Some(ds) => ds,
            None => return Ok(Vec::new()),
        };
        let mut out = Vec::new();
        let mut stream = dataset.scan().try_into_stream().await?;
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
                out.push(SummaryRecord::from(summary_from_batch_row(&batch, i)?));
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }
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
        SummaryRecord {
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
        .ok_or_else(|| anyhow!("missing level column"))?
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .ok_or_else(|| anyhow!("failed to cast level column"))?;
    let texts = batch
        .column_by_name("text")
        .ok_or_else(|| anyhow!("missing text column"))?
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .ok_or_else(|| anyhow!("failed to cast text column"))?;
    let language = optional_column_string(batch, "language", i);
    let module = optional_column_string(batch, "module", i);

    let level_str = levels.value(i).to_string();
    let level = match level_str.to_lowercase().as_str() {
        "function" => SummaryLevel::Function,
        "class" => SummaryLevel::Class,
        "file" => SummaryLevel::File,
        "module" => SummaryLevel::Module,
        "repo" => SummaryLevel::Repo,
        _ => SummaryLevel::File,
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

fn optional_string(arr: &arrow::array::StringArray, i: usize) -> Option<String> {
    let value = arr.value(i);
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
