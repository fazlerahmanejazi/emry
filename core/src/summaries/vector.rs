use anyhow::Result;
use arrow::array::{ArrayRef, Float32Array, RecordBatch, StringArray, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};
use lance::dataset::{Dataset, WriteMode, WriteParams};
use std::path::Path;
use std::sync::Arc;

use super::index::{Summary, SummaryLevel};

pub struct SummaryVectorIndex {
    dataset: Option<Dataset>,
    index_path: std::path::PathBuf,
}

impl SummaryVectorIndex {
    pub async fn new(index_path: &Path) -> Result<Self> {
        let dataset = if index_path.exists() {
            match Dataset::open(index_path.to_str().unwrap()).await {
                Ok(ds) => Some(ds),
                Err(_) => None,
            }
        } else {
            None
        };

        Ok(Self {
            dataset,
            index_path: index_path.to_path_buf(),
        })
    }

    pub async fn add_summaries(&mut self, summaries: &[Summary], overwrite: bool) -> Result<()> {
        let with_emb: Vec<&Summary> = summaries
            .iter()
            .filter(|s| s.embedding.as_ref().map(|e| !e.is_empty()).unwrap_or(false))
            .collect();
        if with_emb.is_empty() {
            return Ok(());
        }

        if overwrite && self.index_path.exists() {
            let _ = std::fs::remove_dir_all(&self.index_path);
            self.dataset = None;
        }

        let embedding_dim = with_emb[0].embedding.as_ref().unwrap().len();
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("target_id", DataType::Utf8, false),
            Field::new("file_path", DataType::Utf8, true),
            Field::new("start_line", DataType::UInt64, true),
            Field::new("end_line", DataType::UInt64, true),
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
            .map(|s| s.file_path.as_ref().map(|p| p.to_string_lossy().to_string()))
            .collect();
        let start_lines: Vec<Option<u64>> = with_emb
            .iter()
            .map(|s| s.start_line.map(|v| v as u64))
            .collect();
        let end_lines: Vec<Option<u64>> = with_emb
            .iter()
            .map(|s| s.end_line.map(|v| v as u64))
            .collect();
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

        let mode = if self.dataset.is_some() && !overwrite {
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

    pub async fn search(&self, query_vector: &[f32], limit: usize) -> Result<Vec<(f32, Summary)>> {
        let dataset = match self.dataset.as_ref() {
            Some(ds) => ds,
            None => return Ok(Vec::new()),
        };

        let query_array = Float32Array::from(query_vector.to_vec());
        let results = dataset
            .scan()
            .nearest("embedding", &query_array, limit)?
            .try_into_stream()
            .await?;

        use futures::stream::TryStreamExt;
        let batches: Vec<RecordBatch> = results.try_collect().await?;

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
                    file_path: file_paths.value(i).is_empty().then(|| None).unwrap_or_else(|| {
                        Some(std::path::PathBuf::from(file_paths.value(i)))
                    }),
                    start_line: Some(start_lines.value(i) as usize),
                    end_line: Some(end_lines.value(i) as usize),
                    name: None,
                    language: None,
                    model: None,
                    prompt_version: None,
                    generated_at: None,
                    source_hash: None,
                    embedding: None,
                };
                out.push((similarity, summary));
            }
        }

        out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(limit);
        Ok(out)
    }
}
