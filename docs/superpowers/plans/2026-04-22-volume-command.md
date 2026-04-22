# Volume Command Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `ins volume` CLI command that stores per-node Docker volume backings (filesystem / cifs), and have the deploy pipeline rewrite the app's `docker-compose.yml` to use `external` volumes created on the target node via `docker volume create`.

**Architecture:** New `src/volume/` module owns the data model (`volumes.json`), JSON I/O, YAML injection (`inject_compose_volumes`), and CLI-facing mutation helpers. `src/cli/volume.rs` implements the clap surface. `pipeline.rs` loads `volumes.json`, threads `ResolvedVolume`s through the copy phase into `ProviderContext.volumes`. `docker_compose.rs` gains `ensure_volumes`, which inspects/creates the actual docker volume on the node before `docker compose up -d`.

**Tech Stack:** Rust, clap, serde / serde_json, serde_yaml, tokio, async-trait, anyhow. Existing `russh`-backed `RemoteFile` for remote SSH commands.

---

## File Structure

**Create:**
- `src/volume/mod.rs` — re-exports the submodules.
- `src/volume/types.rs` — `VolumeRecord`, `FilesystemVolume`, `CifsVolume`, `ResolvedVolume`.
- `src/volume/list.rs` — `volumes_file`, `load_volumes`, `save_volumes`, `add_filesystem`, `add_cifs`, `set_filesystem`, `set_cifs`, `delete_volume`.
- `src/volume/compose.rs` — `inject_compose_volumes`.
- `src/cli/volume.rs` — `VolumeArgs`, subcommands, `VolumeCommand`.
- `docs/volume-command.md` — end-user documentation.

**Modify:**
- `src/main.rs` — register `volume` module, add `Volume(cli::volume::VolumeArgs)` arm to `Command`.
- `src/cli/mod.rs` — register `pub mod volume;`.
- `src/output.rs` — add `TableRenderable` impl for `VolumeRecord`.
- `src/provider/mod.rs` — add `pub volumes: Vec<ResolvedVolume>` field to `ProviderContext`, update `new()` signature.
- `src/pipeline.rs` — load `volumes.json`, thread volumes through copy functions, collect `ResolvedVolume`s, pass into `ProviderContext`.
- `src/provider/docker_compose.rs` — add `ensure_volumes` for both local and remote, call from `run` before the `up -d` loop.
- `README.md` — add `ins volume` section.

---

## Conventions Used in This Plan

- All shell commands use the project's feature flag for tests: `cargo test --features duckdb-bundled`.
- TDD: write the failing test first, run it, then implement, then confirm green.
- "Commit" steps are explicit and small. Don't batch unrelated steps into one commit.
- Any code block in a step is complete — no ellipses. When existing code is modified, show the full replacement.

---

## Task 1: Volume module scaffolding + JSON I/O

**Files:**
- Create: `src/volume/mod.rs`
- Create: `src/volume/types.rs`
- Create: `src/volume/list.rs`
- Modify: `src/main.rs` (add `mod volume;`)

- [ ] **Step 1: Write failing test for round-trip**

Create `src/volume/list.rs`:

```rust
use std::path::{Path, PathBuf};

use anyhow::Context;
use tokio::fs;

use crate::volume::types::{CifsVolume, FilesystemVolume, VolumeRecord};

pub(crate) fn volumes_file(home: &Path) -> PathBuf {
    home.join("volumes.json")
}

pub(crate) async fn load_volumes(path: &Path) -> anyhow::Result<Vec<VolumeRecord>> {
    if !fs::try_exists(path)
        .await
        .with_context(|| format!("check volumes file {}", path.display()))?
    {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(path)
        .await
        .with_context(|| format!("read volumes file {}", path.display()))?;

    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    serde_json::from_str(&content)
        .with_context(|| format!("parse volumes file {}", path.display()))
}

pub(crate) async fn save_volumes(path: &Path, volumes: &[VolumeRecord]) -> anyhow::Result<()> {
    let content = serde_json::to_string_pretty(volumes)?;
    fs::write(path, format!("{content}\n"))
        .await
        .with_context(|| format!("write volumes file {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        env::temp_dir().join(format!(
            "ins-volume-{name}-{}-{nanos}.json",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn load_returns_empty_when_file_missing() {
        let path = unique_test_path("missing");
        let loaded = load_volumes(&path).await.expect("load");
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn save_then_load_roundtrips_mixed_types() -> anyhow::Result<()> {
        let path = unique_test_path("roundtrip");
        let volumes = vec![
            VolumeRecord::Filesystem(FilesystemVolume {
                name: "data".into(),
                node: "node1".into(),
                path: "/mnt/data".into(),
            }),
            VolumeRecord::Cifs(CifsVolume {
                name: "data".into(),
                node: "node2".into(),
                server: "//10.0.0.5/share".into(),
                username: "alice".into(),
                password: "secret".into(),
            }),
        ];

        save_volumes(&path, &volumes).await?;
        let loaded = load_volumes(&path).await?;

        assert_eq!(loaded.len(), 2);
        match &loaded[0] {
            VolumeRecord::Filesystem(v) => {
                assert_eq!(v.name, "data");
                assert_eq!(v.node, "node1");
                assert_eq!(v.path, "/mnt/data");
            }
            _ => panic!("expected filesystem"),
        }
        match &loaded[1] {
            VolumeRecord::Cifs(v) => {
                assert_eq!(v.server, "//10.0.0.5/share");
                assert_eq!(v.username, "alice");
                assert_eq!(v.password, "secret");
            }
            _ => panic!("expected cifs"),
        }

        tokio::fs::remove_file(&path).await.ok();
        Ok(())
    }
}
```

- [ ] **Step 2: Create the types module**

Create `src/volume/types.rs`:

```rust
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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

impl VolumeRecord {
    pub(crate) fn name(&self) -> &str {
        match self {
            VolumeRecord::Filesystem(v) => &v.name,
            VolumeRecord::Cifs(v) => &v.name,
        }
    }

    pub(crate) fn node(&self) -> &str {
        match self {
            VolumeRecord::Filesystem(v) => &v.node,
            VolumeRecord::Cifs(v) => &v.node,
        }
    }

    pub(crate) fn kind_label(&self) -> &'static str {
        match self {
            VolumeRecord::Filesystem(_) => "filesystem",
            VolumeRecord::Cifs(_) => "cifs",
        }
    }

    pub(crate) fn detail_label(&self) -> String {
        match self {
            VolumeRecord::Filesystem(v) => v.path.clone(),
            VolumeRecord::Cifs(v) => format!("{} ({})", v.server, v.username),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedVolume {
    pub docker_name: String,
    pub driver: String,
    pub driver_opts: BTreeMap<String, String>,
}
```

- [ ] **Step 3: Create the mod file**

Create `src/volume/mod.rs`:

```rust
pub(crate) mod list;
pub mod types;
```

- [ ] **Step 4: Register volume module in main.rs**

In `src/main.rs`, add `mod volume;` after `mod tui;` / before `mod version;` (alphabetical ordering of module declarations):

```rust
mod tui;
mod version;
mod volume;
```

- [ ] **Step 5: Run the test to confirm it passes**

Run: `cargo test --features duckdb-bundled volume::list`

Expected: both `load_returns_empty_when_file_missing` and `save_then_load_roundtrips_mixed_types` pass.

- [ ] **Step 6: Run clippy and fmt**

Run:
```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: no errors, no warnings.

- [ ] **Step 7: Commit**

```bash
git add src/volume src/main.rs
git commit -m "feat(volume): add volume module with JSON I/O"
```

---

## Task 2: Volume CLI scaffolding + `list` subcommand

**Files:**
- Create: `src/cli/volume.rs`
- Modify: `src/cli/mod.rs` (add `pub mod volume;`)
- Modify: `src/main.rs` (add `Volume` subcommand arm)
- Modify: `src/output.rs` (add `TableRenderable` impl for `VolumeRecord`)

- [ ] **Step 1: Implement `TableRenderable` for VolumeRecord**

In `src/output.rs`, add at the top with the other `use` statements:

```rust
use crate::volume::types::VolumeRecord;
```

Append this impl after the existing `impl TableRenderable for InstalledServiceRecord` block:

```rust
impl TableRenderable for VolumeRecord {
    fn headers() -> &'static [&'static str] {
        &["name", "node", "type", "detail"]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.name().into(),
            self.node().into(),
            self.kind_label().into(),
            self.detail_label(),
        ]
    }
}
```

- [ ] **Step 2: Create `src/cli/volume.rs` with scaffolding + list**

```rust
use std::path::Path;

