use std::io::IsTerminal;
use std::path::Path;

use anyhow::Context;
use async_trait::async_trait;
use tokio::fs;
use tokio::io::AsyncReadExt;

use super::{progress_for_read, progress_for_write, FileTrait, ProgressFn};

const CHUNK_SIZE: usize = 64 * 1024;

#[derive(Clone, Debug, Default)]
pub struct LocalFile;

#[async_trait]
impl FileTrait for LocalFile {
    async fn read(&self, path: &Path, progress: Option<&ProgressFn>) -> anyhow::Result<String> {
        let (bar, own_prog) = if progress.is_none() && std::io::stdout().is_terminal() {
            let (b, p) = progress_for_read(path);
            (Some(b), Some(p))
        } else {
            (None, None)
        };
        let progress = progress.or(own_prog.as_ref());

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

        String::from_utf8(buf).with_context(|| format!("file not utf-8: {}", path.display()))
    }

    async fn write(
        &self,
        path: &Path,
        content: &str,
        progress: Option<&ProgressFn>,
    ) -> anyhow::Result<()> {
        let (bar, own_prog) = if progress.is_none() && std::io::stdout().is_terminal() {
            let (b, p) = progress_for_write(path, content.len() as u64);
            (Some(b), Some(p))
        } else {
            (None, None)
        };
        let progress = progress.or(own_prog.as_ref());

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create parent dirs for {}", path.display()))?;
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
