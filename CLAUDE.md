# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

`AGENTS.md` is a symlink to this file, so other agent tooling picks up the same instructions.

See `README.md` for the user-facing CLI overview.

## Project Structure & Module Organization

`ins` is a Rust CLI for preparing, validating, and deploying app templates. Entry point: `src/main.rs`.

- `src/cli/` — command entrypoints (`app`, `node`, `deploy`, `check`, `service`, `template`, `tui`, `version`, `volume`)
- `src/pipeline.rs` — shared pipeline for node/app selection, value prompts, workspace copy, and provider execution
- `src/app/` — `qa.yaml` parsing and app schema types
- `src/node/` — node definitions and loaders
- `src/file/` — local and remote file abstractions used during workspace copy
- `src/provider/` — `ProviderTrait` and Docker Compose implementation
- `src/store/` — DuckDB-backed deploy history
- `src/volume/` — per-node volume backings + compose YAML rewrite
- `template/qa.yaml` — starter app template

Tests are colocated (e.g. `src/app/parse_test.rs`, `src/cli/deploy_test.rs`).

## Core commands

- `cargo build` — debug (links system libduckdb)
- `cargo build --release --features duckdb-bundled` — portable release
- `cargo run -- --help` — inspect top-level commands
- `cargo test --features duckdb-bundled` — always use the bundled feature; without it, tests fail in environments without system libduckdb
- Single test: `cargo test --features duckdb-bundled <substring>` (e.g. `cargo test --features duckdb-bundled copy_apps_to_workspace`)
- `cargo fmt` — format
- `cargo clippy --all-targets --all-features -- -D warnings` — CI enforces `-D warnings`

## Coding Style

Follow `rustfmt`. Standard Rust naming: `snake_case` files/modules/functions; `CamelCase` structs/enums/traits. Prefer small command wrappers in `src/cli/`, move shared behavior into `src/pipeline.rs` or domain modules. Use `anyhow::Result` for fallible flows; keep CLI errors specific (node names, file paths, template rendering, provider failures).

## Testing

**Test file layout.** Tests for each `.rs` source file live in a sibling `<filename>_test.rs` file, referenced via `#[path]` — never inline as a `mod tests { ... }` block. One test file per source file, not per module directory. For `src/foo.rs` tests go in `src/foo_test.rs`; the source file ends with:

```rust
#[cfg(test)]
#[path = "foo_test.rs"]
mod foo_test;
```

The test file starts with `use super::{...};` to pull in the items under test (and `use crate::...;` for other modules). This convention applies project-wide: keeps source files focused, avoids huge inline test blocks, and lets each test file grow without inflating its implementation file. See `src/cli/deploy.rs` ↔ `src/cli/deploy_test.rs` and `src/volume/compose.rs` ↔ `src/volume/compose_test.rs` for examples. For directory-style modules with `mod.rs`, the test file is named after the directory (e.g. `src/pipeline/mod.rs` ↔ `src/pipeline/pipeline_test.rs`) since `mod_test.rs` would be meaningless.

When production code needs to expose private items for tests (e.g. a CLI file wanting access to private pipeline helpers), add a `#[cfg(test)] use crate::pipeline::{...};` block in the production file — the test file then accesses those via `super::`.

Use `#[tokio::test]` for async tests. Cover parser changes, workspace copy/render behavior, stored deploy history, and CLI selection logic. Use descriptive names such as `copy_apps_to_workspace_renders_template_files`.

## Commits & PRs

**Pre-commit gate: always run `make clippy` (or `cargo clippy --all-targets --all-features -- -D warnings`) before every `git commit`.** CI runs the same command and rejects any warning; catching it locally is the cheapest place to fix. `cargo fmt --check` and `cargo test --features duckdb-bundled` are also part of the gate — `make check` runs all three together.

Short imperative subjects (e.g. `Extract shared deployment pipeline`). One logical change per commit. PRs should explain CLI behavior changes, list commands run locally, and include sample output for prompt / validation / deploy-flow changes — this repo cares about terminal output shape.

---

## Architecture

### Pipeline-centric, provider-pluggable

The CLI commands in `src/cli/` are thin shells. Real work goes through `src/pipeline.rs`:

```
prepare_deployment → copy_apps_to_workspace_with_output → ProviderTrait::validate | run
```

`check` and `deploy` share the same prepare + copy phases. They diverge only in whether `validate` or `run` is invoked on the provider. Any logic added to the copy phase (e.g. compose rewrites, strict validation) fires in **both** `check` and `deploy`.

### Dual local/remote execution paths

`NodeRecord` is either `Local()` or `Remote(RemoteNodeRecord)`. Every operation that touches a node has two implementations:

- Local: `tokio::process::Command` (no shell, no quoting)
- Remote: `RemoteFile::exec` with a shell-quoted command string assembled via `crate::env::shell_quote`

When changing anything that runs on a node (e.g. in `docker_compose.rs`), both paths must be updated in lockstep. The remote path uses `russh` (SSH) and `russh-sftp`; don't introduce shell metacharacters into remote commands without going through `shell_quote`.

### Copy phase mutates compose files

Files copied from `.ins/app/<app>/` to `workspace/<service>/` are not always byte-for-byte copies:

1. `.j2 / .jinja / .jinja2 / .tmpl` → rendered through minijinja (`build_template_values` feeds `{{ app }}` and `{{ vars.<name> }}` from `qa.yaml`)
2. `docker-compose.y(a)ml` (both templated and plain) → rewritten in-place via `maybe_inject_compose_labels` to attach `ins.*` service labels
3. Concurrent copy uses a `JoinSet` with `COPY_CONCURRENCY = 3`. Per-file processing must be spawn-safe (clone state by move).

When adding new compose-level mutations, follow the `inject_compose_labels` pattern: parse YAML → mutate → serialize, guarded by `is_docker_compose_file(path)`.

### Home directory resolution

`--home` overrides; otherwise `.ins/` in the current working directory if present, else `$HOME/.ins`. Project-local `.ins/` wins so local development can isolate state. Persistent state lives under it:

- `.ins/nodes.json` — cluster nodes (the `local` node is synthetic, always prepended by `load_all_nodes`, not stored)
- `.ins/app/<app>/` — app templates with `qa.yaml`
- `.ins/store/deploy_history.duckdb` — DuckDB history, used to offer "reuse previous settings" prompts

### Interactive vs non-interactive flows

`prepare_deployment` gates all `inquire` prompts behind `std::io::stdin().is_terminal()`. Non-TTY runs silently use defaults / stored values / fail fast. When adding prompts, mirror this check or CI/scripted callers will hang.

### Provider environment variables

`build_provider_envs` in `src/env.rs` assembles per-service env vars (`INS_APP_NAME`, `INS_NODE_NAME`, plus `INS_SERVICE_<DEP>_*` for declared dependencies). These are passed to docker compose via `command.envs` locally and `shell_exports` remotely.

### List command convention

`ins <thing> list` commands use `src/output.rs::TableRenderable` + `print_structured_list`. New list-output record types must impl `TableRenderable { headers(), row() }` and derive `Serialize` so `--output json` works automatically.

## Workflow conventions

### Superpowers specs & plans

Design documents live under `docs/superpowers/specs/YYYY-MM-DD-<topic>-design.md` and implementation plans under `docs/superpowers/plans/YYYY-MM-DD-<topic>.md`. Existing files in those directories show the expected shape and are part of the brainstorm → spec → plan → execute workflow. For non-trivial features, write the spec first (via the `brainstorming` skill), then the plan (via `writing-plans`).
