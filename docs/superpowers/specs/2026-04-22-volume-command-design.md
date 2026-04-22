# Volume Command Design

Date: 2026-04-22
Status: Approved (brainstorming)

## Summary

Add an `ins volume` top-level command that lets users configure per-node Docker volume backings. A single logical volume name (e.g. `data`) can resolve to a local filesystem bind mount on one node and a CIFS remote share on another. At deploy time, `ins` rewrites the app's compose file to reference an `external` volume and ensures the docker volume exists on the target node before `docker compose up -d` runs.

## Goals

- Configure node-specific storage backings for a shared logical volume name.
- Support two volume types in this iteration: `filesystem` (local bind) and `cifs` (SMB).
- Integrate cleanly with the existing deploy pipeline, without changing `qa.yaml` schema or `nodes.json`.
- Apps keep writing standard Docker Compose volume syntax; `ins` handles driver/options injection.
- Fail fast on missing configuration — no silent fallback to a default local volume.

## Non-Goals

- No NFS / custom drivers in this iteration (only `filesystem` and `cifs`).
- No automatic creation in `ins volume add` (save-only, like `ins node add`).
- No encryption or keyring for CIFS passwords; use plaintext in `volumes.json`, matching the current `nodes.json` convention.
- No driver/options diffing: if a docker volume already exists on the node, it is reused as-is.
- No `volume apply` / force-recreate subcommand.

## Architecture

```
.ins/
├── nodes.json                 # existing
├── volumes.json               # NEW — per-node volume backings
├── app/...
└── store/...

src/
├── cli/volume.rs              # NEW — add/set/delete/list subcommands
├── volume/
│   ├── mod.rs
│   ├── list.rs                # load/save volumes.json
│   └── types.rs               # VolumeRecord / FilesystemVolume / CifsVolume / ResolvedVolume
├── pipeline.rs                # CHANGE — inject compose volumes alongside existing label injection
└── provider/
    └── docker_compose.rs      # CHANGE — ensure_volumes before `up -d` on both local and remote
```

### Deployment-time resolution flow

1. While copying app files into the workspace, when writing a `docker-compose.y(a)ml`, `ins` parses the YAML and extracts top-level volume names from `volumes:`. Service-level `volumes:` entries are left untouched.
2. For each top-level volume name, `ins` looks up `(name, current_node)` in `volumes.json`:
   - Not found → return error; deployment aborts (strict mode).
   - Found → rewrite the top-level `volumes.<name>` entry to `{ external: true, name: "ins_<name>" }`.
3. The resolved `(docker_name, driver, driver_opts)` list is threaded through `ProviderContext.volumes` to `docker_compose::run`.
4. `docker_compose::run` runs `ensure_volumes` before `docker compose up -d`:
   - `docker volume inspect ins_<name>` → exit 0 means present, skip.
   - Otherwise `docker volume create --driver local --opt type=… --opt o=… --opt device=… ins_<name>`. Failure aborts the deploy.
5. `docker compose up -d` runs as today.

### `check` vs `deploy`

Both modes go through the copy-to-workspace step, so `inject_compose_volumes` runs in both — meaning missing-volume configuration is caught by `ins check` too, not only at deploy time. `ensure_volumes` (the actual `docker volume inspect` / `create` calls on the node) only runs under `deploy`. `docker compose config -q` in check mode does not require the volume to exist.

## Data Model

### `volumes.json`

Flat array, easy to append and delete. Example:

```json
[
  { "name": "data", "node": "node1", "type": "filesystem", "path": "/mnt/data" },
  {
    "name": "data",
    "node": "node2",
    "type": "cifs",
    "server": "//10.0.0.5/share",
    "username": "alice",
    "password": "secret"
  }
]
```

