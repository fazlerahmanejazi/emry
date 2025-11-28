use anyhow::Result;
use clap::ValueEnum;
use emry_config::Config;
use emry_context as agent_context;
use emry_core::models::Language;
use emry_pipeline::manager::IndexManager;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;


use super::print_snippet::print_snippet;
use super::regex_utils;
use super::utils::{build_single_globset, path_matches};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum CliSearchMode {
    Lexical,
    Semantic,
    Hybrid,
}

pub async fn handle_search(
    query: String,
    config_path: Option<&Path>,
    limit: usize,
    _mode: Option<CliSearchMode>,
    lang: Option<String>,
    path: Option<String>,
    symbol: bool,
    regex: bool,
    no_ignore: bool,
) -> Result<()> {
    println!("Searching for: {}", query);
    let ctx = agent_context::RepoContext::from_env(config_path).await?;
    let root = ctx.root.clone();
    let config = ctx.config.clone();
    let graph_arc = ctx.graph.clone();
    let content_store = ctx.content_store.clone();
    let embedder = ctx.embedder.clone();
    let manager = Arc::new(IndexManager::new(
        ctx.lexical.clone(),
        ctx.vector.clone(),
        embedder.clone(),
        ctx.file_store.clone(),
        ctx.chunk_store.clone(),
        ctx.content_store.clone(),
        ctx.file_blob_store.clone(),
        ctx.graph.clone(),
    ));

    // Symbol search short-circuit
    if symbol {
        let matcher = build_single_globset(path.as_deref());
        let lang_filter = lang.as_deref().map(Language::from_name);
        let mut matches = Vec::new();
        let graph = graph_arc.read().unwrap();
        if let Ok(nodes) = graph.list_symbols() {
            for node in nodes {
                if !node.label.contains(&query) {
                    continue;
                }
                let file_path = PathBuf::from(&node.file_path);
                if let Some(lf) = lang_filter.as_ref() {
                    if let Some(ext) = file_path.extension().and_then(|s| s.to_str()) {
                        if Language::from_extension(ext) != *lf {
                            continue;
                        }
                    }
                }
                if !path_matches(&matcher, &root, &file_path) {
                    continue;
                }
                matches.push((node.label.clone(), file_path));
            }
        }
        println!("Found {} symbol matches:", matches.len());
        for (i, (name, file_path)) in matches.iter().enumerate() {
            println!("{}: {} ({})", i + 1, name, file_path.display());
        }
        return Ok(())
    }

    // Regex short-circuit
    if regex {
        let matcher = build_single_globset(path.as_deref());
        let lang_filter = lang.as_deref().map(Language::from_name);
        let matches = regex_utils::regex_search(&root, &query, &config.core, !no_ignore)?;
        if matches.is_empty() {
            println!("No matches for regex '{}'.", query);
        } else {
            println!("Regex matches for '{}':", query);
            for (p, line, content) in matches {
                if let Some(lf) = lang_filter.as_ref() {
                    if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                        if Language::from_extension(ext) != *lf {
                            continue;
                        }
                    }
                }
                if !path_matches(&matcher, &root, &p) {
                    continue;
                }
                let rel = p.strip_prefix(&root).unwrap_or(&p);
                println!("{}:{}: {}", rel.display(), line, content);
            }
        }
        return Ok(())
    }

    let results = manager
        .search_ranked(&query, limit, Some(rank_cfg(&config)))
        .await?;

    let lang_filter = lang.as_deref().map(Language::from_name);
    let matcher = build_single_globset(path.as_deref());
    let mut filtered = results;
    filtered.retain(|hit| {
        let chunk = &hit.chunk;
        let lang_ok = lang_filter
            .as_ref()
            .map(|l| chunk.language == *l)
            .unwrap_or(true);
        let path_ok = path_matches(&matcher, &root, &chunk.file_path);
        lang_ok && path_ok
    });

    // Learning-to-rank style fusion: normalize components then combine with weights.
    let fused = fuse_scores(filtered, &config);

    // Display results
    for (i, hit) in fused.iter().enumerate() {
        let chunk = &hit.chunk;
        println!(
            "\n#{} [{:.3}] lex={:.3?} vec={:.3?} graph={:.3?} sym={:.3?} {}:{}-{}",
            i + 1,
            hit.score,
            hit.lexical_score,
            hit.vector_score,
            hit.graph_boost,
            hit.symbol_boost,
            chunk.file_path.display(),
            chunk.start_line,
            chunk.end_line
        );
        if let Some(path) = &hit.graph_path {
            println!("Path: {}", path.join(" | "));
        }
        print_snippet(chunk, &root, 2, Some(content_store.as_ref()));
    }

    Ok(())
}

fn rank_cfg(config: &Config) -> emry_core::ranking::RankConfig {
    emry_core::ranking::RankConfig {
        lexical_weight: config.ranking.lexical,
        vector_weight: config.ranking.vector,
        graph_weight: config.ranking.graph,
        graph_max_depth: config.graph.max_depth,
        graph_decay: config.graph.decay,
        graph_path_weight: config.graph.path_weight,
        symbol_weight: config.ranking.symbol,
        bm25_k1: config.bm25.k1,
        bm25_b: config.bm25.b,
        edge_weights: config.graph.edge_weights.clone(),
        bm25_avg_len: config.bm25.avg_len,
    }
}

fn fuse_scores(
    mut hits: Vec<emry_core::models::ScoredChunk>,
    config: &Config,
) -> Vec<emry_core::models::ScoredChunk> {
    if hits.is_empty() {
        return hits;
    }
    let max_lex = hits
        .iter()
        .filter_map(|h| h.lexical_score)
        .fold(0.0_f32, f32::max);
    let max_vec = hits
        .iter()
        .filter_map(|h| h.vector_score)
        .fold(0.0_f32, f32::max);
    let max_graph = hits
        .iter()
        .filter_map(|h| h.graph_boost)
        .fold(0.0_f32, f32::max);
    let max_sym = hits
        .iter()
        .filter_map(|h| h.symbol_boost)
        .fold(0.0_f32, f32::max);

    for hit in hits.iter_mut() {
        let lex_n = hit
            .lexical_score
            .map(|v| if max_lex > 0.0 { v / max_lex } else { 0.0 })
            .unwrap_or(0.0);
        let vec_n = hit
            .vector_score
            .map(|v| if max_vec > 0.0 { v / max_vec } else { 0.0 })
            .unwrap_or(0.0);
        let graph_n = hit
            .graph_boost
            .map(|v| if max_graph > 0.0 { v / max_graph } else { 0.0 })
            .unwrap_or(0.0);
        let sym_n = hit
            .symbol_boost
            .map(|v| if max_sym > 0.0 { v / max_sym } else { 0.0 })
            .unwrap_or(0.0);

        hit.score = lex_n * config.ranking.lexical
            + vec_n * config.ranking.vector
            + graph_n * config.ranking.graph
            + sym_n * config.ranking.symbol;
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits
}