use async_trait::async_trait;

use crate::cli::{CommandContext, CommandTrait};
use crate::output::print_structured_list;
use crate::volume::list::{load_volumes, volumes_file};

#[derive(clap::Args, Clone, Debug)]
/// Manage per-node Docker volume backings.
pub struct VolumeArgs {
    #[command(subcommand)]
    pub command: VolumeSubcommand,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum VolumeSubcommand {
    /// List configured volumes.
    List(VolumeListArgs),
}

#[derive(clap::Args, Clone, Debug, Default)]
pub struct VolumeListArgs {}

pub struct VolumeCommand;

#[async_trait]
impl CommandTrait for VolumeCommand {
    type Args = VolumeArgs;

    async fn run(args: VolumeArgs, ctx: CommandContext) -> anyhow::Result<()> {
        match args.command {
            VolumeSubcommand::List(_) => list_volumes(&volumes_file(&ctx.home), ctx.output).await,
        }
    }
}

async fn list_volumes(path: &Path, output: crate::OutputFormat) -> anyhow::Result<()> {
    let volumes = load_volumes(path).await?;
    print_structured_list(&volumes, output, "no volumes found")
}
```

- [ ] **Step 3: Register the `volume` CLI module**

In `src/cli/mod.rs`, add `pub mod volume;` after `pub mod version;`:

```rust
pub mod app;
pub mod check;
pub mod deploy;
pub mod node;
pub mod service;
pub mod template;
pub mod tui;
pub mod version;
pub mod volume;
```

- [ ] **Step 4: Register the command in main.rs**

Update the `use crate::cli::{...}` block at the top of `src/main.rs` to include `volume::VolumeCommand`:

```rust
use crate::cli::{
    CommandContext, CommandTrait, app::AppCommand, check::CheckCommand, deploy::DeployCommand,
    node::NodeCommand, service::ServiceCommand, template::TemplateCommand, tui::TuiCommand,
    version::VersionCommand, volume::VolumeCommand,
};
```

Add the arm in the `match cli.command` block (put it after the `Version` arm and before the `None` arm):

```rust
Some(Command::Volume(args)) => {
    VolumeCommand::run(
        args,
        CommandContext {
            home,
            output: cli.output,
        },
    )
    .await
}
```

And in the `Command` enum, add (alphabetically after `Version`):

```rust
/// Manage per-node Docker volume backings.
Volume(cli::volume::VolumeArgs),
```

- [ ] **Step 5: Verify `ins volume list` parses and runs**

Run: `cargo build --features duckdb-bundled`

Expected: builds cleanly.

Run: `cargo run --features duckdb-bundled -- volume list`

Expected: prints `no volumes found`.

- [ ] **Step 6: fmt + clippy**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/cli src/main.rs src/output.rs
git commit -m "feat(volume): add 'ins volume list' CLI subcommand"
```

---

## Task 3: `ins volume add filesystem` and `ins volume add cifs`

**Files:**
- Modify: `src/volume/list.rs` (add mutation helpers + tests)
- Modify: `src/cli/volume.rs` (add `Add` subcommand + type args + validation)

- [ ] **Step 1: Write failing tests for `add_filesystem` / `add_cifs`**

Append to the `tests` module in `src/volume/list.rs`:

```rust
    #[tokio::test]
    async fn add_filesystem_persists_record() -> anyhow::Result<()> {
        let path = unique_test_path("add-fs");
        add_filesystem(&path, "data", "node1", "/mnt/data").await?;
        let loaded = load_volumes(&path).await?;
        assert_eq!(loaded.len(), 1);
        match &loaded[0] {
            VolumeRecord::Filesystem(v) => {
                assert_eq!(v.name, "data");
                assert_eq!(v.node, "node1");
                assert_eq!(v.path, "/mnt/data");
            }
            _ => panic!("expected filesystem"),
        }
        tokio::fs::remove_file(&path).await.ok();
        Ok(())
    }

    #[tokio::test]
    async fn add_rejects_duplicate_name_node_pair() -> anyhow::Result<()> {
        let path = unique_test_path("dup");
        add_filesystem(&path, "data", "node1", "/mnt/a").await?;
        let err = add_cifs(
            &path,
            "data",
            "node1",
            "//10.0.0.5/share",
            "alice",
            "secret",
        )
        .await
        .expect_err("duplicate should fail");
        assert!(err.to_string().contains("already exists"));
        tokio::fs::remove_file(&path).await.ok();
        Ok(())
    }
```

- [ ] **Step 2: Implement `add_filesystem` and `add_cifs` in `src/volume/list.rs`**

Add above the `tests` module:

```rust
pub(crate) async fn add_filesystem(
    path: &Path,
    name: &str,
    node: &str,
    host_path: &str,
) -> anyhow::Result<()> {
    let mut volumes = load_volumes(path).await?;
    ensure_unique(&volumes, name, node)?;
    volumes.push(VolumeRecord::Filesystem(FilesystemVolume {
        name: name.to_string(),
        node: node.to_string(),
        path: host_path.to_string(),
    }));
    save_volumes(path, &volumes).await
}

pub(crate) async fn add_cifs(
    path: &Path,
    name: &str,
    node: &str,
    server: &str,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    let mut volumes = load_volumes(path).await?;
    ensure_unique(&volumes, name, node)?;
    volumes.push(VolumeRecord::Cifs(CifsVolume {
        name: name.to_string(),
        node: node.to_string(),
        server: server.to_string(),
        username: username.to_string(),
        password: password.to_string(),
    }));
    save_volumes(path, &volumes).await
}

fn ensure_unique(volumes: &[VolumeRecord], name: &str, node: &str) -> anyhow::Result<()> {
    if volumes
        .iter()
        .any(|v| v.name() == name && v.node() == node)
    {
        anyhow::bail!(
            "volume '{}' on node '{}' already exists",
            name,
            node
        );
    }
    Ok(())
}
```

- [ ] **Step 3: Run the new tests**

Run: `cargo test --features duckdb-bundled volume::list`

Expected: 4 tests pass (2 old + 2 new).

- [ ] **Step 4: Add `Add` subcommand + type args to `src/cli/volume.rs`**

Extend `VolumeSubcommand`:

```rust
#[derive(clap::Subcommand, Clone, Debug)]
pub enum VolumeSubcommand {
    /// Add a volume backing for a node.
    Add(VolumeAddArgs),
    /// List configured volumes.
    List(VolumeListArgs),
}
```

Add these types at the bottom:

```rust
#[derive(clap::Args, Clone, Debug)]
pub struct VolumeAddArgs {
    #[command(subcommand)]
    pub kind: VolumeTypeArgs,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum VolumeTypeArgs {
    /// Local filesystem bind mount on the node.
    Filesystem(FilesystemVolumeArgs),
    /// SMB/CIFS remote share mounted on the node.
    Cifs(CifsVolumeArgs),
}

#[derive(clap::Args, Clone, Debug)]
pub struct FilesystemVolumeArgs {
    /// Logical volume name referenced from compose files.
    #[arg(short, long)]
    pub name: String,
    /// Target node name.
    #[arg(long)]
    pub node: String,
    /// Absolute path on the node to bind-mount.
    #[arg(long)]
    pub path: String,
}

#[derive(clap::Args, Clone, Debug)]
pub struct CifsVolumeArgs {
    /// Logical volume name referenced from compose files.
    #[arg(short, long)]
    pub name: String,
    /// Target node name.
    #[arg(long)]
    pub node: String,
    /// SMB share in the form //server/share.
    #[arg(long)]
    pub server: String,
    /// SMB username.
    #[arg(long)]
    pub username: String,
    /// SMB password (stored in plaintext, matching the nodes.json convention).
    #[arg(long)]
    pub password: String,
}
```

Update `VolumeCommand::run` to dispatch `Add`:

```rust
    async fn run(args: VolumeArgs, ctx: CommandContext) -> anyhow::Result<()> {
        let path = volumes_file(&ctx.home);
        match args.command {
            VolumeSubcommand::Add(add_args) => add_volume(&ctx.home, &path, add_args).await,
            VolumeSubcommand::List(_) => list_volumes(&path, ctx.output).await,
        }
    }
```

Add at the bottom of the file:

```rust
async fn add_volume(
    home: &Path,
    path: &Path,
    args: VolumeAddArgs,
) -> anyhow::Result<()> {
    match args.kind {
        VolumeTypeArgs::Filesystem(fs) => {
            validate_name(&fs.name)?;
            validate_node(home, &fs.node).await?;
            validate_filesystem_path(&fs.path)?;
            crate::volume::list::add_filesystem(path, &fs.name, &fs.node, &fs.path).await?;
            println!("volume add filesystem");
            Ok(())
        }
        VolumeTypeArgs::Cifs(cifs) => {
            validate_name(&cifs.name)?;
            validate_node(home, &cifs.node).await?;
            validate_cifs_server(&cifs.server)?;
            crate::volume::list::add_cifs(
                path,
                &cifs.name,
                &cifs.node,
                &cifs.server,
                &cifs.username,
                &cifs.password,
            )
            .await?;
            println!("volume add cifs");
            Ok(())
        }
    }
}

fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("volume name cannot be empty");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        anyhow::bail!(
            "volume name '{}' contains invalid characters (allowed: a-z A-Z 0-9 _ -)",
            name
        );
    }
    Ok(())
}

async fn validate_node(home: &Path, node: &str) -> anyhow::Result<()> {
    use crate::node::list::load_all_nodes;
    use crate::node::types::NodeRecord;
    let nodes = load_all_nodes(&crate::cli::node::nodes_file(home)).await?;
    let exists = nodes.iter().any(|record| match record {
        NodeRecord::Local() => node == "local",
        NodeRecord::Remote(remote) => remote.name == node,
    });
    if !exists {
        anyhow::bail!("node '{}' not found", node);
    }
    Ok(())
}

fn validate_filesystem_path(path: &str) -> anyhow::Result<()> {
    if path.is_empty() {
        anyhow::bail!("filesystem path cannot be empty");
    }
    if !path.starts_with('/') {
        anyhow::bail!("filesystem path must be absolute (got '{}')", path);
    }
    Ok(())
}

fn validate_cifs_server(server: &str) -> anyhow::Result<()> {
    if !server.starts_with("//") {
        anyhow::bail!(
            "cifs server must start with '//' (got '{}')",
            server
        );
    }
    Ok(())
}
```

- [ ] **Step 5: Build + fmt + clippy**

```bash
cargo fmt
cargo build --features duckdb-bundled
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Manual smoke test**

```bash
cargo run --features duckdb-bundled -- volume add filesystem --name data --node local --path /mnt/data
cargo run --features duckdb-bundled -- volume list
cargo run --features duckdb-bundled -- volume add cifs --name backup --node local --server //10.0.0.5/share --username alice --password secret
cargo run --features duckdb-bundled -- volume list
# Try duplicate:
cargo run --features duckdb-bundled -- volume add filesystem --name data --node local --path /tmp/other
# Expect: "volume 'data' on node 'local' already exists"
# Try unknown node:
cargo run --features duckdb-bundled -- volume add filesystem --name other --node ghost --path /tmp/x
# Expect: "node 'ghost' not found"
```

Clean up: `rm .ins/volumes.json`.

- [ ] **Step 7: Commit**

```bash
git add src/volume/list.rs src/cli/volume.rs
git commit -m "feat(volume): add 'volume add filesystem|cifs' subcommands"
```

---

## Task 4: `ins volume set` and `ins volume delete`

**Files:**
- Modify: `src/volume/list.rs` (add `set_filesystem`, `set_cifs`, `delete_volume` + tests)
- Modify: `src/cli/volume.rs` (add `Set`, `Delete` subcommands)

- [ ] **Step 1: Write failing tests**

Append to the `tests` module in `src/volume/list.rs`:

```rust
    #[tokio::test]
    async fn set_filesystem_updates_existing_record() -> anyhow::Result<()> {
        let path = unique_test_path("set-fs");
        add_filesystem(&path, "data", "node1", "/mnt/old").await?;
        set_filesystem(&path, "data", "node1", "/mnt/new").await?;
        let loaded = load_volumes(&path).await?;
        assert_eq!(loaded.len(), 1);
        match &loaded[0] {
            VolumeRecord::Filesystem(v) => assert_eq!(v.path, "/mnt/new"),
            _ => panic!("expected filesystem"),
        }
        tokio::fs::remove_file(&path).await.ok();
        Ok(())
    }

