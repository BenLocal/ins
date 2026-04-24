use super::{
    app_choice_label, apply_cli_values, apply_stored_values, build_compose_metadata_labels,
    build_deployment_target, build_template_values, copy_apps_to_workspace, is_template_file,
    parse_cli_value_overrides, parse_number_value, rendered_template_name, resolve_apps,
    select_node,
};
use crate::app::types::{AppRecord, AppValue, AppValueOption, ScriptHook};
use crate::node::types::{NodeRecord, RemoteNodeRecord};
use crate::provider::DeploymentTarget;
use crate::store::duck::StoredDeploymentRecord;
use serde_json::json;
use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::fs;

const QA_TEMPLATE: &str = include_str!("../../template/qa.yaml");

#[test]
fn select_node_returns_requested_node_when_it_exists() {
    let nodes = vec![
        NodeRecord::Remote(RemoteNodeRecord {
            name: "node-a".into(),
            ip: "10.0.0.1".into(),
            port: 22,
            user: "root".into(),
            password: "secret".into(),
            key_path: None,
        }),
        NodeRecord::Remote(RemoteNodeRecord {
            name: "node-b".into(),
            ip: "10.0.0.2".into(),
            port: 22,
            user: "root".into(),
            password: "secret".into(),
            key_path: None,
        }),
    ];

    let selected = select_node(&nodes, Some("node-b")).expect("node should exist");
    match selected {
        NodeRecord::Remote(node) => assert_eq!(node.name, "node-b"),
        NodeRecord::Local() => panic!("expected remote node"),
    }
}

#[test]
fn select_node_returns_error_when_no_nodes_exist() {
    let err = select_node(&[], None).expect_err("empty nodes should fail");
    assert!(err.to_string().contains("no nodes found"));
}

#[tokio::test]
async fn resolve_apps_returns_requested_apps_when_present() {
    let apps = resolve_apps(
        Some(vec!["app-a".into(), "app-b".into()]),
        PathBuf::from("/tmp/unused").as_path(),
    )
    .await
    .expect("apps should pass through");
    assert_eq!(apps, vec!["app-a", "app-b"]);
}

#[tokio::test]
async fn resolve_apps_returns_error_when_no_apps_exist() -> anyhow::Result<()> {
    let app_home = unique_test_dir("deploy-apps-empty");
    fs::create_dir_all(&app_home).await?;

    let err = resolve_apps(None, &app_home)
        .await
        .expect_err("missing apps should fail");
    assert!(err.to_string().contains("no apps found"));

    fs::remove_dir_all(&app_home).await?;
    Ok(())
}

#[test]
fn app_choice_label_includes_description_and_author() {
    let app = AppRecord {
        name: "nginx".into(),
        version: None,
        description: Some("Static site server".into()),
        order: None,
        author_name: Some("Alice".into()),
        author_email: Some("alice@example.com".into()),
        dependencies: vec![],
        before: ScriptHook::default(),
        after: ScriptHook::default(),
        volumes: vec![],
        all_volume: false,
        files: None,
        values: vec![],
    };

    assert_eq!(
        app_choice_label(&app),
        "nginx - Static site server - Alice(alice@example.com)"
    );
}

#[tokio::test]
async fn copy_apps_to_workspace_copies_app_files() -> anyhow::Result<()> {
    let home = unique_test_dir("deploy-copy-home");
    let app_home = unique_test_dir("deploy-copy-app-home");
    let workspace = unique_test_dir("deploy-copy-workspace");
    let alpha_dir = app_home.join("alpha");
    let scripts_dir = alpha_dir.join("scripts");

    fs::create_dir_all(&scripts_dir).await?;
    fs::write(
        alpha_dir.join("qa.yaml"),
        QA_TEMPLATE.replace("<name>", "alpha"),
    )
    .await?;
    fs::write(alpha_dir.join("README.md"), "hello").await?;
    fs::write(scripts_dir.join("run.sh"), "#!/bin/bash").await?;

    let targets = vec![DeploymentTarget::new(app_record("alpha"), "alpha".into())];
    copy_apps_to_workspace(&home, &targets, &app_home, &workspace, &NodeRecord::Local()).await?;
    assert!(fs::try_exists(workspace.join("alpha").join("qa.yaml")).await?);
    assert!(fs::try_exists(workspace.join("alpha").join("README.md")).await?);
    assert!(fs::try_exists(workspace.join("alpha").join("scripts").join("run.sh")).await?);

    fs::remove_dir_all(&home).await?;
    fs::remove_dir_all(&app_home).await?;
    fs::remove_dir_all(&workspace).await?;
    Ok(())
}

