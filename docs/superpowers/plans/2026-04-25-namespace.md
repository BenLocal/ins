# Namespace for `check` / `deploy` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `namespace` dimension to `ins check` / `ins deploy` so deployments can be tagged, stored, and disambiguated by `(node, namespace, service)` rather than `(node, service)`, with `qa.yaml` `dependencies` able to target a specific namespace via `namespace:service` syntax.

**Architecture:** Single new CLI flag `--namespace <NAME>` on `PipelineArgs` (default `"default"`, applied to all apps in the run). New parser turns each `dependencies` string into a `DependencyRef { namespace, service, explicit_namespace }`. `deployment_history` gains a `namespace TEXT NOT NULL DEFAULT 'default'` column via `ALTER TABLE IF NOT EXISTS`. Env-var keys for dependencies use a hybrid rule: implicit/empty namespace → `INS_SERVICE_<SVC>_*` (back-compat); explicit non-default → `INS_SERVICE_<NS>_<SVC>_*`. A pre-deploy guard rejects deploys whose service name already exists on the node under a different namespace.

**Tech Stack:** Rust 1.x, `clap` (CLI), `duckdb` (history), `minijinja` (templates), `serde_yaml` (compose rewrite), `tokio` async, `inquire` prompts.

**Reference spec:** `docs/superpowers/specs/2026-04-25-namespace-design.md`.

---

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `src/app/dependency.rs` | **new** | `DependencyRef` struct + `parse_dependency` + `validate_namespace_name`. Owns the only place that knows the `<ns>:<svc>` syntax. |
| `src/app/dependency_test.rs` | **new** | Parser unit tests. |
| `src/app/mod.rs` | modify | Add `pub mod dependency;`. |
| `src/app/types.rs` | modify | Add `AppRecord::parsed_dependencies()` method (delegates to `dependency::parse_dependency`). |
| `src/store/duck.rs` | modify | `namespace` field on three records; `ALTER TABLE` in `ensure_schema`; namespace-aware queries; new `find_service_namespace_on_node`. |
| `src/store/duck_test.rs` | modify | Cover ALTER on legacy table, namespace-keyed save/load, conflict helper. |
| `src/pipeline/mod.rs` | modify | `PipelineArgs.namespace`, `PreparedDeployment.namespace`. Title-print includes namespace. |
| `src/pipeline/prepare.rs` | modify | Resolve & validate namespace; conflict guard after `build_deployment_targets`; thread namespace into `prepare_installed_service_deployment` from stored record. |
| `src/pipeline/prepare_test.rs` | modify | Conflict-guard tests. |
| `src/pipeline/template.rs` | modify | `build_template_values` accepts namespace; `render_template` exposes `{{ namespace }}`; debug print includes it. |
| `src/pipeline/labels.rs` | modify | Emit `ins.namespace` from `template_values.get("namespace")`. |
| `src/pipeline/pipeline_test.rs` | modify | Compose label test asserts `ins.namespace`. |
| `src/env.rs` | modify | Inject `INS_NAMESPACE`; rewrite `append_installed_service_envs` to consume `parsed_dependencies()` and apply hybrid keying; emit `_NAMESPACE` per-dep field. |
| `src/env_test.rs` | modify | Default-ns dep, explicit-ns dep, both coexist, `INS_NAMESPACE` set. |
| `src/cli/check.rs` | modify | (no logic change — flag arrives via flattened `PipelineArgs`). Test file added. |
| `src/cli/check_test.rs` | **new** | Parses `--namespace`; validates name. |
| `src/cli/deploy_test.rs` | modify | Parse `--namespace` on deploy. |
| `src/cli/service.rs` | modify | Print `namespace` first column on `service list`. |
| `src/output.rs` | modify | `TableRenderable for InstalledServiceRecord` adds NAMESPACE column. |
| `src/tui/mod.rs` | modify | `prepare_installed_service_deployment` already passes through; verify/update if it consumes namespace from record. |
| `docs/check-and-deploy.md` | modify | Document `--namespace`. |
| `docs/qa-yaml-dependencies-env.md` | modify | New `<ns>:<svc>` syntax + hybrid env-key rule. |
| `docs/env-vars.md` | modify | `INS_NAMESPACE`; `INS_SERVICE_<NS>_<SVC>_*` shape. |
| `docs/template-values.md` | modify | `{{ namespace }}`. |
| `docs/namespaces.md` | **new** | Concept overview, examples, conflict rule. |
| `CLAUDE.md` | modify | Add `docs/namespaces.md` row to the docs table. |

---

## Constants Used Throughout

```rust
pub const DEFAULT_NAMESPACE: &str = "default";
```

Define once in `src/app/dependency.rs` and re-export from `src/app/mod.rs` if needed. **Do not duplicate the literal `"default"` across files** — every place that compares to or substitutes the default namespace imports this constant.

---

## Task 1: Dependency parser foundation

**Files:**
- Create: `src/app/dependency.rs`
- Create: `src/app/dependency_test.rs`
- Modify: `src/app/mod.rs`

- [ ] **Step 1.1: Write failing parser tests**

Create `src/app/dependency_test.rs`:

```rust
use super::dependency::{DEFAULT_NAMESPACE, DependencyRef, parse_dependency, validate_namespace_name};

#[test]
fn parses_bare_service_as_default_namespace() {
    let dep = parse_dependency("redis").expect("parse");
    assert_eq!(
        dep,
        DependencyRef {
            namespace: DEFAULT_NAMESPACE.into(),
            service: "redis".into(),
            explicit_namespace: false,
        }
    );
}

#[test]
fn parses_empty_namespace_prefix_as_default() {
    let dep = parse_dependency(":redis").expect("parse");
    assert_eq!(
        dep,
        DependencyRef {
            namespace: DEFAULT_NAMESPACE.into(),
            service: "redis".into(),
            explicit_namespace: false,
        }
    );
}

#[test]
fn parses_explicit_namespace() {
    let dep = parse_dependency("staging:redis").expect("parse");
    assert_eq!(
        dep,
        DependencyRef {
            namespace: "staging".into(),
            service: "redis".into(),
            explicit_namespace: true,
        }
    );
}

#[test]
fn rejects_two_colons() {
    let err = parse_dependency("a:b:c").unwrap_err().to_string();
    assert!(err.contains("a:b:c"), "error mentions raw input: {err}");
}

#[test]
fn rejects_empty_service_after_colon() {
    let err = parse_dependency("staging:").unwrap_err().to_string();
    assert!(err.contains("service"), "error mentions service: {err}");
}

#[test]
fn rejects_empty_input() {
    parse_dependency("").unwrap_err();
}

#[test]
fn rejects_namespace_with_uppercase() {
    parse_dependency("Staging:redis").unwrap_err();
}

#[test]
fn rejects_namespace_starting_with_dash() {
    parse_dependency("-bad:redis").unwrap_err();
}

#[test]
fn validate_namespace_name_accepts_default() {
    validate_namespace_name(DEFAULT_NAMESPACE).expect("accept default");
}

#[test]
fn validate_namespace_name_accepts_alnum_dash_underscore() {
    validate_namespace_name("staging-1").unwrap();
    validate_namespace_name("ns_2").unwrap();
    validate_namespace_name("0abc").unwrap();
}

#[test]
fn validate_namespace_name_rejects_too_long() {
    let too_long = "a".repeat(65);
    validate_namespace_name(&too_long).unwrap_err();
}

#[test]
fn validate_namespace_name_rejects_empty() {
    validate_namespace_name("").unwrap_err();
}
```

- [ ] **Step 1.2: Run tests — expect compile failure**

Run: `cargo test --features duckdb-bundled --no-run 2>&1 | tail -30`
Expected: build error `unresolved module: dependency`.

- [ ] **Step 1.3: Implement `src/app/dependency.rs`**

