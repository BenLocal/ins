# `ins web` — Browser UI mirroring the TUI — Design

Date: 2026-05-07
Status: Draft (pending user review)

## 1. Motivation

`ins tui` already exposes a three-pane interactive surface (Nodes / Apps / Services) for cluster operators. Some users prefer a browser over a terminal UI — easier to share over SSH port-forwarding, easier copy/paste of error output, easier to keep open alongside an editor. We want full functional parity with the TUI in a browser, exposed as a new top-level command.

Constraints driving the design:

- **Single static binary** — the project ships as one Rust executable. The web UI cannot require a separate `npm run build` or static-asset directory at runtime.
- **No regression in TUI** — refactors needed to share logic must keep `src/tui` behavior unchanged and pass existing tests.
- **No daemonisation in Rust** — `fork(2)` under a multi-threaded tokio runtime is unsafe and non-portable. Backgrounding is the user's responsibility (`nohup ins web &`).

## 2. CLI Surface

New top-level command (sibling of `tui`):

```text
ins web [--bind 127.0.0.1:7878] [--no-open] [--token <TOKEN>]
```

| Flag        | Default                           | Behavior |
|-------------|-----------------------------------|----------|
| `--bind`    | `127.0.0.1:7878`                  | Listen address. Port `0` → kernel-allocated; the actual address is printed after bind. |
| `--no-open` | unset                             | Skip auto-opening the browser. Always implied when `--bind` is non-loopback. |
| `--token`   | auto-generated when non-loopback  | Capability token. Ignored when bind is loopback. |

Startup sequence:

1. Resolve `home` and load `config.toml` (same as the TUI entrypoint).
2. Bind the TCP listener; abort with a specific error if the address is in use.
3. If bind is loopback **and** `--no-open` is not set, attempt `xdg-open` / `open` via `which` — failure to open is non-fatal (just warn).
4. Print one banner line: `Listening on http://<host>:<port>/  (token: <token-or-"none">)`.
5. Run foreground; `Ctrl+C` triggers axum graceful shutdown. In-flight check/deploy jobs are given up to 5s to finish, then the process exits.

`WebCommand` implements `CommandTrait` and dispatches to `crate::web::run(home, config, args)`, mirroring `TuiCommand`.

The command is wired in `src/cli/mod.rs` via `pub mod web;` and registered in the clap enum next to `Tui(TuiArgs)`.

## 3. Architecture

```text
src/
├── cli/web.rs                # CLI shell (parses args, calls web::run)
├── web/
│   ├── mod.rs                # Router assembly, axum::serve, graceful shutdown
│   ├── state.rs              # AppState { home, config, jobs, token, env }
│   ├── error.rs              # WebError → IntoResponse (renders error.html or returns 4xx text)
│   ├── auth.rs               # token middleware
│   ├── templates.rs          # minijinja Environment, embedded templates
│   ├── assets.rs             # embedded static files (htmx + sse + css)
│   ├── jobs.rs               # JobRegistry + Job spawn/lookup
│   ├── handlers/
│   │   ├── mod.rs
│   │   ├── index.rs
│   │   ├── nodes.rs
│   │   ├── apps.rs
│   │   └── services.rs
│   ├── templates/            # *.html source files (loaded via include_str!)
│   ├── static/               # htmx.min.js, htmx-sse.js, style.css
│   └── web_test.rs           # integration tests (axum oneshot + Router)
├── execution_output.rs       # MODIFIED: add streaming variant + broadcast::Sender
├── app/files.rs              # NEW: pure file CRUD pulled out of tui::state::apps
└── node/persist.rs           # NEW: pure node persistence pulled out of tui::state::nodes
```

Control flow per request:

```text
TCP → axum Router → [auth middleware] → handler → AppState
                                          ├─ domain modules (node, app::files, pipeline)
                                          └─ minijinja render → axum::Html
```

Every handler returns `Result<Response, WebError>`. `WebError` wraps `anyhow::Error`; `IntoResponse` formats it into either an `error.html` page (full-page requests) or plain-text 5xx (HTMX swap targets). Differentiation is by the `HX-Request` header.

`AppState` is `#[derive(Clone)]` with `Arc<…>` interior, passed via `axum::extract::State`. It holds:

- `home: Arc<PathBuf>`
- `config: Arc<InsConfig>`
- `templates: Arc<minijinja::Environment<'static>>`
- `jobs: Arc<JobRegistry>`
- `token: Option<Arc<String>>` — `None` when bind is loopback

The `env` map used by `qa.yaml` rendering (`config.defaults_env()`) is read on demand inside handlers — not cached on `AppState` — because the values can change when the user edits `config.toml` between renders.

## 4. Routes

Page structure: a single full-render route (`/`) returns the layout shell and three column fragments. Every other route returns a fragment to be swapped via HTMX.