#[tokio::test]
async fn copy_apps_to_workspace_preserves_binary_files_for_local_node() -> anyhow::Result<()> {
    let home = unique_test_dir("deploy-copy-binary-store-home");
    let app_home = unique_test_dir("deploy-copy-binary-home");
    let workspace = unique_test_dir("deploy-copy-binary-workspace");
    let alpha_dir = app_home.join("alpha");
    let binary = vec![0_u8, 159, 146, 150, 255, 10];

    fs::create_dir_all(&alpha_dir).await?;
    fs::write(
        alpha_dir.join("qa.yaml"),
        QA_TEMPLATE.replace("<name>", "alpha"),
    )
    .await?;
    fs::write(alpha_dir.join("blob.bin"), &binary).await?;

    let targets = vec![DeploymentTarget::new(
        app_record("alpha"),
        "frontend".into(),
    )];
    copy_apps_to_workspace(&home, &targets, &app_home, &workspace, &NodeRecord::Local()).await?;

    let copied = fs::read(workspace.join("frontend").join("blob.bin")).await?;
    assert_eq!(copied, binary);

    fs::remove_dir_all(&home).await?;
    fs::remove_dir_all(&app_home).await?;
    fs::remove_dir_all(&workspace).await?;
    Ok(())
}

#[tokio::test]
async fn copy_apps_to_workspace_renders_template_files() -> anyhow::Result<()> {
    let home = unique_test_dir("deploy-render-store-home");
    let app_home = unique_test_dir("deploy-render-app-home");
    let workspace = unique_test_dir("deploy-render-workspace");
    let alpha_dir = app_home.join("alpha");
    let qa = r#"
name: alpha
description: demo
before:
shell: bash
script: ./before.sh
after:
shell: bash
script: ./after.sh
values:
  - name: image
    type: string
    description: image name
    options:
      - name: nginx
        description: nginx image
        value: nginx:latest
"#;

    fs::create_dir_all(&alpha_dir).await?;
    fs::write(alpha_dir.join("qa.yaml"), qa.trim_start()).await?;
    fs::write(
        alpha_dir.join("docker-compose.yml.j2"),
        "name={{ app.name }}\nimage={{ vars.image }}\n",
    )
    .await?;

    let targets = vec![DeploymentTarget::new(
        AppRecord {
            name: "alpha".into(),
            version: None,
            description: None,
            order: None,
            author_name: None,
            author_email: None,
            dependencies: vec![],
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
        },
        "alpha".into(),
    )];
    copy_apps_to_workspace(&home, &targets, &app_home, &workspace, &NodeRecord::Local()).await?;

    let rendered = fs::read_to_string(workspace.join("alpha").join("docker-compose.yml")).await?;
    assert_eq!(rendered, "name=alpha\nimage=nginx:latest");

    fs::remove_dir_all(&home).await?;
    fs::remove_dir_all(&app_home).await?;
    fs::remove_dir_all(&workspace).await?;
    Ok(())
}