```rust
use anyhow::{anyhow, Result};

pub const DEFAULT_NAMESPACE: &str = "default";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyRef {
    pub namespace: String,
    pub service: String,
    /// `true` only when the raw entry was `<ns>:<service>` with a non-empty
    /// `<ns>`. Bare `service` and `:service` both produce `false` so the
    /// hybrid env-var keying rule (§5.2 of the spec) can distinguish "user
    /// did not write a namespace" from "user wrote `default:`".
    pub explicit_namespace: bool,
}

pub fn parse_dependency(raw: &str) -> Result<DependencyRef> {
    if raw.is_empty() {
        return Err(anyhow!("invalid dependency '': non-empty value required"));
    }

    let mut parts = raw.splitn(3, ':');
    let first = parts.next().unwrap_or("");
    let second = parts.next();
    let third = parts.next();

    if third.is_some() {
        return Err(anyhow!(
            "invalid dependency '{raw}': at most one ':' separator allowed"
        ));
    }

    let (namespace_raw, service) = match second {
        Some(svc) => (first, svc),
        None => ("", first),
    };

    if service.is_empty() {
        return Err(anyhow!(
            "invalid dependency '{raw}': service name required after ':'"
        ));
    }

    let (namespace, explicit) = if namespace_raw.is_empty() {
        (DEFAULT_NAMESPACE.to_string(), false)
    } else {
        validate_namespace_name(namespace_raw)
            .map_err(|e| anyhow!("invalid dependency '{raw}': {e}"))?;
        (namespace_raw.to_string(), true)
    };

    Ok(DependencyRef {
        namespace,
        service: service.to_string(),
        explicit_namespace: explicit,
    })
}

/// Allowed namespace shape: `^[a-z0-9][a-z0-9_-]{0,63}$`. The constraint
/// flows from the env-var key shape — namespace text gets uppercased and
/// concatenated into `INS_SERVICE_<NS>_<SVC>_*`, so we forbid characters
/// that would either round-trip lossily or produce ambiguous keys.
pub fn validate_namespace_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("namespace name cannot be empty"));
    }
    if name.len() > 64 {
        return Err(anyhow!(
            "namespace name '{name}' exceeds 64-character limit"
        ));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(anyhow!(
            "namespace name '{name}' must start with [a-z0-9]"
        ));
    }
    for ch in chars {
        if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-') {
            return Err(anyhow!(
                "namespace name '{name}' contains invalid character '{ch}'; \
                 only [a-z0-9_-] allowed after the first character"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "dependency_test.rs"]
mod dependency_test;
```

- [ ] **Step 1.4: Wire module in**

In `src/app/mod.rs`:

```rust
pub mod dependency;
pub mod parse;
pub mod types;
```

- [ ] **Step 1.5: Run parser tests — expect PASS**

Run: `cargo test --features duckdb-bundled app::dependency`
Expected: all tests pass.

- [ ] **Step 1.6: Run full check (lint, fmt, tests)**

Run: `make check`
Expected: green.

- [ ] **Step 1.7: Commit**

```bash
git add src/app/dependency.rs src/app/dependency_test.rs src/app/mod.rs
git commit -m "feat(app): add namespace-aware dependency parser

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `AppRecord::parsed_dependencies` accessor

**Files:**
- Modify: `src/app/types.rs`
- Modify: `src/app/parse_test.rs` (add a test verifying the new method)

- [ ] **Step 2.1: Write failing test**

Append to `src/app/parse_test.rs`:

```rust
#[tokio::test]
async fn parsed_dependencies_splits_namespace_prefixes() {
    use crate::app::dependency::{DEFAULT_NAMESPACE, DependencyRef};
    use crate::app::types::AppRecord;

    let app = AppRecord {
        name: "web".into(),
        dependencies: vec![
            "redis".into(),
            ":mysql".into(),
            "staging:cache".into(),
        ],
        ..AppRecord::default()
    };

    let deps = app.parsed_dependencies().expect("parse");
    assert_eq!(deps.len(), 3);
    assert_eq!(
        deps[0],
        DependencyRef {
            namespace: DEFAULT_NAMESPACE.into(),
            service: "redis".into(),
            explicit_namespace: false,
        }
    );
    assert_eq!(deps[1].service, "mysql");
    assert!(!deps[1].explicit_namespace);
    assert_eq!(deps[2].namespace, "staging");
    assert!(deps[2].explicit_namespace);
}
```

- [ ] **Step 2.2: Run test — expect compile failure**

Run: `cargo test --features duckdb-bundled parsed_dependencies_splits --no-run`
Expected: `no method named parsed_dependencies`.

- [ ] **Step 2.3: Add method on `AppRecord`**

In `src/app/types.rs`, append after the existing `impl` blocks (or add one if absent):

```rust
impl AppRecord {
    pub(crate) fn parsed_dependencies(
        &self,
    ) -> anyhow::Result<Vec<crate::app::dependency::DependencyRef>> {
        self.dependencies
            .iter()
            .map(|raw| crate::app::dependency::parse_dependency(raw))
            .collect()
    }
}
```

- [ ] **Step 2.4: Run test — expect PASS**

Run: `cargo test --features duckdb-bundled parsed_dependencies_splits`
Expected: PASS.

- [ ] **Step 2.5: Run `make check`** — expect green.

- [ ] **Step 2.6: Commit**

```bash
git add src/app/types.rs src/app/parse_test.rs
git commit -m "feat(app): expose parsed_dependencies on AppRecord

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: DB schema migration + record fields

**Files:**
- Modify: `src/store/duck.rs`
- Modify: `src/store/duck_test.rs`

- [ ] **Step 3.1: Write failing test for ALTER on legacy DB**

Append to `src/store/duck_test.rs`:

```rust
#[tokio::test]
async fn ensure_schema_alters_legacy_table_to_add_namespace() -> anyhow::Result<()> {
    let home = unique_test_dir("duck-store-legacy-alter");
    let db_path = home.join("store").join("deploy_history.duckdb");
    std::fs::create_dir_all(db_path.parent().unwrap())?;

    // Create a pre-namespace schema by hand and insert one legacy row.
    {
        let conn = duckdb::Connection::open(&db_path)?;
        conn.execute_batch(
            "CREATE TABLE deployment_history (
                node_name TEXT NOT NULL,
                node_json TEXT NOT NULL,
                workspace TEXT NOT NULL,
                app_name TEXT NOT NULL,
                service TEXT NOT NULL,
                app_values_json TEXT NOT NULL,
                qa_yaml TEXT NOT NULL,
                created_at_ms BIGINT NOT NULL
            )",
        )?;
        conn.execute(
            "INSERT INTO deployment_history VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                "local",
                "{}",
                "/tmp/ws",
                "nginx",
                "web",
                "{}",
                "name: nginx\n",
                1_700_000_000_000_i64,
            ],
        )?;
    }

    // Touching any read path triggers ensure_schema, which must ALTER.
    let services = list_installed_services(&home).await?;
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].namespace, "default", "legacy row migrated to default namespace");

    std::fs::remove_dir_all(&home)?;
    Ok(())
}
```

- [ ] **Step 3.2: Run test — expect FAIL**

Run: `cargo test --features duckdb-bundled ensure_schema_alters_legacy --no-run`
Expected: error `no field named namespace on InstalledServiceRecord`.

- [ ] **Step 3.3: Add `namespace` to the three record structs**

In `src/store/duck.rs`:

```rust
#[derive(Clone, Debug)]
pub struct StoredDeploymentRecord {
    pub service: String,
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
    pub namespace: String,
    pub app_name: String,
    pub node_name: String,
    pub workspace: String,
    pub app_values: HashMap<String, Value>,
    pub created_at_ms: i64,
}
```

- [ ] **Step 3.4: Update `ensure_schema` to ALTER legacy tables**

Replace the existing `ensure_schema` body in `src/store/duck.rs`:

```rust
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
    // and a one-shot migration on legacy ones. Existing rows backfill via
    // DEFAULT.
    conn.execute_batch(
        "ALTER TABLE deployment_history \
         ADD COLUMN IF NOT EXISTS namespace TEXT NOT NULL DEFAULT 'default'",
    )
    .context("alter deployment_history to add namespace column")?;

    Ok(())
}
```

- [ ] **Step 3.5: Update `list_installed_services_sync` query and row mapping**

Replace the SQL and row-build code:

