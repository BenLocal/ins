use std::path::Path;

use anyhow::Context;
use async_trait::async_trait;
use tokio::fs;
use tokio::io::AsyncReadExt;

use super::{FileTrait, ProgressFn, with_read_progress, with_write_progress};

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
        let (bar, progress) = with_read_progress(path, progress);

        let meta = fs::metadata(path)
            .await
            .with_context(|| format!("metadata {}", path.display()))?;
        let total = meta.len();
        if progress.is_some() && total > 0 {
            if let Some(ref cb) = progress {
                cb(0, total);
            }
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
            if let Some(ref cb) = progress {
                cb(read, total);
            }
        }

        if let Some(ref b) = bar {
            b.finish_with_message("Done");
        }

        Ok(buf)
    }

    async fn write_bytes(
        &self,
        path: &Path,
        content: &[u8],
        progress: Option<&ProgressFn>,
    ) -> anyhow::Result<()> {
        let (bar, progress) = with_write_progress(path, content.len() as u64, progress);

        if let Some(parent) = path.parent() {
            self.create_dir_all(parent).await?;
        }

        if let Some(ref cb) = progress {
            cb(0, content.len() as u64);
        }
        fs::write(path, content)
            .await
            .with_context(|| format!("write local file {}", path.display()))?;
        if let Some(ref cb) = progress {
            cb(content.len() as u64, content.len() as u64);
        }
        if let Some(ref b) = bar {
            b.finish_with_message("Done");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("ins_local_file_test_{unique}"))
            .join(name)
    }

    #[tokio::test]
    async fn local_file_write_and_read_round_trip() {
        let file = LocalFile;
        let path = test_path("nested/output.txt");
        let content = "hello from local file";

        file.write(&path, content, None).await.unwrap();
        let loaded = file.read(&path, None).await.unwrap();

        assert_eq!(loaded, content);
        fs::remove_file(&path).await.unwrap();
        fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn local_file_reports_progress_for_write_and_read() {
        let file = LocalFile;
        let path = test_path("progress.txt");
        let content = "progress payload";

        let write_events = Arc::new(Mutex::new(Vec::new()));
        let write_progress: ProgressFn = {
            let write_events = write_events.clone();
            Arc::new(move |current, total| {
                write_events.lock().unwrap().push((current, total));
            })
        };

        file.write(&path, content, Some(&write_progress))
            .await
            .unwrap();

        let write_events = write_events.lock().unwrap().clone();
        assert_eq!(
            write_events.first().copied(),
            Some((0, content.len() as u64))
        );
        assert_eq!(
            write_events.last().copied(),
            Some((content.len() as u64, content.len() as u64))
        );

        let read_events = Arc::new(Mutex::new(Vec::new()));
        let read_progress: ProgressFn = {
            let read_events = read_events.clone();
            Arc::new(move |current, total| {
                read_events.lock().unwrap().push((current, total));
            })
        };

        let loaded = file.read(&path, Some(&read_progress)).await.unwrap();
        assert_eq!(loaded, content);

        let read_events = read_events.lock().unwrap().clone();
        assert_eq!(
            read_events.first().copied(),
            Some((0, content.len() as u64))
        );
        assert_eq!(
            read_events.last().copied(),
            Some((content.len() as u64, content.len() as u64))
        );

        fs::remove_file(&path).await.unwrap();
    }
}
