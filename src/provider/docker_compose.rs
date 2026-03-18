use async_trait::async_trait;

use crate::provider::{ProviderContext, ProviderTrait};

pub struct DockerComposeProvider;

#[async_trait]
impl ProviderTrait for DockerComposeProvider {
    async fn run(&self, ctx: ProviderContext) -> anyhow::Result<()> {
        println!("Running docker compose provider...");
        Ok(())
    }
}