```rust
fn list_installed_services_sync(db_path: &Path) -> anyhow::Result<Vec<InstalledServiceRecord>> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let mut stmt = conn
        .prepare(
            "SELECT service, namespace, app_name, node_name, workspace, created_at_ms
             FROM (
                 SELECT
                     service,
                     namespace,
                     app_name,
                     node_name,
                     workspace,
                     created_at_ms,
                     ROW_NUMBER() OVER (
                         PARTITION BY namespace, service
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
```

- [ ] **Step 3.6: Update `load_installed_service_configs_sync` symmetrically**

```rust
fn load_installed_service_configs_sync(
    db_path: &Path,
) -> anyhow::Result<Vec<InstalledServiceConfigRecord>> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let mut stmt = conn
        .prepare(
            "SELECT service, namespace, app_name, node_name, workspace, app_values_json, created_at_ms
             FROM (
                 SELECT
                     service,
                     namespace,
                     app_name,
                     node_name,
                     workspace,
                     app_values_json,
                     created_at_ms,
                     ROW_NUMBER() OVER (
                         PARTITION BY namespace, service
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
```

- [ ] **Step 3.7: Run the new test — expect PASS**

Run: `cargo test --features duckdb-bundled ensure_schema_alters_legacy`
Expected: PASS.

- [ ] **Step 3.8: Other tests will FAIL (signatures changed)**

Run: `cargo test --features duckdb-bundled store::duck`
Expected: existing tests fail because `StoredDeploymentRecord` / records now require `namespace`. **This is fixed in Task 4.** Do not commit yet.

---

## Task 4: Save + load with namespace

**Files:**
- Modify: `src/store/duck.rs`
- Modify: `src/store/duck_test.rs`

- [ ] **Step 4.1: Update save signature and SQL**

In `src/store/duck.rs`, change the public `save_deployment_record` to accept namespace:

```rust
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
```

Update the `SaveDeploymentRecord` struct:

```rust
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
```

Update `save_deployment_record_sync`:

```rust
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
```

- [ ] **Step 4.2: Update load signature and SQL**

```rust
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
            "SELECT service, namespace, app_values_json, qa_yaml, created_at_ms
             FROM deployment_history
             WHERE node_name = ? AND workspace = ? AND namespace = ? AND app_name = ?
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
```

- [ ] **Step 4.3: Update existing duck tests for new signatures**

In `src/store/duck_test.rs`, update every `save_deployment_record(...)` call site to pass `"default"` as the new namespace argument and every `load_latest_deployment_record(...)` to pass `"default"`. Concretely, every call like:

```rust
save_deployment_record(&home, &node, &workspace, &first, "name: nginx\n").await?;
```

becomes:

```rust
save_deployment_record(&home, &node, &workspace, &first, "default", "name: nginx\n").await?;
```

and:

```rust
load_latest_deployment_record(&home, &node, &workspace, "nginx").await?
```

becomes:

```rust
load_latest_deployment_record(&home, &node, &workspace, "default", "nginx").await?
```

Add namespace assertions to the existing `save_and_load_latest_deployment_record_round_trips` test:

```rust
assert_eq!(loaded.namespace, "default");
```

- [ ] **Step 4.4: Add a namespace round-trip test**

Append to `src/store/duck_test.rs`:

```rust
#[tokio::test]
async fn save_and_load_respects_namespace_partition() -> anyhow::Result<()> {
    let home = unique_test_dir("duck-store-namespace");
    let workspace = home.join("workspace");
    let node = NodeRecord::Local();
    let target = DeploymentTarget::new(app_record("nginx", json!("nginx:1.0")), "web".into());

    save_deployment_record(&home, &node, &workspace, &target, "staging", "name: nginx\n").await?;

    // Same app_name, different namespace — must miss.
    let miss = load_latest_deployment_record(&home, &node, &workspace, "default", "nginx").await?;
    assert!(miss.is_none(), "default namespace must not see staging row");

    let hit = load_latest_deployment_record(&home, &node, &workspace, "staging", "nginx")
        .await?
        .expect("staging row");
    assert_eq!(hit.namespace, "staging");

    std::fs::remove_dir_all(&home)?;
    Ok(())
}
```

- [ ] **Step 4.5: Update other call sites of these functions**

`grep` reveals two non-test call sites to update:

- `src/pipeline/target.rs::build_deployment_targets` calls `load_latest_deployment_record(home, node, workspace, &app.name)`. Change signature to take `namespace: &str` and pass it.
- `src/provider/docker_compose.rs` (or wherever `save_deployment_record` is called by `provider.run`). Change signature to take a namespace value.

The actual rewiring happens in Task 6 (pipeline) and Task 7 (provider invocation). For now, **temporarily** update the call sites to pass the literal `"default"` so the build compiles:

```rust
// src/pipeline/target.rs (will be replaced in Task 6)
load_latest_deployment_record(home, node, workspace, "default", &app.name).await?
```

Similar treatment for `save_deployment_record` (likely in `provider/docker_compose.rs`).

To find call sites:

```bash
grep -rn 'save_deployment_record\|load_latest_deployment_record' src/
```

Update each with `"default"` placeholder.

- [ ] **Step 4.6: Run all DB tests — expect PASS**

Run: `cargo test --features duckdb-bundled store::duck`
Expected: all DB tests pass.

- [ ] **Step 4.7: Run `make check`** — expect green.

- [ ] **Step 4.8: Commit**

```bash
git add src/store/duck.rs src/store/duck_test.rs src/pipeline/target.rs src/provider/
git commit -m "feat(store): namespace column and namespace-keyed queries

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Conflict-detection helper

**Files:**
- Modify: `src/store/duck.rs`
- Modify: `src/store/duck_test.rs`

- [ ] **Step 5.1: Write failing test**

Append to `src/store/duck_test.rs`:

```rust
#[tokio::test]
async fn find_service_namespace_on_node_returns_existing_namespace() -> anyhow::Result<()> {
    let home = unique_test_dir("duck-store-find-ns");
    let workspace = home.join("workspace");
    let node = NodeRecord::Local();
    let target = DeploymentTarget::new(app_record("nginx", json!("nginx:1.0")), "web".into());

    save_deployment_record(&home, &node, &workspace, &target, "staging", "name: nginx\n").await?;

    let found = find_service_namespace_on_node(&home, &node, "web").await?;
    assert_eq!(found.as_deref(), Some("staging"));

    let absent = find_service_namespace_on_node(&home, &node, "missing").await?;
    assert_eq!(absent, None);

    std::fs::remove_dir_all(&home)?;
    Ok(())
}
```

- [ ] **Step 5.2: Run — expect FAIL (function does not exist)**

Run: `cargo test --features duckdb-bundled find_service_namespace --no-run`
Expected: `cannot find function find_service_namespace_on_node`.

- [ ] **Step 5.3: Implement the function**

In `src/store/duck.rs`:

```rust
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

