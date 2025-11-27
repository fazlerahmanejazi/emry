use super::splitter::enforce_token_limits;
use super::Chunker;
use crate::models::{Chunk, Language};
use anyhow::{anyhow, Result};
use coderet_config::ChunkingConfig;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor};
use super::languages::{self, ChunkQuery, LanguageSupport};

pub struct GenericChunker {
    language: Language,
    queries: Vec<ChunkQuery>,
    config: ChunkingConfig,
    support: Option<Box<dyn LanguageSupport>>,
}

impl GenericChunker {
    pub fn new(language: Language) -> Self {
        Self::with_config(language, ChunkingConfig::default())
    }

    pub fn with_config(language: Language, config: ChunkingConfig) -> Self {
        let support = languages::get_language_support(language.clone());
        let queries = support.as_ref().map(|s| s.get_queries()).unwrap_or_default();
        Self {
            language,
            queries,
            config,
            support,
        }
    }

    fn create_parser(&self) -> Result<Parser> {
        if let Some(support) = &self.support {
            support.create_parser()
        } else {
            Err(anyhow!("Unknown language or no parser support"))
        }
    }

    fn chunk_cast(
        &self,
        tree: &tree_sitter::Tree,
        content: &str,
        file_path: &Path,
    ) -> Result<Vec<Chunk>> {
        let line_offsets = compute_line_offsets(content);
        let root = tree.root_node();
        let chunks = self.chunk_node_cast(
            root,
            &Vec::new(),
            content,
            file_path,
            &line_offsets,
            false, // do not emit whole-file chunk directly
        );
        // Keep token enforcement as a safety net
        enforce_token_limits(chunks, &self.config)
    }

    fn chunk_node_cast(
        &self,
        node: tree_sitter::Node,
        scopes: &Vec<String>,
        content: &str,
        file_path: &Path,
        line_offsets: &[usize],
        allow_self_chunk: bool,
    ) -> Vec<Chunk> {
        let mut scope_path = scopes.clone();
        let parent_scope = scopes.last().cloned();
        if let Some(lbl) = scope_label(&node, content) {
            scope_path.push(lbl);
        }

        let node_slice = &content[node.start_byte()..node.end_byte()];
        let size = count_non_whitespace(node_slice);

        if allow_self_chunk && size <= self.config.max_chars {
            return vec![make_chunk_from_span(
                node.start_byte(),
                node.end_byte(),
                node.kind().to_string(),
                parent_scope,
                scope_path,
                self.language.clone(),
                content,
                file_path,
                line_offsets,
            )];
        }

        // Recurse into children; if no named children, fall back to slicing text.
        let mut cursor = node.walk();
        let children: Vec<tree_sitter::Node> = node.named_children(&mut cursor).collect();
        if children.is_empty() {
            return split_large_span(
                node.start_byte(),
                node.end_byte(),
                node.kind().to_string(),
                parent_scope,
                scope_path,
                self.config.max_chars,
                self.language.clone(),
                content,
                file_path,
                line_offsets,
            );
        }

        let mut child_chunks: Vec<Chunk> = Vec::new();
        for child in children {
            let mut sub =
                self.chunk_node_cast(child, &scope_path, content, file_path, line_offsets, true);
            child_chunks.append(&mut sub);
        }

        merge_adjacent(
            child_chunks,
            self.config.max_chars,
            content,
            line_offsets,
            file_path,
            self.language.clone(),
        )
    }
}