### Rust types (`src/volume/types.rs`)

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum VolumeRecord {
    Filesystem(FilesystemVolume),
    Cifs(CifsVolume),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct FilesystemVolume {
    pub(crate) name: String,
    pub(crate) node: String,
    pub(crate) path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct CifsVolume {
    pub(crate) name: String,
    pub(crate) node: String,
    pub(crate) server: String,
    pub(crate) username: String,
    pub(crate) password: String,
}

#[derive(Clone, Debug)]
pub struct ResolvedVolume {
    pub docker_name: String,                    // "ins_<name>"
    pub driver: String,                         // always "local" for v1
    pub driver_opts: BTreeMap<String, String>,
}
```

### Uniqueness

`(name, node)` is the primary key. `add` with a duplicate key errors out; `set` and `delete` with a non-existent key error out.

### Docker volume naming

The actual docker volume name on the node is fixed to `ins_<name>` to avoid collisions with volumes created by other tooling.

## CLI

Nested subcommands, mirroring `ins node` style:

```
ins volume add filesystem  --name <N> --node <NODE> --path <HOST_PATH>
ins volume add cifs        --name <N> --node <NODE> --server <//srv/share> --username <U> --password <P>
ins volume set filesystem  --name <N> --node <NODE> --path <HOST_PATH>
ins volume set cifs        --name <N> --node <NODE> --server ... --username ... --password ...
ins volume delete          --name <N> --node <NODE>
ins volume list
```

### Clap shape

```rust
#[derive(clap::Args)]
pub struct VolumeArgs {
    #[command(subcommand)]
    pub command: VolumeSubcommand,
}

#[derive(clap::Subcommand)]
pub enum VolumeSubcommand {
    Add(VolumeAddArgs),
    Set(VolumeSetArgs),
    Delete(VolumeDeleteArgs),
    List(VolumeListArgs),
}

#[derive(clap::Args)]
pub struct VolumeAddArgs {
    #[command(subcommand)]
    pub kind: VolumeTypeArgs,
}

#[derive(clap::Args)]
pub struct VolumeSetArgs {
    #[command(subcommand)]
    pub kind: VolumeTypeArgs,
}

#[derive(clap::Subcommand)]
pub enum VolumeTypeArgs {
    Filesystem(FilesystemVolumeArgs),
    Cifs(CifsVolumeArgs),
}
```

### Validation rules

Shared between `add` and `set`:

- `--name`: non-empty, `^[a-zA-Z0-9_-]+$` (matches Docker volume naming rules).
- `--node`: must exist in `nodes.json` or be the reserved `local` node.
- `filesystem.path`: non-empty, must be absolute. Existence on the target node is **not** checked — keep the "save, don't connect" semantics.
- `cifs.server`: must start with `//`.
- `add` with duplicate `(name, node)` → error.
- `set` / `delete` with missing `(name, node)` → error.

### `list` output

Uses the existing `print_structured_list` helper so `--output table|json` works the same as other commands.

Table form:

```
NAME    NODE    TYPE         DETAIL
data    node1   filesystem   /mnt/data
data    node2   cifs         //10.0.0.5/share (alice)
```

JSON form: the raw `VolumeRecord` array (password field included, consistent with `nodes.json`).

## Deploy Pipeline Integration

### `inject_compose_volumes` (new, in `pipeline.rs`)

Called from `copy_file_to_workspace` next to `maybe_inject_compose_labels`.

```rust
fn inject_compose_volumes(
    content: &str,
    node: &NodeRecord,
    volumes: &[VolumeRecord],
) -> anyhow::Result<(String, Vec<ResolvedVolume>)>;
```

Behavior:

1. Parse the compose YAML. If there is no top-level `volumes:` mapping, return `(content, vec![])`.
2. For each key in top-level `volumes`:
   - Look up `(name, node)` in `volumes`. Missing → return error.
   - Replace the value with `{ external: true, name: "ins_<name>" }`.
   - Emit a `ResolvedVolume` with the docker name and the driver_opts listed below.
3. Serialize the modified YAML and return.

Volume-type → driver_opts mapping:

| Type | driver | driver_opts |
|---|---|---|
| `filesystem` | `local` | `type=none`, `o=bind`, `device=<path>` |
| `cifs`       | `local` | `type=cifs`, `o=username=<u>,password=<p>`, `device=<server>` |

### `ProviderContext.volumes`

Add a new field to `ProviderContext`:

```rust
pub struct ProviderContext {
    // existing fields ...
    pub volumes: Vec<ResolvedVolume>,
}
```

Populated in `execute_pipeline_with_output` from the resolved volumes collected during the compose-file copy step. All `ResolvedVolume`s used across the prepared targets on this deploy are aggregated (deduped by `docker_name`).

### `docker_compose::ensure_volumes` (new)

Called from `DockerComposeProvider::run` on both local and remote branches, **before** the `up -d` loop.

```rust
async fn ensure_volumes(
    node: &NodeRecord,
    volumes: &[ResolvedVolume],
    output: &ExecutionOutput,
) -> anyhow::Result<()>;
```

Per volume:

1. `docker volume inspect <docker_name>` — exit 0 means the volume exists, skip. Do not compare driver_opts.
2. Otherwise, run `docker volume create --driver <d> --opt <k>=<v> ... <docker_name>`. Failure aborts the deploy, surfacing stderr verbatim.

Local branch runs via `tokio::process::Command`. Remote branch goes through `remote_file.exec` with shell-quoted arguments, reusing the existing `shell_quote` helper so CIFS credentials with special characters are safe.

`validate` (check mode) does not call `ensure_volumes`.

## Error Handling

| Scenario | Behavior |
|---|---|
| `volumes.json` missing or empty | Treated as empty; only errors at deploy time if a volume is actually referenced |
| `ins volume add` with unknown `--node` | Error: `node '<n>' not found` |
| `ins volume add` with duplicate `(name, node)` | Error: `volume '<name>' on node '<node>' already exists` |
| `ins volume set` / `delete` with missing `(name, node)` | Error: `volume '<name>' on node '<node>' not found` |
| Deploy references volume missing on current node | Error: `volume '<name>' is not configured on node '<node>'`, abort |
| `docker volume create` fails | Surface stderr, abort deploy |
| `docker volume inspect` returns existing volume with different driver_opts | Reused as-is (not detected). Operator must `docker volume rm` manually |
| Docker not available on node | Reuse existing `resolve_*_compose_command` error path |

## Testing Strategy

### Unit tests (no docker required)

- `volume/list.rs`: load/save round-trip; missing file → empty; malformed JSON → error.
- `cli/volume.rs`: uniqueness & existence checks for add/set/delete.
- `pipeline::inject_compose_volumes`:
  - top-level `volumes: { data: {} }` rewritten to `external: true` / `name: ins_data`
  - compose without a top-level `volumes:` passes through unchanged
  - missing configuration on current node → error
  - `filesystem` and `cifs` generate the correct `ResolvedVolume.driver_opts`
- `docker_compose::ensure_volumes` command-construction helpers: assert the generated `docker volume create` string for both types, and verify CIFS password goes through shell quoting (mirrors the existing `docker_compose_shell_command_prefixes_env_exports` test).

### Integration tests

- Extend the `prepare_installed_service_deployment_reuses_saved_service_and_values` pattern with a `.ins/volumes.json` fixture; assert the rewritten compose YAML content through `copy_prepared_apps_to_workspace_with_output`.
- Actual `docker volume create` execution is **not** automated (requires docker); manual verification steps go in the docs.

### Docs

- README "CLI Overview" gets an `ins volume` section.
- New `docs/volume-command.md` covers usage flow, CIFS options, and common troubleshooting (kernel CIFS module missing, credential escaping).

## Backward Compatibility

- No changes to `nodes.json`, `qa.yaml`, or `deploy_history.duckdb` schemas.
- Apps that do not declare top-level `volumes:` are unaffected.
- Existing deploys continue to work unchanged — `volumes.json` simply does not exist for them.