fn find_service_namespace_on_node_sync(
    db_path: &Path,
    node_name: &str,
    service: &str,
) -> anyhow::Result<Option<String>> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let mut stmt = conn
        .prepare(
            "SELECT namespace
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
```

- [ ] **Step 5.4: Run — expect PASS** (`cargo test --features duckdb-bundled find_service_namespace`).

- [ ] **Step 5.5: Run `make check`** — expect green.

- [ ] **Step 5.6: Commit**

```bash
git add src/store/duck.rs src/store/duck_test.rs
git commit -m "feat(store): add find_service_namespace_on_node conflict helper

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: PipelineArgs gains `--namespace`

**Files:**
- Modify: `src/pipeline/mod.rs`
- Modify: `src/pipeline/prepare.rs`
- Modify: `src/pipeline/prepare_test.rs`

- [ ] **Step 6.1: Write failing test for resolution + validation**

Append to `src/pipeline/prepare_test.rs`:

```rust
use crate::app::dependency::DEFAULT_NAMESPACE;

#[test]
fn resolve_namespace_defaults_when_unset() {
    let resolved = super::resolve_namespace(None).expect("resolve");
    assert_eq!(resolved, DEFAULT_NAMESPACE);
}

#[test]
fn resolve_namespace_passes_through_valid_input() {
    assert_eq!(super::resolve_namespace(Some("staging".into())).unwrap(), "staging");
    assert_eq!(super::resolve_namespace(Some("ns_1".into())).unwrap(), "ns_1");
}

#[test]
fn resolve_namespace_rejects_invalid_chars() {
    super::resolve_namespace(Some("Bad".into())).unwrap_err();
    super::resolve_namespace(Some("with space".into())).unwrap_err();
}
```

- [ ] **Step 6.2: Run — expect FAIL** (`resolve_namespace` does not exist).

- [ ] **Step 6.3: Add the flag and resolver**

In `src/pipeline/mod.rs`, extend `PipelineArgs` and `PreparedDeployment`:

```rust
#[derive(clap::Args, Clone, Debug)]
pub struct PipelineArgs {
    /// Provider name. Falls back to config.toml, then "docker-compose".
    #[arg(short, long)]
    pub provider: Option<String>,
    /// Workspace directory for copied app files. Falls back to config.toml per-node or defaults.
    #[arg(short, long)]
    pub workspace: Option<PathBuf>,
    /// Target node name.
    #[arg(short, long)]
    pub node: Option<String>,
    /// Deployment namespace. Defaults to "default". Names must match
    /// `[a-z0-9][a-z0-9_-]{0,63}`.
    #[arg(long)]
    pub namespace: Option<String>,
    /// Override qa values. Can be specified multiple times as key=value.
    #[arg(short = 'v', long = "value", value_name = "KEY=VALUE")]
    pub values: Vec<String>,
    #[arg(short = 'd', long = "defaults", default_value_t = false)]
    pub defaults: bool,
    pub apps: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct PreparedDeployment {
    pub provider: String,
    pub node: NodeRecord,
    pub namespace: String,
    pub app_names: Vec<String>,
    pub app_home: PathBuf,
    pub workspace: PathBuf,
    pub targets: Vec<DeploymentTarget>,
    pub user_env: BTreeMap<String, String>,
}
```

In `src/pipeline/prepare.rs`, add the resolver:

```rust
use crate::app::dependency::{DEFAULT_NAMESPACE, validate_namespace_name};

pub(crate) fn resolve_namespace(input: Option<String>) -> anyhow::Result<String> {
    let raw = input
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_NAMESPACE.to_string());
    validate_namespace_name(&raw)?;
    Ok(raw)
}
```

Inside `prepare_deployment`, destructure `namespace` and call the resolver:

```rust
let PipelineArgs {
    provider,
    workspace,
    node: requested_node,
    namespace: requested_namespace,
    values: requested_values,
    defaults: use_defaults,
    apps: requested_apps,
} = ctx.args;

let namespace = resolve_namespace(requested_namespace)?;
```

Pass `namespace` into `build_deployment_targets` (signature change in next step) and into `PreparedDeployment`:

```rust
Ok(PreparedDeployment {
    provider,
    node,
    namespace,
    app_names,
    app_home,
    workspace,
    targets,
    user_env,
})
```

- [ ] **Step 6.4: Update `build_deployment_targets` to thread namespace**

In `src/pipeline/target.rs`:

```rust
pub(super) async fn build_deployment_targets(
    apps: Vec<AppRecord>,
    home: &Path,
    node: &NodeRecord,
    workspace: &Path,
    namespace: &str,
    use_defaults: bool,
) -> anyhow::Result<Vec<DeploymentTarget>> {
    let mut targets = Vec::with_capacity(apps.len());

    for app in apps {
        let preset = if use_defaults {
            None
        } else {
            load_latest_deployment_record(home, node, workspace, namespace, &app.name).await?
        };
        let target = build_deployment_target(app, preset.as_ref(), use_defaults)?;
        targets.push(target);
    }

    Ok(targets)
}
```

Replace the call site in `prepare.rs`:

```rust
let targets =
    build_deployment_targets(apps, &home, &node, &workspace, &namespace, use_defaults).await?;
```

- [ ] **Step 6.5: Update `prepare_installed_service_deployment` to read namespace from record**

```rust
pub async fn prepare_installed_service_deployment(
    home: &Path,
    config: &InsConfig,
    provider: Option<String>,
    service: &InstalledServiceRecord,
) -> anyhow::Result<PreparedDeployment> {
    // ... existing code unchanged up to building `target` ...

    let target = DeploymentTarget::new(app, service.service.clone());
    let provider = resolve_provider(provider, config, &service.node_name);
    let user_env = config.env_for(&service.node_name);

    Ok(PreparedDeployment {
        provider,
        node,
        namespace: service.namespace.clone(),
        app_names: vec![service.app_name.clone()],
        app_home,
        workspace: absolute_workspace(Path::new(&service.workspace))?,
        targets: vec![target],
        user_env,
    })
}
```

- [ ] **Step 6.6: Update title-print**

`print_prepared_deployment_to_output` in `src/pipeline/mod.rs`:

```rust
output.line(format!("Namespace: {}", prepared.namespace));
```

inserted after the `Node Name:` line.

- [ ] **Step 6.7: Run namespace-resolver tests — expect PASS**

Run: `cargo test --features duckdb-bundled resolve_namespace`
Expected: 3 tests pass.

- [ ] **Step 6.8: Run `make check`** — expect green. Fix any signature ripples (the placeholder `"default"` strings inserted in Task 4.5 should now be replaced with the actual `namespace` variable).

- [ ] **Step 6.9: Commit**

```bash
git add src/pipeline/ src/store/duck.rs
git commit -m "feat(pipeline): add --namespace flag and thread it through prepare

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Conflict guard at prepare time

**Files:**
- Modify: `src/pipeline/prepare.rs`
- Modify: `src/pipeline/prepare_test.rs`

- [ ] **Step 7.1: Write failing test**

Append to `src/pipeline/prepare_test.rs`:

```rust
#[tokio::test]
async fn check_namespace_conflicts_errors_when_service_uses_other_namespace() -> anyhow::Result<()>
{
    use crate::app::types::AppRecord;
    use crate::provider::DeploymentTarget;
    use crate::store::duck::save_deployment_record;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let home: PathBuf = std::env::temp_dir().join(format!(
        "ins-prep-conflict-{}-{}",
        std::process::id(),
        nanos
    ));
    let workspace = home.join("ws");
    let node = NodeRecord::Local();

    let existing = DeploymentTarget::new(
        AppRecord {
            name: "nginx".into(),
            ..AppRecord::default()
        },
        "web".into(),
    );
    save_deployment_record(&home, &node, &workspace, &existing, "default", "name: nginx\n")
        .await?;

    let new_target = DeploymentTarget::new(
        AppRecord {
            name: "nginx".into(),
            ..AppRecord::default()
        },
        "web".into(),
    );

    let err = super::check_namespace_conflicts(&home, &node, "staging", &[new_target])
        .await
        .expect_err("should conflict");
    let msg = err.to_string();
    assert!(msg.contains("'web'"), "error mentions service: {msg}");
    assert!(msg.contains("default"), "error mentions existing ns: {msg}");
    assert!(msg.contains("staging"), "error mentions requested ns: {msg}");

    std::fs::remove_dir_all(&home)?;
    Ok(())
}

#[tokio::test]
async fn check_namespace_conflicts_passes_when_same_namespace() -> anyhow::Result<()> {
    use crate::app::types::AppRecord;
    use crate::provider::DeploymentTarget;
    use crate::store::duck::save_deployment_record;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let home: PathBuf = std::env::temp_dir().join(format!(
        "ins-prep-conflict-pass-{}-{}",
        std::process::id(),
        nanos
    ));
    let workspace = home.join("ws");
    let node = NodeRecord::Local();

    let existing = DeploymentTarget::new(
        AppRecord {
            name: "nginx".into(),
            ..AppRecord::default()
        },
        "web".into(),
    );
    save_deployment_record(&home, &node, &workspace, &existing, "default", "name: nginx\n")
        .await?;

    let new_target = DeploymentTarget::new(
        AppRecord {
            name: "nginx".into(),
            ..AppRecord::default()
        },
        "web".into(),
    );

    super::check_namespace_conflicts(&home, &node, "default", &[new_target])
        .await
        .expect("same-namespace redeploy must pass");

    std::fs::remove_dir_all(&home)?;
    Ok(())
}

#[tokio::test]
async fn check_namespace_conflicts_passes_when_no_existing_record() -> anyhow::Result<()> {
    use crate::app::types::AppRecord;
    use crate::provider::DeploymentTarget;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let home: PathBuf = std::env::temp_dir().join(format!(
        "ins-prep-conflict-empty-{}-{}",
        std::process::id(),
        nanos
    ));
    let node = NodeRecord::Local();

    let new_target = DeploymentTarget::new(
        AppRecord {
            name: "nginx".into(),
            ..AppRecord::default()
        },
        "web".into(),
    );

    super::check_namespace_conflicts(&home, &node, "staging", &[new_target])
        .await
        .expect("no existing record means no conflict");

    if home.exists() {
        std::fs::remove_dir_all(&home)?;
    }
    Ok(())
}
```

- [ ] **Step 7.2: Run — expect FAIL** (`check_namespace_conflicts` does not exist).

- [ ] **Step 7.3: Implement the guard**

In `src/pipeline/prepare.rs`:

```rust
use crate::store::duck::find_service_namespace_on_node;

pub(crate) async fn check_namespace_conflicts(
    home: &Path,
    node: &NodeRecord,
    namespace: &str,
    targets: &[DeploymentTarget],
) -> anyhow::Result<()> {
    for target in targets {
        let Some(existing) = find_service_namespace_on_node(home, node, &target.service).await?
        else {
            continue;
        };
        if existing != namespace {
            return Err(anyhow!(
                "service '{}' already exists on node '{}' under namespace '{}'; \
                 cannot deploy under namespace '{}'. \
                 Run `ins service rm {}` first or redeploy under namespace '{}'.",
                target.service,
                node_name(node),
                existing,
                namespace,
                target.service,
                existing
            ));
        }
    }
    Ok(())
}
```

Wire it into `prepare_deployment` after `build_deployment_targets`:

```rust
let targets =
    build_deployment_targets(apps, &home, &node, &workspace, &namespace, use_defaults).await?;
check_namespace_conflicts(&home, &node, &namespace, &targets).await?;
```

- [ ] **Step 7.4: Run conflict tests — expect PASS**

Run: `cargo test --features duckdb-bundled check_namespace_conflicts`

- [ ] **Step 7.5: Run `make check`** — expect green.

- [ ] **Step 7.6: Commit**

```bash
git add src/pipeline/prepare.rs src/pipeline/prepare_test.rs
git commit -m "feat(pipeline): reject same-name-different-namespace deploys

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Provider records namespace on save

**Files:**
- Modify: `src/provider/docker_compose.rs` (and any other place calling `save_deployment_record`)
- Modify: `src/provider/mod.rs` — `ProviderContext` carries namespace
- Modify: `src/pipeline/mod.rs` — passes namespace into `ProviderContext::new`
- Modify: any docker_compose tests verifying save behavior

- [ ] **Step 8.1: Add `namespace` to `ProviderContext`**

In `src/provider/mod.rs`:

```rust
#[derive(Clone, Debug)]
pub struct ProviderContext {
    pub provider: String,
    pub node: NodeRecord,
    pub namespace: String,
    pub targets: Vec<DeploymentTarget>,
    pub workspace: PathBuf,
    pub envs: BTreeMap<String, BTreeMap<String, String>>,
    pub output: ExecutionOutput,
    pub volumes: Vec<ResolvedVolume>,
}

impl ProviderContext {
    pub fn new(
        provider: String,
        node: NodeRecord,
        namespace: String,
        targets: Vec<DeploymentTarget>,
        workspace: PathBuf,
        envs: BTreeMap<String, BTreeMap<String, String>>,
        output: ExecutionOutput,
        volumes: Vec<ResolvedVolume>,
    ) -> Self {
        Self {
            provider,
            node,
            namespace,
            targets,
            workspace,
            envs,
            output,
            volumes,
        }
    }
    // ...
}
```

- [ ] **Step 8.2: Pass namespace into the constructor**

In `src/pipeline/mod.rs::execute_pipeline_with_output`:

```rust
let provider_ctx = ProviderContext::new(
    prepared.provider.clone(),
    prepared.node.clone(),
    prepared.namespace.clone(),
    prepared.targets.clone(),
    prepared.workspace.clone(),
    envs,
    output.clone(),
    resolved_volumes,
);
```

(Note: `prepared.workspace` may currently be moved; clone if needed.)

- [ ] **Step 8.3: Update save call in docker_compose**

Find the call:

```bash
grep -n save_deployment_record src/provider/docker_compose.rs
```

Replace the placeholder `"default"` (added in Task 4.5) with `ctx.namespace.as_str()`:

```rust
save_deployment_record(home, &ctx.node, &ctx.workspace, target, &ctx.namespace, &qa_yaml).await?;
```

- [ ] **Step 8.4: Update any docker_compose tests that construct `ProviderContext`**

```bash
grep -n 'ProviderContext::new' src/
```

Pass `"default".to_string()` as the new arg in test fixtures.

- [ ] **Step 8.5: Run `make check`** — expect green.

- [ ] **Step 8.6: Commit**

```bash
git add src/provider/ src/pipeline/mod.rs
git commit -m "feat(provider): persist namespace alongside deploy history

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: `INS_NAMESPACE` and dependency env hybrid keying

**Files:**
- Modify: `src/env.rs`
- Modify: `src/env_test.rs`

- [ ] **Step 9.1: Write failing tests**

Append to `src/env_test.rs`:

```rust
#[test]
fn build_provider_envs_includes_ins_namespace_for_current_app() {
    let targets = vec![DeploymentTarget::new(
        AppRecord {
            name: "alpha".into(),
            ..AppRecord::default()
        },
        "web".into(),
    )];
    let node = NodeRecord::Local();

    let envs = build_provider_envs(
        &targets,
        &node,
        "staging",
        &[],
        &BTreeMap::new(),
    )
    .expect("envs");
    let env = envs.get("web").expect("web env");
    assert_eq!(env.get("INS_NAMESPACE"), Some(&String::from("staging")));
}

#[test]
fn build_provider_envs_uses_unprefixed_keys_for_default_ns_dependency() {
    let targets = vec![DeploymentTarget::new(
        AppRecord {
            name: "alpha".into(),
            dependencies: vec!["redis".into()],
            ..AppRecord::default()
        },
        "web".into(),
    )];
    let node = NodeRecord::Local();
    let installed = vec![InstalledServiceConfigRecord {
        service: "redis".into(),
        namespace: "default".into(),
        app_name: "redis".into(),
        node_name: "node-a".into(),
        workspace: "/srv/redis".into(),
        app_values: BTreeMap::from([("port".into(), json!(6379))]).into_iter().collect(),
        created_at_ms: 1,
    }];

    let envs = build_provider_envs(
        &targets,
        &node,
        "default",
        &installed,
        &BTreeMap::new(),
    )
    .expect("envs");
    let env = envs.get("web").expect("web env");
    assert_eq!(env.get("INS_SERVICE_REDIS_SERVICE"), Some(&String::from("redis")));
    assert_eq!(env.get("INS_SERVICE_REDIS_NAMESPACE"), Some(&String::from("default")));
    assert_eq!(env.get("INS_SERVICE_REDIS_PORT"), Some(&String::from("6379")));
    assert!(!env.keys().any(|k| k.starts_with("INS_SERVICE_DEFAULT_REDIS_")));
}

#[test]
fn build_provider_envs_uses_prefixed_keys_for_explicit_namespace_dependency() {
    let targets = vec![DeploymentTarget::new(
        AppRecord {
            name: "alpha".into(),
            dependencies: vec!["staging:redis".into()],
            ..AppRecord::default()
        },
        "web".into(),
    )];
    let node = NodeRecord::Local();
    let installed = vec![InstalledServiceConfigRecord {
        service: "redis".into(),
        namespace: "staging".into(),
        app_name: "redis".into(),
        node_name: "node-a".into(),
        workspace: "/srv/redis".into(),
        app_values: BTreeMap::from([("port".into(), json!(6380))]).into_iter().collect(),
        created_at_ms: 1,
    }];

    let envs = build_provider_envs(
        &targets,
        &node,
        "default",
        &installed,
        &BTreeMap::new(),
    )
    .expect("envs");
    let env = envs.get("web").expect("web env");
    assert_eq!(
        env.get("INS_SERVICE_STAGING_REDIS_SERVICE"),
        Some(&String::from("redis"))
    );
    assert_eq!(
        env.get("INS_SERVICE_STAGING_REDIS_NAMESPACE"),
        Some(&String::from("staging"))
    );
    assert_eq!(
        env.get("INS_SERVICE_STAGING_REDIS_PORT"),
        Some(&String::from("6380"))
    );
    assert!(!env.keys().any(|k| k == "INS_SERVICE_REDIS_SERVICE"));
}

#[test]
fn build_provider_envs_supports_dep_in_default_and_explicit_namespaces_simultaneously() {
    let targets = vec![DeploymentTarget::new(
        AppRecord {
            name: "alpha".into(),
            dependencies: vec!["redis".into(), "staging:redis".into()],
            ..AppRecord::default()
        },
        "web".into(),
    )];
    let node = NodeRecord::Local();
    let installed = vec![
        InstalledServiceConfigRecord {
            service: "redis".into(),
            namespace: "default".into(),
            app_name: "redis".into(),
            node_name: "node-a".into(),
            workspace: "/srv/redis".into(),
            app_values: BTreeMap::from([("port".into(), json!(6379))]).into_iter().collect(),
            created_at_ms: 1,
        },
        InstalledServiceConfigRecord {
            service: "redis".into(),
            namespace: "staging".into(),
            app_name: "redis".into(),
            node_name: "node-a".into(),
            workspace: "/srv/redis-staging".into(),
            app_values: BTreeMap::from([("port".into(), json!(6380))]).into_iter().collect(),
            created_at_ms: 2,
        },
    ];

    let envs = build_provider_envs(
        &targets,
        &node,
        "default",
        &installed,
        &BTreeMap::new(),
    )
    .expect("envs");
    let env = envs.get("web").expect("web env");
    assert_eq!(env.get("INS_SERVICE_REDIS_PORT"), Some(&String::from("6379")));
    assert_eq!(env.get("INS_SERVICE_STAGING_REDIS_PORT"), Some(&String::from("6380")));
}
```

- [ ] **Step 9.2: Run — expect FAIL** (`build_provider_envs` signature mismatch).

- [ ] **Step 9.3: Update `build_provider_envs` signature**

In `src/env.rs`, change to accept namespace and require namespace-aware matching:

```rust
pub(crate) fn build_provider_envs(
    targets: &[DeploymentTarget],
    node: &NodeRecord,
    namespace: &str,
    installed_services: &[InstalledServiceConfigRecord],
    user_env: &BTreeMap<String, String>,
) -> anyhow::Result<BTreeMap<String, BTreeMap<String, String>>> {
    let mut envs = BTreeMap::new();

    for target in targets {
        let mut target_envs = BTreeMap::new();
        for (k, v) in user_env {
            target_envs.insert(k.clone(), v.clone());
        }
        let ins_envs = build_target_envs(&target.app, &target.service, node, namespace)?;
        for (k, v) in ins_envs {
            target_envs.insert(k, v);
        }
        append_installed_service_envs(
            &mut target_envs,
            installed_services,
            &target.service,
            namespace,
            &target.app,
        )?;
        envs.insert(target.service.clone(), target_envs);
    }

    Ok(envs)
}
```

- [ ] **Step 9.4: Update `build_target_envs` to inject `INS_NAMESPACE`**

```rust
fn build_target_envs(
    app: &AppRecord,
    service: &str,
    node: &NodeRecord,
    namespace: &str,
) -> anyhow::Result<BTreeMap<String, String>> {
    let resolved_values = resolve_app_values_for_env(app)?;
    let mut envs = BTreeMap::new();

    envs.insert("INS_APP_NAME".into(), app.name.clone());
    envs.insert("INS_SERVICE_NAME".into(), service.to_string());
    envs.insert("INS_NODE_NAME".into(), node_name(node).to_string());
    envs.insert("INS_NAMESPACE".into(), namespace.to_string());

    // ...rest unchanged...
    Ok(envs)
}
```

- [ ] **Step 9.5: Rewrite `append_installed_service_envs` to use parsed deps**

```rust
fn append_installed_service_envs(
    envs: &mut BTreeMap<String, String>,
    installed_services: &[InstalledServiceConfigRecord],
    current_service: &str,
    current_namespace: &str,
    app: &AppRecord,
) -> anyhow::Result<()> {
    for dep in app.parsed_dependencies()? {
        // The current service never satisfies its own dependency entry.
        if dep.service == current_service && dep.namespace == current_namespace {
            continue;
        }

        let Some(installed) = installed_services
            .iter()
            .find(|s| s.service == dep.service && s.namespace == dep.namespace)
        else {
            continue;
        };

        let prefix = if dep.explicit_namespace {
            format!(
                "INS_SERVICE_{}_{}",
                env_key_for_value_name(&dep.namespace),
                env_key_for_value_name(&dep.service)
            )
        } else {
            format!("INS_SERVICE_{}", env_key_for_value_name(&dep.service))
        };

        envs.insert(format!("{prefix}_SERVICE"), installed.service.clone());
        envs.insert(format!("{prefix}_NAMESPACE"), installed.namespace.clone());
        envs.insert(format!("{prefix}_APP_NAME"), installed.app_name.clone());
        envs.insert(format!("{prefix}_NODE_NAME"), installed.node_name.clone());
        envs.insert(format!("{prefix}_WORKSPACE"), installed.workspace.clone());
        envs.insert(
            format!("{prefix}_CREATED_AT_MS"),
            installed.created_at_ms.to_string(),
        );

        for (name, value) in &installed.app_values {
            envs.insert(
                format!("{prefix}_{}", env_key_for_value_name(name)),
                provider_env_value(value),
            );
        }
    }

    Ok(())
}
```

- [ ] **Step 9.6: Update the caller in `pipeline/mod.rs::execute_pipeline_with_output`**

```rust
let envs = build_provider_envs(
    &prepared.targets,
    &prepared.node,
    &prepared.namespace,
    &load_installed_service_configs(home).await?,
    &prepared.user_env,
)?;
```

- [ ] **Step 9.7: Update the existing env test fixtures**

The existing `build_provider_envs_includes_app_metadata_and_values` test must add `namespace: "default".into()` to its `InstalledServiceConfigRecord` literals (DB struct gained the field in Task 3) and pass `"default"` as the new `build_provider_envs` argument.

- [ ] **Step 9.8: Run — expect PASS**

Run: `cargo test --features duckdb-bundled env`
Expected: all env tests pass.

- [ ] **Step 9.9: Run `make check`** — expect green.

- [ ] **Step 9.10: Commit**

```bash
git add src/env.rs src/env_test.rs src/pipeline/mod.rs
git commit -m "feat(env): INS_NAMESPACE and hybrid dependency env-key shape

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 10: Template context + compose label

**Files:**
- Modify: `src/pipeline/template.rs`
- Modify: `src/pipeline/labels.rs`
- Modify: `src/pipeline/mod.rs` (caller threading)
- Modify: `src/pipeline/copy.rs` (caller threading — accepts namespace)
- Modify: `src/pipeline/pipeline_test.rs` (extend label test)

- [ ] **Step 10.1: Write failing label test**

In `src/pipeline/pipeline_test.rs`, find the existing `copy_apps_to_workspace_adds_metadata_labels_to_docker_compose_yml` test and extend the rendered-yaml assertions:

```rust
let yaml: serde_yaml::Value = serde_yaml::from_str(&rendered)?;
let labels = yaml
    .get("services")
    .and_then(|s| s.get("web"))
    .and_then(|svc| svc.get("labels"))
    .and_then(|l| l.as_mapping())
    .expect("labels mapping");
assert_eq!(
    labels.get(&serde_yaml::Value::String("ins.namespace".into())),
    Some(&serde_yaml::Value::String("staging".into()))
);
```

(Adjust to whatever fixture / assertion style the existing tests use; the gist is: assert `ins.namespace=<value>` ends up under each service's labels.)

- [ ] **Step 10.2: Run — expect FAIL** (label not yet emitted, value not threaded).

- [ ] **Step 10.3: Add namespace to template values**

In `src/pipeline/template.rs::build_target_template_values`:

```rust
pub(super) fn build_target_template_values(
    target: &DeploymentTarget,
    node: &NodeRecord,
    namespace: &str,
    volumes_config: &[VolumeRecord],
) -> anyhow::Result<Value> {
    let mut template_values = build_template_values(&target.app)?;
    if let Some(obj) = template_values.as_object_mut() {
        obj.insert("service".into(), Value::String(target.service.clone()));
        obj.insert("namespace".into(), Value::String(namespace.to_string()));
        let volumes_json = resolved_volumes_to_json(&target.app, node, volumes_config)?;
        obj.insert("volumes".into(), volumes_json);
    }
    Ok(template_values)
}
```

In `print_target_template_values`, pass through:

```rust
pub(super) fn print_target_template_values(
    target: &DeploymentTarget,
    node: &NodeRecord,
    namespace: &str,
    volumes_config: &[VolumeRecord],
    output: &ExecutionOutput,
) -> anyhow::Result<()> {
    let template_values = build_target_template_values(target, node, namespace, volumes_config)?;
    debug_print_template_values(&target.app.name, &template_values, output);
    Ok(())
}
```

In `render_template`, expose `namespace` to minijinja:

```rust
template
    .render(context! {
        app => template_values.get("app").cloned().unwrap_or(Value::Null),
        vars => template_values.get("vars").cloned().unwrap_or(Value::Null),
        volumes => template_values.get("volumes").cloned().unwrap_or(Value::Null),
        service => template_values.get("service").cloned().unwrap_or(Value::Null),
        namespace => template_values.get("namespace").cloned().unwrap_or(Value::Null),
    })
    .map_err(|e| anyhow!("render template: {}", e))
```

In `debug_print_template_values`, add `"namespace"` to the iterator over sections:

```rust
for section in ["service", "namespace", "app", "vars", "volumes"] {
```

- [ ] **Step 10.4: Add `ins.namespace` to compose labels**

In `src/pipeline/labels.rs::build_compose_metadata_labels`:

```rust
labels.insert("ins.node_name".into(), node_name(node).to_string());
insert_compose_label(&mut labels, "ins.service", template_values.get("service"));
insert_compose_label(&mut labels, "ins.namespace", template_values.get("namespace"));
```

- [ ] **Step 10.5: Thread namespace through copy & pipeline**

`src/pipeline/copy.rs::copy_apps_to_workspace_with_output` (and `copy_apps_to_workspace`) must accept `namespace: &str` and pass it into `build_target_template_values`. Update each call.

In `src/pipeline/mod.rs::execute_pipeline_with_output`, replace:

```rust
for target in &prepared.targets {
    print_target_template_values(target, &prepared.node, &prepared.namespace, &volumes_config, &output)?;
}

let resolved_volumes = copy_apps_to_workspace_with_output(
    home,
    &prepared.targets,
    &prepared.app_home,
    &prepared.workspace,
    &prepared.node,
    &prepared.namespace,
    &volumes_config,
    &probe_cache,
    &output,
)
.await?;
```

- [ ] **Step 10.6: Run — expect PASS**

Run: `cargo test --features duckdb-bundled pipeline`
Expected: pass, including the new label assertion.

- [ ] **Step 10.7: Run `make check`** — expect green.

- [ ] **Step 10.8: Commit**

```bash
git add src/pipeline/
git commit -m "feat(pipeline): expose namespace via template and ins.namespace label

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: `ins service list` shows namespace

**Files:**
- Modify: `src/output.rs`
- Modify: `src/output_test.rs`

- [ ] **Step 11.1: Write failing test**

Append to `src/output_test.rs`:

```rust
use crate::store::duck::InstalledServiceRecord;

#[test]
fn installed_service_record_table_includes_namespace_column() {
    let record = InstalledServiceRecord {
        service: "web".into(),
        namespace: "staging".into(),
        app_name: "nginx".into(),
        node_name: "node-a".into(),
        workspace: "/srv/ws".into(),
        created_at_ms: 1_700_000_000_000,
    };

    let headers = <InstalledServiceRecord as crate::output::TableRenderable>::headers();
    assert_eq!(headers.first().copied(), Some("namespace"));
    let row = record.row();
    assert_eq!(row.first(), Some(&String::from("staging")));
}
```

- [ ] **Step 11.2: Run — expect FAIL** (header order differs).

- [ ] **Step 11.3: Update `TableRenderable for InstalledServiceRecord`**

In `src/output.rs`:

```rust
impl TableRenderable for InstalledServiceRecord {
    fn headers() -> &'static [&'static str] {
        &["namespace", "service", "app", "node", "workspace", "created_at_ms"]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.namespace.clone(),
            self.service.clone(),
            self.app_name.clone(),
            self.node_name.clone(),
            self.workspace.clone(),
            self.created_at_ms.to_string(),
        ]
    }
}
```

- [ ] **Step 11.4: Run — expect PASS** (`cargo test --features duckdb-bundled output`).

- [ ] **Step 11.5: Run `make check`** — expect green.

- [ ] **Step 11.6: Commit**

```bash
git add src/output.rs src/output_test.rs
git commit -m "feat(cli): show namespace column in ins service list

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 12: CLI parsing tests for `--namespace`

