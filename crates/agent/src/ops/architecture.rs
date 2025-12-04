use crate::project::context::RepoContext;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use emry_store::{ModuleCoupling, CentralNode};

pub struct ArchitectureTool {
    ctx: Arc<RepoContext>,
}

impl ArchitectureTool {
    pub fn new(ctx: Arc<RepoContext>) -> Self {
        Self { ctx }
    }

    pub async fn analyze_structure(&self) -> Result<(Vec<ModuleCoupling>, Vec<CentralNode>)> {
        let store = self.ctx.surreal_store.as_ref()
            .ok_or_else(|| anyhow!("SurrealStore not initialized"))?;

        let coupling = store.get_module_coupling().await?;
        let central_nodes = store.get_central_nodes(10).await?;

        Ok((coupling, central_nodes))
    }

    pub fn get_root(&self) -> std::path::PathBuf {
        self.ctx.root.clone()
    }
}