use anyhow::{anyhow, Result};
use arrow::array::{ArrayRef, Float32Array, RecordBatch, StringArray, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};
use coderet_core::models::{Chunk, Language};
use futures::stream::TryStreamExt;

use lance::dataset::{Dataset, WriteMode, WriteParams};
use std::path::Path;
use std::sync::Arc;

pub struct VectorIndex {
    dataset: Option<Dataset>,
    index_path: std::path::PathBuf,
}

impl VectorIndex {
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

    pub async fn add_chunks(&mut self, chunks: &[Chunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        let chunks_with_embeddings: Vec<&Chunk> =
            chunks.iter().filter(|c| c.embedding.is_some()).collect();

        if chunks_with_embeddings.is_empty() {
            return Ok(());
        }

        let embedding_dim = chunks_with_embeddings[0].embedding.as_ref().unwrap().len();

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("file_path", DataType::Utf8, false),
            Field::new("start_line", DataType::UInt64, false),
            Field::new("end_line", DataType::UInt64, false),
            Field::new("language", DataType::Utf8, false),
            Field::new("content_hash", DataType::Utf8, false),
            Field::new(
                "embedding",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    embedding_dim as i32,
                ),
                false,
            ),
        ]));

        let ids: Vec<String> = chunks_with_embeddings
            .iter()
            .map(|c| c.id.clone())
            .collect();
        let file_paths: Vec<String> = chunks_with_embeddings
            .iter()
            .map(|c| c.file_path.to_string_lossy().to_string())
            .collect();
        let languages: Vec<String> = chunks_with_embeddings
            .iter()
            .map(|c| c.language.to_string())
            .collect();
        let start_lines: Vec<u64> = chunks_with_embeddings
            .iter()
            .map(|c| c.start_line as u64)
            .collect();
        let end_lines: Vec<u64> = chunks_with_embeddings
            .iter()
            .map(|c| c.end_line as u64)
            .collect();
        let content_hashes: Vec<String> = chunks_with_embeddings
            .iter()
            .map(|c| c.content_hash.clone())
            .collect();

        let embeddings: Vec<f32> = chunks_with_embeddings
            .iter()
            .flat_map(|c| c.embedding.as_ref().unwrap().clone())
            .collect();

        let id_array: ArrayRef = Arc::new(StringArray::from(ids));
        let file_path_array: ArrayRef = Arc::new(StringArray::from(file_paths));
        let start_line_array: ArrayRef = Arc::new(UInt64Array::from(start_lines));
        let end_line_array: ArrayRef = Arc::new(UInt64Array::from(end_lines));
        let language_array: ArrayRef = Arc::new(StringArray::from(languages));
        let content_hash_array: ArrayRef = Arc::new(StringArray::from(content_hashes));
        let embedding_array: ArrayRef = {
            let values = Float32Array::from(embeddings);
            let field = Arc::new(Field::new("item", DataType::Float32, true));
            Arc::new(arrow::array::FixedSizeListArray::new(
                field,
                embedding_dim as i32,
                Arc::new(values),
                None,
            ))
        };

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                id_array,
                file_path_array,
                start_line_array,
                end_line_array,
                language_array,
                content_hash_array,
                embedding_array,
            ],
        )?;

        let write_mode = if self.dataset.is_some() {
            WriteMode::Append
        } else {
            WriteMode::Create
        };

        use arrow::array::RecordBatchIterator;
        let batches = vec![Ok(batch.clone())];
        let reader = RecordBatchIterator::new(batches.into_iter(), schema.clone());

        let dataset = Dataset::write(
            reader,
            self.index_path.to_str().unwrap(),
            Some(WriteParams {
                mode: write_mode,
                ..Default::default()
            }),
        )
        .await?;

        self.dataset = Some(dataset);
        Ok(())
    }

    pub async fn search(&self, query_vector: &[f32], limit: usize) -> Result<Vec<(f32, Chunk)>> {
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
        let mut batches = Vec::new();
        let mut stream = results;
        while let Some(batch) = stream.try_next().await? {
            batches.push(batch);
        }

        let mut scored_chunks = Vec::new();

        for batch in batches {
            let ids = batch
                .column_by_name("id")
                .ok_or_else(|| anyhow!("Missing id column"))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow!("Failed to cast id column"))?;

            let file_paths = batch
                .column_by_name("file_path")
                .ok_or_else(|| anyhow!("Missing file_path column"))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow!("Failed to cast file_path column"))?;

            let start_lines = batch
                .column_by_name("start_line")
                .ok_or_else(|| anyhow!("Missing start_line column"))?
                .as_any()
                .downcast_ref::<UInt64Array>()
                .ok_or_else(|| anyhow!("Failed to cast start_line column"))?;

            let end_lines = batch
                .column_by_name("end_line")
                .ok_or_else(|| anyhow!("Missing end_line column"))?
                .as_any()
                .downcast_ref::<UInt64Array>()
                .ok_or_else(|| anyhow!("Failed to cast end_line column"))?;
            let languages = batch
                .column_by_name("language")
                .and_then(|col| col.as_any().downcast_ref::<StringArray>());

            let (content_hashes, use_hash) = if let Some(col) = batch.column_by_name("content_hash")
            {
                (
                    col.as_any()
                        .downcast_ref::<StringArray>()
                        .ok_or_else(|| anyhow!("Failed to cast content_hash column"))?,
                    true,
                )
            } else {
                let content_col = batch
                    .column_by_name("content")
                    .ok_or_else(|| anyhow!("Missing content/content_hash column"))?
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .ok_or_else(|| anyhow!("Failed to cast content column"))?;
                (content_col, false)
            };

            let distances = batch
                .column_by_name("_distance")
                .ok_or_else(|| anyhow!("Missing _distance column"))?
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(|| anyhow!("Failed to cast _distance column"))?;

            for i in 0..batch.num_rows() {
                let chunk = Chunk {
                    id: ids.value(i).to_string(),
                    language: languages
                        .map(|arr| Language::from_name(arr.value(i)))
                        .unwrap_or(Language::Unknown),
                    file_path: std::path::PathBuf::from(file_paths.value(i)),
                    start_line: start_lines.value(i) as usize,
                    end_line: end_lines.value(i) as usize,
                    start_byte: None,
                    end_byte: None,
                    node_type: String::new(),
                    content_hash: if use_hash {
                        content_hashes.value(i).to_string()
                    } else {
                        "".to_string()
                    },
                    content: String::new(),
                    embedding: None,
                    parent_scope: None,
                    scope_path: Vec::new(),
                };

                let distance = distances.value(i);
                let similarity = 1.0 / (1.0 + distance);

                scored_chunks.push((similarity, chunk));
            }
        }

        Ok(scored_chunks)
    }

    pub async fn delete_chunks(&mut self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let dataset = match self.dataset.as_mut() {
            Some(ds) => ds,
            None => return Ok(()),
        };

        // Build a simple SQL IN clause: "id IN ('a','b',...)"
        let mut parts = Vec::new();
        for id in ids {
            // Escape single quotes by doubling them (best-effort).
            let escaped = id.replace('\'', "''");
            parts.push(format!("'{}'", escaped));
        }
        let predicate = format!("id IN ({})", parts.join(","));

        dataset.delete(&predicate).await?;
        Ok(())
    }
}
