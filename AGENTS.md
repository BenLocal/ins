# Repository Guidelines

## Project Structure & Module Organization

`ins` is a Rust CLI for preparing, validating, and deploying app templates. The entry point is [src/main.rs](/root/workspace/master/ins/src/main.rs).

- `src/cli/`: command entrypoints for `app`, `node`, `deploy`, and `check`
- `src/pipeline.rs`: shared pipeline for node/app selection, value prompts, workspace copy, and provider execution
- `src/app/`: `qa.yaml` parsing and app schema types
- `src/node/`: node definitions and node loading helpers
- `src/file/`: local and remote file abstractions used during workspace copy
- `src/provider/`: provider interface and Docker Compose implementation
- `src/store/`: DuckDB-backed deploy history storage
- `template/qa.yaml`: starter app template

Tests are colocated, for example [src/app/parse_test.rs](/root/workspace/master/ins/src/app/parse_test.rs) and [src/cli/deploy_test.rs](/root/workspace/master/ins/src/cli/deploy_test.rs).

## Build, Test, and Development Commands

- `cargo build`: compile the CLI
- `cargo run -- --help`: inspect top-level commands
- `cargo run --features duckdb-bundled -- check --help`: inspect the check workflow
- `cargo test --features duckdb-bundled`: run the test suite with bundled DuckDB
- `cargo fmt`: format Rust code
- `cargo clippy --all-targets --all-features`: lint before review
- `cargo build --release --features duckdb-bundled`: build a portable release binary

Use `--features duckdb-bundled` whenever tests or runtime code need DuckDB in environments without a system `libduckdb`.

## Coding Style & Naming Conventions

Follow `rustfmt` output and standard Rust naming: `snake_case` for files, modules, functions, and tests; `CamelCase` for structs, enums, and traits. Prefer small command wrappers in `src/cli/` and move shared behavior into `src/pipeline.rs` or domain modules.

Use `anyhow::Result` for fallible flows and keep CLI errors specific, especially for node names, file paths, template rendering, and provider failures.

## Testing Guidelines

Write colocated tests with `#[tokio::test]` for async behavior. Cover parser changes, workspace copy/render behavior, stored deploy history, and CLI selection logic. Use descriptive names such as `copy_apps_to_workspace_renders_template_files`.

## Commit & Pull Request Guidelines

Keep commits short and imperative, for example `Extract shared deployment pipeline`. PRs should explain CLI behavior changes, list commands run locally, and include sample output for prompt, validation, or deploy-flow changes.
