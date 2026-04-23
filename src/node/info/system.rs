//! System-level probe: OS, arch, kernel, hostname, CPU count.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::node::info::NodeInfoProbe;
use crate::node::info::exec::run_probe_script;
use crate::node::types::NodeRecord;

pub(crate) struct SystemProbe;

#[async_trait]
impl NodeInfoProbe for SystemProbe {
    fn name(&self) -> &str {
        "system"
    }

    async fn probe(&self, node: &NodeRecord) -> anyhow::Result<Value> {
        // One SSH/shell round-trip: echo each field on its own line with a tag.
        let script = "printf 'os=%s\\n' \"$(uname -s 2>/dev/null)\"; \
                      printf 'arch=%s\\n' \"$(uname -m 2>/dev/null)\"; \
                      printf 'kernel=%s\\n' \"$(uname -r 2>/dev/null)\"; \
                      printf 'hostname=%s\\n' \"$(hostname 2>/dev/null)\"; \
                      printf 'cpus=%s\\n' \"$(nproc 2>/dev/null || echo 1)\"";
        let stdout = run_probe_script(node, script).await?;
        Ok(parse_system_output(&stdout))
    }
}

pub(super) fn parse_system_output(stdout: &str) -> Value {
    let mut os = "";
    let mut arch = "";
    let mut kernel = "";
    let mut hostname = "";
    let mut cpus = "";
    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "os" => os = value.trim(),
            "arch" => arch = value.trim(),
            "kernel" => kernel = value.trim(),
            "hostname" => hostname = value.trim(),
            "cpus" => cpus = value.trim(),
            _ => {}
        }
    }
    json!({
        "os": os,
        "arch": arch,
        "kernel": kernel,
        "hostname": hostname,
        "cpus": cpus,
    })
}
