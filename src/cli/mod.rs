pub mod app;
pub mod check;
pub mod deploy;
pub mod docker;
pub mod node;
pub mod service;
pub mod template;
pub mod tui;
pub mod version;
pub mod volume;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use crate::OutputFormat;
use crate::config::InsConfig;

#[derive(Clone, Debug)]
pub struct CommandContext {
    pub home: PathBuf,
    pub output: OutputFormat,
    pub config: Arc<InsConfig>,
}

impl CommandContext {
    pub fn new(home: PathBuf, output: OutputFormat, config: Arc<InsConfig>) -> Self {
        Self {
            home,
            output,
            config,
        }
    }
}

#[async_trait]
pub trait CommandTrait {
    type Args: Send + Sync + 'static;

    async fn run(args: Self::Args, ctx: CommandContext) -> anyhow::Result<()>;
}
