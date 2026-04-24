use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, anyhow};
use serde_yaml::from_str;
use tokio::fs;

use crate::app::types::{AppFileEntry, AppRecord};

pub async fn load_app_record(
    qa_file: &Path,
    extra_env: &BTreeMap<String, String>,
) -> anyhow::Result<AppRecord> {
    let content = fs::read_to_string(qa_file)
        .await
        .with_context(|| format!("read app file {}", qa_file.display()))?;

    let expanded = expand_env_vars(&content, extra_env)
        .with_context(|| format!("expand env vars in {}", qa_file.display()))?;

    let mut record: AppRecord =
        from_str(&expanded).with_context(|| format!("parse app file {}", qa_file.display()))?;
    record.files = Some(load_app_files(qa_file).await?);
    Ok(record)
}

/// Expand shell-style env var references inside qa.yaml content.
///
/// Lookup order: `extra_env` first (typically `config.env_for(node)` merging
/// `[defaults.env]` + `[nodes.<n>.env]` from config.toml), then the process
/// environment. This lets users pin per-node overrides in config without
/// leaking them into their shell.
///
/// Supported syntax:
/// - `${NAME}` — substitute `NAME` from env; error if unset.
/// - `${NAME:-fallback}` — use `fallback` when `NAME` is not set.
/// - `$$` — literal `$` (escape).
///
/// A bare `$foo` (no braces) is left untouched so Jinja templates embedded in
/// the qa.yaml — which use their own `{{ }}` syntax — keep working.
pub(crate) fn expand_env_vars(
    content: &str,
    extra_env: &BTreeMap<String, String>,
) -> anyhow::Result<String> {
    let mut out = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                chars.next();
                out.push('$');
            }
            Some('{') => {
                chars.next();
                let mut spec = String::new();
                let mut closed = false;
                for next in chars.by_ref() {
                    if next == '}' {
                        closed = true;
                        break;
                    }
                    spec.push(next);
                }
                if !closed {
                    return Err(anyhow!(
                        "unterminated env var reference '${{{}...' in qa.yaml",
                        spec
                    ));
                }
                let (name, fallback) = match spec.split_once(":-") {
                    Some((n, f)) => (n.trim(), Some(f)),
                    None => (spec.trim(), None),
                };
                if name.is_empty() {
                    return Err(anyhow!("empty env var name in qa.yaml: '${{{}}}'", spec));
                }
                let value = if let Some(v) = extra_env.get(name) {
                    v.clone()
                } else {
                    match std::env::var(name) {
                        Ok(v) => v,
                        Err(_) => match fallback {
                            Some(f) => f.to_string(),
                            None => {
                                return Err(anyhow!(
                                    "env var '{}' referenced in qa.yaml but not set; \
                                     use ${{{}:-default}} to provide a fallback \
                                     or add it under [defaults.env] / [nodes.<n>.env] in config.toml",
                                    name,
                                    name
                                ));
                            }
                        },
                    }
                };
                out.push_str(&value);
            }
            _ => out.push('$'),
        }
    }
    Ok(out)
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
    let qa_file_name = qa_file.file_name().map(|name| name.to_owned());

    while let Some(entry) = entries
        .next_entry()
        .await
        .with_context(|| format!("iterate app dir {}", app_dir.display()))?
    {
        let path = entry.path();
        if path == qa_file
            || qa_file_name
                .as_ref()
                .is_some_and(|qa_name| entry.file_name() == *qa_name)
        {
            continue;
        }
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

#[cfg(test)]
#[path = "parse_test.rs"]
mod parse_test;
