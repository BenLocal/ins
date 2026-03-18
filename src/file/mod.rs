use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use indicatif::{ProgressBar, ProgressStyle};

pub mod local;
pub mod remote;

/// Callback for progress: (current_bytes, total_bytes). total=0 means unknown (spinner).
pub type ProgressFn = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// Builds a progress bar and callback for file read (total unknown until metadata).
pub fn progress_for_read(path: &Path) -> (ProgressBar, ProgressFn) {
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
pub fn progress_for_write(path: &Path, total: u64) -> (ProgressBar, ProgressFn) {
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
pub trait FileTrait {
    /// Read file; progress callback is optional: (current, total), total=0 if unknown.
    async fn read(&self, path: &Path, progress: Option<&ProgressFn>) -> anyhow::Result<String>;
    /// Write file; progress callback is optional.
    async fn write(
        &self,
        path: &Path,
        content: &str,
        progress: Option<&ProgressFn>,
    ) -> anyhow::Result<()>;
}