**Files:**
- Create: `src/cli/check_test.rs`
- Modify: `src/cli/check.rs` (add `#[cfg(test)] mod` line)
- Modify: `src/cli/deploy_test.rs`

- [ ] **Step 12.1: Write `src/cli/check_test.rs`**

```rust
use clap::Parser;

#[derive(clap::Parser, Debug)]
struct Wrapper {
    #[command(flatten)]
    args: super::CheckArgs,
}

#[test]
fn check_parses_namespace_flag() {
    let parsed = Wrapper::parse_from(["test", "--namespace", "staging", "web"]);
    assert_eq!(parsed.args.pipeline.namespace.as_deref(), Some("staging"));
    assert_eq!(parsed.args.pipeline.apps.as_deref(), Some(&["web".to_string()][..]));
}

#[test]
fn check_namespace_absent_yields_none() {
    let parsed = Wrapper::parse_from(["test", "web"]);
    assert!(parsed.args.pipeline.namespace.is_none());
}
```

- [ ] **Step 12.2: Wire test module into `src/cli/check.rs`**

Append at the end:

```rust
#[cfg(test)]
#[path = "check_test.rs"]
mod check_test;
```

- [ ] **Step 12.3: Mirror in `src/cli/deploy_test.rs`**

Append:

```rust
#[test]
fn deploy_parses_namespace_flag() {
    use clap::Parser;
    #[derive(clap::Parser, Debug)]
    struct Wrapper {
        #[command(flatten)]
        args: super::DeployArgs,
    }
    let parsed = Wrapper::parse_from(["test", "--namespace", "prod", "redis"]);
    assert_eq!(parsed.args.pipeline.namespace.as_deref(), Some("prod"));
}
```

