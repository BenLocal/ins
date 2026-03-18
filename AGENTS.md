# Repository Guidelines

## Project Structure & Module Organization
`ins` is a Rust CLI application. The entry point is `src/main.rs`, which wires the `app`, `node`, `cli`, and `provider` modules together. Command implementations live under `src/cli/` (`app.rs`, `node.rs`, `deploy.rs`). App parsing and schema types live under `src/app/`, node models under `src/node/`, and provider backends under `src/provider/`.

Repository assets are minimal: `template/qa.yaml` is the built-in app template, and package metadata lives in `Cargo.toml`. There is no separate `tests/` directory today; tests are colocated with the modules they exercise.

## Build, Test, and Development Commands
- `cargo build`: compile the debug build for local development.
- `cargo run -- --help`: run the CLI and inspect available commands.
- `cargo test`: run unit and async tests across the crate.
- `cargo fmt`: apply standard Rust formatting before submitting changes.
- `cargo clippy --all-targets --all-features`: catch common Rust issues before opening a PR.
- `cargo build --release --features duckdb-bundled`: produce a portable release build with bundled DuckDB.

## Coding Style & Naming Conventions
Follow standard Rust formatting with 4-space indentation and `rustfmt` output. Use `snake_case` for files, modules, functions, and test names; use `CamelCase` for structs, enums, and traits. Keep modules focused by domain (`app`, `node`, `provider`, `cli`) and prefer small helper functions over large command handlers when logic grows.

Use `anyhow::Result` for fallible command paths and keep user-facing CLI errors specific, including the affected file or path when possible.

## Testing Guidelines
Prefer colocated tests with the code under test, using either `mod tests` blocks or sibling files like `src/app/parse_test.rs`. Use descriptive test names such as `load_app_record_parses_template_yaml`. Async behavior should use `#[tokio::test]`.

Run `cargo test` before every PR. Add tests for parser changes, CLI argument handling, and file-system behavior that touches app templates or workspace copying.

## Commit & Pull Request Guidelines
Recent commits use short, imperative summaries, for example `Improve error handling in node loading functions`. Keep commit subjects concise and focused on one logical change.

PRs should include a clear description, note any CLI behavior changes, and list verification steps run locally. Include sample command output when changing interactive flows or deployment behavior.
