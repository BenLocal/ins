use super::{
    apply_stored_values, build_deployment_target, build_template_values, copy_apps_to_workspace,
    is_template_file, load_available_apps, parse_number_value, rendered_template_name,
    resolve_apps, select_node,
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

#[tokio::test]
async fn load_available_apps_reads_apps_from_qa_files() -> anyhow::Result<()> {
    let app_home = unique_test_dir("deploy-apps-list");
    let alpha_dir = app_home.join("alpha");
    let beta_dir = app_home.join("beta");
    fs::create_dir_all(&alpha_dir).await?;
    fs::create_dir_all(&beta_dir).await?;
    fs::write(
        alpha_dir.join("qa.yaml"),
        QA_TEMPLATE.replace("<name>", "alpha"),
    )
    .await?;
    fs::write(
        beta_dir.join("qa.yaml"),
        QA_TEMPLATE.replace("<name>", "beta"),
    )
    .await?;

    let apps = load_available_apps(&app_home).await?;
    assert_eq!(apps, vec!["alpha".to_string(), "beta".to_string()]);

    fs::remove_dir_all(&app_home).await?;
    Ok(())
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
            description: None,
            before: ScriptHook::default(),
            after: ScriptHook::default(),
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
        description: None,
        before: ScriptHook::default(),
        after: ScriptHook::default(),
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
fn build_deployment_target_defaults_service_to_app_name() {
    let target = build_deployment_target(app_record("alpha"), None).expect("deployment target");

    assert_eq!(target.app.name, "alpha");
    assert_eq!(target.service, "alpha");
}

#[test]
fn apply_stored_values_overrides_matching_app_values() {
    let mut app = AppRecord {
        name: "alpha".into(),
        description: None,
        before: ScriptHook::default(),
        after: ScriptHook::default(),
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
        description: None,
        before: ScriptHook::default(),
        after: ScriptHook::default(),
        files: None,
        values: vec![],
    }
}