    #[tokio::test]
    async fn set_changes_type_when_switching_filesystem_to_cifs() -> anyhow::Result<()> {
        let path = unique_test_path("set-switch");
        add_filesystem(&path, "data", "node1", "/mnt/a").await?;
        set_cifs(&path, "data", "node1", "//10.0.0.5/share", "alice", "secret").await?;
        let loaded = load_volumes(&path).await?;
        assert!(matches!(&loaded[0], VolumeRecord::Cifs(_)));
        tokio::fs::remove_file(&path).await.ok();
        Ok(())
    }

    #[tokio::test]
    async fn set_errors_when_volume_missing() -> anyhow::Result<()> {
        let path = unique_test_path("set-miss");
        let err = set_filesystem(&path, "data", "node1", "/mnt/new")
            .await
            .expect_err("missing record should fail");
        assert!(err.to_string().contains("not found"));
        tokio::fs::remove_file(&path).await.ok();
        Ok(())
    }

    #[tokio::test]
    async fn delete_removes_single_record_by_name_and_node() -> anyhow::Result<()> {
        let path = unique_test_path("delete");
        add_filesystem(&path, "data", "node1", "/mnt/a").await?;
        add_filesystem(&path, "data", "node2", "/mnt/b").await?;
        delete_volume(&path, "data", "node1").await?;
        let loaded = load_volumes(&path).await?;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].node(), "node2");
        tokio::fs::remove_file(&path).await.ok();
        Ok(())
    }

    #[tokio::test]
    async fn delete_errors_when_volume_missing() -> anyhow::Result<()> {
        let path = unique_test_path("delete-miss");
        let err = delete_volume(&path, "data", "node1")
            .await
            .expect_err("missing record should fail");
        assert!(err.to_string().contains("not found"));
        tokio::fs::remove_file(&path).await.ok();
        Ok(())
    }
```

- [ ] **Step 2: Implement set/delete in `src/volume/list.rs`**

Add above the `tests` module (below the existing `add_*` functions):

```rust
pub(crate) async fn set_filesystem(
    path: &Path,
    name: &str,
    node: &str,
    host_path: &str,
) -> anyhow::Result<()> {
    let mut volumes = load_volumes(path).await?;
    let index = find_index(&volumes, name, node)?;
    volumes[index] = VolumeRecord::Filesystem(FilesystemVolume {
        name: name.to_string(),
        node: node.to_string(),
        path: host_path.to_string(),
    });
    save_volumes(path, &volumes).await
}

pub(crate) async fn set_cifs(
    path: &Path,
    name: &str,
    node: &str,
    server: &str,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    let mut volumes = load_volumes(path).await?;
    let index = find_index(&volumes, name, node)?;
    volumes[index] = VolumeRecord::Cifs(CifsVolume {
        name: name.to_string(),
        node: node.to_string(),
        server: server.to_string(),
        username: username.to_string(),
        password: password.to_string(),
    });
    save_volumes(path, &volumes).await
}

pub(crate) async fn delete_volume(path: &Path, name: &str, node: &str) -> anyhow::Result<()> {
    let mut volumes = load_volumes(path).await?;
    let index = find_index(&volumes, name, node)?;
    volumes.remove(index);
    save_volumes(path, &volumes).await
}

