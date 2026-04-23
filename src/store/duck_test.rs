use std::path::PathBuf;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::app::types::{AppValue, ScriptHook};
use crate::provider::DeploymentTarget;
use serde_json::json;

#[tokio::test]
async fn save_and_load_latest_deployment_record_round_trips() -> anyhow::Result<()> {
    let home = unique_test_dir("duck-store-home");
    let workspace = home.join("workspace");
    let node = NodeRecord::Local();
    let first = DeploymentTarget::new(app_record("nginx", json!("nginx:1.0")), "web".into());
    let second = DeploymentTarget::new(app_record("nginx", json!("nginx:1.1")), "frontend".into());

    save_deployment_record(&home, &node, &workspace, &first, "name: nginx\n").await?;
    tokio::time::sleep(Duration::from_millis(2)).await;
    save_deployment_record(&home, &node, &workspace, &second, "name: nginx\n").await?;

    let loaded = load_latest_deployment_record(&home, &node, &workspace, "nginx")
        .await?
        .expect("stored deployment record");

    assert_eq!(loaded.service, "frontend");
    assert_eq!(loaded.app_values.get("image"), Some(&json!("nginx:1.1")));
    assert_eq!(loaded.qa_yaml, "name: nginx\n");

    std::fs::remove_dir_all(&home)?;
    Ok(())
}

#[tokio::test]
async fn list_installed_services_keeps_latest_record_per_service() -> anyhow::Result<()> {
    let home = unique_test_dir("duck-store-services-home");
    let workspace = home.join("workspace");
    let node = NodeRecord::Local();
    let original = DeploymentTarget::new(app_record("nginx", json!("nginx:1.0")), "web".into());
    let newer = DeploymentTarget::new(app_record("caddy", json!("caddy:1.0")), "web".into());
    let other = DeploymentTarget::new(app_record("redis", json!("redis:7")), "cache".into());

    save_deployment_record(&home, &node, &workspace, &original, "name: nginx\n").await?;
    tokio::time::sleep(Duration::from_millis(2)).await;
    save_deployment_record(&home, &node, &workspace, &newer, "name: caddy\n").await?;
    tokio::time::sleep(Duration::from_millis(2)).await;
    save_deployment_record(&home, &node, &workspace, &other, "name: redis\n").await?;

    let services = list_installed_services(&home).await?;

    assert_eq!(services.len(), 2);
    assert_eq!(services[0].service, "cache");
    assert_eq!(services[0].app_name, "redis");
    assert_eq!(services[1].service, "web");
    assert_eq!(services[1].app_name, "caddy");

    std::fs::remove_dir_all(&home)?;
    Ok(())
}

#[tokio::test]
async fn load_installed_service_configs_returns_latest_values_per_service() -> anyhow::Result<()> {
    let home = unique_test_dir("duck-store-service-config-home");
    let workspace = home.join("workspace");
    let node = NodeRecord::Local();
    let original = DeploymentTarget::new(app_record("nginx", json!("nginx:1.0")), "web".into());
    let newer = DeploymentTarget::new(app_record("caddy", json!("caddy:1.0")), "web".into());

    save_deployment_record(&home, &node, &workspace, &original, "name: nginx\n").await?;
    tokio::time::sleep(Duration::from_millis(2)).await;
    save_deployment_record(&home, &node, &workspace, &newer, "name: caddy\n").await?;

    let services = load_installed_service_configs(&home).await?;

    assert_eq!(services.len(), 1);
    assert_eq!(services[0].service, "web");
    assert_eq!(services[0].app_name, "caddy");
    assert_eq!(
        services[0].app_values.get("image"),
        Some(&json!("caddy:1.0"))
    );

    std::fs::remove_dir_all(&home)?;
    Ok(())
}

fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("ins-{name}-{}-{nanos}", std::process::id()))
}

fn app_record(name: &str, value: Value) -> AppRecord {
    AppRecord {
        name: name.into(),
        version: None,
        description: None,
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
            value: Some(value),
            default: None,
            options: vec![],
        }],
    }
}
