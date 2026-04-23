use super::load::load_config;
use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_home(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("ins-config-{name}-{}-{nanos}", std::process::id()))
}

#[tokio::test]
async fn load_returns_default_when_file_missing() -> anyhow::Result<()> {
    let home = unique_home("missing");
    tokio::fs::create_dir_all(&home).await?;
    let cfg = load_config(&home).await?;
    assert!(cfg.nodes.is_empty());
    assert!(cfg.defaults.workspace.is_none());
    assert!(cfg.defaults.app_home.is_none());
    assert!(cfg.defaults.provider.is_none());
    tokio::fs::remove_dir_all(&home).await.ok();
    Ok(())
}

#[tokio::test]
async fn load_parses_defaults_and_nodes() -> anyhow::Result<()> {
    let home = unique_home("load");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::write(
        home.join("config.toml"),
        r#"
[defaults]
workspace = "/srv/ws"
app_home = "/opt/apps"
provider = "docker-compose"

[nodes.local]
workspace = "/home/me/ws"

[nodes.node1]
workspace = "/srv/node1"
provider = "docker-compose"
"#,
    )
    .await?;
    let cfg = load_config(&home).await?;

    assert_eq!(cfg.defaults.workspace.as_deref(), Some("/srv/ws"));
    assert_eq!(cfg.defaults.app_home.as_deref(), Some("/opt/apps"));
    assert_eq!(cfg.defaults.provider.as_deref(), Some("docker-compose"));

    assert_eq!(cfg.workspace_for("local"), Some("/home/me/ws"));
    assert_eq!(cfg.workspace_for("node1"), Some("/srv/node1"));
    assert_eq!(cfg.workspace_for("other"), Some("/srv/ws"));
    assert_eq!(cfg.provider_for("node1"), Some("docker-compose"));
    assert_eq!(cfg.app_home_override(), Some("/opt/apps"));

    tokio::fs::remove_dir_all(&home).await.ok();
    Ok(())
}

#[tokio::test]
async fn workspace_for_unknown_node_falls_back_to_defaults() -> anyhow::Result<()> {
    let home = unique_home("fallback");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::write(
        home.join("config.toml"),
        r#"
[defaults]
workspace = "/srv/default"
"#,
    )
    .await?;
    let cfg = load_config(&home).await?;
    assert_eq!(cfg.workspace_for("anywhere"), Some("/srv/default"));
    assert!(cfg.provider_for("anywhere").is_none());
    tokio::fs::remove_dir_all(&home).await.ok();
    Ok(())
}

#[tokio::test]
async fn load_rejects_unknown_fields() -> anyhow::Result<()> {
    let home = unique_home("unknown");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::write(home.join("config.toml"), "[defaults]\nunknown_field = 1\n").await?;
    let err = load_config(&home)
        .await
        .expect_err("unknown field should fail");
    assert!(
        err.to_string().to_lowercase().contains("unknown"),
        "unexpected error: {err}"
    );
    tokio::fs::remove_dir_all(&home).await.ok();
    Ok(())
}
