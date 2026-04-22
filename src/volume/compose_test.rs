use super::inject_compose_volumes;
use crate::app::types::{AppRecord, ScriptHook};
use crate::volume::types::{CifsVolume, FilesystemVolume, VolumeRecord};

fn fs(name: &str, node: &str, path: &str) -> VolumeRecord {
    VolumeRecord::Filesystem(FilesystemVolume {
        name: name.into(),
        node: node.into(),
        path: path.into(),
    })
}

fn cifs(name: &str, node: &str, server: &str, username: &str, password: &str) -> VolumeRecord {
    VolumeRecord::Cifs(CifsVolume {
        name: name.into(),
        node: node.into(),
        server: server.into(),
        username: username.into(),
        password: password.into(),
    })
}

fn app(name: &str, volumes: &[&str], all_volume: bool) -> AppRecord {
    AppRecord {
        name: name.into(),
        version: None,
        description: None,
        author_name: None,
        author_email: None,
        dependencies: vec![],
        before: ScriptHook::default(),
        after: ScriptHook::default(),
        values: vec![],
        volumes: volumes.iter().map(|s| s.to_string()).collect(),
        all_volume,
        files: None,
    }
}

#[test]
fn returns_unchanged_when_app_declares_no_volumes() {
    let compose = "services:\n  web:\n    image: nginx\n";
    let app = app("demo", &[], false);
    let (rewritten, resolved) = inject_compose_volumes(compose, &app, "node1", &[]).expect("ok");
    assert_eq!(resolved.len(), 0);
    assert_eq!(rewritten, compose);
}

#[test]
fn ignores_compose_top_level_volumes_block_without_qa_declaration() {
    let compose = "services:\n  web: { image: nginx }\nvolumes:\n  data: {}\n";
    let volumes = vec![fs("data", "node1", "/mnt/data")];
    let app = app("demo", &[], false);
    let (rewritten, resolved) =
        inject_compose_volumes(compose, &app, "node1", &volumes).expect("ok");
    assert_eq!(resolved.len(), 0);
    let doc: serde_yaml::Value = serde_yaml::from_str(&rewritten).expect("yaml");
    let data = doc
        .get("volumes")
        .and_then(|v| v.get("data"))
        .expect("data entry retained untouched");
    assert_eq!(
        data,
        &serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
    );
}

#[test]
fn injects_filesystem_volume_from_qa_declaration() {
    let compose = "services:\n  web:\n    image: nginx\n    volumes:\n      - data:/var/lib/app\n";
    let volumes = vec![fs("data", "node1", "/mnt/data")];
    let app = app("demo", &["data"], false);
    let (rewritten, resolved) =
        inject_compose_volumes(compose, &app, "node1", &volumes).expect("ok");

    let doc: serde_yaml::Value = serde_yaml::from_str(&rewritten).expect("yaml");
    let data = doc
        .get("volumes")
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_mapping())
        .expect("data mapping");
    assert_eq!(
        data.get(serde_yaml::Value::String("external".into())),
        Some(&serde_yaml::Value::Bool(true))
    );
    assert_eq!(
        data.get(serde_yaml::Value::String("name".into()))
            .and_then(|v| v.as_str()),
        Some("ins_data")
    );
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].docker_name, "ins_data");
    assert_eq!(resolved[0].driver, "local");
    assert_eq!(resolved[0].driver_opts.get("device").unwrap(), "/mnt/data");
}

#[test]
fn injects_cifs_volume_credentials_into_resolved_opts() {
    let compose = "services:\n  web: { image: nginx }\n";
    let volumes = vec![cifs("data", "node2", "//10.0.0.5/share", "alice", "s3cr3t")];
    let app = app("demo", &["data"], false);
    let (_rewritten, resolved) =
        inject_compose_volumes(compose, &app, "node2", &volumes).expect("ok");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].driver_opts.get("type").unwrap(), "cifs");
    assert_eq!(
        resolved[0].driver_opts.get("o").unwrap(),
        "username=alice,password=s3cr3t"
    );
    assert_eq!(
        resolved[0].driver_opts.get("device").unwrap(),
        "//10.0.0.5/share"
    );
}

#[test]
fn all_volume_true_injects_every_configured_volume_for_current_node() {
    let compose = "services:\n  web: { image: nginx }\n";
    let volumes = vec![
        fs("a", "node1", "/mnt/a"),
        fs("b", "node1", "/mnt/b"),
        fs("c", "node2", "/mnt/c"),
    ];
    let app = app("demo", &[], true);
    let (_rewritten, resolved) =
        inject_compose_volumes(compose, &app, "node1", &volumes).expect("ok");
    let names: Vec<_> = resolved.iter().map(|r| r.docker_name.clone()).collect();
    assert_eq!(names, vec!["ins_a".to_string(), "ins_b".to_string()]);
}

#[test]
fn all_volume_true_ignores_qa_volumes_list() {
    let compose = "services:\n  web: { image: nginx }\n";
    let volumes = vec![fs("a", "node1", "/mnt/a"), fs("b", "node1", "/mnt/b")];
    let app = app("demo", &["a"], true);
    let (_rewritten, resolved) =
        inject_compose_volumes(compose, &app, "node1", &volumes).expect("ok");
    let names: Vec<_> = resolved.iter().map(|r| r.docker_name.clone()).collect();
    assert_eq!(names, vec!["ins_a".to_string(), "ins_b".to_string()]);
}

#[test]
fn all_volume_true_with_no_config_returns_empty_without_error() {
    let compose = "services:\n  web: { image: nginx }\n";
    let app = app("demo", &[], true);
    let (rewritten, resolved) = inject_compose_volumes(compose, &app, "node1", &[]).expect("ok");
    assert_eq!(resolved.len(), 0);
    assert_eq!(rewritten, compose);
}

#[test]
fn errors_when_required_volume_missing_on_node() {
    let compose = "services:\n  web: { image: nginx }\n";
    let volumes = vec![fs("data", "other-node", "/mnt/data")];
    let app = app("myapp", &["data"], false);
    let err = inject_compose_volumes(compose, &app, "node1", &volumes)
        .expect_err("missing config should fail");
    let msg = err.to_string();
    assert!(msg.contains("volume 'data'"), "unexpected message: {msg}");
    assert!(msg.contains("app 'myapp'"), "unexpected message: {msg}");
    assert!(msg.contains("node 'node1'"), "unexpected message: {msg}");
}

#[test]
fn picks_node_specific_record_when_duplicates_exist() {
    let compose = "services:\n  web: { image: nginx }\n";
    let volumes = vec![
        fs("data", "node1", "/mnt/one"),
        fs("data", "node2", "/mnt/two"),
    ];
    let app = app("demo", &["data"], false);
    let (_rewritten, resolved) =
        inject_compose_volumes(compose, &app, "node2", &volumes).expect("ok");
    assert_eq!(resolved[0].driver_opts.get("device").unwrap(), "/mnt/two");
}

#[test]
fn creates_top_level_volumes_block_when_compose_has_none() {
    let compose = "services:\n  web:\n    image: nginx\n";
    let volumes = vec![fs("data", "node1", "/mnt/data")];
    let app = app("demo", &["data"], false);
    let (rewritten, _resolved) =
        inject_compose_volumes(compose, &app, "node1", &volumes).expect("ok");
    let doc: serde_yaml::Value = serde_yaml::from_str(&rewritten).expect("yaml");
    assert!(
        doc.get("volumes").and_then(|v| v.get("data")).is_some(),
        "expected top-level volumes.data, got: {rewritten}"
    );
}
