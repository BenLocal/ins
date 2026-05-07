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
pub mod web;

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

    /// Directory where app templates live. Honors `[defaults] app_home` in
    /// config.toml (absolute or relative — relative is resolved against the
    /// home dir so `"../app"` means "app/ next to .ins/"); otherwise defaults
    /// to `<home>/app`.
    pub fn app_home(&self) -> PathBuf {
        match self.config.app_home_override() {
            Some(path) => {
                let p = PathBuf::from(path);
                if p.is_absolute() {
                    p
                } else {
                    self.home.join(p)
                }
            }
            None => self.home.join("app"),
        }
    }
}

#[async_trait]
pub trait CommandTrait {
    type Args: Send + Sync + 'static;

    async fn run(args: Self::Args, ctx: CommandContext) -> anyhow::Result<()>;
}
