use async_trait::async_trait;
use std::path::PathBuf;

use crate::{app::types::AppRecord, node::types::NodeRecord};

pub mod docker_compose;

#[derive(Clone, Debug)]
pub struct DeploymentTarget {
    pub app: AppRecord,
    pub service: String,
}

impl DeploymentTarget {
    pub fn new(app: AppRecord, service: String) -> Self {
        Self { app, service }
    }
}

#[derive(Clone, Debug)]
pub struct ProviderContext {
    pub provider: String,
    pub node: NodeRecord,
    pub targets: Vec<DeploymentTarget>,
    pub workspace: PathBuf,
}

impl ProviderContext {
    pub fn new(
        provider: String,
        node: NodeRecord,
        targets: Vec<DeploymentTarget>,
        workspace: PathBuf,
    ) -> Self {
        Self {
            provider,
            node,
            targets,
            workspace,
        }
    }
}

#[async_trait]
pub trait ProviderTrait {
    async fn validate(&self, ctx: ProviderContext) -> anyhow::Result<()>;
    async fn run(&self, ctx: ProviderContext) -> anyhow::Result<()>;
}
