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
    #[allow(dead_code)]
    pub namespace: String,
    pub app_values: HashMap<String, Value>,
    #[allow(dead_code)]
    pub qa_yaml: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstalledServiceRecord {
    pub service: String,
    pub namespace: String,
    pub app_name: String,
    pub node_name: String,
    pub workspace: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug)]
pub struct InstalledServiceConfigRecord {
    pub service: String,
    #[allow(dead_code)]
    pub namespace: String,
    pub app_name: String,
    pub node_name: String,
    pub workspace: String,
    pub app_values: HashMap<String, Value>,
    pub created_at_ms: i64,
}

pub async fn load_latest_deployment_record(
    home: &Path,
    node: &NodeRecord,
    workspace: &Path,
    namespace: &str,
    app_name: &str,
) -> anyhow::Result<Option<StoredDeploymentRecord>> {
    let db_path = db_path(home);
    let workspace = workspace.display().to_string();
    let app_name = app_name.to_string();
    let node_name = node_name(node).to_string();
    let namespace = namespace.to_string();

    tokio::task::spawn_blocking(move || {
        load_latest_deployment_record_sync(&db_path, &node_name, &workspace, &namespace, &app_name)
    })
    .await
    .map_err(|e| anyhow!("join duckdb lookup: {}", e))?
}

pub async fn save_deployment_record(
    home: &Path,
    node: &NodeRecord,
    workspace: &Path,
    target: &DeploymentTarget,
    namespace: &str,
    qa_yaml: &str,
) -> anyhow::Result<()> {
    let db_path = db_path(home);
    let record = SaveDeploymentRecord {
        node_name: node_name(node).to_string(),
        node_json: serde_json::to_string(node).context("serialize node record")?,
        workspace: workspace.display().to_string(),
        app_name: target.app.name.clone(),
        service: target.service.clone(),
        namespace: namespace.to_string(),
        app_values_json: serde_json::to_string(&app_values_map(&target.app))
            .context("serialize app values")?,
        qa_yaml: qa_yaml.to_string(),
        created_at_ms: current_time_millis()?,
    };

    tokio::task::spawn_blocking(move || save_deployment_record_sync(&db_path, &record))
        .await
        .map_err(|e| anyhow!("join duckdb insert: {}", e))?
}

pub async fn find_service_namespace_on_node(
    home: &Path,
    node: &NodeRecord,
    service: &str,
) -> anyhow::Result<Option<String>> {
    let db_path = db_path(home);
    let node_name = node_name(node).to_string();
    let service = service.to_string();

    tokio::task::spawn_blocking(move || {
        find_service_namespace_on_node_sync(&db_path, &node_name, &service)
    })
    .await
    .map_err(|e| anyhow!("join duckdb conflict lookup: {}", e))?
}

pub async fn list_installed_services(home: &Path) -> anyhow::Result<Vec<InstalledServiceRecord>> {
    let db_path = db_path(home);

    tokio::task::spawn_blocking(move || list_installed_services_sync(&db_path))
        .await
        .map_err(|e| anyhow!("join duckdb service list: {}", e))?
}

pub async fn load_installed_service_configs(
    home: &Path,
) -> anyhow::Result<Vec<InstalledServiceConfigRecord>> {
    let db_path = db_path(home);

    tokio::task::spawn_blocking(move || load_installed_service_configs_sync(&db_path))
        .await
        .map_err(|e| anyhow!("join duckdb service config list: {}", e))?
}

fn find_service_namespace_on_node_sync(
    db_path: &Path,
    node_name: &str,
    service: &str,
) -> anyhow::Result<Option<String>> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let mut stmt = conn
        .prepare(
            "SELECT COALESCE(namespace, 'default')
             FROM deployment_history
             WHERE node_name = ? AND service = ?
             ORDER BY created_at_ms DESC
             LIMIT 1",
        )
        .context("prepare conflict lookup")?;
    let mut rows = stmt
        .query(params![node_name, service])
        .context("query conflict lookup")?;
    if let Some(row) = rows.next().context("read conflict lookup row")? {
        Ok(Some(row.get(0).context("read namespace")?))
    } else {
        Ok(None)
    }
}

