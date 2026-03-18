use async_trait::async_trait;
use std::path::PathBuf;

use crate::{app::types::AppRecord, node::types::NodeRecord};

pub mod docker_compose;

#[derive(Clone, Debug)]
pub struct ProviderContext {
    pub provider: String,
    pub node: NodeRecord,
    pub apps: Vec<AppRecord>,
    pub workspace: PathBuf,
}

impl ProviderContext {
    pub fn new(
        provider: String,
        node: NodeRecord,
        apps: Vec<AppRecord>,
        workspace: PathBuf,
    ) -> Self {
        Self {
            provider,
            node,
            apps,
            workspace,
        }
    }
}

#[async_trait]
pub trait ProviderTrait {
    async fn run(&self, ctx: ProviderContext) -> anyhow::Result<()>;
}
