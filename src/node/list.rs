use std::path::Path;

use anyhow::Context;
use tokio::fs;

use crate::node::types::{NodeRecord, RemoteNodeRecord};

/// Resolve `(ip, extern_ip)` for a node by name. Pure / no logging — callers
/// decide what to do with `None` (fallback or warn). Both `env.rs` and the
/// template-side `services.<dep>` builder share this so the env vars and
/// template view stay consistent.
///
/// - `node_name == "local"`: returns `Some(("127.0.0.1", local_extern_ip || "127.0.0.1"))`.
/// - Match on a `Remote` node in `nodes`: returns `Some((r.ip, r.ip))`.
/// - Otherwise: returns `None`.
pub(crate) fn lookup_node_ips(
    node_name: &str,
    nodes: &[NodeRecord],
    local_extern_ip: Option<&str>,
) -> Option<(String, String)> {
    if node_name == "local" {
        let ip = "127.0.0.1".to_string();
        let extern_ip = local_extern_ip
            .map(str::to_string)
            .unwrap_or_else(|| "127.0.0.1".to_string());
        return Some((ip, extern_ip));
    }
    for node in nodes {
        if let NodeRecord::Remote(r) = node
            && r.name == node_name
        {
            return Some((r.ip.clone(), r.ip.clone()));
        }
    }
    None
}

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
