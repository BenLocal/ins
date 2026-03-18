use std::path::Path;

use anyhow::{Context, anyhow};
use serde_yaml::from_str;
use tokio::fs;

use crate::app::types::{AppFileEntry, AppRecord};

pub async fn load_app_record(qa_file: &Path) -> anyhow::Result<AppRecord> {
    let content = fs::read_to_string(qa_file)
        .await
        .with_context(|| format!("read app file {}", qa_file.display()))?;

    let mut record: AppRecord =
        from_str(&content).with_context(|| format!("parse app file {}", qa_file.display()))?;
    record.files = Some(load_app_files(qa_file).await?);
    Ok(record)
}

async fn load_app_files(qa_file: &Path) -> anyhow::Result<Vec<AppFileEntry>> {
    let Some(app_dir) = qa_file.parent() else {
        return Err(anyhow!(
            "app file '{}' has no parent directory",
            qa_file.display()
        ));
    };

    let mut entries = fs::read_dir(app_dir)
        .await
        .with_context(|| format!("read app dir {}", app_dir.display()))?;
    let mut files = Vec::new();

    while let Some(entry) = entries
        .next_entry()
        .await
        .with_context(|| format!("iterate app dir {}", app_dir.display()))?
    {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .await
            .with_context(|| format!("read file type {}", path.display()))?;

        files.push(AppFileEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: path.display().to_string(),
            is_dir: file_type.is_dir(),
        });
    }

    files.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(files)
}
