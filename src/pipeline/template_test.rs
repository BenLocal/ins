use super::{build_services_template_value, build_target_template_values};
use crate::app::types::AppRecord;
use crate::execution_output::ExecutionOutput;
use crate::node::types::{NodeRecord, RemoteNodeRecord};
use crate::provider::DeploymentTarget;
use crate::store::duck::InstalledServiceConfigRecord;
use serde_json::json;
use std::collections::HashMap;

fn target() -> DeploymentTarget {
    DeploymentTarget::new(
        AppRecord {
            name: "demo".into(),
            ..AppRecord::default()
        },
        "web".into(),
    )
}

fn target_with_deps(deps: &[&str]) -> DeploymentTarget {
    DeploymentTarget::new(
        AppRecord {
            name: "demo".into(),
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
            ..AppRecord::default()
        },
        "web".into(),
    )
}

fn redis_record(node_name: &str, namespace: &str, port: i64) -> InstalledServiceConfigRecord {
    let mut values = HashMap::new();
    values.insert("port".into(), json!(port));
    InstalledServiceConfigRecord {
        service: "redis".into(),
        namespace: namespace.into(),
        app_name: "redis".into(),
        node_name: node_name.into(),
        workspace: format!("/srv/{node_name}/redis"),
        app_values: values,
        created_at_ms: 1,
    }
}