- [ ] **Step 12.4: Run — expect PASS** (`cargo test --features duckdb-bundled cli`).

- [ ] **Step 12.5: Run `make check`** — expect green.

- [ ] **Step 12.6: Commit**

```bash
git add src/cli/check.rs src/cli/check_test.rs src/cli/deploy_test.rs
git commit -m "test(cli): cover --namespace parsing on check and deploy

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 13: TUI integration sanity check

**Files:**
- Modify: `src/tui/mod.rs` (only if needed)

- [ ] **Step 13.1: Audit `prepare_installed_service_deployment` callers**

Run:

```bash
grep -n prepare_installed_service_deployment src/tui/
```

Confirm the call already passes the `InstalledServiceRecord` it loaded — that record now carries `namespace`, and `prepare_installed_service_deployment` (Task 6.5) reads it. No code change should be needed unless the TUI shows a service-detail panel; if it does, surface `service.namespace` in the rendered text.

- [ ] **Step 13.2: Run TUI tests**

Run: `cargo test --features duckdb-bundled tui`
Expected: still green; fix any compile errors caused by the `InstalledServiceRecord.namespace` addition (test fixtures must include `namespace: "default".into()`).

- [ ] **Step 13.3: Run `make check`** — expect green.

- [ ] **Step 13.4: Commit**

```bash
git add src/tui/
git commit -m "fix(tui): keep tests green after namespace addition

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

