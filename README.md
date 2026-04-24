# ins

`ins` is a Rust CLI for preparing app templates, validating rendered deployment files, and running deployments against named nodes.

## Features

- Manage app templates under `.ins/app/<app>/qa.yaml`
- Manage named nodes with `ins node ...`
- Reuse recent deploy settings from DuckDB-backed history
- Copy and render app files into `workspace/<service>`
- Validate rendered Docker Compose files with `ins check`
- Deploy validated workspaces with `ins deploy`

## Project Layout

- `src/cli/`: command entrypoints
- `src/pipeline.rs`: shared deploy/check pipeline
- `src/app/`: app schema and `qa.yaml` parsing
- `src/node/`: node models and loaders
- `src/file/`: local and remote file adapters
- `src/provider/`: provider trait and Docker Compose provider
- `src/store/`: DuckDB deploy history storage
- `template/qa.yaml`: starter template

## Build and Test

DuckDB is used for deployment history. In environments without a system `libduckdb`, use the bundled feature (the Makefile passes this automatically).

### Install

`make install` compiles with `--features duckdb-bundled` and copies the binary to `~/.cargo/bin/ins` so you can invoke `ins` from anywhere on your PATH.

```bash
make install
ins --help
```

### Common Makefile targets

| Target             | What it does                                         |
| ------------------ | ---------------------------------------------------- |
| `make build`       | Debug build with bundled DuckDB                      |
| `make release`     | Portable release build with bundled DuckDB           |
| `make install`     | `cargo install --path .` into `~/.cargo/bin`         |
| `make test`        | Run the full test suite                              |
| `make test-one TEST=<substring>` | Run tests matching a substring         |
| `make fmt` / `make fmt-check`    | Format / verify formatting             |
| `make clippy`      | Lint with `-D warnings`                              |
| `make check`       | Full CI gate: `fmt-check` + `clippy` + `test`        |
| `make run ARGS="volume list"`    | Run the CLI with args                  |
| `make clean`       | `cargo clean`                                        |

### Raw cargo

If you prefer cargo directly, remember to pass `--features duckdb-bundled` when `libduckdb` isn't installed system-wide:

```bash
cargo build --features duckdb-bundled
cargo run --features duckdb-bundled -- check --help
cargo test --features duckdb-bundled
cargo build --release --features duckdb-bundled
```

## CI/CD

- Push and pull request validation runs `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --features duckdb-bundled` on Linux.
- Pushing a `v*` tag creates a GitHub Release and uploads archives for:
  - `x86_64-apple-darwin`
  - `aarch64-apple-darwin`
  - `x86_64-pc-windows-gnu`
  - `aarch64-pc-windows-gnullvm`
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
  - `x86_64-unknown-linux-musl`
  - `aarch64-unknown-linux-musl`
- Linux and Windows targets use `cross` default images directly.
- macOS `cross` builds are disabled by default. To enable them, set repository variables:
  - `CROSS_MACOS_ENABLED=true`
  - `CROSS_TARGET_X86_64_APPLE_DARWIN_IMAGE`
  - `CROSS_TARGET_AARCH64_APPLE_DARWIN_IMAGE`
- Those macOS images must be your own `cross`-compatible images containing a valid Apple SDK.
- Release jobs verify that the pushed `v*` tag matches `Cargo.toml`'s `version`.
- Each release archive is uploaded with a matching `.sha256` checksum file.
- The published GitHub Release also includes a combined `checksums.txt`.

## CLI Overview

### Inspect commands

```bash
ins --help
ins template init --name nginx
ins version
ins app list
ins node list
```

### Validate without deploying

`check` runs the same preparation pipeline as `deploy`: select node/apps, resolve service names and values, copy files into the workspace, render templates, then validate with the provider.

```bash
ins check \
  --provider docker-compose \
  --workspace ./workspace \
  --node local \
  nginx
```

### Deploy

`deploy` uses the same pipeline, then runs the provider after validation-ready files are prepared.

```bash
ins deploy \
  --provider docker-compose \
  --workspace ./workspace \
  --node local \
  nginx
```

### Non-interactive runs: `-d` / `--defaults`

Both `check` and `deploy` accept `-d` / `--defaults` to skip every prompt and use each qa.yaml value's `default` instead. CLI `-v KEY=VALUE` overrides still win; stored-history reuse is skipped so every run is deterministic from qa.yaml alone.

```bash
# CI-safe: no prompts, no drift from previous deploys
ins deploy -d --node prod mysql redis nginx
```

If any value has no `default`, the run fails fast and lists every missing key so you can add them to qa.yaml (or pass `-v KEY=VALUE`).

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

## App Templates

Each app lives under `.ins/app/<app>/` and should include `qa.yaml`. Template files such as `docker-compose.yaml.j2` or `nginx.conf.j2` are rendered into the target workspace directory. Normal files are copied as-is.

Use `ins template init --name <app>` to scaffold a new app template with `qa.yaml`, `before.sh`, and `after.sh` under `.ins/app/<app>/`.

For `qa.yaml` field meanings, dependency env mapping, and usage examples, see [docs/qa-yaml-dependencies-env.md](docs/qa-yaml-dependencies-env.md).

Migrating a legacy `docker-compose.yaml` + install scripts + config files into an ins app template: Claude Code users invoke the `migrate-to-ins-template` skill (see [.claude/skills/migrate-to-ins-template/SKILL.md](.claude/skills/migrate-to-ins-template/SKILL.md)); humans can read the same file as a step-by-step playbook.

During `check` and `deploy`, the CLI can:

- prompt for service names
- prompt for app values
- ask whether to use defaults
- offer to reuse the most recent saved settings for the same `node + workspace + app`

## Deployment History

Recent settings are stored in DuckDB at:

```text
.ins/store/deploy_history.duckdb
```

Stored records include node, workspace, app, service, selected values, `qa.yaml`, and creation time. This allows later runs to reuse prior settings quickly.
