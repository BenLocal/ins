use std::path::{Path, PathBuf};

use anyhow::Context;
use tokio::fs;

use super::types::{InsConfig, NodeConfig};

pub(crate) fn config_file(home: &Path) -> PathBuf {
    home.join("config.toml")
}

pub(crate) async fn load_config(home: &Path) -> anyhow::Result<InsConfig> {
    let path = config_file(home);
    if !fs::try_exists(&path)
        .await
        .with_context(|| format!("check config file {}", path.display()))?
    {
        return Ok(InsConfig::default());
    }
    let content = fs::read_to_string(&path)
        .await
        .with_context(|| format!("read config file {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("parse config file {}", path.display()))
}

pub(crate) async fn save_config(home: &Path, config: &InsConfig) -> anyhow::Result<()> {
    let path = config_file(home);
    let content = toml::to_string_pretty(config).context("serialize config.toml")?;
    fs::write(&path, content)
        .await
        .with_context(|| format!("write config file {}", path.display()))?;
    Ok(())
}

/// Record a node's workspace into config.toml if no per-node entry exists.
/// Read-modify-write against the on-disk file (doesn't mutate the in-memory snapshot).
pub(crate) async fn persist_node_workspace_if_missing(
    home: &Path,
    node: &str,
    workspace: &str,
) -> anyhow::Result<()> {
    let mut current = load_config(home).await?;
    if current.has_node_workspace(node) {
        return Ok(());
    }
    let entry = current
        .nodes
        .entry(node.to_string())
        .or_insert_with(NodeConfig::default);
    entry.workspace = Some(workspace.to_string());
    save_config(home, &current).await
}
