# TUI Command Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `ins tui` command backed by `ratatui` that lets users browse and manage nodes, apps, and services from one terminal UI.

**Architecture:** Keep all new TUI-specific code under `src/tui/`, with a small CLI adapter in `src/cli/tui.rs`. Reuse existing node/app/service file-loading logic where possible, and move any shared persistence helpers to crate-visible functions instead of duplicating filesystem behavior inside the UI.

**Tech Stack:** Rust, clap, tokio, crossterm, ratatui, anyhow

---

### Task 1: Define the TUI entrypoint surface

**Files:**
- Modify: `src/main.rs`
- Modify: `src/cli/mod.rs`
- Create: `src/cli/tui.rs`

- [ ] **Step 1: Write the failing test**

Add a CLI parsing test that expects `ins tui` to parse as a valid subcommand.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test tui_command`
Expected: FAIL because the subcommand/module does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Add the clap subcommand enum variant, add the command module export, and route execution to a `TuiCommand`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test tui_command`
Expected: PASS

### Task 2: Add reusable TUI data helpers

**Files:**
- Modify: `src/cli/node.rs`
- Modify: `src/cli/app.rs`
- Modify: `src/cli/service.rs`
- Create: `src/tui/state.rs`

- [ ] **Step 1: Write the failing tests**

Add focused tests for:
- loading nodes/apps/services into a unified TUI snapshot
- adding a node through a TUI helper
- updating an existing node through a TUI helper

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test tui_state`
Expected: FAIL because the TUI state helpers and shared command helpers do not exist yet.

- [ ] **Step 3: Write minimal implementation**

Expose crate-visible async helpers for list/add/set/inspect operations and implement a `TuiState` model that consumes them.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test tui_state`
Expected: PASS

### Task 3: Render the ratatui interface

**Files:**
- Modify: `Cargo.toml`
- Create: `src/tui/mod.rs`
- Create: `src/tui/ui.rs`

- [ ] **Step 1: Write the failing test**

Add a rendering-oriented unit test that verifies the frame title/help text or tab labels for the initial state.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test tui_ui`
Expected: FAIL because `ratatui` rendering code is not implemented.

- [ ] **Step 3: Write minimal implementation**

Add `ratatui`, build the frame renderer, and render:
- tabs for `Nodes`, `Apps`, `Services`
- a list panel
- a detail panel
- a footer help line

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test tui_ui`
Expected: PASS

### Task 4: Wire keyboard actions and the event loop

**Files:**
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/state.rs`
- Modify: `src/tui/ui.rs`

- [ ] **Step 1: Write the failing tests**

Add state-transition tests for:
- tab switching
- list movement
- opening add/edit node forms
- committing add/set actions

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test tui_`
Expected: FAIL because keyboard handling and form transitions are missing.

- [ ] **Step 3: Write minimal implementation**

Implement the crossterm event loop and map:
- `Tab`/`Shift-Tab` for section switching
- arrow keys / `j` / `k` for movement
- `Enter` for inspect/select
- `a` for add node
- `e` for edit node
- `q` for quit

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test tui_`
Expected: PASS

### Task 5: Verify and clean up

**Files:**
- Modify: touched files as needed

- [ ] **Step 1: Run targeted tests**

Run: `cargo test tui_`
Expected: PASS

- [ ] **Step 2: Run formatting**

Run: `cargo fmt`
Expected: no diff or formatting-only diff

- [ ] **Step 3: Run broader verification**

Run: `cargo test`
Expected: PASS

- [ ] **Step 4: Run linting if practical**

Run: `cargo clippy --all-targets --all-features`
Expected: PASS or actionable warnings fixed
