use super::gpu::parse_gpu_output;
use super::system::parse_system_output;
use super::types::NodeInfo;

#[test]
fn system_probe_parses_structured_output() {
    let stdout = "os=Linux\n\
                  arch=x86_64\n\
                  kernel=5.15.0-25-generic\n\
                  hostname=node1\n\
                  cpus=8\n";
    let v = parse_system_output(stdout);
    assert_eq!(v["os"], "Linux");
    assert_eq!(v["arch"], "x86_64");
    assert_eq!(v["kernel"], "5.15.0-25-generic");
    assert_eq!(v["hostname"], "node1");
    assert_eq!(v["cpus"], "8");
}

#[test]
fn gpu_probe_parses_nvidia_smi_output() {
    let stdout = "vendor=nvidia\n\
                  NVIDIA A100 80GB PCIe, 550.54.15\n\
                  NVIDIA A100 80GB PCIe, 550.54.15\n";
    let v = parse_gpu_output(stdout);
    assert_eq!(v["vendor"], "nvidia");
    assert_eq!(v["count"], 2);
    assert_eq!(v["models"][0], "NVIDIA A100 80GB PCIe");
    assert_eq!(v["driver"], "550.54.15");
}

#[test]
fn gpu_probe_detects_no_gpu() {
    let v = parse_gpu_output("vendor=none\n");
    assert_eq!(v["vendor"], "none");
    assert_eq!(v["count"], 0);
    assert!(v["models"].as_array().unwrap().is_empty());
    assert!(v["driver"].is_null());
}

#[test]
fn node_info_from_probes_combines_system_and_gpu() {
    let system = parse_system_output("os=Linux\narch=x86_64\ncpus=16\nhostname=h\nkernel=6.1\n");
    let gpu = parse_gpu_output("vendor=nvidia\nNVIDIA A100, 550.0\n");
    let info = NodeInfo::from_probes(&system, Some(&gpu));
    assert_eq!(info.os, "Linux");
    assert_eq!(info.cpus, "16");
    let gpu = info.gpu.expect("gpu present");
    assert_eq!(gpu.count, 1);
    assert_eq!(gpu.models[0], "NVIDIA A100");
    assert_eq!(gpu.driver.as_deref(), Some("550.0"));
}

#[test]
fn node_info_from_probes_drops_gpu_when_absent() {
    let system = parse_system_output("os=Linux\narch=x86_64\n");
    let gpu = parse_gpu_output("vendor=none\n");
    let info = NodeInfo::from_probes(&system, Some(&gpu));
    assert!(info.gpu.is_none());
}

#[test]
fn env_pairs_flatten_typed_fields_plus_extra() {
    let mut info = NodeInfo {
        os: "Linux".into(),
        arch: "x86_64".into(),
        kernel: "6.1".into(),
        hostname: "h".into(),
        cpus: "8".into(),
        gpu: Some(super::GpuInfo {
            vendor: "nvidia".into(),
            count: 2,
            models: vec!["A100".into(), "A100".into()],
            driver: Some("550".into()),
        }),
        extra: Default::default(),
    };
    info.extra.insert("datacenter".into(), "us-west-2".into());
    let env = info.to_env_pairs();
    assert_eq!(env.get("INS_NODE_OS"), Some(&"Linux".to_string()));
    assert_eq!(env.get("INS_NODE_ARCH"), Some(&"x86_64".to_string()));
    assert_eq!(env.get("INS_NODE_CPUS"), Some(&"8".to_string()));
    assert_eq!(env.get("INS_NODE_GPU_VENDOR"), Some(&"nvidia".to_string()));
    assert_eq!(env.get("INS_NODE_GPU_COUNT"), Some(&"2".to_string()));
    assert_eq!(env.get("INS_NODE_GPU_MODEL"), Some(&"A100".to_string()));
    assert_eq!(env.get("INS_NODE_GPU_DRIVER"), Some(&"550".to_string()));
    assert_eq!(
        env.get("INS_NODE_DATACENTER"),
        Some(&"us-west-2".to_string())
    );
}

#[test]
fn env_pairs_omit_gpu_when_missing() {
    let info = NodeInfo {
        os: "Linux".into(),
        ..Default::default()
    };
    let env = info.to_env_pairs();
    assert!(!env.contains_key("INS_NODE_GPU_VENDOR"));
    assert!(!env.contains_key("INS_NODE_GPU_COUNT"));
}