#[test]
fn local_node_template_value_uses_loopback_ip() {
    let output = ExecutionOutput::stdout();
    let values = build_target_template_values(
        &target(),
        &NodeRecord::Local(),
        "default",
        Some("203.0.113.5"),
        &[],
        &[],
        &[],
        &output,
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
    let output = ExecutionOutput::stdout();
    let values =
        build_target_template_values(&target(), &node, "default", None, &[], &[], &[], &output)
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
    let output = ExecutionOutput::stdout();
    let values =
        build_target_template_values(&target(), &node, "default", None, &[], &[], &[], &output)
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
    let output = ExecutionOutput::stdout();
    build_target_template_values(
        &target(),
        &NodeRecord::Local(),
        "default",
        None,
        &[],
        &[],
        &[],
        &output,
    )
    .expect("template values");
}

// --- services template value tests ---

#[test]
fn services_template_includes_default_ns_dep_with_remote_ip() {
    let nodes = vec![NodeRecord::Remote(RemoteNodeRecord {
        name: "node-a".into(),
        ip: "10.0.0.1".into(),
        port: 22,
        user: "root".into(),
        password: "".into(),
        key_path: None,
    })];
    let installed = vec![redis_record("node-a", "default", 6379)];
    let output = ExecutionOutput::stdout();

    let values = build_target_template_values(
        &target_with_deps(&["redis"]),
        &nodes[0].clone(),
        "default",
        None,
        &[],
        &installed,
        &nodes,
        &output,
    )
    .unwrap();
    let svc = values.pointer("/services/redis").expect("services.redis");
    assert_eq!(svc["service"], json!("redis"));
    assert_eq!(svc["ip"], json!("10.0.0.1"));
    assert_eq!(svc["extern_ip"], json!("10.0.0.1"));
    assert_eq!(svc["node_name"], json!("node-a"));
    assert_eq!(svc["values"]["port"], json!(6379));
}

#[test]
fn services_template_uses_loopback_and_local_extern_ip_for_local_dep() {
    let installed = vec![redis_record("local", "default", 6379)];
    let output = ExecutionOutput::stdout();
    let values = build_target_template_values(
        &target_with_deps(&["redis"]),
        &NodeRecord::Local(),
        "default",
        Some("203.0.113.5"),
        &[],
        &installed,
        &[],
        &output,
    )
    .unwrap();
    let svc = values.pointer("/services/redis").unwrap();
    assert_eq!(svc["ip"], json!("127.0.0.1"));
    assert_eq!(svc["extern_ip"], json!("203.0.113.5"));
}

#[test]
fn services_template_uses_namespaced_key_for_explicit_ns_dep() {
    let nodes = vec![NodeRecord::Remote(RemoteNodeRecord {
        name: "node-staging".into(),
        ip: "10.0.0.7".into(),
        port: 22,
        user: "root".into(),
        password: "".into(),
        key_path: None,
    })];
    let installed = vec![redis_record("node-staging", "staging", 6380)];
    let output = ExecutionOutput::stdout();

    let values = build_target_template_values(
        &target_with_deps(&["staging:redis"]),
        &nodes[0].clone(),
        "default",
        None,
        &[],
        &installed,
        &nodes,
        &output,
    )
    .unwrap();
    assert!(
        values.pointer("/services/redis").is_none(),
        "must not appear under bare key"
    );
    let svc = values.pointer("/services/staging_redis").unwrap();
    assert_eq!(svc["namespace"], json!("staging"));
    assert_eq!(svc["ip"], json!("10.0.0.7"));
}

#[test]
fn services_template_skips_uninstalled_deps() {
    let output = ExecutionOutput::stdout();
    let values = build_target_template_values(
        &target_with_deps(&["redis"]),
        &NodeRecord::Local(),
        "default",
        Some("203.0.113.5"),
        &[],
        &[],
        &[],
        &output,
    )
    .unwrap();
    let svc = values.get("services").unwrap();
    assert!(
        svc.as_object().unwrap().is_empty(),
        "uninstalled deps must not appear: {svc}"
    );
}

#[test]
fn services_template_falls_back_when_node_missing_from_nodes_json() {
    let installed = vec![redis_record("ghost-node", "default", 6379)];
    let output = ExecutionOutput::buffered();
    let values = build_target_template_values(
        &target_with_deps(&["redis"]),
        &NodeRecord::Local(),
        "default",
        Some("203.0.113.5"),
        &[],
        &installed,
        &[], // empty nodes list
        &output,
    )
    .unwrap();
    let svc = values.pointer("/services/redis").unwrap();
    assert_eq!(svc["ip"], json!("ghost-node"), "fallback uses node_name");
    assert_eq!(svc["extern_ip"], json!("ghost-node"));
    let snap = output.snapshot();
    assert!(
        snap.contains("warning: dep node 'ghost-node' not found"),
        "expected warning in output: {snap}"
    );
}

#[test]
fn services_template_skips_self_dependency_loop() {
    // App "demo" deployed as service "redis" in default ns.
    let mut target = target_with_deps(&["redis"]);
    target.service = "redis".into();
    let installed = vec![redis_record("local", "default", 6379)];
    let output = ExecutionOutput::stdout();

    let values = build_target_template_values(
        &target,
        &NodeRecord::Local(),
        "default",
        Some("203.0.113.5"),
        &[],
        &installed,
        &[],
        &output,
    )
    .unwrap();
    let svc = values.get("services").unwrap();
    assert!(
        svc.as_object().unwrap().is_empty(),
        "self-dep must not appear"
    );
}

#[test]
fn build_services_template_value_hyphenated_service_name_becomes_underscore_key() {
    let installed = vec![InstalledServiceConfigRecord {
        service: "mysql-main".into(),
        namespace: "default".into(),
        app_name: "mysql-main".into(),
        node_name: "local".into(),
        workspace: "/srv/local/mysql-main".into(),
        app_values: HashMap::new(),
        created_at_ms: 1,
    }];
    let app = AppRecord {
        name: "demo".into(),
        dependencies: vec!["mysql-main".into()],
        ..AppRecord::default()
    };
    let output = ExecutionOutput::stdout();
    let result = build_services_template_value(
        &app,
        "web",
        "default",
        &installed,
        &[],
        Some("1.2.3.4"),
        &output,
    )
    .unwrap();
    assert!(
        result.pointer("/mysql_main").is_some(),
        "hyphenated name should become underscore key: {result}"
    );
    assert_eq!(
        result.pointer("/mysql_main/service").unwrap(),
        &json!("mysql-main")
    );
}
