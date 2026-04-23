use super::resolve_node;
use crate::node::types::{NodeRecord, RemoteNodeRecord};

fn remote(name: &str) -> NodeRecord {
    NodeRecord::Remote(RemoteNodeRecord {
        name: name.into(),
        ip: "127.0.0.1".into(),
        port: 22,
        user: "root".into(),
        password: "".into(),
        key_path: None,
    })
}

#[test]
fn resolve_node_defaults_to_local_when_unspecified() {
    let nodes = vec![NodeRecord::Local(), remote("node1")];
    let node = resolve_node(&nodes, None).expect("resolved");
    assert!(matches!(node, NodeRecord::Local()));
}

#[test]
fn resolve_node_matches_remote_name() {
    let nodes = vec![NodeRecord::Local(), remote("node1"), remote("node2")];
    let node = resolve_node(&nodes, Some("node2")).expect("resolved");
    match node {
        NodeRecord::Remote(r) => assert_eq!(r.name, "node2"),
        _ => panic!("expected remote node2"),
    }
}

#[test]
fn resolve_node_errors_on_unknown_name() {
    let nodes = vec![NodeRecord::Local()];
    let err = resolve_node(&nodes, Some("ghost")).expect_err("should fail");
    assert!(err.to_string().contains("node 'ghost' not found"));
}
