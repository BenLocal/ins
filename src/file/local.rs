use std::path::Path;

use anyhow::Context;
use async_trait::async_trait;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{FileTrait, ProgressFn};

const CHUNK_SIZE: usize = 64 * 1024;

#[derive(Clone, Debug, Default)]
pub struct LocalFile;

#[async_trait]
impl FileTrait for LocalFile {
    async fn create_dir_all(&self, path: &Path) -> anyhow::Result<()> {
        fs::create_dir_all(path)
            .await
            .with_context(|| format!("create local dir {}", path.display()))
    }

    async fn read_bytes(
        &self,
        path: &Path,
        progress: Option<&ProgressFn>,
    ) -> anyhow::Result<Vec<u8>> {
        let meta = fs::metadata(path)
            .await
            .with_context(|| format!("metadata {}", path.display()))?;
        let total = meta.len();
        if total > 0
            && let Some(cb) = progress
        {
            cb(0, total);
        }

        let mut file = fs::File::open(path)
            .await
            .with_context(|| format!("read local file {}", path.display()))?;
        let mut buf = Vec::with_capacity(total as usize);
        let mut read = 0u64;
        loop {
            let mut chunk = vec![0u8; CHUNK_SIZE.min((total - read) as usize).max(1)];
            let n = file
                .read(&mut chunk)
                .await
                .with_context(|| format!("read chunk {}", path.display()))?;
            if n == 0 {
                break;
            }
            chunk.truncate(n);
            buf.extend_from_slice(&chunk);
            read += n as u64;
            if let Some(cb) = progress {
                cb(read, total);
            }
        }

        Ok(buf)
    }

    async fn write_bytes(
        &self,
        path: &Path,
        content: &[u8],
        progress: Option<&ProgressFn>,
    ) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            self.create_dir_all(parent).await?;
        }

        if let Some(cb) = progress {
            cb(0, content.len() as u64);
        }
        let mut file = fs::File::create(path)
            .await
            .with_context(|| format!("create local file {}", path.display()))?;
        let mut written = 0u64;
        while written < content.len() as u64 {
            let end = (written as usize + CHUNK_SIZE).min(content.len());
            file.write_all(&content[written as usize..end])
                .await
                .with_context(|| format!("write local file {}", path.display()))?;
            written = end as u64;
            if let Some(cb) = progress {
                cb(written, content.len() as u64);
            }
        }
        file.flush()
            .await
            .with_context(|| format!("flush local file {}", path.display()))?;
        if let Some(cb) = progress {
            cb(content.len() as u64, content.len() as u64);
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "local_test.rs"]
mod local_test;