fn find_index(volumes: &[VolumeRecord], name: &str, node: &str) -> anyhow::Result<usize> {
    volumes
        .iter()
        .position(|v| v.name() == name && v.node() == node)
        .ok_or_else(|| anyhow::anyhow!("volume '{}' on node '{}' not found", name, node))
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --features duckdb-bundled volume::list`

Expected: all 9 tests pass.

- [ ] **Step 4: Add `Set` and `Delete` subcommands to `src/cli/volume.rs`**

Extend `VolumeSubcommand`:

```rust
#[derive(clap::Subcommand, Clone, Debug)]
pub enum VolumeSubcommand {
    /// Add a volume backing for a node.
    Add(VolumeAddArgs),
    /// Update an existing volume backing.
    Set(VolumeSetArgs),
    /// Delete a volume backing.
    Delete(VolumeDeleteArgs),
    /// List configured volumes.
    List(VolumeListArgs),
}
```

Add at the bottom:

```rust
#[derive(clap::Args, Clone, Debug)]
pub struct VolumeSetArgs {
    #[command(subcommand)]
    pub kind: VolumeTypeArgs,
}

#[derive(clap::Args, Clone, Debug)]
pub struct VolumeDeleteArgs {
    /// Logical volume name.
    #[arg(short, long)]
    pub name: String,
    /// Target node name.
    #[arg(long)]
    pub node: String,
}
```

Update `run`:

```rust
    async fn run(args: VolumeArgs, ctx: CommandContext) -> anyhow::Result<()> {
        let path = volumes_file(&ctx.home);
        match args.command {
            VolumeSubcommand::Add(add_args) => add_volume(&ctx.home, &path, add_args).await,
            VolumeSubcommand::Set(set_args) => set_volume(&ctx.home, &path, set_args).await,
            VolumeSubcommand::Delete(delete_args) => delete_volume_cmd(&path, delete_args).await,
            VolumeSubcommand::List(_) => list_volumes(&path, ctx.output).await,
        }
    }
```

Add implementations at the bottom:

```rust
async fn set_volume(
    home: &Path,
    path: &Path,
    args: VolumeSetArgs,
) -> anyhow::Result<()> {
    match args.kind {
        VolumeTypeArgs::Filesystem(fs) => {
            validate_name(&fs.name)?;
            validate_node(home, &fs.node).await?;
            validate_filesystem_path(&fs.path)?;
            crate::volume::list::set_filesystem(path, &fs.name, &fs.node, &fs.path).await?;
            println!("volume set filesystem");
            Ok(())
        }
        VolumeTypeArgs::Cifs(cifs) => {
            validate_name(&cifs.name)?;
            validate_node(home, &cifs.node).await?;
            validate_cifs_server(&cifs.server)?;
            crate::volume::list::set_cifs(
                path,
                &cifs.name,
                &cifs.node,
                &cifs.server,
                &cifs.username,
                &cifs.password,
            )
            .await?;
            println!("volume set cifs");
            Ok(())
        }
    }
}

async fn delete_volume_cmd(path: &Path, args: VolumeDeleteArgs) -> anyhow::Result<()> {
    validate_name(&args.name)?;
    crate::volume::list::delete_volume(path, &args.name, &args.node).await?;
    println!("volume delete");
    Ok(())
}
```

- [ ] **Step 5: Build + fmt + clippy**

```bash
cargo fmt
cargo build --features duckdb-bundled
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Manual smoke test**

```bash
cargo run --features duckdb-bundled -- volume add filesystem --name data --node local --path /mnt/a
cargo run --features duckdb-bundled -- volume set filesystem --name data --node local --path /mnt/b
cargo run --features duckdb-bundled -- volume list
cargo run --features duckdb-bundled -- volume set cifs --name data --node local --server //10.0.0.5/share --username alice --password secret
cargo run --features duckdb-bundled -- volume list
cargo run --features duckdb-bundled -- volume delete --name data --node local
cargo run --features duckdb-bundled -- volume list
# Expect: no volumes found
cargo run --features duckdb-bundled -- volume delete --name missing --node local
# Expect: "volume 'missing' on node 'local' not found"
```

- [ ] **Step 7: Commit**

```bash
git add src/volume/list.rs src/cli/volume.rs
git commit -m "feat(volume): add 'set' and 'delete' subcommands"
```

---

## Task 5: `inject_compose_volumes` YAML rewrite + ResolvedVolume

**Files:**
- Create: `src/volume/compose.rs`
- Modify: `src/volume/mod.rs` (add `pub(crate) mod compose;`)

- [ ] **Step 1: Write failing tests for `inject_compose_volumes`**

Create `src/volume/compose.rs`:

```rust
use std::collections::BTreeMap;

use anyhow::{anyhow, bail};

use crate::volume::types::{ResolvedVolume, VolumeRecord};

pub(crate) fn inject_compose_volumes(
    content: &str,
    node_name: &str,
    volumes: &[VolumeRecord],
) -> anyhow::Result<(String, Vec<ResolvedVolume>)> {
    let mut document: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|e| anyhow!("parse compose yaml: {}", e))?;

    let Some(root) = document.as_mapping_mut() else {
        return Ok((content.to_string(), Vec::new()));
    };

    let volumes_key = serde_yaml::Value::String("volumes".into());
    let Some(top_volumes) = root.get_mut(&volumes_key).and_then(|v| v.as_mapping_mut()) else {
        return Ok((content.to_string(), Vec::new()));
    };

    let mut resolved: Vec<ResolvedVolume> = Vec::new();

    let names: Vec<String> = top_volumes
        .keys()
        .filter_map(|k| k.as_str().map(str::to_string))
        .collect();

    for name in &names {
        let record = volumes
            .iter()
            .find(|v| v.name() == name && v.node() == node_name);
        let Some(record) = record else {
            bail!(
                "volume '{}' is not configured on node '{}'",
                name,
                node_name
            );
        };

        let docker_name = format!("ins_{}", name);
        let (driver, driver_opts) = driver_opts_for(record);

        let mut replacement = serde_yaml::Mapping::new();
        replacement.insert(
            serde_yaml::Value::String("external".into()),
            serde_yaml::Value::Bool(true),
        );
        replacement.insert(
            serde_yaml::Value::String("name".into()),
            serde_yaml::Value::String(docker_name.clone()),
        );
        top_volumes.insert(
            serde_yaml::Value::String(name.clone()),
            serde_yaml::Value::Mapping(replacement),
        );

        resolved.push(ResolvedVolume {
            docker_name,
            driver,
            driver_opts,
        });
    }

    let rewritten =
        serde_yaml::to_string(&document).map_err(|e| anyhow!("serialize compose yaml: {}", e))?;
    Ok((rewritten, resolved))
}

fn driver_opts_for(record: &VolumeRecord) -> (String, BTreeMap<String, String>) {
    let mut opts = BTreeMap::new();
    match record {
        VolumeRecord::Filesystem(v) => {
            opts.insert("type".into(), "none".into());
            opts.insert("o".into(), "bind".into());
            opts.insert("device".into(), v.path.clone());
        }
        VolumeRecord::Cifs(v) => {
            opts.insert("type".into(), "cifs".into());
            opts.insert(
                "o".into(),
                format!("username={},password={}", v.username, v.password),
            );
            opts.insert("device".into(), v.server.clone());
        }
    }
    ("local".into(), opts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::types::{CifsVolume, FilesystemVolume, VolumeRecord};

    fn fs(name: &str, node: &str, path: &str) -> VolumeRecord {
        VolumeRecord::Filesystem(FilesystemVolume {
            name: name.into(),
            node: node.into(),
            path: path.into(),
        })
    }

    fn cifs(name: &str, node: &str, server: &str, username: &str, password: &str) -> VolumeRecord {
        VolumeRecord::Cifs(CifsVolume {
            name: name.into(),
            node: node.into(),
            server: server.into(),
            username: username.into(),
            password: password.into(),
        })
    }

    #[test]
    fn returns_unchanged_when_no_top_level_volumes() {
        let compose = "services:\n  web:\n    image: nginx\n";
        let (rewritten, resolved) = inject_compose_volumes(compose, "node1", &[]).expect("ok");
        assert_eq!(resolved.len(), 0);
        assert!(rewritten.contains("nginx"));
    }

    #[test]
    fn rewrites_filesystem_volume_to_external() {
        let compose = r#"
services:
  web:
    image: nginx
    volumes:
      - data:/var/lib/app
volumes:
  data: {}
"#;
        let volumes = vec![fs("data", "node1", "/mnt/data")];
        let (rewritten, resolved) =
            inject_compose_volumes(compose, "node1", &volumes).expect("ok");

        let doc: serde_yaml::Value = serde_yaml::from_str(&rewritten).expect("yaml");
        let data = doc
            .get("volumes")
            .and_then(|v| v.get("data"))
            .and_then(|v| v.as_mapping())
            .expect("data mapping");
        assert_eq!(
            data.get(&serde_yaml::Value::String("external".into())),
            Some(&serde_yaml::Value::Bool(true))
        );
        assert_eq!(
            data.get(&serde_yaml::Value::String("name".into()))
                .and_then(|v| v.as_str()),
            Some("ins_data")
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].docker_name, "ins_data");
        assert_eq!(resolved[0].driver, "local");
        assert_eq!(resolved[0].driver_opts.get("type").unwrap(), "none");
        assert_eq!(resolved[0].driver_opts.get("o").unwrap(), "bind");
        assert_eq!(resolved[0].driver_opts.get("device").unwrap(), "/mnt/data");
    }

    #[test]
    fn rewrites_cifs_volume_with_credentials_in_options() {
        let compose = r#"
services:
  web: { image: nginx }
volumes:
  data: {}
"#;
        let volumes = vec![cifs("data", "node2", "//10.0.0.5/share", "alice", "s3cr3t")];
        let (_rewritten, resolved) =
            inject_compose_volumes(compose, "node2", &volumes).expect("ok");

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].driver_opts.get("type").unwrap(), "cifs");
        assert_eq!(
            resolved[0].driver_opts.get("o").unwrap(),
            "username=alice,password=s3cr3t"
        );
        assert_eq!(
            resolved[0].driver_opts.get("device").unwrap(),
            "//10.0.0.5/share"
        );
    }

    #[test]
    fn errors_when_volume_not_configured_for_node() {
        let compose = r#"
services:
  web: { image: nginx }
volumes:
  data: {}
"#;
        let volumes = vec![fs("data", "other-node", "/mnt/data")];
        let err = inject_compose_volumes(compose, "node1", &volumes)
            .expect_err("missing config should fail");
        let msg = err.to_string();
        assert!(msg.contains("volume 'data'"), "unexpected message: {}", msg);
        assert!(msg.contains("node 'node1'"), "unexpected message: {}", msg);
    }

    #[test]
    fn picks_node_specific_record_when_duplicates_exist() {
        let compose = r#"
services:
  web: { image: nginx }
volumes:
  data: {}
"#;
        let volumes = vec![
            fs("data", "node1", "/mnt/one"),
            fs("data", "node2", "/mnt/two"),
        ];
        let (_rewritten, resolved) =
            inject_compose_volumes(compose, "node2", &volumes).expect("ok");
        assert_eq!(resolved[0].driver_opts.get("device").unwrap(), "/mnt/two");
    }
}
```

- [ ] **Step 2: Register the module**

Update `src/volume/mod.rs`:

```rust
pub(crate) mod compose;
pub(crate) mod list;
pub mod types;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --features duckdb-bundled volume::compose`

Expected: 5 tests pass.

- [ ] **Step 4: fmt + clippy**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/volume/compose.rs src/volume/mod.rs
git commit -m "feat(volume): add inject_compose_volumes YAML rewrite"
```

---

## Task 6: Thread ResolvedVolume through pipeline and ProviderContext

**Files:**
- Modify: `src/provider/mod.rs`
- Modify: `src/pipeline.rs`

- [ ] **Step 1: Extend `ProviderContext`**

In `src/provider/mod.rs`, add the import and replace the struct + `impl`:

```rust
use crate::volume::types::ResolvedVolume;

#[derive(Clone, Debug)]
pub struct ProviderContext {
    pub provider: String,
    pub node: NodeRecord,
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
        targets: Vec<DeploymentTarget>,
        workspace: PathBuf,
        envs: BTreeMap<String, BTreeMap<String, String>>,
        output: ExecutionOutput,
        volumes: Vec<ResolvedVolume>,
    ) -> Self {
        Self {
            provider,
            node,
            targets,
            workspace,
            envs,
            output,
            volumes,
        }
    }

    pub fn env_for_target(&self, service: &str) -> BTreeMap<String, String> {
        self.envs.get(service).cloned().unwrap_or_default()
    }
}
```

- [ ] **Step 2: Add imports in `pipeline.rs`**

At the top of `src/pipeline.rs`, add next to the other `use crate::...` lines:

```rust
use crate::volume::compose::inject_compose_volumes;
use crate::volume::list::{load_volumes, volumes_file};
use crate::volume::types::{ResolvedVolume, VolumeRecord};
```

- [ ] **Step 3: Change `copy_apps_to_workspace_with_output` to accept volumes and return resolved list**

Replace its signature and body:

```rust
pub async fn copy_apps_to_workspace_with_output(
    home: &Path,
    targets: &[DeploymentTarget],
    app_home: &Path,
    workspace: &Path,
    node: &NodeRecord,
    volumes_config: &[VolumeRecord],
    output: &ExecutionOutput,
) -> anyhow::Result<Vec<ResolvedVolume>> {
    output.line("Saving deployment records...");
    for target in targets {
        let source_dir = app_home.join(&target.app.name);
        let qa_file = app_qa_file(&source_dir);
        let qa_yaml = fs::read_to_string(&qa_file)
            .await
            .map_err(|e| anyhow!("read qa file {}: {}", qa_file.display(), e))?;
        output.line(format!(
            "Save deployment record for app '{}' into service '{}'",
            target.app.name, target.service
        ));
        save_deployment_record(home, node, workspace, target, &qa_yaml).await?;
    }

    target_file_for_node(node).create_dir_all(workspace).await?;

    let mut resolved_volumes: Vec<ResolvedVolume> = Vec::new();

    output.line("Copying app files to workspace...");
    for target in targets {
        let source_dir = app_home.join(&target.app.name);
        let target_dir = workspace.join(&target.service);

        if let Some(progress) = CopyAppProgress::new(
            &target.app.name,
            &target.service,
            &source_dir,
            &target_dir,
            output.echo_enabled(),
        )
        .await?
        {
            let mut batch = copy_dir_recursive(
                &source_dir,
                &target_dir,
                &target.app,
                node,
                volumes_config,
                Some(progress.clone()),
                output,
            )
            .await?;
            resolved_volumes.append(&mut batch);
            progress.finish();
        } else {
            output.line(format!(
                "  Copying app '{}' into service '{}' at {}",
                target.app.name,
                target.service,
                target_dir.display()
            ));
            let mut batch = copy_dir_recursive(
                &source_dir,
                &target_dir,
                &target.app,
                node,
                volumes_config,
                None,
                output,
            )
            .await?;
            resolved_volumes.append(&mut batch);
        }
    }

    Ok(dedupe_volumes(resolved_volumes))
}

