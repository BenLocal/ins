pub mod app;
pub mod check;
pub mod deploy;
pub mod node;
pub mod service;
pub mod template;
pub mod tui;
pub mod version;
pub mod volume;

use std::path::PathBuf;

use async_trait::async_trait;

use crate::OutputFormat;

#[derive(Clone, Debug)]
pub struct CommandContext {
    pub home: PathBuf,
    pub output: OutputFormat,
}

#[async_trait]
pub trait CommandTrait {
    type Args: Send + Sync + 'static;

    async fn run(args: Self::Args, ctx: CommandContext) -> anyhow::Result<()>;
}
