pub mod app;
pub mod deploy;
pub mod node;

use std::path::PathBuf;

use async_trait::async_trait;

#[derive(Clone, Debug)]
pub struct CommandContext {
    pub home: PathBuf,
}

#[async_trait]
pub trait CommandTrait {
    type Args: Send + Sync + 'static;

    async fn run(args: Self::Args, ctx: CommandContext) -> anyhow::Result<()>;
}
