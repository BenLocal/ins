use async_trait::async_trait;

use crate::node::types::NodeRecord;

pub mod docker_compose;

#[derive(Clone, Debug)]
pub struct ProviderContext {
    pub provider: String,
    pub node: NodeRecord,
    pub apps: Vec<String>,
}

#[async_trait]
pub trait ProviderTrait {
    async fn run(&self, ctx: ProviderContext) -> anyhow::Result<()>;
}