#[tokio::test]
async fn copy_apps_to_workspace_adds_metadata_labels_to_docker_compose_yml() -> anyhow::Result<()> {
    let home = unique_test_dir("deploy-compose-labels-yml-home");
    let app_home = unique_test_dir("deploy-compose-labels-yml-app-home");
    let workspace = unique_test_dir("deploy-compose-labels-yml-workspace");
    let alpha_dir = app_home.join("alpha");

    fs::create_dir_all(&alpha_dir).await?;
    fs::write(
        alpha_dir.join("qa.yaml"),
        r#"
name: alpha
version: 1.2.3
description: demo app
author_name: Alice
author_email: alice@example.com
values: []
"#
        .trim_start(),
    )
    .await?;
    fs::write(
        alpha_dir.join("docker-compose.yml"),
        r#"
services:
  web:
    image: nginx:latest
    labels:
      com.example.keep: "yes"
  worker:
    image: busybox
"#
        .trim_start(),
    )
    .await?;

    let targets = vec![DeploymentTarget::new(
        AppRecord {
            name: "alpha".into(),
            version: Some("1.2.3".into()),
            description: Some("demo app".into()),
            order: None,
            author_name: Some("Alice".into()),
            author_email: Some("alice@example.com".into()),
            dependencies: vec![],
            before: ScriptHook::default(),
            after: ScriptHook::default(),
            volumes: vec![],
            all_volume: false,
            files: None,
            values: vec![],
        },
        "alpha".into(),
    )];
    copy_apps_to_workspace(&home, &targets, &app_home, &workspace, &NodeRecord::Local()).await?;

    let rendered = fs::read_to_string(workspace.join("alpha").join("docker-compose.yml")).await?;
    let compose: serde_yaml::Value = serde_yaml::from_str(&rendered)?;
    let web_labels = &compose["services"]["web"]["labels"];
    let worker_labels = &compose["services"]["worker"]["labels"];

    assert_eq!(web_labels["com.example.keep"], "yes");
    assert_eq!(web_labels["ins.node_name"], "local");
    assert_eq!(web_labels["ins.description"], "demo app");
    assert_eq!(web_labels["ins.author_name"], "Alice");
    assert_eq!(web_labels["ins.author_email"], "alice@example.com");
    assert_eq!(web_labels["ins.version"], "1.2.3");

    assert_eq!(worker_labels["ins.node_name"], "local");
    assert_eq!(worker_labels["ins.description"], "demo app");
    assert_eq!(worker_labels["ins.author_name"], "Alice");
    assert_eq!(worker_labels["ins.author_email"], "alice@example.com");
    assert_eq!(worker_labels["ins.version"], "1.2.3");

    fs::remove_dir_all(&home).await?;
    fs::remove_dir_all(&app_home).await?;
    fs::remove_dir_all(&workspace).await?;
    Ok(())
}

#[tokio::test]
async fn copy_apps_to_workspace_adds_metadata_labels_to_docker_compose_yaml_template()
-> anyhow::Result<()> {
    let home = unique_test_dir("deploy-compose-labels-yaml-home");
    let app_home = unique_test_dir("deploy-compose-labels-yaml-app-home");
    let workspace = unique_test_dir("deploy-compose-labels-yaml-workspace");
    let alpha_dir = app_home.join("alpha");

    fs::create_dir_all(&alpha_dir).await?;
    fs::write(
        alpha_dir.join("qa.yaml"),
        r#"
name: alpha
version: 2.0.0
description: template app
author_name: Bob
author_email: bob@example.com
values: []
"#
        .trim_start(),
    )
    .await?;
    fs::write(
        alpha_dir.join("docker-compose.yaml.j2"),
        r#"
services:
  web:
    image: {{ app.name }}:latest
    labels:
      - traefik.enable=true
"#
        .trim_start(),
    )
    .await?;

    let targets = vec![DeploymentTarget::new(
        AppRecord {
            name: "alpha".into(),
            version: Some("2.0.0".into()),
            description: Some("template app".into()),
            order: None,
            author_name: Some("Bob".into()),
            author_email: Some("bob@example.com".into()),
            dependencies: vec![],
            before: ScriptHook::default(),
            after: ScriptHook::default(),
            volumes: vec![],
            all_volume: false,
            files: None,
            values: vec![],
        },
        "alpha".into(),
    )];

    copy_apps_to_workspace(&home, &targets, &app_home, &workspace, &NodeRecord::Local()).await?;

    let rendered = fs::read_to_string(workspace.join("alpha").join("docker-compose.yaml")).await?;
    let compose: serde_yaml::Value = serde_yaml::from_str(&rendered)?;
    let labels = &compose["services"]["web"]["labels"];

    assert_eq!(labels["traefik.enable"], "true");
    assert_eq!(labels["ins.node_name"], "local");
    assert_eq!(labels["ins.description"], "template app");
    assert_eq!(labels["ins.author_name"], "Bob");
    assert_eq!(labels["ins.author_email"], "bob@example.com");
    assert_eq!(labels["ins.version"], "2.0.0");

    fs::remove_dir_all(&home).await?;
    fs::remove_dir_all(&app_home).await?;
    fs::remove_dir_all(&workspace).await?;
    Ok(())
}

