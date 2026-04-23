use super::{build_provider_envs, shell_exports};
use crate::app::types::{AppRecord, AppValue, ScriptHook};
use crate::node::types::{NodeRecord, RemoteNodeRecord};
use crate::provider::DeploymentTarget;
use crate::store::duck::InstalledServiceConfigRecord;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn build_provider_envs_includes_app_metadata_and_values() {
    let targets = vec![DeploymentTarget::new(
        AppRecord {
            name: "alpha".into(),
            version: Some("1.2.3".into()),
            description: Some("demo".into()),
            author_name: Some("Alice".into()),
            author_email: Some("alice@example.com".into()),
            dependencies: vec!["redis".into()],
            before: ScriptHook::default(),
            after: ScriptHook::default(),
            volumes: vec![],
            all_volume: false,
            files: None,
            values: vec![AppValue {
                name: "image_tag".into(),
                value_type: "string".into(),
                description: None,
                value: Some(json!("v1")),
                default: None,
                options: vec![],
            }],
        },
        "frontend".into(),
    )];
    let node = NodeRecord::Remote(RemoteNodeRecord {
        name: "node-a".into(),
        ip: "10.0.0.1".into(),
        port: 22,
        user: "root".into(),
        password: "secret".into(),
        key_path: None,
    });

    let installed = vec![
        InstalledServiceConfigRecord {
            service: "redis".into(),
            app_name: "redis".into(),
            node_name: "node-b".into(),
            workspace: "/srv/redis".into(),
            app_values: BTreeMap::from([(String::from("port"), json!(6379))])
                .into_iter()
                .collect(),
            created_at_ms: 1,
        },
        InstalledServiceConfigRecord {
            service: "mysql".into(),
            app_name: "mysql".into(),
            node_name: "node-c".into(),
            workspace: "/srv/mysql".into(),
            app_values: BTreeMap::from([(String::from("port"), json!(3306))])
                .into_iter()
                .collect(),
            created_at_ms: 2,
        },
    ];

    let envs = build_provider_envs(&targets, &node, &installed).expect("envs");
    let service_env = envs.get("frontend").expect("service env");

    assert_eq!(
        service_env.get("INS_APP_NAME"),
        Some(&String::from("alpha"))
    );
    assert_eq!(
        service_env.get("INS_SERVICE_NAME"),
        Some(&String::from("frontend"))
    );
    assert_eq!(
        service_env.get("INS_NODE_NAME"),
        Some(&String::from("node-a"))
    );
    assert_eq!(service_env.get("INS_VERSION"), Some(&String::from("1.2.3")));
    assert_eq!(service_env.get("IMAGE_TAG"), Some(&String::from("v1")));
    assert_eq!(
        service_env.get("INS_SERVICE_REDIS_APP_NAME"),
        Some(&String::from("redis"))
    );
    assert_eq!(
        service_env.get("INS_SERVICE_REDIS_PORT"),
        Some(&String::from("6379"))
    );
    assert!(!service_env.contains_key("INS_SERVICE_MYSQL_APP_NAME"));
}

#[test]
fn shell_exports_quotes_values() {
    let exports = shell_exports(&BTreeMap::from([
        ("A".into(), "1".into()),
        ("B".into(), "x y".into()),
    ]));

    assert!(exports.contains("A='1'"));
    assert!(exports.contains("B='x y'"));
}
