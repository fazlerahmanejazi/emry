use crate::models::Chunk;
use anyhow::Result;
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::TantivyDocument;
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, Term};

pub struct LexicalIndex {
    index: Index,
    reader: IndexReader,
    // Fields
    id_field: Field,
    content_field: Field,
    path_field: Field,
    start_line_field: Field,
    end_line_field: Field,
    language_field: Field,
}

impl LexicalIndex {
    pub fn new(index_path: &Path) -> Result<Self> {
        let mut schema_builder = Schema::builder();

        let id_field = schema_builder.add_text_field("id", STRING | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT | STORED);
        let path_field = schema_builder.add_text_field("path", STRING | STORED);
        let start_line_field = schema_builder.add_u64_field("start_line", STORED);
        let end_line_field = schema_builder.add_u64_field("end_line", STORED);
        let language_field = schema_builder.add_text_field("language", STRING | STORED);

        let schema = schema_builder.build();

        std::fs::create_dir_all(index_path)?;

        let index = Index::open_or_create(
            tantivy::directory::MmapDirectory::open(index_path)?,
            schema.clone(),
        )?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        Ok(Self {
            index,
            reader,
            id_field,
            content_field,
            path_field,
            start_line_field,
            end_line_field,
            language_field,
        })
    }

    pub fn add_chunks(&mut self, chunks: &[Chunk]) -> Result<()> {
        let mut writer: IndexWriter = self.index.writer(50_000_000)?; // 50MB buffer

        for chunk in chunks {
            writer.add_document(doc!(
                self.id_field => chunk.id.as_str(),
                self.content_field => chunk.content.as_str(),
                self.path_field => chunk.file_path.to_string_lossy().as_ref(),
                self.start_line_field => chunk.start_line as u64,
                self.end_line_field => chunk.end_line as u64,
                self.language_field => chunk.language.to_string(),
            ))?;
        }

        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    pub fn delete_chunks(&mut self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut writer: IndexWriter = self.index.writer(10_000_000)?;
        for id in ids {
            let term = Term::from_field_text(self.id_field, id);
            writer.delete_term(term);
        }
        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<(f32, Chunk)>> {
        let searcher = self.reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![self.content_field]);
        let query = query_parser.parse_query(query_str)?;

        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;

        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;

            // Reconstruct Chunk from doc
            // Note: We might not need to reconstruct the full Chunk if we just want display info,
            // but for now we map back to Chunk.

            let id = retrieved_doc
                .get_first(self.id_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let content = retrieved_doc
                .get_first(self.content_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let path_str = retrieved_doc
                .get_first(self.path_field)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let start_line = retrieved_doc
                .get_first(self.start_line_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let end_line = retrieved_doc
                .get_first(self.end_line_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let lang_str = retrieved_doc
                .get_first(self.language_field)
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            // We don't store everything (like byte offsets or node type) in lexical index for now,
            // or we could add them to schema. For Phase 1, this is enough for search results.

            let chunk = Chunk {
                id,
                language: crate::models::Language::from_name(lang_str),
                file_path: std::path::PathBuf::from(path_str),
                start_line,
                end_line,
                start_byte: None,
                end_byte: None,
                node_type: "unknown".to_string(), // Not stored
                content_hash: "".to_string(),     // Not stored
                content,
                embedding: None,
                parent_scope: None,
                scope_path: Vec::new(),
            };

            results.push((score, chunk));
        }

        Ok(results)
    }
}