fn dedupe_volumes(volumes: Vec<ResolvedVolume>) -> Vec<ResolvedVolume> {
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for v in volumes {
        if seen.insert(v.docker_name.clone()) {
            result.push(v);
        }
    }
    result
}
```

- [ ] **Step 4: Update the test-only `copy_apps_to_workspace` to pass an empty slice**

```rust
#[cfg(test)]
pub async fn copy_apps_to_workspace(
    home: &Path,
    targets: &[DeploymentTarget],
    app_home: &Path,
    workspace: &Path,
    node: &NodeRecord,
) -> anyhow::Result<()> {
    let output = ExecutionOutput::stdout();
    copy_apps_to_workspace_with_output(home, targets, app_home, workspace, node, &[], &output)
        .await?;
    Ok(())
}
```

- [ ] **Step 5: Update `copy_dir_recursive` to thread volumes_config and return Vec<ResolvedVolume>**

```rust
async fn copy_dir_recursive(
    source: &Path,
    target: &Path,
    app_record: &AppRecord,
    node: &NodeRecord,
    volumes_config: &[VolumeRecord],
    progress: Option<Arc<CopyAppProgress>>,
    output: &ExecutionOutput,
) -> anyhow::Result<Vec<ResolvedVolume>> {
    let template_values = build_template_values(app_record)?;
    let jobs = collect_copy_jobs(source, target).await?;
    target_file_for_node(node).create_dir_all(target).await?;

    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let mut join_set = JoinSet::new();
    let mut next_job = 0usize;
    let mut available_slots: Vec<usize> = (0..COPY_CONCURRENCY.min(jobs.len())).rev().collect();
    let mut resolved = Vec::new();

    loop {
        while next_job < jobs.len() && !available_slots.is_empty() {
            let slot = available_slots.pop().expect("slot available");
            let job = jobs[next_job].clone();
            let template_values = template_values.clone();
            let node = node.clone();
            let volumes_config = volumes_config.to_vec();
            let output = output.clone();
            let slot_progress = progress.as_ref().map(|progress| progress.slot(slot));
            join_set.spawn(async move {
                let result = copy_file_to_workspace(
                    job,
                    &template_values,
                    &node,
                    &volumes_config,
                    slot_progress,
                    &output,
                )
                .await;
                (slot, result)
            });
            next_job += 1;
        }

        let Some(joined) = join_set.join_next().await else {
            break;
        };
        let (slot, result) = joined.map_err(|e| anyhow!("copy task join error: {}", e))?;
        let mut batch = result?;
        resolved.append(&mut batch);
        available_slots.push(slot);
    }

    Ok(resolved)
}
```

- [ ] **Step 6: Update `copy_file_to_workspace` to accept volumes_config, inject, and return ResolvedVolumes**

Replace the function with:

```rust
async fn copy_file_to_workspace(
    job: CopyJob,
    template_values: &serde_json::Value,
    node: &NodeRecord,
    volumes_config: &[VolumeRecord],
    progress: Option<CopyProgressSlot>,
    output: &ExecutionOutput,
) -> anyhow::Result<Vec<ResolvedVolume>> {
    let source_file = LocalFile;
    let target_file = target_file_for_node(node);

    if job.render_as_template {
        if let Some(progress) = progress.as_ref() {
            progress.start_template(&job.target_path);
        } else {
            output.line(format!(
                "    Rendering template {} -> {}",
                job.source_path.display(),
                job.target_path.display()
            ));
        }
        let source = source_file
            .read(&job.source_path, None)
            .await
            .map_err(|e| anyhow!("read template {}: {}", job.source_path.display(), e))?;
        let rendered = render_template(&source, template_values)?;
        let rendered =
            maybe_inject_compose_labels(&job.target_path, &rendered, template_values, node)?;
        let (rendered, resolved) =
            maybe_inject_compose_volumes(&job.target_path, rendered, node, volumes_config)?;
        let rendered_size = rendered.len() as u64;
        if let Some(progress) = progress.as_ref() {
            progress.begin_write_phase(rendered_size);
        }
        let progress_write = progress.as_ref().map(|progress| progress.write_progress());
        target_file
            .write(&job.target_path, &rendered, progress_write.as_ref())
            .await
            .map_err(|e| anyhow!("write rendered file {}: {}", job.target_path.display(), e))?;
        if let Some(progress) = progress.as_ref() {
            progress.finish_file();
        }
        return Ok(resolved);
    }

    let source_meta = fs::metadata(&job.source_path)
        .await
        .map_err(|e| anyhow!("metadata source file {}: {}", job.source_path.display(), e))?;
    let source_size = source_meta.len();
    if let Some(progress) = progress.as_ref() {
        progress.start_copy(&job.target_path, source_size);
    } else {
        output.line(format!(
            "    Copying file {} -> {}",
            job.source_path.display(),
            job.target_path.display()
        ));
    }
    let source_bytes = source_file
        .read_bytes(&job.source_path, None)
        .await
        .map_err(|e| anyhow!("read source file {}: {}", job.source_path.display(), e))?;
    if is_docker_compose_file(&job.target_path) {
        let source = String::from_utf8(source_bytes).map_err(|e| {
            anyhow!(
                "read compose file {} as utf-8: {}",
                job.source_path.display(),
                e
            )
        })?;
        let rendered =
            maybe_inject_compose_labels(&job.target_path, &source, template_values, node)?;
        let (rendered, resolved) =
            maybe_inject_compose_volumes(&job.target_path, rendered, node, volumes_config)?;
        let rendered_size = rendered.len() as u64;
        if let Some(progress) = progress.as_ref() {
            progress.begin_write_phase(rendered_size);
        }
        let progress_write = progress.as_ref().map(|progress| progress.write_progress());
        target_file
            .write(&job.target_path, &rendered, progress_write.as_ref())
            .await
            .map_err(|e| anyhow!("write compose file {}: {}", job.target_path.display(), e))?;
        if let Some(progress) = progress.as_ref() {
            progress.finish_file();
        }
        return Ok(resolved);
    }
    if let Some(progress) = progress.as_ref() {
        progress.begin_write_phase(source_size);
    }
    let progress_write = progress.as_ref().map(|progress| progress.write_progress());
    target_file
        .write_bytes(&job.target_path, &source_bytes, progress_write.as_ref())
        .await
        .map_err(|e| {
            anyhow!(
                "copy {} to {}: {}",
                job.source_path.display(),
                job.target_path.display(),
                e
            )
        })?;
    if let Some(progress) = progress.as_ref() {
        progress.finish_file();
    }
    Ok(Vec::new())
}
```

Add this helper next to `maybe_inject_compose_labels`:

```rust
fn maybe_inject_compose_volumes(
    path: &Path,
    content: String,
    node: &NodeRecord,
    volumes_config: &[VolumeRecord],
) -> anyhow::Result<(String, Vec<ResolvedVolume>)> {
    if !is_docker_compose_file(path) {
        return Ok((content, Vec::new()));
    }
    let node_name_str = node_name(node).to_string();
    inject_compose_volumes(&content, &node_name_str, volumes_config)
}
```

- [ ] **Step 7: Update `copy_prepared_apps_to_workspace_with_output` and `copy_prepared_apps_to_workspace`**

```rust
pub async fn copy_prepared_apps_to_workspace_with_output(
    home: &Path,
    prepared: &PreparedDeployment,
    output: &ExecutionOutput,
) -> anyhow::Result<Vec<ResolvedVolume>> {
    let volumes_config = load_volumes(&volumes_file(home)).await?;
    copy_apps_to_workspace_with_output(
        home,
        &prepared.targets,
        &prepared.app_home,
        &prepared.workspace,
        &prepared.node,
        &volumes_config,
        output,
    )
    .await
}

