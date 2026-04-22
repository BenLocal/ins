use super::{prepare_installed_service_deployment, print_provider_envs};
use crate::execution_output::ExecutionOutput;
use crate::{
    app::types::{AppRecord, AppValue, ScriptHook},
    cli::node::{NodeAddArgs, add_node_record, nodes_file},
    node::types::{NodeRecord, RemoteNodeRecord},
    provider::DeploymentTarget,
    store::duck::{InstalledServiceRecord, save_deployment_record},
};
use serde_json::json;
use std::collections::BTreeMap;
use std::{
    env,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::fs;

#[test]
fn print_provider_envs_lists_services_and_values_in_order() {
    let envs = BTreeMap::from([
        (
            "api".to_string(),
            BTreeMap::from([
                ("INS_APP_NAME".to_string(), "backend".to_string()),
                ("INS_NODE_NAME".to_string(), "local".to_string()),
            ]),
        ),
        (
            "worker".to_string(),
            BTreeMap::from([("INS_APP_NAME".to_string(), "jobs".to_string())]),
        ),
    ]);

    let output = ExecutionOutput::buffered();
    print_provider_envs(&envs, &output);

    assert_eq!(
        output.snapshot(),
        "Provider Environment Variables:\n  [api]\n    INS_APP_NAME=backend\n    INS_NODE_NAME=local\n  [worker]\n    INS_APP_NAME=jobs"
    );
}

#[test]
fn print_provider_envs_handles_empty_maps() {
    let output = ExecutionOutput::buffered();
    print_provider_envs(&BTreeMap::new(), &output);
    assert_eq!(
        output.snapshot(),
        "Provider Environment Variables:\n  (none)"
    );
}

#[test]
fn absolute_workspace_resolves_relative_path_against_cwd() {
    use super::absolute_workspace;
    use std::path::Path;

    let resolved = absolute_workspace(Path::new("./workspace")).expect("absolute");
    assert!(
        resolved.is_absolute(),
        "expected absolute, got {:?}",
        resolved
    );
    assert!(resolved.ends_with("workspace"));
}

#[test]
fn absolute_workspace_preserves_already_absolute_path() {
    use super::absolute_workspace;
    use std::path::Path;

    let resolved = absolute_workspace(Path::new("/srv/ins-ws")).expect("absolute");
    assert_eq!(resolved, Path::new("/srv/ins-ws"));
}

#[tokio::test]
async fn prepare_installed_service_deployment_reuses_saved_service_and_values() -> anyhow::Result<()>
{
    let home = unique_test_dir("pipeline-installed-service");
    let app_dir = home.join("app").join("demo");
    fs::create_dir_all(&app_dir).await?;
    fs::write(
        app_dir.join("qa.yaml"),
        r#"
name: demo
values:
  - name: image
    type: string
  - name: port
    type: number
"#
        .trim_start(),
    )
    .await?;

    let node = NodeRecord::Remote(RemoteNodeRecord {
        name: "node-a".into(),
        ip: "10.0.0.1".into(),
        port: 22,
        user: "root".into(),
        password: "secret".into(),
        key_path: None,
    });
    add_node_record(
        &nodes_file(&home),
        NodeAddArgs {
            name: "node-a".into(),
            ip: "10.0.0.1".into(),
            port: 22,
            user: "root".into(),
            password: "secret".into(),
            key_path: None,
        },
    )
    .await?;
    let target = DeploymentTarget::new(
        AppRecord {
            name: "demo".into(),
            version: None,
            description: None,
            author_name: None,
            author_email: None,
            dependencies: vec![],
            before: ScriptHook::default(),
            after: ScriptHook::default(),
            files: None,
            values: vec![
                AppValue {
                    name: "image".into(),
                    value_type: "string".into(),
                    description: None,
                    value: Some(json!("nginx:1.27")),
                    default: None,
                    options: vec![],
                },
                AppValue {
                    name: "port".into(),
                    value_type: "number".into(),
                    description: None,
                    value: Some(json!(8080)),
                    default: None,
                    options: vec![],
                },
            ],
        },
        "demo-web".into(),
    );
    save_deployment_record(
        &home,
        &node,
        PathBuf::from("/srv/demo").as_path(),
        &target,
        "name: demo\nvalues: []\n",
    )
    .await?;

    let prepared = prepare_installed_service_deployment(
        &home,
        "docker-compose".into(),
        &InstalledServiceRecord {
            service: "demo-web".into(),
            app_name: "demo".into(),
            node_name: "node-a".into(),
            workspace: "/srv/demo".into(),
            created_at_ms: 1,
        },
    )
    .await?;

    assert_eq!(prepared.targets.len(), 1);
    assert_eq!(prepared.targets[0].service, "demo-web");
    assert_eq!(
        prepared.targets[0].app.values[0].value,
        Some(json!("nginx:1.27"))
    );
    assert_eq!(prepared.targets[0].app.values[1].value, Some(json!(8080)));

    fs::remove_dir_all(&home).await?;
    Ok(())
}

#[tokio::test]
async fn copy_apps_to_workspace_rewrites_compose_volumes_and_returns_resolved() -> anyhow::Result<()>
{
    use crate::pipeline::copy_apps_to_workspace_with_output;
    use crate::provider::DeploymentTarget;

    let home = unique_test_dir("pipeline-volume-inject");
    let app_dir = home.join("app").join("vol-demo");
    fs::create_dir_all(&app_dir).await?;
    fs::write(app_dir.join("qa.yaml"), "name: vol-demo\nvalues: []\n").await?;
    fs::write(
        app_dir.join("docker-compose.yml"),
        "services:\n  web:\n    image: nginx\n    volumes:\n      - data:/var/lib/app\nvolumes:\n  data: {}\n",
    )
    .await?;

    let node = NodeRecord::Local();
    let workspace = home.join("workspace");
    let target = DeploymentTarget::new(
        AppRecord {
            name: "vol-demo".into(),
            version: None,
            description: None,
            author_name: None,
            author_email: None,
            dependencies: vec![],
            before: ScriptHook::default(),
            after: ScriptHook::default(),
            files: None,
            values: vec![],
        },
        "vol-demo".into(),
    );

    let volumes_config = vec![crate::volume::types::VolumeRecord::Filesystem(
        crate::volume::types::FilesystemVolume {
            name: "data".into(),
            node: "local".into(),
            path: "/mnt/data".into(),
        },
    )];

    let resolved = copy_apps_to_workspace_with_output(
        &home,
        std::slice::from_ref(&target),
        &home.join("app"),
        &workspace,
        &node,
        &volumes_config,
        &crate::execution_output::ExecutionOutput::stdout(),
    )
    .await?;

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].docker_name, "ins_data");

    let rendered =
        fs::read_to_string(workspace.join("vol-demo").join("docker-compose.yml")).await?;
    assert!(rendered.contains("external: true"));
    assert!(rendered.contains("ins_data"));

    fs::remove_dir_all(&home).await?;
    Ok(())
}

fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("ins-{name}-{}-{nanos}", std::process::id()))
}