fn load_latest_deployment_record_sync(
    db_path: &Path,
    node_name: &str,
    workspace: &str,
    namespace: &str,
    app_name: &str,
) -> anyhow::Result<Option<StoredDeploymentRecord>> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let mut stmt = conn
        .prepare(
            "SELECT service, COALESCE(namespace, 'default'), app_values_json, qa_yaml, created_at_ms
             FROM deployment_history
             WHERE node_name = ? AND workspace = ? AND COALESCE(namespace, 'default') = ? AND app_name = ?
             ORDER BY created_at_ms DESC
             LIMIT 1",
        )
        .context("prepare deployment history lookup")?;
    let mut rows = stmt
        .query(params![node_name, workspace, namespace, app_name])
        .context("query deployment history")?;

    let Some(row) = rows.next().context("read deployment history row")? else {
        return Ok(None);
    };

    let app_values_json: String = row.get(2).context("read app_values_json")?;
    let app_values: HashMap<String, Value> =
        serde_json::from_str(&app_values_json).context("parse app_values_json")?;

    Ok(Some(StoredDeploymentRecord {
        service: row.get(0).context("read service")?,
        namespace: row.get(1).context("read namespace")?,
        app_values,
        qa_yaml: row.get(3).context("read qa_yaml")?,
        created_at_ms: row.get(4).context("read created_at_ms")?,
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
            namespace,
            app_values_json,
            qa_yaml,
            created_at_ms
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            record.node_name,
            record.node_json,
            record.workspace,
            record.app_name,
            record.service,
            record.namespace,
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
            "SELECT service, COALESCE(namespace, 'default'), app_name, node_name, workspace, created_at_ms
             FROM (
                 SELECT
                     service,
                     COALESCE(namespace, 'default') AS namespace,
                     app_name,
                     node_name,
                     workspace,
                     created_at_ms,
                     ROW_NUMBER() OVER (
                         PARTITION BY COALESCE(namespace, 'default'), service
                         ORDER BY created_at_ms DESC
                     ) AS row_num
                 FROM deployment_history
             )
             WHERE row_num = 1
             ORDER BY namespace ASC, service ASC",
        )
        .context("prepare installed services lookup")?;
    let mut rows = stmt.query([]).context("query installed services")?;
    let mut services = Vec::new();

    while let Some(row) = rows.next().context("read installed services row")? {
        services.push(InstalledServiceRecord {
            service: row.get(0).context("read service")?,
            namespace: row.get(1).context("read namespace")?,
            app_name: row.get(2).context("read app_name")?,
            node_name: row.get(3).context("read node_name")?,
            workspace: row.get(4).context("read workspace")?,
            created_at_ms: row.get(5).context("read created_at_ms")?,
        });
    }

    Ok(services)
}

fn load_installed_service_configs_sync(
    db_path: &Path,
) -> anyhow::Result<Vec<InstalledServiceConfigRecord>> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let mut stmt = conn
        .prepare(
            "SELECT service, COALESCE(namespace, 'default'), app_name, node_name, workspace, app_values_json, created_at_ms
             FROM (
                 SELECT
                     service,
                     COALESCE(namespace, 'default') AS namespace,
                     app_name,
                     node_name,
                     workspace,
                     app_values_json,
                     created_at_ms,
                     ROW_NUMBER() OVER (
                         PARTITION BY COALESCE(namespace, 'default'), service
                         ORDER BY created_at_ms DESC
                     ) AS row_num
                 FROM deployment_history
             )
             WHERE row_num = 1
             ORDER BY namespace ASC, service ASC",
        )
        .context("prepare installed service config lookup")?;
    let mut rows = stmt.query([]).context("query installed service configs")?;
    let mut services = Vec::new();

    while let Some(row) = rows.next().context("read installed service config row")? {
        let app_values_json: String = row.get(5).context("read app_values_json")?;
        let app_values: HashMap<String, Value> =
            serde_json::from_str(&app_values_json).context("parse app_values_json")?;

        services.push(InstalledServiceConfigRecord {
            service: row.get(0).context("read service")?,
            namespace: row.get(1).context("read namespace")?,
            app_name: row.get(2).context("read app_name")?,
            node_name: row.get(3).context("read node_name")?,
            workspace: row.get(4).context("read workspace")?,
            app_values,
            created_at_ms: row.get(6).context("read created_at_ms")?,
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
            namespace TEXT NOT NULL DEFAULT 'default',
            app_values_json TEXT NOT NULL,
            qa_yaml TEXT NOT NULL,
            created_at_ms BIGINT NOT NULL
        )",
    )
    .context("create deployment_history table")?;

    // Pre-namespace installs created the table without the column. ADD COLUMN
    // IF NOT EXISTS is a no-op on fresh schemas (column already created above)
    // and a one-shot migration on legacy ones. DuckDB does not support NOT NULL
    // constraints in ALTER TABLE ADD COLUMN, so we add it nullable; SELECTs
    // use COALESCE(namespace, 'default') to normalise legacy NULLs.
    conn.execute_batch(
        "ALTER TABLE deployment_history \
         ADD COLUMN IF NOT EXISTS namespace TEXT",
    )
    .context("alter deployment_history to add namespace column")?;

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
    namespace: String,
    app_values_json: String,
    qa_yaml: String,
    created_at_ms: i64,
}

#[cfg(test)]
#[path = "duck_test.rs"]
mod duck_test;