#[allow(dead_code)]
pub async fn copy_prepared_apps_to_workspace(
    home: &Path,
    prepared: &PreparedDeployment,
) -> anyhow::Result<Vec<ResolvedVolume>> {
    let output = ExecutionOutput::stdout();
    copy_prepared_apps_to_workspace_with_output(home, prepared, &output).await
}
```

- [ ] **Step 8: Pass resolved volumes into `ProviderContext::new`**

Replace the body of `execute_pipeline_with_output`:

```rust
pub async fn execute_pipeline_with_output(
    home: &Path,
    prepared: PreparedDeployment,
    title: &str,
    mode: PipelineMode,
    output: ExecutionOutput,
) -> anyhow::Result<()> {
    let provider = ensure_supported_provider(&prepared.provider)?;

    print_prepared_deployment_to_output(title, &prepared, &output);
    let resolved_volumes =
        copy_prepared_apps_to_workspace_with_output(home, &prepared, &output).await?;

    let provider_ctx = ProviderContext::new(
        prepared.provider.clone(),
        prepared.node.clone(),
        prepared.targets.clone(),
        prepared.workspace,
        build_provider_envs(
            &prepared.targets,
            &prepared.node,
            &load_installed_service_configs(home).await?,
        )?,
        output.clone(),
        resolved_volumes,
    );

    match mode {
        PipelineMode::Check => {
            print_provider_envs(&provider_ctx.envs, &output);
            output.line("Validating with provider...");
            provider.validate(provider_ctx).await?;
            output.line("Check completed.");
            Ok(())
        }
        PipelineMode::Deploy => {
            output.line("Running provider...");
            provider.run(provider_ctx).await
        }
    }
}
```

- [ ] **Step 9: Build + run existing tests**

Run:
```bash
cargo build --features duckdb-bundled
cargo test --features duckdb-bundled
```

Expected: full build + all tests pass.

- [ ] **Step 10: Add an integration test confirming the rewritten compose content lands on disk**

Append to the `tests` module at the bottom of `src/pipeline.rs`:

```rust
    #[tokio::test]
    async fn copy_apps_to_workspace_rewrites_compose_volumes_and_returns_resolved() -> anyhow::Result<()> {
        use crate::pipeline::copy_apps_to_workspace_with_output;
        use crate::provider::DeploymentTarget;

        let home = unique_test_dir("pipeline-volume-inject");
        let app_dir = home.join("app").join("vol-demo");
        fs::create_dir_all(&app_dir).await?;
        fs::write(
            app_dir.join("qa.yaml"),
            "name: vol-demo\nvalues: []\n",
        )
        .await?;
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
            &[target.clone()],
            &home.join("app"),
            &workspace,
            &node,
            &volumes_config,
            &crate::execution_output::ExecutionOutput::stdout(),
        )
        .await?;

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].docker_name, "ins_data");

        let rendered = fs::read_to_string(workspace.join("vol-demo").join("docker-compose.yml")).await?;
        assert!(rendered.contains("external: true"));
        assert!(rendered.contains("ins_data"));

        fs::remove_dir_all(&home).await?;
        Ok(())
    }
