use std::io::IsTerminal;
use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::fs;

use crate::file::ProgressFn;

use super::COPY_CONCURRENCY;

pub(crate) struct CopyAppProgress {
    _multi: Arc<MultiProgress>,
    total_files: u64,
    app_bar: ProgressBar,
    file_bars: Vec<ProgressBar>,
}

#[derive(Clone)]
pub(crate) struct CopyProgressSlot {
    app_bar: ProgressBar,
    file_bar: ProgressBar,
}

impl CopyAppProgress {
    pub(crate) async fn new(
        app_name: &str,
        _service: &str,
        source_dir: &Path,
        target_dir: &Path,
        enable: bool,
    ) -> anyhow::Result<Option<Arc<Self>>> {
        if !enable || !std::io::stdout().is_terminal() {
            return Ok(None);
        }

        let total_files = count_files_recursive(source_dir).await?;
        let multi = Arc::new(MultiProgress::new());
        let app_bar = multi.add(ProgressBar::new(total_files.max(1)));
        app_bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} {msg:<24} [{bar:24.cyan/blue}] {pos}/{len} files {elapsed_precise}",
            )
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        app_bar.set_message(format!("{app_name} -> {}", target_dir.display()));

        let mut file_bars = Vec::new();
        for index in 0..COPY_CONCURRENCY.min(total_files.max(1) as usize) {
            let file_bar = multi.add(ProgressBar::new_spinner());
            file_bar.set_style(
                ProgressStyle::with_template(
                    "   {spinner:.green} {msg:<64} {bytes}/{total_bytes} {elapsed_precise}",
                )
                .unwrap(),
            );
            file_bar.set_message(format!(
                "Waiting {}/{} in {}",
                index + 1,
                COPY_CONCURRENCY,
                target_dir.display()
            ));
            file_bar.finish_and_clear();
            file_bars.push(file_bar);
        }

        Ok(Some(Arc::new(Self {
            _multi: multi,
            total_files: total_files.max(1),
            app_bar,
            file_bars,
        })))
    }

    pub(crate) fn slot(&self, index: usize) -> CopyProgressSlot {
        CopyProgressSlot {
            app_bar: self.app_bar.clone(),
            file_bar: self.file_bars[index].clone(),
        }
    }

    pub(crate) fn finish(&self) {
        for file_bar in &self.file_bars {
            file_bar.finish_and_clear();
        }
        if self.total_files == 0 {
            self.app_bar.set_length(0);
        }
        self.app_bar.finish_with_message("Copy complete");
    }
}

impl CopyProgressSlot {
    pub(crate) fn start_copy(&self, path: &Path, size: u64) {
        self.file_bar.reset();
        self.file_bar.reset_elapsed();
        self.file_bar
            .enable_steady_tick(std::time::Duration::from_millis(100));
        self.file_bar.set_length(size.max(1));
        self.file_bar.set_position(0);
        self.file_bar
            .set_message(format!("Copying {}", path.display()));
    }

    pub(crate) fn start_template(&self, path: &Path) {
        self.file_bar.reset();
        self.file_bar.reset_elapsed();
        self.file_bar
            .enable_steady_tick(std::time::Duration::from_millis(100));
        self.file_bar.set_length(0);
        self.file_bar.set_position(0);
        self.file_bar
            .set_message(format!("Rendering {}", path.display()));
    }

    pub(crate) fn begin_write_phase(&self, size: u64) {
        self.file_bar.set_length(size.max(1));
        self.file_bar.set_position(0);
    }

    pub(crate) fn write_progress(&self) -> ProgressFn {
        let file_bar = self.file_bar.clone();
        Arc::new(move |current, total| {
            let target = total.max(1);
            file_bar.set_length(target);
            file_bar.set_position(current.min(target));
        })
    }

    pub(crate) fn finish_file(&self) {
        self.file_bar.disable_steady_tick();
        self.file_bar.finish_and_clear();
        self.app_bar.inc(1);
    }
}

async fn count_files_recursive(root: &Path) -> anyhow::Result<u64> {
    let mut count = 0u64;
    let mut stack = vec![root.to_path_buf()];

    while let Some(current) = stack.pop() {
        let mut entries = fs::read_dir(&current)
            .await
            .map_err(|e| anyhow!("read source dir {}: {}", current.display(), e))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| anyhow!("iterate source dir {}: {}", current.display(), e))?
        {
            let path = entry.path();
            let file_type = entry
                .file_type()
                .await
                .map_err(|e| anyhow!("read file type {}: {}", path.display(), e))?;
            if file_type.is_dir() {
                stack.push(path);
            } else {
                count += 1;
            }
        }
    }

    Ok(count)
}
