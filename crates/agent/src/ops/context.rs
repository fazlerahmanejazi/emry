use crate::project::context::RepoContext;
use anyhow::Result;
use std::sync::Arc;
use emry_engine::search::service::SearchService;
use crate::ops::fs::FsTool;

#[derive(serde::Deserialize)]
struct RelatedFile { 
    path: String 
}

pub struct SmartContext {
    ctx: Arc<RepoContext>,
    search_service: Arc<SearchService>,
    fs_tool: FsTool,
}

impl SmartContext {
    pub fn new(ctx: Arc<RepoContext>) -> Result<Self> {
        let store = ctx.surreal_store.clone().ok_or_else(|| anyhow::anyhow!("Store not available"))?;
        let embedder = ctx.embedder.clone();
        let search_service = Arc::new(SearchService::new(store, embedder));
        let fs_tool = FsTool::new(ctx.clone());
        
        Ok(Self {
            ctx,
            search_service,
            fs_tool,
        })
    }

    pub async fn focus<F>(&self, topic: &str, callback: F) -> Result<String> 
    where F: Fn(String) + Send + Sync
    {
        callback(format!("Searching for topic '{}'...", topic));
        let results = self.search_service.search(topic, 3, None).await?;
        callback(format!("Found {} relevant search results.", results.len()));
        
        if results.is_empty() {
            return Ok(format!("No relevant context found for topic '{}'", topic));
        }

        let mut report = String::new();
        report.push_str(&format!("Smart Focus Context for '{}':\n\n", topic));

        for (i, result) in results.iter().enumerate() {
            let file_path_str = match &result.file.id {
                surrealdb::sql::Id::String(s) => s.clone(),
                _ => result.file.id.to_string(),
            };
            
            let file_path = std::path::Path::new(&file_path_str).to_path_buf();
            
            callback(format!("Found relevant file: {}", file_path.display()));
            report.push_str(&format!("--- File {}: {} ---\n", i+1, file_path.display()));
            
            callback("Generating outline...".to_string());
            if let Ok(outline) = self.fs_tool.generate_outline(&file_path) {
                report.push_str(&format!("Outline:\n{}\n", outline));
            } else {
                report.push_str("Outline: (Unavailable)\n");
            }
            
            callback("Checking for related files (imports)...".to_string());
            if let Some(store) = &self.ctx.surreal_store {
                let q = "SELECT in.file_path as path FROM imports WHERE out.file_path = $path LIMIT 3";
                
                if let Ok(mut res) = store.db().query(q).bind(("path", file_path_str.clone())).await {
                    let related: Vec<RelatedFile> = res.take(0).unwrap_or_default();
                    
                    if !related.is_empty() {
                        callback(format!("Found {} files importing {}", related.len(), file_path.display()));
                        report.push_str("Imported by:\n");
                        for r in related {
                            report.push_str(&format!("- {}\n", r.path));
                        }
                    }
                }
            }
            report.push_str("\n");
        }
        Ok(report)
    }
}
