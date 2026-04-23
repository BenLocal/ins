use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct NodeInfo {
    pub(crate) os: String,
    pub(crate) arch: String,
    pub(crate) kernel: String,
    pub(crate) hostname: String,
    pub(crate) cpus: String,
    pub(crate) gpu: Option<GpuInfo>,
    /// Open-ended extension map. Custom probes can stash arbitrary
    /// key=value pairs here without changing the struct definition.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) extra: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct GpuInfo {
    pub(crate) vendor: String,
    pub(crate) count: u32,
    pub(crate) models: Vec<String>,
    pub(crate) driver: Option<String>,
}

impl NodeInfo {
    pub(crate) fn from_probes(system: &Value, gpu: Option<&Value>) -> Self {
        let s = |key: &str| {
            system
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let gpu = gpu
            .and_then(|v| serde_json::from_value::<GpuInfo>(v.clone()).ok())
            .filter(|g| g.count > 0);
        Self {
            os: s("os"),
            arch: s("arch"),
            kernel: s("kernel"),
            hostname: s("hostname"),
            cpus: s("cpus"),
            gpu,
            extra: BTreeMap::new(),
        }
    }

    /// Flatten node info into INS_NODE_* env vars injected per-service.
    pub(crate) fn to_env_pairs(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let mut insert_non_empty = |key: &str, value: &str| {
            if !value.is_empty() {
                out.insert(key.to_string(), value.to_string());
            }
        };
        insert_non_empty("INS_NODE_OS", &self.os);
        insert_non_empty("INS_NODE_ARCH", &self.arch);
        insert_non_empty("INS_NODE_KERNEL", &self.kernel);
        insert_non_empty("INS_NODE_HOSTNAME", &self.hostname);
        insert_non_empty("INS_NODE_CPUS", &self.cpus);
        if let Some(gpu) = &self.gpu {
            out.insert("INS_NODE_GPU_VENDOR".into(), gpu.vendor.clone());
            out.insert("INS_NODE_GPU_COUNT".into(), gpu.count.to_string());
            if let Some(model) = gpu.models.first() {
                out.insert("INS_NODE_GPU_MODEL".into(), model.clone());
            }
            if let Some(driver) = &gpu.driver {
                out.insert("INS_NODE_GPU_DRIVER".into(), driver.clone());
            }
        }
        for (k, v) in &self.extra {
            out.insert(format!("INS_NODE_{}", k.to_ascii_uppercase()), v.clone());
        }
        out
    }
}