impl Chunker for GenericChunker {
    fn chunk(&self, content: &str, file_path: &Path) -> Result<Vec<Chunk>> {
        let mut parser = self.create_parser()?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse {:?} code", self.language))?;

        if self.config.use_cast {
            let chunks = self.chunk_cast(&tree, content, file_path)?;
            return Ok(chunks);
        }

        let mut chunks = Vec::new();

        // Process each query
        for query_def in &self.queries {
            let query = Query::new(&parser.language().unwrap(), &query_def.pattern)
                .map_err(|e| anyhow!("Failed to create query: {}", e))?;

            let mut cursor = QueryCursor::new();
            let matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

            for m in matches {
                for capture in m.captures {
                    let node = capture.node;
                    let start_pos = node.start_position();
                    let end_pos = node.end_position();

                    let start_line = start_pos.row + 1;
                    let end_line = end_pos.row + 1;

                    let chunk_content = &content[node.start_byte()..node.end_byte()];

                    let mut hasher = Sha256::new();
                    hasher.update(file_path.to_string_lossy().as_bytes());
                    hasher.update(chunk_content.as_bytes());
                    let hash = hex::encode(hasher.finalize());
                    let id = hash[..16].to_string();

                    let node_type = node.kind().to_string();

                    chunks.push(Chunk {
                        id,
                        language: self.language.clone(),
                        file_path: file_path.to_path_buf(),
                        start_line,
                        end_line,
                        start_byte: Some(node.start_byte()),
                        end_byte: Some(node.end_byte()),
                        node_type,
                        content_hash: hash,
                        content: chunk_content.to_string(),
                        embedding: None,
                        parent_scope: None,
                        scope_path: Vec::new(),
                    });
                }
            }
        }

        // Enforce token limits
        enforce_token_limits(chunks, &self.config)
    }
}

fn compute_line_offsets(content: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    offsets.push(0);
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

fn byte_to_line(byte: usize, offsets: &[usize]) -> usize {
    match offsets.binary_search(&byte) {
        Ok(idx) => idx + 1,
        Err(idx) => idx,
    }
}

fn count_non_whitespace(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count()
}

fn scope_label(node: &tree_sitter::Node, content: &str) -> Option<String> {
    let kind = node.kind();
    if !is_scope_kind(kind) {
        return None;
    }
    if let Some(name_node) = node.child_by_field_name("name") {
        if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
            return Some(format!("{} {}", kind, name.trim()));
        }
    }
    // Heuristic fallbacks
    for candidate in [
        "identifier",
        "type_identifier",
        "property_identifier",
        "field_identifier",
    ] {
        if let Some(child) = first_named_child_of_kind(node, candidate) {
            if let Ok(name) = child.utf8_text(content.as_bytes()) {
                return Some(format!("{} {}", kind, name.trim()));
            }
        }
    }
    Some(kind.to_string())
}

fn is_scope_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "function_item"
            | "function_declaration"
            | "method_declaration"
            | "method_definition"
            | "constructor_declaration"
            | "class_definition"
            | "class_declaration"
            | "class_specifier"
            | "struct_item"
            | "struct_specifier"
            | "enum_item"
            | "trait_item"
            | "impl_item"
            | "interface_declaration"
            | "module"
    ) || kind.contains("function")
        || kind.contains("class")
        || kind.contains("method")
        || kind.contains("module")
        || kind.contains("impl")
        || kind.contains("trait")
}

fn first_named_child_of_kind<'a>(
    node: &'a tree_sitter::Node,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == kind {
            return Some(child);
        }
    }
    None
}

fn make_chunk_from_span(
    start_byte: usize,
    end_byte: usize,
    node_type: String,
    parent_scope: Option<String>,
    scope_path: Vec<String>,
    language: Language,
    content: &str,
    file_path: &Path,
    line_offsets: &[usize],
) -> Chunk {
    let text = &content[start_byte..end_byte];
    let mut hasher = Sha256::new();
    hasher.update(file_path.to_string_lossy().as_bytes());
    hasher.update(text.as_bytes());
    let hash = hex::encode(hasher.finalize());
    let id = hash[..16].to_string();
    Chunk {
        id,
        language,
        file_path: file_path.to_path_buf(),
        start_line: byte_to_line(start_byte, line_offsets),
        end_line: byte_to_line(end_byte, line_offsets),
        start_byte: Some(start_byte),
        end_byte: Some(end_byte),
        node_type,
        content_hash: hash,
        content: text.to_string(),
        embedding: None,
        parent_scope,
        scope_path,
    }
}

