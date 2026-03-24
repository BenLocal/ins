use std::path::Path;

use anyhow::Context;
use tokio::fs;

use crate::node::types::{NodeRecord, RemoteNodeRecord};

pub(crate) async fn load_all_nodes(nodes_path: &Path) -> anyhow::Result<Vec<NodeRecord>> {
    let mut nodes = vec![NodeRecord::Local()];
    nodes.extend(
        load_remote_nodes(nodes_path)
            .await?
            .into_iter()
            .map(NodeRecord::Remote),
    );
    Ok(nodes)
}

pub(crate) async fn load_remote_nodes(nodes_path: &Path) -> anyhow::Result<Vec<RemoteNodeRecord>> {
    if !fs::try_exists(nodes_path)
        .await
        .with_context(|| format!("check nodes file {}", nodes_path.display()))?
    {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(nodes_path)
        .await
        .with_context(|| format!("read nodes file {}", nodes_path.display()))?;

    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    serde_json::from_str(&content)
        .with_context(|| format!("parse nodes file {}", nodes_path.display()))
}
