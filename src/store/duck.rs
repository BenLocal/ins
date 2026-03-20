use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use duckdb::{Connection, params};
use serde::Serialize;
use serde_json::Value;

use crate::app::types::AppRecord;
use crate::node::types::NodeRecord;
use crate::provider::DeploymentTarget;

#[derive(Clone, Debug)]
pub struct StoredDeploymentRecord {
    pub service: String,
    pub app_values: HashMap<String, Value>,
    pub qa_yaml: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstalledServiceRecord {
    pub service: String,
    pub app_name: String,
    pub node_name: String,
    pub workspace: String,
    pub created_at_ms: i64,
}

pub async fn load_latest_deployment_record(
    home: &Path,
    node: &NodeRecord,
    workspace: &Path,
    app_name: &str,
) -> anyhow::Result<Option<StoredDeploymentRecord>> {
    let db_path = db_path(home);
    let workspace = workspace.display().to_string();
    let app_name = app_name.to_string();
    let node_name = node_name(node).to_string();

    tokio::task::spawn_blocking(move || {
        load_latest_deployment_record_sync(&db_path, &node_name, &workspace, &app_name)
    })
    .await
    .map_err(|e| anyhow!("join duckdb lookup: {}", e))?
}

pub async fn save_deployment_record(
    home: &Path,
    node: &NodeRecord,
    workspace: &Path,
    target: &DeploymentTarget,
    qa_yaml: &str,
) -> anyhow::Result<()> {
    let db_path = db_path(home);
    let record = SaveDeploymentRecord {
        node_name: node_name(node).to_string(),
        node_json: serde_json::to_string(node).context("serialize node record")?,
        workspace: workspace.display().to_string(),
        app_name: target.app.name.clone(),
        service: target.service.clone(),
        app_values_json: serde_json::to_string(&app_values_map(&target.app))
            .context("serialize app values")?,
        qa_yaml: qa_yaml.to_string(),
        created_at_ms: current_time_millis()?,
    };

    tokio::task::spawn_blocking(move || save_deployment_record_sync(&db_path, &record))
        .await
        .map_err(|e| anyhow!("join duckdb insert: {}", e))?
}

pub async fn list_installed_services(home: &Path) -> anyhow::Result<Vec<InstalledServiceRecord>> {
    let db_path = db_path(home);

    tokio::task::spawn_blocking(move || list_installed_services_sync(&db_path))
        .await
        .map_err(|e| anyhow!("join duckdb service list: {}", e))?
}

fn load_latest_deployment_record_sync(
    db_path: &Path,
    node_name: &str,
    workspace: &str,
    app_name: &str,
) -> anyhow::Result<Option<StoredDeploymentRecord>> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let mut stmt = conn
        .prepare(
            "SELECT service, app_values_json, qa_yaml, created_at_ms
             FROM deployment_history
             WHERE node_name = ? AND workspace = ? AND app_name = ?
             ORDER BY created_at_ms DESC
             LIMIT 1",
        )
        .context("prepare deployment history lookup")?;
    let mut rows = stmt
        .query(params![node_name, workspace, app_name])
        .context("query deployment history")?;

    let Some(row) = rows.next().context("read deployment history row")? else {
        return Ok(None);
    };

    let app_values_json: String = row.get(1).context("read app_values_json")?;
    let app_values: HashMap<String, Value> =
        serde_json::from_str(&app_values_json).context("parse app_values_json")?;

    Ok(Some(StoredDeploymentRecord {
        service: row.get(0).context("read service")?,
        app_values,
        qa_yaml: row.get(2).context("read qa_yaml")?,
        created_at_ms: row.get(3).context("read created_at_ms")?,
    }))
}

fn save_deployment_record_sync(
    db_path: &Path,
    record: &SaveDeploymentRecord,
) -> anyhow::Result<()> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;
    conn.execute(
        "INSERT INTO deployment_history (
            node_name,
            node_json,
            workspace,
            app_name,
            service,
            app_values_json,
            qa_yaml,
            created_at_ms
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            record.node_name,
            record.node_json,
            record.workspace,
            record.app_name,
            record.service,
            record.app_values_json,
            record.qa_yaml,
            record.created_at_ms
        ],
    )
    .context("insert deployment history")?;
    Ok(())
}

fn list_installed_services_sync(db_path: &Path) -> anyhow::Result<Vec<InstalledServiceRecord>> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let mut stmt = conn
        .prepare(
            "SELECT service, app_name, node_name, workspace, created_at_ms
             FROM (
                 SELECT
                     service,
                     app_name,
                     node_name,
                     workspace,
                     created_at_ms,
                     ROW_NUMBER() OVER (
                         PARTITION BY service
                         ORDER BY created_at_ms DESC
                     ) AS row_num
                 FROM deployment_history
             )
             WHERE row_num = 1
             ORDER BY service ASC",
        )
        .context("prepare installed services lookup")?;
    let mut rows = stmt.query([]).context("query installed services")?;
    let mut services = Vec::new();

    while let Some(row) = rows.next().context("read installed services row")? {
        services.push(InstalledServiceRecord {
            service: row.get(0).context("read service")?,
            app_name: row.get(1).context("read app_name")?,
            node_name: row.get(2).context("read node_name")?,
            workspace: row.get(3).context("read workspace")?,
            created_at_ms: row.get(4).context("read created_at_ms")?,
        });
    }

    Ok(services)
}

fn open_db(path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create duckdb dir {}", parent.display()))?;
    }
    Connection::open(path).with_context(|| format!("open duckdb {}", path.display()))
}

fn ensure_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS deployment_history (
            node_name TEXT NOT NULL,
            node_json TEXT NOT NULL,
            workspace TEXT NOT NULL,
            app_name TEXT NOT NULL,
            service TEXT NOT NULL,
            app_values_json TEXT NOT NULL,
            qa_yaml TEXT NOT NULL,
            created_at_ms BIGINT NOT NULL
        )",
    )
    .context("create deployment_history table")?;
    Ok(())
}

fn db_path(home: &Path) -> PathBuf {
    home.join("store").join("deploy_history.duckdb")
}

fn node_name(node: &NodeRecord) -> &str {
    match node {
        NodeRecord::Local() => "local",
        NodeRecord::Remote(node) => &node.name,
    }
}

fn app_values_map(app: &AppRecord) -> HashMap<String, Value> {
    app.values
        .iter()
        .filter_map(|value| {
            value
                .value
                .clone()
                .map(|current| (value.name.clone(), current))
        })
        .collect()
}

fn current_time_millis() -> anyhow::Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before unix epoch")?;
    i64::try_from(duration.as_millis()).context("timestamp overflow")
}

struct SaveDeploymentRecord {
    node_name: String,
    node_json: String,
    workspace: String,
    app_name: String,
    service: String,
    app_values_json: String,
    qa_yaml: String,
    created_at_ms: i64,
}

#[cfg(test)]
mod tests {
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
        let second =
            DeploymentTarget::new(app_record("nginx", json!("nginx:1.1")), "frontend".into());

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
            description: None,
            before: ScriptHook::default(),
            after: ScriptHook::default(),
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
}