(Skip the commit if no code changed — only test fixtures.)

---

## Task 14: Documentation updates

**Files:**
- Modify: `docs/check-and-deploy.md`
- Modify: `docs/qa-yaml-dependencies-env.md`
- Modify: `docs/env-vars.md`
- Modify: `docs/template-values.md`
- Create: `docs/namespaces.md`
- Modify: `CLAUDE.md`

- [ ] **Step 14.1: `docs/check-and-deploy.md` — add `--namespace`**

Insert a new subsection after the `--node` subsection:

```markdown
### `--namespace <NAME>`

部署 namespace。默认 `default`。同一次 `check` / `deploy` 调用所有 app 共享同一个 namespace。

- 命名规则：`^[a-z0-9][a-z0-9_-]{0,63}$`
- 同一节点上同一个 service name **不允许**跨 namespace 共存：如果节点上已有 `default:web`，再跑 `ins deploy --namespace staging web` 会报错
- 跨节点不受限：`default:web` 在 node-A、`staging:web` 在 node-B 是允许的

```bash
ins deploy --namespace staging --node prod web api
ins check  --node prod web                    # 不传 → namespace=default
```
```

Add `--namespace` to the §"参数（按常用度排序）" overview list.

- [ ] **Step 14.2: `docs/qa-yaml-dependencies-env.md` — namespace prefix syntax**