```

Run: `cargo test --features duckdb-bundled copy_apps_to_workspace_rewrites`

Expected: passes.

- [ ] **Step 11: fmt + clippy**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: clean.

- [ ] **Step 12: Commit**

```bash
git add src/pipeline.rs src/provider/mod.rs
git commit -m "feat(volume): inject external volumes during deploy copy phase"
```

---

## Task 7: `ensure_volumes` on local node

**Files:**
- Modify: `src/provider/docker_compose.rs`

- [ ] **Step 1: Write failing tests for command construction**

In `src/provider/docker_compose.rs`, append to the existing `#[cfg(test)] mod tests` block:

```rust
    use crate::volume::types::ResolvedVolume;
    use std::collections::BTreeMap;

    fn resolved(name: &str, opts: &[(&str, &str)]) -> ResolvedVolume {
        let mut map = BTreeMap::new();
        for (k, v) in opts {
            map.insert((*k).into(), (*v).into());
        }
        ResolvedVolume {
            docker_name: name.into(),
            driver: "local".into(),
            driver_opts: map,
        }
    }

    #[test]
    fn docker_volume_create_command_includes_all_opts_for_filesystem() {
        let volume = resolved(
            "ins_data",
            &[("type", "none"), ("o", "bind"), ("device", "/mnt/data")],
        );
        let cmd = super::docker_volume_create_shell_command(&volume);
        assert!(cmd.contains("docker volume create"));
        assert!(cmd.contains("--driver 'local'"));
        assert!(cmd.contains("--opt 'type=none'"));
        assert!(cmd.contains("--opt 'o=bind'"));
        assert!(cmd.contains("--opt 'device=/mnt/data'"));
        assert!(cmd.contains("'ins_data'"));
    }

    #[test]
    fn docker_volume_create_command_quotes_cifs_credentials() {
        let volume = resolved(
            "ins_secret",
            &[
                ("type", "cifs"),
                ("o", "username=alice,password=pa ss!word"),
                ("device", "//10.0.0.5/share"),
            ],
        );
        let cmd = super::docker_volume_create_shell_command(&volume);
        assert!(cmd.contains("--opt 'o=username=alice,password=pa ss!word'"));
        assert!(cmd.contains("--opt 'device=//10.0.0.5/share'"));
    }
```

- [ ] **Step 2: Implement `docker_volume_create_shell_command` and `ensure_volumes_local`**

Add the `ResolvedVolume` import near the other `use crate::...` lines at the top of the file:

```rust
use crate::volume::types::ResolvedVolume;
```

Add the two functions above the existing `#[cfg(test)]` block:

