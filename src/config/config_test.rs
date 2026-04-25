use super::load::{load_config, persist_local_extern_ip, persist_node_workspace_if_missing};
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

#[tokio::test]
async fn persist_node_workspace_writes_new_entry_when_absent() -> anyhow::Result<()> {
    let home = unique_home("persist-new");
    tokio::fs::create_dir_all(&home).await?;

    persist_node_workspace_if_missing(&home, "node1", "/srv/apps").await?;

    let cfg = load_config(&home).await?;
    assert_eq!(cfg.workspace_for("node1"), Some("/srv/apps"));
    assert!(cfg.has_node_workspace("node1"));
    tokio::fs::remove_dir_all(&home).await.ok();
    Ok(())
}

#[tokio::test]
async fn persist_node_workspace_skips_when_per_node_entry_exists() -> anyhow::Result<()> {
    let home = unique_home("persist-skip");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::write(
        home.join("config.toml"),
        "[nodes.node1]\nworkspace = \"/existing\"\n",
    )
    .await?;

    persist_node_workspace_if_missing(&home, "node1", "/new").await?;

    let cfg = load_config(&home).await?;
    assert_eq!(cfg.workspace_for("node1"), Some("/existing"));
    tokio::fs::remove_dir_all(&home).await.ok();
    Ok(())
}

#[tokio::test]
async fn persist_node_workspace_records_even_when_defaults_cover_it() -> anyhow::Result<()> {
    // [defaults].workspace shouldn't block per-node recording — per-node is more specific.
    let home = unique_home("persist-over-defaults");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::write(
        home.join("config.toml"),
        "[defaults]\nworkspace = \"/srv/defaults\"\n",
    )
    .await?;

    persist_node_workspace_if_missing(&home, "node1", "/srv/node1").await?;

    let cfg = load_config(&home).await?;
    assert_eq!(cfg.defaults.workspace.as_deref(), Some("/srv/defaults"));
    assert_eq!(cfg.workspace_for("node1"), Some("/srv/node1"));
    tokio::fs::remove_dir_all(&home).await.ok();
    Ok(())
}

#[tokio::test]
async fn local_extern_ip_round_trips_through_toml() -> anyhow::Result<()> {
    let home = unique_home("extern-ip-roundtrip");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::write(
        home.join("config.toml"),
        "[defaults]\nlocal_extern_ip = \"1.2.3.4\"\n",
    )
    .await?;
    let cfg = load_config(&home).await?;
    assert_eq!(cfg.local_extern_ip(), Some("1.2.3.4"));
    // Round-trip serialization should preserve the field.
    let serialized = toml::to_string_pretty(&cfg).expect("serialize");
    assert!(
        serialized.contains("local_extern_ip"),
        "local_extern_ip missing from serialized: {serialized}"
    );
    assert!(
        serialized.contains("1.2.3.4"),
        "IP value missing from serialized: {serialized}"
    );
    tokio::fs::remove_dir_all(&home).await.ok();
    Ok(())
}

#[tokio::test]
async fn persist_local_extern_ip_writes_when_absent() -> anyhow::Result<()> {
    let home = unique_home("persist-extern-ip-new");
    tokio::fs::create_dir_all(&home).await?;

    persist_local_extern_ip(&home, "5.6.7.8").await?;

    let cfg = load_config(&home).await?;
    assert_eq!(cfg.local_extern_ip(), Some("5.6.7.8"));
    tokio::fs::remove_dir_all(&home).await.ok();
    Ok(())
}

#[tokio::test]
async fn persist_local_extern_ip_skips_when_already_set() -> anyhow::Result<()> {
    let home = unique_home("persist-extern-ip-skip");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::write(
        home.join("config.toml"),
        "[defaults]\nlocal_extern_ip = \"existing-ip\"\n",
    )
    .await?;

    persist_local_extern_ip(&home, "new-ip").await?;

    let cfg = load_config(&home).await?;
    assert_eq!(
        cfg.local_extern_ip(),
        Some("existing-ip"),
        "pre-existing value should not be overwritten"
    );
    tokio::fs::remove_dir_all(&home).await.ok();
    Ok(())
}
