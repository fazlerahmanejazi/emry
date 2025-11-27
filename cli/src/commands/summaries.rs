use anyhow::Result;
use coderet_config::{Config, SummaryLevel};
use coderet_core::models::Symbol;
use coderet_core::summaries::generator::SummaryGenerator;
use coderet_core::traits::Embedder;
use coderet_index::summaries::SummaryIndex as SimpleSummaryIndex;
use coderet_store::file_store::FileStore;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info, warn};

use coderet_pipeline::index::compute_hash;

pub async fn maybe_generate_summaries(
    config: &Config,
    summary_index: &mut SimpleSummaryIndex,
    embedder: Option<&Arc<dyn Embedder + Send + Sync>>,
    file_store: &FileStore,
    root: &Path,
    symbol_registry: &[Symbol],
    file_content_map: &HashMap<PathBuf, String>,
    removed_files: &[PathBuf],
) -> Result<()> {
    if !config.summary.enabled {
        return Ok(());
    }

    let mut generated = Vec::new();
    let mut summaries_changed = false;
    let level_set: HashSet<SummaryLevel> = config.summary.levels.iter().cloned().collect();

    // Initialize generator if LLM is enabled
    let generator = if config.summary.use_llm {
        Some(SummaryGenerator::new(
            Some(config.summary.model.clone()),
            config.summary.max_tokens,
            config.summary.prompt_version.clone(),
            2, // Retries
        )?)
    } else {
        None
    };

    // Calculate total work for progress bar
    let mut total_items: usize = 0;
    if !symbol_registry.is_empty()
        && (level_set.contains(&SummaryLevel::Function) || level_set.contains(&SummaryLevel::Class))
    {
        total_items += symbol_registry.len();
    }
    if level_set.contains(&SummaryLevel::File) {
        total_items += file_content_map.len();
    }
    // Modules/Repo are fast/few, we can ignore or just add a buffer, but let's stick to the big ones.

    let pb = if total_items > 0 && config.summary.use_llm {
        let pb = ProgressBar::new(total_items as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:30.cyan/blue} {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );
        pb.set_message("Generating summaries");
        Some(pb)
    } else {
        None
    };

    // Drop stale summaries for removed/changed files
    if !removed_files.is_empty() {
        summary_index.remove_by_files(removed_files).await?;
        summaries_changed = true;
    }

    // Symbol-level summaries (functions/classes)
    if !symbol_registry.is_empty()
        && (level_set.contains(&SummaryLevel::Function) || level_set.contains(&SummaryLevel::Class))
    {
        for sym in symbol_registry {
            let level = if sym.kind == "class" {
                SummaryLevel::Class
            } else {
                SummaryLevel::Function
            };
            if !level_set.contains(&level) {
                if let Some(pb) = &pb {
                    pb.inc(1);
                }
                continue;
            }
            let base_text = summarize_symbol(sym, file_content_map);
            let text = if let Some(gen) = &generator {
                let context = format!("Symbol {} in {}", sym.name, sym.file_path.display());
                gen.generate(&base_text, &context)
                    .await
                    .unwrap_or(base_text.clone())
            } else {
                base_text.clone()
            };

            let source_hash = compute_hash(&text);
            generated.push(coderet_core::models::Summary {
                id: sym.id.clone(),
                level,
                target_id: sym.id.clone(),
                canonical_target_id: Some(sym.id.clone()),
                text,
                file_path: Some(sym.file_path.clone()),
                start_line: Some(sym.start_line),
                end_line: Some(sym.end_line),
                name: Some(sym.name.clone()),
                language: Some(sym.language.to_string()),
                module: module_name(root, &sym.file_path),
                model: Some(config.summary.model.clone()),
                prompt_version: Some(config.summary.prompt_version.clone()),
                generated_at: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                ),
                source_hash: Some(source_hash),
                embedding: None, // Will be filled by batch embedding
            });
            if let Some(pb) = &pb {
                pb.inc(1);
            }
        }
    }

    // File-level summaries
    if level_set.contains(&SummaryLevel::File) {
        use futures::stream::{self, StreamExt};
        let concurrency = config.summary.concurrency;
        let pb_ref = pb.as_ref();

        let file_summaries: Vec<coderet_core::models::Summary> =
            stream::iter(file_content_map.iter().map(|(path, content)| {
                let config = config.clone();
                let file_store = file_store.clone();
                let generator = generator.clone();
                let root = root.to_path_buf();
                async move {
                    let file_hash = compute_hash(content);
                    let file_id = file_store
                        .get_or_create_file_id(path.as_path(), &file_hash)
                        .ok()?;
                    let symbols_for_file: Vec<Symbol> = symbol_registry
                        .iter()
                        .filter(|s| &s.file_path == path)
                        .cloned()
                        .collect();
                    let base_text = summarize_file(path, content, &symbols_for_file);
                    let text = if let Some(gen) = &generator {
                        let context = format!("File {}", path.display());
                        gen.generate(&base_text, &context)
                            .await
                            .unwrap_or(base_text.clone())
                    } else {
                        base_text.clone()
                    };

                    let source_hash = compute_hash(&text);
                    Some(coderet_core::models::Summary {
                        id: format!("file:{}", file_id),
                        level: SummaryLevel::File,
                        target_id: format!("file:{}", file_id),
                        canonical_target_id: Some(format!("file:{}", file_id)),
                        text,
                        file_path: Some(path.clone()),
                        start_line: Some(1),
                        end_line: Some(content.lines().count()),
                        name: path.file_name().map(|n| n.to_string_lossy().to_string()),
                        language: path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|s| s.to_string()),
                        module: module_name(&root, path),
                        model: Some(config.summary.model.clone()),
                        prompt_version: Some(config.summary.prompt_version.clone()),
                        generated_at: Some(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0),
                        ),
                        source_hash: Some(source_hash),
                        embedding: None,
                    })
                }
            }))
            .buffer_unordered(concurrency)
            .inspect(|_| {
                if let Some(pb) = pb_ref {
                    pb.inc(1);
                }
            })
            .filter_map(|x| async move { x })
            .collect()
            .await;

        generated.extend(file_summaries);
    }

    // Module-level summaries (per top-level directory)
    if level_set.contains(&SummaryLevel::Module) {
        let mut modules: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for path in file_content_map.keys() {
            let rel = path.strip_prefix(root).unwrap_or(path);
            let module = rel
                .components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .unwrap_or_else(|| "root".to_string());
            modules.entry(module).or_default().push(path.clone());
        }
        for (module, files) in modules {
            let mut names = Vec::new();
            for f in &files {
                let syms: Vec<_> = symbol_registry
                    .iter()
                    .filter(|s| &s.file_path == f)
                    .map(|s| format!("{} ({})", s.name, s.kind))
                    .collect();
                names.extend(syms);
            }
            let preview = names
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let base_text = format!("Module {} includes: {}", module, preview);
            let text = if let Some(gen) = &generator {
                let context = format!("Module {}", module);
                gen.generate(&base_text, &context)
                    .await
                    .unwrap_or(base_text.clone())
            } else {
                base_text.clone()
            };

            generated.push(coderet_core::models::Summary {
                id: format!("module:{}", module),
                level: SummaryLevel::Module,
                target_id: module.clone(),
                canonical_target_id: Some(module.clone()),
                text,
                file_path: None,
                start_line: None,
                end_line: None,
                name: Some(module.clone()),
                language: None,
                module: Some(module.clone()),
                model: Some(config.summary.model.clone()),
                prompt_version: Some(config.summary.prompt_version.clone()),
                generated_at: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                ),
                source_hash: Some(compute_hash(&base_text)),
                embedding: None, // Will be filled by batch embedding
            });
        }
    }

    // Repo-level summary
    if level_set.contains(&SummaryLevel::Repo) {
        let symbol_names: Vec<String> = symbol_registry
            .iter()
            .take(100)
            .map(|s| format!("{} ({})", s.name, s.kind))
            .collect();
        let base_text = format!(
            "Repository summary: {} files, {} symbols. Sample symbols: {}",
            file_content_map.len(),
            symbol_registry.len(),
            symbol_names.join(", ")
        );
        let text = if let Some(gen) = &generator {
            let context = format!("Repository {}", root.display());
            gen.generate(&base_text, &context)
                .await
                .unwrap_or(base_text.clone())
        } else {
            base_text.clone()
        };

        generated.push(coderet_core::models::Summary {
            id: "repo:root".to_string(),
            level: SummaryLevel::Repo,
            target_id: "repo:root".to_string(),
            canonical_target_id: Some("repo:root".to_string()),
            text,
            file_path: None,
            start_line: None,
            end_line: None,
            name: Some(
                root.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "repo".to_string()),
            ),
            language: None,
            module: None,
            model: Some(config.summary.model.clone()),
            prompt_version: Some(config.summary.prompt_version.clone()),
            generated_at: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            ),
            source_hash: Some(compute_hash(&base_text)),
            embedding: None, // Will be filled by batch embedding
        });
    }

    if !generated.is_empty() {
        if let Some(e) = embedder {
            let texts: Vec<String> = generated.iter().map(|s| s.text.clone()).collect();
            // Batch embed all summaries
            if let Ok(embeddings) = e.embed_batch(&texts).await {
                if embeddings.len() == generated.len() {
                    for (summary, emb) in generated.iter_mut().zip(embeddings) {
                        summary.embedding = Some(emb);
                    }
                } else {
                    warn!(
                        "Summary embedding count mismatch (got {}, expected {}), skipping embeddings",
                        embeddings.len(),
                        generated.len()
                    );
                }
            } else {
                error!("Failed to batch embed summaries");
            }
        }

        summary_index.add_summaries(&generated).await?;
        summaries_changed = true;
    }

    if let Some(pb) = pb {
        pb.finish_with_message("Summary generation complete");
    }
    if summaries_changed {
        // Sled-backed store is already durable; nothing extra required.
    }
    Ok(())
}

