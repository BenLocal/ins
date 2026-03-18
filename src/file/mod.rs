use std::path::Path;

use async_trait::async_trait;

pub mod local;
pub mod remote;

#[async_trait]
pub trait FileTrait {
    async fn read(&self, path: &Path) -> anyhow::Result<String>;
    async fn write(&self, path: &Path, content: &str) -> anyhow::Result<()>;
}