#[test]
fn build_compose_metadata_labels_uses_remote_node_name() {
    let template_values = serde_json::json!({
        "app": {
            "name": "alpha",
            "version": "3.0.0",
            "description": "demo",
            "author_name": "Alice",
            "author_email": "alice@example.com"
        }
    });
    let node = NodeRecord::Remote(RemoteNodeRecord {
        name: "node-a".into(),
        ip: "10.0.0.1".into(),
        port: 22,
        user: "root".into(),
        password: "secret".into(),
        key_path: None,
    });

    let labels = build_compose_metadata_labels(&template_values, &node);

    assert_eq!(labels.get("ins.node_name"), Some(&String::from("node-a")));
    assert_eq!(labels.get("ins.description"), Some(&String::from("demo")));
    assert_eq!(labels.get("ins.author_name"), Some(&String::from("Alice")));
    assert_eq!(
        labels.get("ins.author_email"),
        Some(&String::from("alice@example.com"))
    );
    assert_eq!(labels.get("ins.version"), Some(&String::from("3.0.0")));
}

#[tokio::test]
async fn copy_apps_to_workspace_allows_missing_template_values() -> anyhow::Result<()> {
    let home = unique_test_dir("deploy-render-missing-store-home");
    let app_home = unique_test_dir("deploy-render-missing-home");
    let workspace = unique_test_dir("deploy-render-missing-workspace");
    let alpha_dir = app_home.join("alpha");
    let qa = r#"
name: alpha
description: demo
before:
shell: bash
script: ./before.sh
after:
shell: bash
script: ./after.sh
values: []
"#;

    fs::create_dir_all(&alpha_dir).await?;
    fs::write(alpha_dir.join("qa.yaml"), qa.trim_start()).await?;
    fs::write(
        alpha_dir.join("app.conf.j2"),
        "name={{ app.name }}\nmissing={{ vars.not_found }}\n",
    )
    .await?;

    let targets = vec![DeploymentTarget::new(app_record("alpha"), "alpha".into())];
    copy_apps_to_workspace(&home, &targets, &app_home, &workspace, &NodeRecord::Local()).await?;

    let rendered = fs::read_to_string(workspace.join("alpha").join("app.conf")).await?;
    assert_eq!(rendered, "name=alpha\nmissing=");

    fs::remove_dir_all(&home).await?;
    fs::remove_dir_all(&app_home).await?;
    fs::remove_dir_all(&workspace).await?;
    Ok(())
}

#[test]
fn template_file_detection_and_output_name_work() {
    assert!(is_template_file("a.j2"));
    assert!(is_template_file("a.jinja"));
    assert!(is_template_file("a.jinja2"));
    assert!(is_template_file("a.tmpl"));
    assert!(!is_template_file("a.yaml"));
    assert_eq!(rendered_template_name("a.j2"), "a");
    assert_eq!(rendered_template_name("a.jinja"), "a");
    assert_eq!(rendered_template_name("a.jinja2"), "a");
    assert_eq!(rendered_template_name("a.tmpl"), "a");
}

#[test]
fn build_template_values_prefers_value_then_default_then_option() {
    let record = AppRecord {
        name: "demo".into(),
        version: None,
        description: None,
        order: None,
        author_name: None,
        author_email: None,
        dependencies: vec![],
        before: ScriptHook::default(),
        after: ScriptHook::default(),
        volumes: vec![],
        all_volume: false,
        files: None,
        values: vec![
            AppValue {
                name: "from_value".into(),
                value_type: "string".into(),
                description: None,
                value: Some(json!("explicit")),
                default: Some(json!("default")),
                options: vec![AppValueOption {
                    name: "opt".into(),
                    description: None,
                    value: Some(json!("option")),
                }],
            },
            AppValue {
                name: "from_default".into(),
                value_type: "number".into(),
                description: None,
                value: None,
                default: Some(json!(5)),
                options: vec![],
            },
            AppValue {
                name: "from_option".into(),
                value_type: "string".into(),
                description: None,
                value: None,
                default: None,
                options: vec![AppValueOption {
                    name: "opt".into(),
                    description: None,
                    value: Some(json!("picked")),
                }],
            },
        ],
    };

    let template_values = build_template_values(&record).expect("template values");
    assert_eq!(template_values["vars"]["from_value"], json!("explicit"));
    assert_eq!(template_values["vars"]["from_default"], json!(5));
    assert_eq!(template_values["vars"]["from_option"], json!("picked"));
}

#[test]
fn parse_cli_value_overrides_supports_repeated_v_flags() {
    let overrides = parse_cli_value_overrides(&[
        "image=nginx:1.27".into(),
        "port=8080".into(),
        "image=nginx:1.28".into(),
        "notes=hello world & more".into(),
    ])
    .expect("cli overrides should parse");

    assert_eq!(overrides.get("image"), Some(&"nginx:1.28".to_string()));
    assert_eq!(overrides.get("port"), Some(&"8080".to_string()));
    assert_eq!(
        overrides.get("notes"),
        Some(&"hello world & more".to_string())
    );
}

