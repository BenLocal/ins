use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

pub mod local;
pub mod remote;

/// Callback for progress: (current_bytes, total_bytes). total=0 means unknown (spinner).
pub type ProgressFn = Arc<dyn Fn(u64, u64) + Send + Sync>;

#[async_trait]
pub trait FileTrait: Send + Sync {
    /// Create a directory and all parent directories.
    async fn create_dir_all(&self, path: &Path) -> anyhow::Result<()>;
    /// Read raw bytes; progress callback is optional: (current, total), total=0 if unknown.
    async fn read_bytes(
        &self,
        path: &Path,
        progress: Option<&ProgressFn>,
    ) -> anyhow::Result<Vec<u8>>;
    /// Write raw bytes; progress callback is optional.
    async fn write_bytes(
        &self,
        path: &Path,
        content: &[u8],
        progress: Option<&ProgressFn>,
    ) -> anyhow::Result<()>;
    /// Read file; progress callback is optional: (current, total), total=0 if unknown.
    async fn read(&self, path: &Path, progress: Option<&ProgressFn>) -> anyhow::Result<String> {
        let bytes = self.read_bytes(path, progress).await?;
        String::from_utf8(bytes).map_err(|e| anyhow::anyhow!("file not utf-8: {}", e))
    }
    /// Write file; progress callback is optional.
    async fn write(
        &self,
        path: &Path,
        content: &str,
        progress: Option<&ProgressFn>,
    ) -> anyhow::Result<()> {
        self.write_bytes(path, content.as_bytes(), progress).await
    }
}