fn split_large_span(
    start_byte: usize,
    end_byte: usize,
    node_type: String,
    parent_scope: Option<String>,
    scope_path: Vec<String>,
    max_chars: usize,
    language: Language,
    content: &str,
    file_path: &Path,
    line_offsets: &[usize],
) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut cursor = start_byte;
    while cursor < end_byte {
        // move forward by max_chars worth of non-ws chars; approximate by bytes window stepping
        let mut window_end = cursor;
        let mut seen = 0;
        for (idx, ch) in content[cursor..end_byte].char_indices() {
            if !ch.is_whitespace() {
                seen += 1;
            }
            window_end = cursor + idx + ch.len_utf8();
            if seen >= max_chars {
                break;
            }
        }
        if seen == 0 {
            break;
        }
        let actual_end = window_end.min(end_byte);
        chunks.push(make_chunk_from_span(
            cursor,
            actual_end,
            format!("{}_part", node_type),
            parent_scope.clone(),
            scope_path.clone(),
            language.clone(),
            content,
            file_path,
            line_offsets,
            ));
        cursor = actual_end;
    }
    chunks
}

fn merge_adjacent(
    mut chunks: Vec<Chunk>,
    max_chars: usize,
    content: &str,
    line_offsets: &[usize],
    file_path: &Path,
    language: Language,
) -> Vec<Chunk> {
    chunks.sort_by(|a, b| match (a.start_byte, b.start_byte) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => Ordering::Equal,
    });

    let mut merged = Vec::new();
    let mut current_start: Option<usize> = None;
    let mut current_end: Option<usize> = None;
    let mut current_node_type = String::new();
    let mut current_scope: Option<String> = None;
    let mut current_scope_path: Vec<String> = Vec::new();

    for ch in chunks {
        let s = ch.start_byte.unwrap_or(0);
        let e = ch.end_byte.unwrap_or(s);
        let proposed_start = current_start.unwrap_or(s);
        let proposed_end = current_end.unwrap_or(e).max(e);
        let span_text = &content[proposed_start..proposed_end];
        let span_len = count_non_whitespace(span_text);

        if current_start.is_none() {
            current_start = Some(s);
            current_end = Some(e);
            current_node_type = ch.node_type.clone();
            current_scope = ch.parent_scope.clone();
            current_scope_path = ch.scope_path.clone();
            continue;
        }

        if span_len <= max_chars {
            current_end = Some(proposed_end);
        } else {
            merged.push(make_chunk_from_span(
                current_start.unwrap(),
                current_end.unwrap(),
                format!("{}_merged", current_node_type),
                current_scope.clone(),
                current_scope_path.clone(),
                language.clone(),
                content,
                file_path,
                line_offsets,
            ));
            current_start = Some(s);
            current_end = Some(e);
            current_node_type = ch.node_type.clone();
            current_scope = ch.parent_scope.clone();
            current_scope_path = ch.scope_path.clone();
        }
    }

    if let (Some(s), Some(e)) = (current_start, current_end) {
        merged.push(make_chunk_from_span(
            s,
            e,
            format!("{}_merged", current_node_type),
            current_scope,
            current_scope_path,
            language,
            content,
            file_path,
            line_offsets,
        ));
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn cast_chunking_produces_chunks_with_scope() {
        let code = r#"
        fn alpha() {
            let x = 1;
        }

        fn beta(y: i32) -> i32 {
            y + 1
        }
        "#;

        let mut config = ChunkingConfig::default();
        config.use_cast = true;
        config.max_chars = 200;
        let chunker = GenericChunker::with_config(Language::Rust, config);
        let chunks = chunker
            .chunk(code, Path::new("test.rs"))
            .expect("chunking should succeed");

        assert!(
            !chunks.is_empty(),
            "CAST chunking should yield at least one chunk"
        );
        assert!(
            chunks.iter().any(|c| !c.scope_path.is_empty()),
            "expected at least one chunk with scope metadata"
        );
    }
}
