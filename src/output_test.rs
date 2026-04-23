use serde_json::json;

use super::render_structured_list;
use crate::OutputFormat;
use crate::app::types::{AppRecord, AppValue, ScriptHook};
use crate::node::types::{NodeRecord, RemoteNodeRecord};
use crate::store::duck::InstalledServiceRecord;

#[test]
fn render_structured_list_formats_nodes_as_table() {
    let nodes = vec![
        NodeRecord::Local(),
        NodeRecord::Remote(RemoteNodeRecord {
            name: "node-a".into(),
            ip: "10.0.0.1".into(),
            port: 22,
            user: "root".into(),
            password: "secret".into(),
            key_path: None,
        }),
    ];

    let rendered =
        render_structured_list(&nodes, OutputFormat::Table, "no nodes found").expect("table");

    assert!(rendered.contains("type"));
    assert!(rendered.contains("local"));
    assert!(rendered.contains("node-a"));
    assert!(rendered.contains("password"));
}

#[test]
fn render_structured_list_formats_apps_as_json_when_requested() {
    let apps = vec![AppRecord {
        name: "demo".into(),
        version: Some("1.0.0".into()),
        description: Some("sample".into()),
        author_name: Some("Alice".into()),
        author_email: Some("alice@example.com".into()),
        dependencies: vec!["redis".into()],
        before: ScriptHook::default(),
        after: ScriptHook::default(),
        volumes: vec![],
        all_volume: false,
        files: None,
        values: vec![AppValue {
            name: "image".into(),
            value_type: "string".into(),
            description: None,
            value: Some(json!("nginx:latest")),
            default: None,
            options: vec![],
        }],
    }];

    let rendered =
        render_structured_list(&apps, OutputFormat::Json, "no apps found").expect("json");

    assert!(rendered.contains("\"name\": \"demo\""));
    assert!(rendered.contains("\"dependencies\": ["));
}

#[test]
fn render_structured_list_uses_empty_message() {
    let services: Vec<InstalledServiceRecord> = Vec::new();

    let rendered = render_structured_list(&services, OutputFormat::Table, "no services found")
        .expect("empty message");

    assert_eq!(rendered, "no services found");
}
