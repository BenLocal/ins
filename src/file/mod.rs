use std::io::IsTerminal;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use indicatif::{ProgressBar, ProgressStyle};

pub mod local;
pub mod remote;

/// Callback for progress: (current_bytes, total_bytes). total=0 means unknown (spinner).
pub type ProgressFn = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// Resolves progress for read: if caller passed None and stdout is TTY, creates a bar and callback.
/// Returns (bar to finish on done, progress to use). Caller must call `bar.finish_with_message("Done")` when done.
pub fn with_read_progress(path: &Path, progress: Option<&ProgressFn>) -> (Option<ProgressBar>, Option<ProgressFn>) {
    if let Some(p) = progress {
        return (None, Some(p.clone()));
    }
    if !std::io::stdout().is_terminal() {
        return (None, None);
    }
    let (bar, cb) = progress_for_read(path);
    (Some(bar), Some(cb))
}

/// Resolves progress for write: if caller passed None and stdout is TTY, creates a bar and callback.
pub fn with_write_progress(
    path: &Path,
    total: u64,
    progress: Option<&ProgressFn>,
) -> (Option<ProgressBar>, Option<ProgressFn>) {
    if let Some(p) = progress {
        return (None, Some(p.clone()));
    }
    if !std::io::stdout().is_terminal() {
        return (None, None);
    }
    let (bar, cb) = progress_for_write(path, total);
    (Some(bar), Some(cb))
}

/// Builds a progress bar and callback for file read (total unknown until metadata).
fn progress_for_read(path: &Path) -> (ProgressBar, ProgressFn) {
    let msg = path.display().to_string();
    let bar = ProgressBar::new_spinner()
        .with_style(ProgressStyle::default_spinner().template("{spinner:.dim} {msg}").unwrap())
        .with_message(msg);
    let bar_clone = bar.clone();
    let cb: ProgressFn = Arc::new(move |current, total| {
        if total > 0 {
            bar_clone.set_length(total);
            bar_clone.set_position(current);
        } else {
            bar_clone.inc(1);
        }
    });
    (bar, cb)
}

/// Builds a progress bar and callback for file write (total known).
fn progress_for_write(path: &Path, total: u64) -> (ProgressBar, ProgressFn) {
    let msg = path.display().to_string();
    let bar = ProgressBar::new(total)
        .with_style(
            ProgressStyle::default_bar()
                .template("{spinner:.dim} {msg}: {bar:40.cyan/blue} {pos}/{len}")
                .unwrap()
                .progress_chars("=>-"),
        )
        .with_message(msg);
    let bar_clone = bar.clone();
    let cb: ProgressFn = Arc::new(move |current, _| {
        bar_clone.set_position(current);
    });
    (bar, cb)
}

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