#[test]
fn apply_cli_values_overrides_default_and_option_values() {
    let mut apps = vec![AppRecord {
        name: "demo".into(),
        version: None,
        description: None,
        order: None,
        author_name: None,
        author_email: None,
        dependencies: vec![],
        before: ScriptHook::default(),
        after: ScriptHook::default(),
        volumes: vec![],
        all_volume: false,
        files: None,
        values: vec![
            AppValue {
                name: "port".into(),
                value_type: "number".into(),
                description: None,
                value: None,
                default: Some(json!(80)),
                options: vec![],
            },
            AppValue {
                name: "image".into(),
                value_type: "string".into(),
                description: None,
                value: None,
                default: None,
                options: vec![AppValueOption {
                    name: "nginx".into(),
                    description: None,
                    value: Some(json!("nginx:latest")),
                }],
            },
        ],
    }];

    let overrides = parse_cli_value_overrides(&["port=8080".into(), "image=caddy:2".into()])
        .expect("cli overrides should parse");

    apply_cli_values(&mut apps, &overrides).expect("cli overrides should apply");

    assert_eq!(apps[0].values[0].value, Some(json!(8080)));
    assert_eq!(apps[0].values[1].value, Some(json!("caddy:2")));
}

#[test]
fn build_deployment_target_defaults_service_to_app_name() {
    let target = build_deployment_target(app_record("alpha"), None).expect("deployment target");

    assert_eq!(target.app.name, "alpha");
    assert_eq!(target.service, "alpha");
}

#[test]
fn apply_stored_values_overrides_matching_app_values() {
    let mut app = AppRecord {
        name: "alpha".into(),
        version: None,
        description: None,
        order: None,
        author_name: None,
        author_email: None,
        dependencies: vec![],
        before: ScriptHook::default(),
        after: ScriptHook::default(),
        volumes: vec![],
        all_volume: false,
        files: None,
        values: vec![AppValue {
            name: "image".into(),
            value_type: "string".into(),
            description: None,
            value: None,
            default: Some(json!("nginx:latest")),
            options: vec![],
        }],
    };
    let preset = StoredDeploymentRecord {
        service: "frontend".into(),
        app_values: HashMap::from([(String::from("image"), json!("nginx:1.27"))]),
        qa_yaml: String::new(),
        created_at_ms: 1,
    };

    apply_stored_values(&mut app, &preset);

    assert_eq!(app.values[0].value, Some(json!("nginx:1.27")));
}

#[test]
fn apply_stored_values_does_not_override_existing_cli_values() {
    let mut app = AppRecord {
        name: "alpha".into(),
        version: None,
        description: None,
        order: None,
        author_name: None,
        author_email: None,
        dependencies: vec![],
        before: ScriptHook::default(),
        after: ScriptHook::default(),
        volumes: vec![],
        all_volume: false,
        files: None,
        values: vec![AppValue {
            name: "outbound_port".into(),
            value_type: "number".into(),
            description: None,
            value: Some(json!(1456)),
            default: Some(json!(3480)),
            options: vec![],
        }],
    };
    let preset = StoredDeploymentRecord {
        service: "frontend".into(),
        app_values: HashMap::from([(String::from("outbound_port"), json!(3480))]),
        qa_yaml: String::new(),
        created_at_ms: 1,
    };

    apply_stored_values(&mut app, &preset);

    assert_eq!(app.values[0].value, Some(json!(1456)));
}

#[test]
fn parse_number_value_keeps_integer_without_decimal_suffix() {
    assert_eq!(parse_number_value("70", "port").unwrap(), json!(70));
    assert_eq!(parse_number_value("70.5", "port").unwrap(), json!(70.5));
}

fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("ins-{name}-{}-{nanos}", std::process::id()))
}

fn app_record(name: &str) -> AppRecord {
    AppRecord {
        name: name.into(),
        version: None,
        description: None,
        order: None,
        author_name: None,
        author_email: None,
        dependencies: vec![],
        before: ScriptHook::default(),
        after: ScriptHook::default(),
        volumes: vec![],
        all_volume: false,
        files: None,
        values: vec![],
    }
}
