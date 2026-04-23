pub(crate) mod exec;
pub(crate) mod gpu;
pub(crate) mod system;
pub(crate) mod types;

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::time::timeout;

use crate::node::types::NodeRecord;

pub(crate) use types::NodeInfo;
#[cfg(test)]
pub(crate) use types::GpuInfo;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// An extensible probe that collects a JSON blob of node info.
///
/// Built-in implementations: [`system::SystemProbe`] and [`gpu::GpuProbe`].
/// To add a new source (hardware dump, cloud metadata, …), implement this
/// trait and extend [`probe_node_info`] to merge its output into `NodeInfo`.
#[async_trait]
pub(crate) trait NodeInfoProbe: Send + Sync {
    /// Stable identifier used as the top-level JSON key for this probe.
    #[allow(dead_code)]
    fn name(&self) -> &str;
    async fn probe(&self, node: &NodeRecord) -> anyhow::Result<Value>;
}

/// Run all built-in probes against the node and return a combined [`NodeInfo`].
///
/// Probe failures are non-fatal: a warning is emitted and missing fields fall
/// back to defaults. Rationale — a flaky SSH connection shouldn't abort
/// `ins check` / `ins deploy` before the user sees the real failure from the
/// downstream copy / ensure_volumes step (which has a clearer error message).
pub(crate) async fn probe_node_info(node: &NodeRecord) -> NodeInfo {
    let system = match timeout(PROBE_TIMEOUT, system::SystemProbe.probe(node)).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            eprintln!("warning: node-info system probe failed: {e}");
            serde_json::json!({})
        }
        Err(_) => {
            eprintln!(
                "warning: node-info system probe timed out after {:?}",
                PROBE_TIMEOUT
            );
            serde_json::json!({})
        }
    };
    let gpu = match timeout(PROBE_TIMEOUT, gpu::GpuProbe.probe(node)).await {
        Ok(Ok(v)) => Some(v),
        _ => None,
    };
    NodeInfo::from_probes(&system, gpu.as_ref())
}

#[cfg(test)]
#[path = "info_test.rs"]
mod info_test;