| Method | Path                                | TUI counterpart            | Returns |
|--------|-------------------------------------|----------------------------|---------|
| GET    | `/`                                 | full TUI screen            | full HTML page |
| GET    | `/static/<file>`                    | —                          | embedded asset |
| **Nodes** | | | |
| GET    | `/nodes`                            | nodes pane                 | nodes/list fragment |
| GET    | `/nodes/new`                        | `a` add                    | nodes/form (create) — modal |
| GET    | `/nodes/:name`                      | select node (detail)       | nodes/detail fragment |
| GET    | `/nodes/:name/edit`                 | `e` edit                   | nodes/form (edit) — modal |
| POST   | `/nodes`                            | submit add                 | nodes/list fragment + close-modal OOB |
| POST   | `/nodes/:name`                      | submit edit                | nodes/list fragment + close-modal OOB |
| POST   | `/nodes/:name/delete`               | `d` delete (with confirm)  | nodes/list fragment |
| **Apps** | | | |
| GET    | `/apps`                             | apps pane                  | apps/list fragment |
| GET    | `/apps/:app`                        | enter file manager         | apps/files fragment |
| GET    | `/apps/:app/files/*path`            | select file                | apps/editor fragment (textarea + Save) |
| POST   | `/apps/:app/files`                  | `a` create file (form: path, kind) | apps/files fragment |
| POST   | `/apps/:app/files/*path`            | `e` save edits             | apps/files fragment + status flash |
| POST   | `/apps/:app/files/*path/delete`     | `d` delete file            | apps/files fragment |
| **Services** | | | |
| GET    | `/services`                         | services pane              | services/list fragment |
| GET    | `/services/:idx`                    | select service             | services/detail fragment |
| POST   | `/services/:idx/check`              | `c` check                  | redirect/swap to `/jobs/:id` |
| POST   | `/services/:idx/deploy`             | `d` deploy                 | redirect/swap to `/jobs/:id` |
| **Jobs** | | | |
| GET    | `/jobs/:id`                         | service action overlay     | services/job modal |
| GET    | `/jobs/:id/stream`                  | streaming output           | `text/event-stream` |

URL choices:

- `/services/:idx` uses the list index rather than a `(node, app, name)` triple. The triple makes URLs ugly and the list is short-lived from HTMX's view (every swap re-fetches). If we later need stable URLs we can switch to `/services/:node/:app/:name` without breaking the rest of the design.
- `*path` segments use axum's wildcard capture and are rejected by handlers if they contain `..`, leading `/`, or NUL bytes (defense in depth — the per-app dir is the only rooted location).

## 5. Templates and Static Assets

Templating engine: `minijinja` (already a project dependency, used for `qa.yaml` rendering). A single `Environment` is built at startup from a static slice of `(name, source)` pairs:

```rust
static TEMPLATES: &[(&str, &str)] = &[
    ("layout.html",        include_str!("templates/layout.html")),
    ("index.html",         include_str!("templates/index.html")),
    ("error.html",         include_str!("templates/error.html")),
    ("nodes/list.html",    include_str!("templates/nodes/list.html")),
    ("nodes/form.html",    include_str!("templates/nodes/form.html")),
    ("nodes/detail.html",  include_str!("templates/nodes/detail.html")),
    ("apps/list.html",     include_str!("templates/apps/list.html")),
    ("apps/files.html",    include_str!("templates/apps/files.html")),
    ("apps/editor.html",   include_str!("templates/apps/editor.html")),
    ("services/list.html", include_str!("templates/services/list.html")),
    ("services/detail.html", include_str!("templates/services/detail.html")),
    ("services/job.html",  include_str!("templates/services/job.html")),
];
```

Templates are loaded once (at server startup) into a long-lived `Environment` and shared via `Arc`. `auto_escape=true` per-extension is left at minijinja's default (HTML escapes by extension `.html`).

Static asset routes (no `tower-http::ServeDir`):

```rust
const HTMX:     &[u8] = include_bytes!("static/htmx.min.js");
const HTMX_SSE: &[u8] = include_bytes!("static/htmx-sse.js");
const STYLE:    &[u8] = include_bytes!("static/style.css");
```

Three explicit handlers — keeps the surface tiny and deterministic, no path traversal questions.

HTMX UX patterns we rely on:

- `hx-get`/`hx-post` with `hx-target` and `hx-swap` for fragment swaps
- `hx-confirm="…"` for destructive actions (delete node/file)
- A persistent `<div id="modal">` near `</body>`; forms swap into it; submit handlers return `<div id="modal" hx-swap-oob="true"></div>` to close
- `<div id="status-bar">` at the bottom of the page; flash messages OOB-swapped on every state-changing handler
- SSE via the HTMX SSE extension (`hx-ext="sse" sse-connect="/jobs/:id/stream"`)

