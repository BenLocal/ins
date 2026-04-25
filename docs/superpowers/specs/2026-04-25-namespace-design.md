# Namespace for `check` / `deploy` — Design

Date: 2026-04-25
Status: Approved (pending implementation)

## 1. Motivation

`ins check` / `ins deploy` currently identify a service on a node by its service name alone. The user wants a `namespace` dimension layered on top so that:

- A deploy run can be tagged with a namespace (e.g. `staging`, `prod`, `default`).
- Stored deploy history is keyed by `(node, namespace, service)` instead of `(node, service)`.
- A `qa.yaml` `dependencies` entry can target a specific namespace via the `namespace:service_name` syntax. Bare `service_name` and `:service_name` mean the `default` namespace.

Constraint from the user discussion: **same node + same service name + different namespace is forbidden.** Service name remains globally unique on a node; namespace is a logical label, not a physical partition. Across nodes there is no constraint — `default:web` on `node-A` and `staging:web` on `node-B` are independent.

## 2. CLI Surface

New flag on `PipelineArgs` (so it appears on both `ins check` and `ins deploy`):

```text
--namespace <NAME>
```

- No short option. `-n` is already `--node`; introducing `-N` is more confusing than clarifying.
- Default: `"default"`. Resolved once during `prepare_deployment`, stored on `PreparedDeployment.namespace: String`.
- Applies to **all** apps in the run. Multi-namespace deploys require multiple invocations (YAGNI on per-app override).
- Validation regex: `^[a-z0-9][a-z0-9_-]{0,63}$`. Anything else → fail fast with a specific error mentioning the offending value. The regex matters because the namespace participates in env-var keys (which are case-sensitive ASCII) and DB rows.

CLI examples:

```bash
ins check  --namespace staging --node prod web api
ins deploy --namespace prod    --node prod redis
ins deploy --node prod redis              # implicit default namespace
```

The flag is orthogonal to `-d / --defaults`, `-v`, `-w`, `-p`.

## 3. `qa.yaml` Dependency Syntax

Wire format unchanged: `dependencies` is still `Vec<String>`. Parsing rules:

| Raw entry         | `(namespace, service)`     | `explicit_namespace` |
|-------------------|----------------------------|----------------------|
| `redis`           | `(default, redis)`         | false                |
| `:redis`          | `(default, redis)`         | false                |
| `staging:redis`   | `(staging, redis)`         | true                 |
| `prod:mysql-main` | `(prod, mysql-main)`       | true                 |

Rejected during parse (errors propagate from `load_app_record`):
- Empty service name (`""`, `"foo:"`, `":"`)
- More than one `:` (`a:b:c`) — kept off-limits to avoid future ambiguity
- Namespace failing the §2 regex

Internal type:

```rust
pub struct DependencyRef {
    pub namespace: String,        // "default" when prefix is empty
    pub service: String,
    pub explicit_namespace: bool, // drives env-var key shape (§5)
}
```

`AppRecord.dependencies: Vec<String>` is preserved (template `{{ app.dependencies }}` still returns raw strings, backward compatible). A new derived method `AppRecord::parsed_dependencies() -> Vec<DependencyRef>` is the canonical accessor for env-var generation and conflict checks.

## 4. DB Schema and Migration

Single new column on `deployment_history`:

```sql
namespace TEXT NOT NULL DEFAULT 'default'
```

`ensure_schema` runs (in order, idempotently) on every connection:

1. `CREATE TABLE IF NOT EXISTS deployment_history (...)` — for fresh installs the column is included from the start.
2. `ALTER TABLE deployment_history ADD COLUMN IF NOT EXISTS namespace TEXT NOT NULL DEFAULT 'default'` — for installs that pre-date this change. DuckDB supports `ADD COLUMN IF NOT EXISTS`. Existing rows backfill to `'default'` via the column default.

Query changes (all in `src/store/duck.rs`):

| Function                             | Change                                                                                  |
|--------------------------------------|------------------------------------------------------------------------------------------|
| `load_latest_deployment_record`      | Add `AND namespace = ?` to the `WHERE` clause; signature gains `namespace: &str`.       |
| `list_installed_services`            | `PARTITION BY namespace, service` in the window; SELECT and struct return namespace.    |
| `load_installed_service_configs`     | Same window change; struct returns namespace.                                           |
| `save_deployment_record`             | INSERT 9 columns instead of 8.                                                           |
| `find_service_namespace_on_node` (new)| `SELECT namespace FROM (... PARTITION BY service ORDER BY created_at_ms DESC) WHERE row_num=1 AND node_name=?`. Used by §6 conflict check. |

