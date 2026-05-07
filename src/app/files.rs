use anyhow::{Context, anyhow};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Text,
    Directory,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct TreeEntry {
    pub relative_path: String,
    pub kind: FileKind,
}

fn safe_join(app_dir: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    if rel.is_empty() {
        return Err(anyhow!("invalid relative path: empty"));
    }
    if rel.contains('\0') {
        return Err(anyhow!("invalid relative path: contains NUL"));
    }
    let candidate = Path::new(rel);
    if candidate.is_absolute() {
        return Err(anyhow!("invalid relative path: absolute path '{rel}'"));
    }
    for component in candidate.components() {
        use std::path::Component;
        match component {
            Component::Normal(_) => {}
            _ => return Err(anyhow!("invalid relative path: '{rel}'")),
        }
    }
    Ok(app_dir.join(candidate))
}

#[allow(dead_code)]
pub async fn read_file(app_dir: &Path, rel: &str) -> anyhow::Result<String> {
    let path = safe_join(app_dir, rel)?;
    fs::read_to_string(&path)
        .await
        .with_context(|| format!("read app file {}", path.display()))
}

pub async fn write_file(app_dir: &Path, rel: &str, contents: &str) -> anyhow::Result<()> {
    let path = safe_join(app_dir, rel)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create app parent {}", parent.display()))?;
    }
    fs::write(&path, contents)
        .await
        .with_context(|| format!("write app file {}", path.display()))
}

pub async fn create_file(app_dir: &Path, rel: &str, kind: FileKind) -> anyhow::Result<PathBuf> {
    let path = safe_join(app_dir, rel)?;
    match kind {
        FileKind::Directory => fs::create_dir_all(&path)
            .await
            .with_context(|| format!("create app directory {}", path.display()))?,
        FileKind::Text => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("create app parent {}", parent.display()))?;
            }
            fs::write(&path, "")
                .await
                .with_context(|| format!("create app file {}", path.display()))?;
        }
    }
    Ok(path)
}

pub async fn delete_file(app_dir: &Path, rel: &str) -> anyhow::Result<()> {
    let path = safe_join(app_dir, rel)?;
    let metadata = fs::metadata(&path)
        .await
        .with_context(|| format!("read metadata {}", path.display()))?;
    if metadata.is_dir() {
        fs::remove_dir_all(&path)
            .await
            .with_context(|| format!("remove app directory {}", path.display()))
    } else {
        fs::remove_file(&path)
            .await
            .with_context(|| format!("remove app file {}", path.display()))
    }
}

#[allow(dead_code)]
pub async fn list_tree(app_dir: &Path) -> anyhow::Result<Vec<TreeEntry>> {
    let mut out = Vec::new();
    let mut stack = vec![app_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = fs::read_dir(&dir)
            .await
            .with_context(|| format!("read dir {}", dir.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let rel = path
                .strip_prefix(app_dir)
                .map_err(|_| anyhow!("path escapes root"))?
                .to_string_lossy()
                .to_string();
            let ft = entry.file_type().await?;
            let kind = if ft.is_dir() {
                FileKind::Directory
            } else {
                FileKind::Text
            };
            out.push(TreeEntry {
                relative_path: rel,
                kind,
            });
            if ft.is_dir() {
                stack.push(path);
            }
        }
    }
    out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(out)
}

#[cfg(test)]
#[path = "files_test.rs"]
mod files_test;