fn summarize_symbol(sym: &Symbol, file_content_map: &HashMap<PathBuf, String>) -> String {
    if let Some(content) = file_content_map.get(&sym.file_path) {
        let lines: Vec<&str> = content.lines().collect();
        if sym.start_line > 0 && sym.end_line <= lines.len() && sym.start_line <= sym.end_line {
            let snippet = lines[sym.start_line - 1..sym.end_line].join("\n");
            let first_non_empty = snippet
                .lines()
                .find(|l| !l.trim().is_empty())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| sym.name.clone());
            return first_non_empty;
        }
    }
    sym.name.clone()
}

fn summarize_file(path: &Path, content: &str, symbols: &[Symbol]) -> String {
    let symbol_list: Vec<String> = symbols
        .iter()
        .take(8)
        .map(|s| format!("{} ({})", s.name, s.kind))
        .collect();
    let headline = if symbol_list.is_empty() {
        format!("File {} with no parsed symbols", path.display())
    } else {
        format!(
            "File {} symbols: {}",
            path.display(),
            symbol_list.join(", ")
        )
    };
    let preview = content.lines().take(24).collect::<Vec<_>>().join("\n");
    format!("{}\n{}", headline, preview)
}

/// Infer the top-level module/namespace for a file relative to the repo root.
fn module_name(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    rel.components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
}
