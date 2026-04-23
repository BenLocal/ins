//! GPU probe — currently detects NVIDIA via nvidia-smi.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::node::info::NodeInfoProbe;
use crate::node::info::exec::run_probe_script;
use crate::node::types::NodeRecord;

pub(crate) struct GpuProbe;

#[async_trait]
impl NodeInfoProbe for GpuProbe {
    fn name(&self) -> &str {
        "gpu"
    }

    async fn probe(&self, node: &NodeRecord) -> anyhow::Result<Value> {
        // Probe nvidia-smi if available. If the binary is missing the outer
        // script still exits 0 (so one SSH round-trip covers both branches);
        // we distinguish by inspecting stdout.
        let script = "if command -v nvidia-smi >/dev/null 2>&1; then \
                          printf 'vendor=nvidia\\n'; \
                          nvidia-smi --query-gpu=name,driver_version --format=csv,noheader 2>/dev/null; \
                      else \
                          printf 'vendor=none\\n'; \
                      fi";
        let stdout = run_probe_script(node, script).await?;
        Ok(parse_gpu_output(&stdout))
    }
}

pub(super) fn parse_gpu_output(stdout: &str) -> Value {
    let mut vendor = "none";
    let mut models: Vec<String> = Vec::new();
    let mut driver: Option<String> = None;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("vendor=") {
            vendor = match rest {
                "nvidia" => "nvidia",
                _ => "none",
            };
            continue;
        }
        // nvidia-smi --query-gpu=name,driver_version --format=csv,noheader
        // → "NVIDIA A100 80GB PCIe, 550.54.15"
        let (name, ver) = trimmed.split_once(',').unwrap_or((trimmed, ""));
        models.push(name.trim().to_string());
        let ver = ver.trim();
        if !ver.is_empty() && driver.is_none() {
            driver = Some(ver.to_string());
        }
    }
    if vendor == "none" || models.is_empty() {
        let empty: Vec<String> = Vec::new();
        return json!({
            "vendor": "none",
            "count": 0,
            "models": empty,
            "driver": Value::Null,
        });
    }
    json!({
        "vendor": vendor,
        "count": models.len() as u32,
        "models": models,
        "driver": driver,
    })
}
