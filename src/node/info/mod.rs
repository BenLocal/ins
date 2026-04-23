pub(crate) mod exec;
pub(crate) mod gpu;
pub(crate) mod system;

use async_trait::async_trait;
use serde_json::Value;

use crate::node::types::NodeRecord;

/// An extensible probe that collects a JSON blob of node info.
///
/// Built-in implementations: [`system::SystemProbe`] and [`gpu::GpuProbe`].
/// Probes are invoked lazily from Jinja templates via the `system_info()` /
/// `gpu_info()` functions registered in `pipeline::template`, with per-probe
/// caching so unused probes never trigger SSH.
#[async_trait]
pub(crate) trait NodeInfoProbe: Send + Sync {
    /// Stable identifier used as the top-level JSON key for this probe.
    #[allow(dead_code)]
    fn name(&self) -> &str;
    async fn probe(&self, node: &NodeRecord) -> anyhow::Result<Value>;
}

#[cfg(test)]
#[path = "info_test.rs"]
mod info_test;