Replace the existing `dependencies` section with:

```markdown
### `dependencies`

含义：声明当前 app 依赖哪些"已安装的 service"。每个条目可选地带 namespace 前缀。

是否必填：否。

格式：

| 写法 | 解析为 |
|---|---|
| `redis` | (default, redis) |
| `:redis` | (default, redis) |
| `staging:redis` | (staging, redis) |
| `prod:mysql-main` | (prod, mysql-main) |

只有依赖 service 在指定 namespace 下已安装时才注入对应环境变量。

环境变量前缀规则（hybrid）：

- 默认 namespace（`redis` / `:redis`）→ `INS_SERVICE_<SERVICE>_*`
- 显式非默认 namespace（`<ns>:<service>`）→ `INS_SERVICE_<NS>_<SERVICE>_*`

举例 ——

```yaml
dependencies:
  - redis
  - staging:redis
```

会注入：

```text
INS_SERVICE_REDIS_*           # 来自 default 命名空间
INS_SERVICE_STAGING_REDIS_*   # 来自 staging 命名空间
```
```

Update §3 to add `INS_SERVICE_REDIS_NAMESPACE` and the `INS_SERVICE_<NS>_<SVC>_*` shape with worked example.

- [ ] **Step 14.3: `docs/env-vars.md`**

In the section listing generated `INS_*` vars, add:

```markdown
- `INS_NAMESPACE` — 当前部署的 namespace（默认 `default`）。在 `before.sh` / `after.sh` 与容器中可见。
```

In the dependency-env section, add the hybrid-keying rule with the same examples as `qa-yaml-dependencies-env.md`.

- [ ] **Step 14.4: `docs/template-values.md`**

Add to the available context bag:

```markdown
- `{{ namespace }}` — 当前部署的 namespace 字符串（默认 `default`）。
```

Add an example:

```jinja
# generated for {{ app.name }} ({{ namespace }})
```

- [ ] **Step 14.5: Create `docs/namespaces.md`**

```markdown
# Namespaces

`ins` 的 namespace 是一个逻辑标签，附加在每次 `check` / `deploy` 上，影响：

1. `deploy_history` 的存储维度（按 `(node, namespace, service)` 唯一）
2. `qa.yaml` `dependencies` 的查找目标（`<ns>:<svc>` 语法）
3. provider 环境变量的命名（hybrid 规则）
4. compose 文件里注入的 `ins.namespace` label

## CLI

```bash
ins deploy --namespace staging --node prod web api    # staging 命名空间
ins deploy --node prod redis                          # 不传 → default
```

参数详情见 [check-and-deploy.md](./check-and-deploy.md)。

## 命名规则

正则：`^[a-z0-9][a-z0-9_-]{0,63}$`

理由：namespace 文本会进入环境变量 key，必须是 ASCII 大小写无歧义可转换的形态。

## 同节点 service name 唯一

> 同一台机器上不能部署相同的 service name 且 namespace 不同的服务。

如果节点上已经有 `default:web`，再执行 `ins deploy --namespace staging web` 会报错：

```text
service 'web' already exists on node 'prod' under namespace 'default'; \
cannot deploy under namespace 'staging'. \
Run `ins service rm web` first or redeploy under namespace 'default'.
```

跨节点不受这个限制。

## 依赖 namespace 前缀

```yaml
dependencies:
  - redis            # default 命名空间
  - :mysql           # default 命名空间（写法等价于 `mysql`）
  - staging:cache    # staging 命名空间
```

env 注入：

| dependency | env 前缀 |
|---|---|
| `redis` / `:redis` | `INS_SERVICE_REDIS_*` |
| `staging:redis` | `INS_SERVICE_STAGING_REDIS_*` |

每个前缀下都会带：`_SERVICE`、`_NAMESPACE`、`_APP_NAME`、`_NODE_NAME`、`_WORKSPACE`、`_CREATED_AT_MS`、`_<VALUE>` ...

## 重新部署已装服务

`ins service list` 已带 NAMESPACE 列。从 TUI 触发的"重新部署"会沿用记录里的 namespace，不需要再传 `--namespace`。

## 模板变量

模板上下文里有 `{{ namespace }}`，可用于在生成的文件中标注当前归属：

```jinja
# {{ app.name }} ({{ namespace }})
```

## Compose label

每个 service 自动注入：

```yaml
labels:
  ins.namespace: <namespace>
```
```

- [ ] **Step 14.6: `CLAUDE.md` — add row to docs table**

In the table that lists `docs/env-vars.md`, `docs/template-values.md`, etc., insert:

```markdown
| `docs/namespaces.md`               | namespace CLI flag、qa.yaml `<ns>:<svc>` 解析、env-key hybrid 规则、`ALTER TABLE` 迁移、conflict guard。代码改动涉及 namespace 相关行为时需要同步更新。 |
```

- [ ] **Step 14.7: Run `make check`** — expect green (docs alone change nothing in code, but the gate is mandatory).

- [ ] **Step 14.8: Commit**

```bash
git add docs/ CLAUDE.md
git commit -m "docs: namespace coverage across check/deploy reference and qa.yaml

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Final Verification

- [ ] **Step F.1: `make check`** — full repo, expect green.
- [ ] **Step F.2: End-to-end smoke (manual)**

```bash
# Fresh ins home with no existing deploys
mkdir /tmp/ins-smoke && cd /tmp/ins-smoke
cargo run --features duckdb-bundled -- --home . app add < some-template
cargo run --features duckdb-bundled -- check  --namespace staging --node local web
cargo run --features duckdb-bundled -- deploy --namespace staging --node local web
cargo run --features duckdb-bundled -- service list
# Expect web row with NAMESPACE=staging
cargo run --features duckdb-bundled -- deploy --namespace prod --node local web
# Expect error: "service 'web' already exists on node 'local' under namespace 'staging'"
```

Note: this is manual; not a test step. Skip if no usable template handy — the unit tests already cover the behaviors.

- [ ] **Step F.3: Final commit (only if any cleanup left)** — most likely no-op.

---

## Self-Review Notes

Spec coverage check:

- §1 motivation, §2 CLI surface → Tasks 6, 12, 14.1
- §3 dependency syntax → Task 1, 2, 14.2
- §4 DB schema + migration + queries → Tasks 3, 4, 5
- §5 env-var generation → Task 9
- §6 conflict check → Task 7
- §7 compose labels + template context → Task 10
- §8 redeploy path → Task 6.5, Task 13
- §9 service list → Task 11
- §10 test coverage → embedded in each task
- §11 documentation → Task 14
- §12 out of scope, §13 risks → no implementation work; tests in Tasks 3 + 7 cover the named risks

No placeholders, no "similar to X" cross-refs without showing code, every step has either exact code or exact command + expected outcome.