## 6. Long-Running Actions and SSE

`ExecutionOutput` is extended in-place; `stdout()` and `buffered()` constructors stay byte-compatible:

```rust
pub struct ExecutionOutput {
    inner: Arc<Mutex<String>>,
    echo: bool,
    tx: Option<tokio::sync::broadcast::Sender<String>>,  // None for non-streaming
}

impl ExecutionOutput {
    pub fn streaming() -> Self { /* tx = Some(...) */ }
    pub fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<String>> { ... }
    // line()/error_line() additionally tx.send(message) when tx.is_some()
}
```

`broadcast::Sender::send` is non-blocking and returns Err only when there are no receivers — that's fine, we ignore. Capacity is 1024 lines; lagged receivers see a `Lagged` variant and we render a `[stream lagged]` marker plus reload from `snapshot()`.

`JobRegistry`:

```rust
pub struct Job {
    pub id: String,                              // "20260507-141233-3f9a"
    pub mode: PipelineMode,
    pub service: InstalledServiceRecord,
    pub output: ExecutionOutput,                 // streaming
    pub state: Arc<RwLock<JobState>>,            // Running | Done(Ok) | Done(Err(String))
    pub started_at: chrono::DateTime<Utc>,
}

pub struct JobRegistry {
    jobs: RwLock<VecDeque<Arc<Job>>>,            // ring of last 20
}
```

Spawning a job:

```rust
pub fn spawn(self: &Arc<Self>, mode, service, home, config) -> Arc<Job> {
    let job = Arc::new(Job::new(mode, service.clone(), ExecutionOutput::streaming()));
    self.push(job.clone());                      // also evicts oldest Done if >20
    let job_for_task = job.clone();
    tokio::spawn(async move {
        let result = async {
            let prepared = prepare_installed_service_deployment(&home, &config, None, &service).await?;
            execute_pipeline_with_output(&home, prepared, title_for(mode), mode, job_for_task.output.clone()).await
        }.await;
        let mut state = job_for_task.state.write().await;
        *state = match result { Ok(()) => JobState::Done(Ok(())), Err(e) => JobState::Done(Err(e.to_string())) };
        // also push a synthetic line so SSE clients receive a final "done" event
        job_for_task.output.line(if matches!(*state, JobState::Done(Ok(()))) { "[ins:done] ok" } else { "[ins:done] err" });
    });
    job
}
```

SSE response shape (event names are stable contract):

```text
event: backlog
data: <full snapshot at time of subscription, base64 or url-encoded>

event: line
data: [check] starting...

event: line
data: [check] node=local: docker compose pull...

event: done
data: ok

event: done
data: err: <first line of error>
```

The `backlog` event is sent first so a late subscriber catches up; subsequent `line` events flow from the broadcast receiver. When the job state transitions to `Done`, the handler sends one `done` event then closes.

Client side (HTMX SSE extension): the modal's `<pre id="job-output">` declares `sse-swap="line"` to append; a sibling element handles `done` to flip status and trigger a `services` reload via `hx-trigger="sse:done from:body"`.

Job retention: 20 most recent jobs in memory, evict by FIFO with `Done` jobs preferred for eviction. No persistence across server restarts — `store::duck` already records final deploy outcomes; live job buffers are ephemeral by design.

## 7. Domain-Logic Reuse

Goal: zero coupling from `web` to `tui`. Both call shared domain modules.

### 7.1 Node persistence

Pull pure logic out of `src/tui/state/nodes.rs`:

```rust
// src/node/persist.rs (NEW)
pub async fn upsert_node(home: &Path, record: NodeRecord) -> anyhow::Result<()>;
pub async fn delete_node_by_name(home: &Path, name: &str) -> anyhow::Result<()>;
pub fn parse_node_form(input: NodeFormInput) -> anyhow::Result<NodeRecord>;
```

`tui::state::nodes::apply_node_form` becomes a thin wrapper: validate via `parse_node_form`, persist via `upsert_node`, then `reload_nodes` (its own concern). Web handlers do `parse_node_form` + `upsert_node` directly.

### 7.2 App file CRUD

Pull pure FS ops out of `src/tui/state/apps.rs`:

```rust
// src/app/files.rs (NEW)
pub enum FileKind { Text, Directory }

pub struct TreeEntry {
    pub relative_path: String,
    pub kind: FileKind,
}

pub async fn list_tree(app_dir: &Path) -> anyhow::Result<Vec<TreeEntry>>;
pub async fn read_file(app_dir: &Path, rel: &str) -> anyhow::Result<String>;
pub async fn create_file(app_dir: &Path, rel: &str, kind: AppCreateKind) -> anyhow::Result<PathBuf>;
pub async fn write_file(app_dir: &Path, rel: &str, contents: &str) -> anyhow::Result<()>;
pub async fn delete_file(app_dir: &Path, rel: &str) -> anyhow::Result<()>;
```

