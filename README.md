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

DuckDB is used for deployment history. In environments without a system `libduckdb`, use the bundled feature:

```bash
cargo test --features duckdb-bundled
```

Useful commands:

```bash
cargo build
cargo run -- --help
cargo run --features duckdb-bundled -- check --help
cargo fmt
cargo clippy --all-targets --all-features
cargo build --release --features duckdb-bundled
```

## CLI Overview

### Inspect commands

```bash
ins --help
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

## App Templates

Each app lives under `.ins/app/<app>/` and should include `qa.yaml`. Template files such as `docker-compose.yaml.j2` or `nginx.conf.j2` are rendered into the target workspace directory. Normal files are copied as-is.

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
