use std::path::Path;

use anyhow::Context;
use async_trait::async_trait;
use tokio::fs;

use super::FileTrait;

#[derive(Clone, Debug, Default)]
pub struct LocalFile;

#[async_trait]
impl FileTrait for LocalFile {
    async fn read(&self, path: &Path) -> anyhow::Result<String> {
        fs::read_to_string(path)
            .await
            .with_context(|| format!("read local file {}", path.display()))
    }

    async fn write(&self, path: &Path, content: &str) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create parent dirs for {}", path.display()))?;
        }

        fs::write(path, content)
            .await
            .with_context(|| format!("write local file {}", path.display()))
    }
}
