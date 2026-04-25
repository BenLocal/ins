use super::build_target_template_values;
use crate::app::types::AppRecord;
use crate::node::types::{NodeRecord, RemoteNodeRecord};
use crate::provider::DeploymentTarget;

fn target() -> DeploymentTarget {
    DeploymentTarget::new(
        AppRecord {
            name: "demo".into(),
            ..AppRecord::default()
        },
        "web".into(),
    )
}

#[test]
fn local_node_template_value_uses_loopback_ip() {
    let values = build_target_template_values(
        &target(),
        &NodeRecord::Local(),
        "default",
        Some("203.0.113.5"),
        &[],
    )
    .expect("template values");
    let node = values.get("node").expect("node key");
    assert_eq!(node.get("name").and_then(|v| v.as_str()), Some("local"));
    assert_eq!(node.get("ip").and_then(|v| v.as_str()), Some("127.0.0.1"));
    assert_eq!(
        node.get("extern_ip").and_then(|v| v.as_str()),
        Some("203.0.113.5")
    );
}

#[test]
fn remote_node_template_value_uses_registered_name_and_ip() {
    let node = NodeRecord::Remote(RemoteNodeRecord {
        name: "node-a".into(),
        ip: "10.0.0.1".into(),
        port: 22,
        user: "root".into(),
        password: "secret".into(),
        key_path: None,
    });
    let values = build_target_template_values(&target(), &node, "default", None, &[])
        .expect("template values");
    let node_v = values.get("node").expect("node key");
    assert_eq!(node_v.get("name").and_then(|v| v.as_str()), Some("node-a"));
    assert_eq!(node_v.get("ip").and_then(|v| v.as_str()), Some("10.0.0.1"));
    assert_eq!(
        node_v.get("extern_ip").and_then(|v| v.as_str()),
        Some("10.0.0.1"),
        "remote node extern_ip should equal node.ip"
    );
}

#[test]
fn remote_node_template_value_does_not_expose_secrets() {
    let node = NodeRecord::Remote(RemoteNodeRecord {
        name: "node-a".into(),
        ip: "10.0.0.1".into(),
        port: 22,
        user: "root".into(),
        password: "super-secret-pwd".into(),
        key_path: Some("/home/user/.ssh/id_rsa".into()),
    });
    let values = build_target_template_values(&target(), &node, "default", None, &[])
        .expect("template values");
    let serialized = serde_json::to_string(&values).expect("serialize");
    assert!(
        !serialized.contains("super-secret-pwd"),
        "password leaked into template values: {serialized}"
    );
    assert!(
        !serialized.contains("id_rsa"),
        "key path leaked into template values: {serialized}"
    );
}

#[test]
#[should_panic(expected = "local_extern_ip must be resolved before template build for local node")]
fn local_node_template_value_panics_when_extern_ip_missing() {
    build_target_template_values(&target(), &NodeRecord::Local(), "default", None, &[])
        .expect("template values");
}
