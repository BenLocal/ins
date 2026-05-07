# `ins web`

Browser UI mirroring `ins tui`. Three-pane layout (Nodes, Apps, Services) with HTMX-driven fragment updates. SSE streams check/deploy output live.

## Flags

| Flag        | Default              | Description |
|-------------|----------------------|-------------|
| `--bind`    | `127.0.0.1:7878`     | TCP listen address. Port `0` → kernel-allocated. |
| `--no-open` | unset                | Skip opening the browser on startup. |
| `--token`   | auto-generated       | Required for non-loopback binds; printed on startup. |

Backgrounding is the user's responsibility — `nohup ins web --bind 0.0.0.0:7878 &`.

## Auth model

- Loopback bind: no auth.
- Non-loopback: every request must present `?token=…` (first time) or the `ins_token` cookie (set by the server on the first valid query). Constant-time comparison, no other auth path.

## SSE event contract

`GET /jobs/:id/stream` emits, in order:

1. `event: backlog` — full snapshot at subscription time
2. `event: line` — per emitted line during the run
3. `event: done` — final event, data is `[ins:done] ok` or `[ins:done] err: <message>`

## Route ↔ TUI mapping

| TUI section | Route prefix | Notes |
|-------------|--------------|-------|
| Nodes       | `/nodes/*`   | CRUD via HTMX-driven modal forms. |
| Apps        | `/apps/*`    | File tree + textarea editor. No `$EDITOR` integration. |
| Services    | `/services/*` + `/jobs/:id/stream` | check/deploy spawn a Job; modal subscribes to the SSE stream. |