All accept `rel: &str`; each function calls a private `safe_join(app_dir, rel)` that rejects `..` segments, absolute paths, and NUL bytes. `tui::state::apps::AppFileManager` is updated to call these for the actual IO; its index/cursor/refresh logic stays in `tui::state`.

### 7.3 Services

`cli::service::list_service_records`, `pipeline::prepare_installed_service_deployment`, `pipeline::execute_pipeline_with_output` are already pure and reusable. No changes.

## 8. Auth and Bind Safety

- Loopback bind (`127.0.0.1` / `::1`): no token, no middleware
- Non-loopback bind: `Option<Arc<String>>` token in `AppState`; missing/wrong → `401`
- Token sources accepted (in order): `?token=…` query, `ins_token` cookie. First request with a valid `?token=` sets the cookie (`HttpOnly`, `Path=/`, `SameSite=Strict`); subsequent requests use the cookie.
- Comparison is constant-time (6-line manual byte XOR — no `subtle` dep)
- `Authorization: Bearer …` is **not** accepted to keep the surface small (one auth path)

Explicit non-goals:

- No user/password login
- No CSRF tokens — token itself is the capability
- No HTTPS — terminate at nginx/caddy when exposing to the network

## 9. Testing

### 9.1 Unit tests (colocated `<file>_test.rs`)

- `src/node/persist_test.rs` — upsert, delete, idempotency, conflict detection (reusing tempdir)
- `src/app/files_test.rs` — `safe_join` rejection cases (`..`, absolute, NUL); CRUD round-trip; `list_tree` ordering
- `src/web/jobs_test.rs` — spawn job with a fake pipeline closure (doesn't touch docker), subscribe SSE, assert `backlog` + N `line` + `done` ordering

### 9.2 Web integration tests (`src/web/web_test.rs`)

Use `axum::Router::oneshot` (via `tower::ServiceExt`) — no real socket, no `reqwest` dep. Each test builds an `AppState` over a tempdir and exercises the routes:

- `GET /` returns 200 with `<div id="nodes-pane">` and the three columns
- Round-trip add → list → edit → list → delete on `/nodes`
- Round-trip on `/apps/:app/files/...`
- Auth: with token configured, missing token → 401; with cookie or query → 200; the cookie is set on first valid query
- SSE: subscribe to a stub job and assert event sequence
- Path traversal: `GET /apps/foo/files/..%2Fetc%2Fpasswd` → 400

### 9.3 TUI regression

Existing `src/tui/tui_test.rs` runs unchanged. One new test asserts that after the persistence refactor (§7.1), `apply_node_form` still writes the same JSON shape to `nodes.json`.

## 10. Dependencies

Hard increments to `Cargo.toml`:

```toml
axum = "0.7"
axum-extra = { version = "0.9", features = ["cookie"] }
tokio-stream = "0.1"
```

Already pulled transitively: `tower`, `tower-http`, `tower-layer`, `tower-service`, `http`, `futures-util`.

Avoided:

- `uuid` — job IDs use `chrono` + 4 random bytes
- `mime_guess` — three hardcoded mime types
- `subtle` — manual constant-time compare (6 lines)
- `reqwest` — `axum::Router::oneshot` for tests

Estimated release binary impact: +1.5MB. No new feature flag — `web` ships in the default build (this is a CLI tool, not a library).

## 11. Documentation Updates

CLAUDE.md `## Project Structure & Module Organization` table grows one row:

```text
- `src/web/` — axum-based browser UI mirroring the TUI; exposes `ins web`
```

CLAUDE.md `## Workflow conventions` `### Project-local skills` is unaffected. No env-var, template-values, qa-yaml, volume, check-and-deploy, or namespace docs change.

A new `docs/web-command.md` is created covering: flag reference, auth model, SSE event contract, and how the route layout maps to TUI sections.

## 12. Out of Scope

- Multi-user sessions / RBAC
- Editing `qa.yaml` with field-level validation in the browser (the TUI doesn't either — it's a textarea editor)
- Live log tailing beyond the active check/deploy job
- Persistent job history beyond `store::duck` records
- Mobile-responsive design (target is desktop browser at >=1200px)
- Daemonisation/pidfile management — user uses shell `&`/`nohup`

## 13. Open Questions (none blocking)

- Should `--bind 0.0.0.0:0` be allowed or hard-rejected? Allowed; warn on stderr, generate token, print clearly.
- Should we keep stdout `tracing` logs identical to the TUI's? Yes — `tracing_subscriber` already configured in `main.rs`; web handlers add an `info!` per state-changing route.
- Does `ins web` work on Windows? Untested but should — no Unix-only APIs in the design.
