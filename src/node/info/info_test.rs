use super::gpu::parse_gpu_output;
use super::system::parse_system_output;

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