```rust
fn docker_volume_create_shell_command(volume: &ResolvedVolume) -> String {
    let mut parts = vec![
        "docker".to_string(),
        "volume".to_string(),
        "create".to_string(),
        "--driver".to_string(),
        crate::env::shell_quote(&volume.driver),
    ];
    for (k, v) in &volume.driver_opts {
        parts.push("--opt".to_string());
        parts.push(crate::env::shell_quote(&format!("{k}={v}")));
    }
    parts.push(crate::env::shell_quote(&volume.docker_name));
    parts.join(" ")
}

async fn ensure_volumes_local(
    volumes: &[ResolvedVolume],
    output: &ExecutionOutput,
) -> anyhow::Result<()> {
    for volume in volumes {
        let inspect = Command::new("docker")
            .args(["volume", "inspect", &volume.docker_name])
            .output()
            .await
            .context("run 'docker volume inspect'")?;
        if inspect.status.success() {
            output.line(format!(
                "Reusing existing docker volume '{}'",
                volume.docker_name
            ));
            continue;
        }

        output.line(format!(
            "Creating docker volume '{}' ({})",
            volume.docker_name, volume.driver
        ));

        let mut create = Command::new("docker");
        create
            .arg("volume")
            .arg("create")
            .arg("--driver")
            .arg(&volume.driver);
        for (k, v) in &volume.driver_opts {
            create.arg("--opt").arg(format!("{k}={v}"));
        }
        create.arg(&volume.docker_name);
        let result = create
            .output()
            .await
            .with_context(|| format!("run docker volume create for '{}'", volume.docker_name))?;
        append_command_output(output, &result.stdout, &result.stderr);
        if !result.status.success() {
            return Err(anyhow!(
                "docker volume create failed for '{}' (exit code {:?})",
                volume.docker_name,
                result.status.code()
            ));
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Call `ensure_volumes_local` from `run` before the per-target loop**

In `DockerComposeProvider::run`, find the `NodeRecord::Local()` branch. After `let compose_command = resolve_local_compose_command(&ctx.output).await?;` and before the `for target in &ctx.targets {` loop, add:

```rust
                ensure_volumes_local(&ctx.volumes, &ctx.output).await?;
```

- [ ] **Step 4: Run the new tests**

Run: `cargo test --features duckdb-bundled docker_volume_create_command`

Expected: both tests pass.

- [ ] **Step 5: fmt + clippy + full build**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo build --features duckdb-bundled
```

Expected: clean.

- [ ] **Step 6: Manual end-to-end verification (requires docker locally)**

```bash
mkdir -p /tmp/ins-vol-test/app/demo
cat > /tmp/ins-vol-test/app/demo/qa.yaml <<'EOF'
name: demo
values: []
EOF
cat > /tmp/ins-vol-test/app/demo/docker-compose.yml <<'EOF'
services:
  web:
    image: nginx:alpine
    volumes:
      - data:/usr/share/nginx/html
    ports:
      - "18080:80"
volumes:
  data: {}
EOF

cargo run --features duckdb-bundled -- --home /tmp/ins-vol-test volume add filesystem --name data --node local --path /tmp/ins-vol-test-data
mkdir -p /tmp/ins-vol-test-data
echo "<h1>hello</h1>" > /tmp/ins-vol-test-data/index.html

cargo run --features duckdb-bundled -- --home /tmp/ins-vol-test deploy --workspace /tmp/ins-vol-test-ws --node local demo
```

Verify:

```bash
docker volume inspect ins_data
curl -s http://localhost:18080
```

Expected: volume has `driver_opts.device=/tmp/ins-vol-test-data`; curl returns `<h1>hello</h1>`.

Teardown:

```bash
docker compose -f /tmp/ins-vol-test-ws/demo/docker-compose.yml down
docker volume rm ins_data
rm -rf /tmp/ins-vol-test /tmp/ins-vol-test-ws /tmp/ins-vol-test-data
```

- [ ] **Step 7: Commit**

```bash
git add src/provider/docker_compose.rs
git commit -m "feat(volume): ensure_volumes on local node before compose up"
```

---

## Task 8: `ensure_volumes` on remote node

**Files:**
- Modify: `src/provider/docker_compose.rs`

- [ ] **Step 1: Write failing test for remote command construction**

Append to the existing `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn docker_volume_ensure_remote_shell_command_has_inspect_guard() {
        let volume = resolved(
            "ins_data",
            &[("type", "none"), ("o", "bind"), ("device", "/mnt/data")],
        );
        let cmd = super::docker_volume_ensure_shell_command(&volume);
        assert!(cmd.contains("docker volume inspect 'ins_data'"));
        assert!(cmd.contains("docker volume create"));
        assert!(cmd.contains("--opt 'device=/mnt/data'"));
    }
```

- [ ] **Step 2: Implement `docker_volume_ensure_shell_command` + `ensure_volumes_remote`**

Below `docker_volume_create_shell_command`, add:

```rust
fn docker_volume_ensure_shell_command(volume: &ResolvedVolume) -> String {
    let inspect_name = crate::env::shell_quote(&volume.docker_name);
    let create_cmd = docker_volume_create_shell_command(volume);
    format!(
        "if docker volume inspect {inspect_name} >/dev/null 2>&1; then \
echo \"volume {} already exists\"; \
else {create_cmd}; fi",
        volume.docker_name
    )
}

async fn ensure_volumes_remote(
    remote_file: &RemoteFile,
    node_name: &str,
    volumes: &[ResolvedVolume],
    output: &ExecutionOutput,
) -> anyhow::Result<()> {
    for volume in volumes {
        output.line(format!(
            "Ensuring docker volume '{}' on remote node '{}'",
            volume.docker_name, node_name
        ));
        let command = docker_volume_ensure_shell_command(volume);
        let result = remote_file.exec(&command).await.with_context(|| {
            format!(
                "ensure docker volume '{}' on node '{}'",
                volume.docker_name, node_name
            )
        })?;
        let rendered = render_remote_output(&result.stdout, &result.stderr);
        if rendered != "no remote output" {
            output.line(rendered.clone());
        }
        if result.exit_status != 0 {
            return Err(anyhow!(
                "ensure docker volume '{}' failed on node '{}' (exit {})\n{}",
                volume.docker_name,
                node_name,
                result.exit_status,
                rendered
            ));
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Call `ensure_volumes_remote` from `run`**

In the `NodeRecord::Remote(remote)` branch of `DockerComposeProvider::run`, after `let compose_command = resolve_remote_compose_command(&remote_file).await?;` and before the `for target in &ctx.targets {` loop, add:

```rust
                ensure_volumes_remote(&remote_file, &remote.name, &ctx.volumes, &ctx.output).await?;
```

- [ ] **Step 4: Run tests**

Run: `cargo test --features duckdb-bundled docker_volume`

Expected: all three command-construction tests pass (2 local + 1 remote).

- [ ] **Step 5: fmt + clippy + build**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo build --features duckdb-bundled
```

Expected: clean.

- [ ] **Step 6: Manual verification against a remote node (optional)**

Requires an SSH-reachable node with docker installed:

```bash
ins node add --name remote1 --ip <ip> --port 22 --user <user> --password <pw>
ins volume add filesystem --name data --node remote1 --path /srv/data
ssh <user>@<ip> mkdir -p /srv/data
ins deploy --workspace /srv/ins-ws --node remote1 demo
ssh <user>@<ip> docker volume inspect ins_data
```

Expected: volume exists on the remote with the specified driver_opts.

- [ ] **Step 7: Commit**

```bash
git add src/provider/docker_compose.rs
git commit -m "feat(volume): ensure_volumes on remote node before compose up"
```

---

## Task 9: User-facing documentation

**Files:**
- Modify: `README.md`
- Create: `docs/volume-command.md`

- [ ] **Step 1: Add a `Volumes` section to the README CLI overview**

In `README.md`, right after the `### Deploy` section (below the `ins deploy ...` fenced block), add:

````markdown
### Volumes

Configure per-node Docker volume backings. The same logical volume name can map to a local bind mount on one node and a CIFS share on another.

```bash
ins volume add filesystem --name data --node node1 --path /mnt/data
ins volume add cifs --name data --node node2 \
  --server //10.0.0.5/share --username alice --password secret
ins volume list
ins volume set filesystem --name data --node node1 --path /mnt/new
ins volume delete --name data --node node1
```

Apps declare volumes with standard Docker Compose syntax (top-level `volumes: { data: {} }`, referenced by services). On deploy, `ins` rewrites each top-level volume to `external: true, name: ins_<name>` and runs `docker volume create` on the target node before `docker compose up -d`.

See [docs/volume-command.md](docs/volume-command.md) for the full flow.
````

- [ ] **Step 2: Create `docs/volume-command.md`**

Write this file:

````markdown
# Volume Command

`ins volume` stores per-node Docker volume backings. A single logical volume name can resolve to different storage drivers depending on the target node.

## Model

Configuration lives in `.ins/volumes.json`. Each record has a `(name, node)` primary key.

- `filesystem` — the node mounts a host directory via `docker volume create --driver local --opt type=none --opt o=bind --opt device=<path>`.
- `cifs` — the node mounts an SMB share via `docker volume create --driver local --opt type=cifs --opt o=username=<u>,password=<p> --opt device=<//server/share>`.

On the node, the actual Docker volume is named `ins_<name>` so it does not collide with volumes created by other tooling.

## Configuring

```bash
ins volume add filesystem --name data --node node1 --path /mnt/data

ins volume add cifs --name data --node node2 \
  --server //10.0.0.5/share --username alice --password secret

ins volume set filesystem --name data --node node1 --path /mnt/new
ins volume set cifs --name data --node node1 \
  --server //10.0.0.5/share --username alice --password secret

ins volume delete --name data --node node1

ins volume list
ins --output json volume list
```

Passwords are stored in plaintext in `volumes.json`, consistent with how SSH passwords are stored in `nodes.json`.

## Using volumes in an app

App templates use standard Docker Compose volume syntax:

```yaml
services:
  web:
    image: nginx
    volumes:
      - data:/var/lib/app
volumes:
  data: {}
```

On `ins deploy`, `ins` rewrites the top-level `volumes:` block for the target node:

```yaml
volumes:
  data:
    external: true
    name: ins_data
```

Before `docker compose up -d`, `ins` runs `docker volume inspect ins_data`; if absent, it runs `docker volume create --driver local --opt type=... --opt o=... --opt device=... ins_data`.

## Error behavior

- If an app references a top-level volume that is not configured on the current node, both `ins check` and `ins deploy` abort with `volume '<name>' is not configured on node '<node>'`.
- If `docker volume create` fails on the node (for example, missing kernel CIFS module, wrong credentials, unreachable server), the error from `docker` is surfaced and the deploy aborts before any service starts.
- If the Docker volume already exists on the node, `ins` reuses it without comparing `driver_opts`. To pick up a configuration change on an already-created volume, remove it manually on the node: `docker volume rm ins_<name>`.

## Troubleshooting CIFS

- Kernel CIFS module missing — most minimal Linux images do not include CIFS. Install `cifs-utils` (Debian/Ubuntu: `apt-get install cifs-utils`; RHEL/CentOS: `yum install cifs-utils`).
- Special characters in CIFS passwords — `,` or `=` inside the password will break the `o=username=...,password=...` form. Avoid those characters, or escape per the kernel's `cifs.ko` option syntax.
- Version negotiation — some servers require `vers=3.0` or higher. This version of `ins volume` does not expose an option for that; if needed, create the docker volume manually on the node (use the `ins_<name>` naming convention) and `ins` will reuse it.
````

- [ ] **Step 3: Commit**

```bash
git add README.md docs/volume-command.md
git commit -m "docs(volume): document the volume command"
```

---

## Final Verification

- [ ] **Step 1: Run the full test suite**

```bash
cargo test --features duckdb-bundled
```

Expected: all tests pass.

- [ ] **Step 2: Run lints**

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: no formatting diffs, no warnings.

- [ ] **Step 3: Confirm behavior end-to-end (local)**

Follow Task 7 Step 6's manual verification flow to confirm a real deploy still works.

- [ ] **Step 4: Review the commit log**

```bash
git log --oneline
```

Expected: 9 clean, focused commits corresponding to the tasks above.

---

## Self-Review Notes

- Spec coverage:
  - Architecture — Task 6.
  - Types — Task 1.
  - JSON I/O — Task 1.
  - CLI list — Task 2.
  - CLI add — Task 3.
  - CLI set/delete — Task 4.
  - Validation — Task 3 (add path) and Task 4 (set path).
  - Injection — Task 5.
  - Pipeline threading — Task 6.
  - ensure_volumes local — Task 7.
  - ensure_volumes remote — Task 8.
  - Docs — Task 9.
- Type and signature consistency:
  - `ResolvedVolume { docker_name, driver, driver_opts }` is identical across Tasks 1, 5, 6, 7, 8.
  - `ProviderContext::new` gains one `volumes` parameter in Task 6; the only caller (`execute_pipeline_with_output`) is updated in the same task.
- Strict-mode error for missing volume configuration fires during both `check` and `deploy` because both go through `copy_apps_to_workspace_with_output`.
- `ensure_volumes` is only called from `run`, never from `validate` — matches the spec.
- CIFS password safety: local path uses `Command::arg` (no shell, no quoting needed); remote path goes through `shell_quote`, already used for other secrets in the codebase.
