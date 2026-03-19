# Repository Guidelines

## Project Structure & Module Organization

`ins` is a Rust CLI application. The entry point is [src/main.rs](/root/workspace/master/ins/src/main.rs), which wires together the `app`, `node`, `cli`, `file`, and `provider` modules.

- `src/cli/`: command handlers such as `app.rs`, `node.rs`, and `deploy.rs`
- `src/app/`: app parsing and schema types
- `src/node/`: node models and listing logic
- `src/file/`: local and remote file operations
- `src/provider/`: provider backends, currently including Docker Compose support
- `template/qa.yaml`: built-in app template

Tests are colocated with implementation files, for example [src/app/parse_test.rs](/root/workspace/master/ins/src/app/parse_test.rs) and [src/cli/deploy_test.rs](/root/workspace/master/ins/src/cli/deploy_test.rs).

## Build, Test, and Development Commands

- `cargo build`: compile a debug build for local development
- `cargo run -- --help`: run the CLI and inspect available commands
- `cargo test`: run unit and async tests across the crate
- `cargo fmt`: apply standard Rust formatting
- `cargo clippy --all-targets --all-features`: catch common Rust issues before review
- `cargo build --release --features duckdb-bundled`: build a portable release with bundled DuckDB

Run `fmt`, `clippy`, and `test` before submitting changes.

## Coding Style & Naming Conventions

Use standard `rustfmt` formatting with 4-space indentation. Follow Rust naming conventions: `snake_case` for modules, files, functions, and test names; `CamelCase` for structs, enums, and traits.

Prefer small helpers over large command handlers when logic grows. Use `anyhow::Result` for fallible CLI paths, and make user-facing errors specific, especially for file paths, templates, or remote operations.

## Testing Guidelines

Place tests next to the code they cover using `mod tests` blocks or sibling files ending in `_test.rs`. Use descriptive names such as `load_app_record_parses_template_yaml`. Async tests should use `#[tokio::test]`.

Add tests for parser changes, CLI argument behavior, and filesystem workflows that copy templates or interact with remote/local file layers.

## Commit & Pull Request Guidelines

Keep commit subjects short, imperative, and focused on one change, for example: `Improve error handling in node loading functions`.

Pull requests should describe the behavior change, note any CLI surface changes, and list local verification steps. Include sample command output when adjusting interactive flows or deployment behavior.