Struct changes:

- `StoredDeploymentRecord` — add `namespace: String`
- `InstalledServiceRecord` — add `namespace: String`
- `InstalledServiceConfigRecord` — add `namespace: String`

## 5. Env-Var Generation

Three changes in `src/env.rs`.

### 5.1 Current-app env

`build_target_envs` adds:

```text
INS_NAMESPACE=<current-namespace>
```

placed alongside `INS_NODE_NAME` / `INS_APP_NAME` / `INS_SERVICE_NAME`. Available to `before.sh`, `after.sh`, and the container at runtime.

### 5.2 Dependency env (hybrid keying)

`append_installed_service_envs` is restructured:

- Old: scan all `installed_services`, find any whose `service` matches a string in `dependencies`.
- New: iterate `app.parsed_dependencies()`. For each `DependencyRef`, do a precise `(namespace, service)` lookup against `installed_services`. Miss → silently skip (preserves existing "dep not yet installed → no vars injected" semantics).

Env-key prefix rule (matches the user's Q1 = C choice):

| `explicit_namespace` | Prefix                                |
|----------------------|---------------------------------------|
| `false`              | `INS_SERVICE_<SERVICE>_*`             |
| `true`               | `INS_SERVICE_<NAMESPACE>_<SERVICE>_*` |

`<NAMESPACE>` and `<SERVICE>` are both run through the existing `env_key_for_value_name` helper (uppercase ASCII; non-alphanumeric becomes `_`; leading digit gets a `_` prefix). The §2 regex guarantees this transformation is loss-free for the namespace.

Under each prefix, the existing six entries are emitted plus a new `_NAMESPACE` field:

- `_SERVICE`
- `_NAMESPACE` (new — always present, equal to the dependency's stored namespace)
- `_APP_NAME`
- `_NODE_NAME`
- `_WORKSPACE`
- `_CREATED_AT_MS`
- `_<VALUE_NAME>` for each entry in the dependency's `app_values`

### 5.3 Worked example

App declares:

```yaml
dependencies:
  - redis            # default ns
  - staging:redis    # staging ns (a different deployment of the same service)
```

Result (with both deps installed, each with their own `port` value):

```text
INS_NAMESPACE=default
INS_SERVICE_REDIS_SERVICE=redis
INS_SERVICE_REDIS_NAMESPACE=default
INS_SERVICE_REDIS_PORT=6379
INS_SERVICE_STAGING_REDIS_SERVICE=redis
INS_SERVICE_STAGING_REDIS_NAMESPACE=staging
INS_SERVICE_STAGING_REDIS_PORT=6380
```

Existing templates that reference `${INS_SERVICE_REDIS_*}` continue to work unchanged because the user's Q3 answer (same-name-cross-namespace forbidden) means there is at most one `redis` deployment per node. The hybrid keying matters only when the *consumer* explicitly chooses to depend on a specific namespace.

## 6. Conflict Check

New function `find_service_namespace_on_node(home, node, service)` (see §4).

Wired into `prepare_deployment` after `build_deployment_targets`, before returning `PreparedDeployment`:

```rust
for target in &targets {
    if let Some(existing_ns) = find_service_namespace_on_node(home, &node, &target.service).await?
        && existing_ns != namespace
    {
        return Err(anyhow!(
            "service '{}' already exists on node '{}' under namespace '{}'; \
             cannot deploy under namespace '{}'. Run `ins service rm {}` first \
             or redeploy under namespace '{}'.",
            target.service, node_name(&node), existing_ns,
            namespace, target.service, existing_ns,
        ));
    }
}
```

Both `check` and `deploy` go through `prepare_deployment`, so `check` surfaces the conflict early without side effects.

The redeploy path (`prepare_installed_service_deployment`) does **not** run this check — the namespace it uses comes from the existing `InstalledServiceRecord`, so it is by definition consistent.

## 7. Compose Labels and Template Context

`build_compose_metadata_labels` already reads from `template_values`. Adding namespace to that bag in `pipeline/template.rs::build_template_values` exposes it to two places in one shot:

1. Templates: `{{ namespace }}` becomes available alongside `{{ app }}`, `{{ vars }}`, `{{ volumes }}`, `{{ service }}`.
2. Compose labels: `build_compose_metadata_labels` calls `insert_compose_label(&mut labels, "ins.namespace", template_values.get("namespace"))`. Output adds `ins.namespace=<value>` to every service in every rewritten compose file.

## 8. Re-deploy Path (TUI)

`prepare_installed_service_deployment` (called from `src/tui/mod.rs` when a TUI user redeploys an already-installed service):

- Reads `service.namespace` from the loaded `InstalledServiceRecord`.
- Sets `PreparedDeployment.namespace` accordingly.
- Does **not** consult the CLI `--namespace` flag — the deploy is implicitly tied to the existing namespace. Switching namespaces requires `ins service rm <name>` then a fresh `ins deploy --namespace <new> ...`.

## 9. `ins service list`

`InstalledServiceRecord` gaining `namespace` automatically flows into JSON output (it derives `Serialize`). The table renderer (`TableRenderable::headers()` + `row()`) prepends a `NAMESPACE` column so terminal users see it without `--output json`.

## 10. Test Coverage

Following the project convention of one `*_test.rs` sibling per source file (CLAUDE.md):

| Test file                          | New cases                                                                                       |
|-----------------------------------|-------------------------------------------------------------------------------------------------|
| `src/app/dependency_test.rs` (new, with new `src/app/dependency.rs` module that owns the parser) | `redis`, `:redis`, `staging:redis` → expected `DependencyRef`; rejects `a:b:c`, empty service, illegal namespace chars |
| `src/store/duck_test.rs`           | save+load roundtrip preserves namespace; legacy table without column ALTERed → reads `default`; `find_service_namespace_on_node` returns `Some(ns)` only when an entry exists |
| `src/env_test.rs`                  | default-ns dep → `INS_SERVICE_REDIS_*`; explicit-ns dep → `INS_SERVICE_STAGING_REDIS_*`; both coexist; `INS_NAMESPACE` set; `INS_SERVICE_*_NAMESPACE` set |
| `src/pipeline/prepare_test.rs`     | conflict check fires when same service exists under another namespace; passes when same namespace; passes when no record exists |
| `src/pipeline/labels_test.rs` (extend or add) | `ins.namespace` appears in injected labels of every rewritten service |
| `src/cli/deploy_test.rs` (existing); add `src/cli/check_test.rs` (new) | `--namespace` parsed; absent → `"default"`; invalid name → error |

Async tests use `#[tokio::test]`. All test files reachable via `#[cfg(test)] #[path = "..."] mod ..._test;` blocks per CLAUDE.md.

## 11. Documentation Updates (same commit as code)

| Doc                                | Update                                                                                  |
|------------------------------------|------------------------------------------------------------------------------------------|
| `docs/check-and-deploy.md`         | Add `--namespace` to the args table and worked examples.                                |
| `docs/qa-yaml-dependencies-env.md` | New "namespace 前缀" subsection in §2 dependencies; updated env-var examples for §3, §5. |
| `docs/env-vars.md`                 | `INS_NAMESPACE` appears in the layer that lists generated `INS_*` vars; add the `INS_SERVICE_<NS>_<SVC>_*` shape.       |
| `docs/template-values.md`          | `{{ namespace }}` listed in the available context bag.                                  |
| `docs/namespaces.md` (new)         | Concept overview: what a namespace is, how it interacts with dependencies, the same-node / same-name-different-namespace ban, and the redeploy story. |
| `CLAUDE.md` doc table              | Add a row for `docs/namespaces.md` describing what a code change must revisit.          |

## 12. Out of Scope

- Per-app namespace override (`--namespace web=staging --namespace api=prod`). Not requested; YAGNI until a real workflow surfaces.
- A `ins namespace` command (list / rename / move). Today the only mutation channel is via `ins deploy` and `ins service rm`; that is sufficient for the stated requirement.
- Re-keying historical env-var consumers when a namespaced redeploy happens — out of scope because the cross-node usage doesn't change keys, and the same-node cross-namespace case is forbidden.

## 13. Open Risks

- **Backfill correctness on legacy DBs.** `ALTER TABLE ... ADD COLUMN ... DEFAULT 'default'` must succeed on a populated table. Mitigated by a test that inserts old-shape rows then runs `ensure_schema`.
- **Namespace name regex creep.** If users want uppercase or `.` in namespace names later, the regex must relax in lockstep with env-var key rewriting. Locked down for now to keep the env-key story uniform.
- **Existing deployments running today.** They live under the implicit `default` namespace after migration. No rename / re-tag needed.
