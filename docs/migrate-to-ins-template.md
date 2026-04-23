# Migrate Legacy Artifacts to an ins App Template

Playbook for converting a legacy deployment bundle (`docker-compose.yaml` + install scripts + config files) into an `ins` app template at `.ins/app/<name>/`. Follow it top to bottom; the last step validates the output with `ins check`.

## When to use

- You have a running `docker-compose.yaml` that you deploy by hand (scp + `docker compose up -d`) and want to manage it with `ins deploy` instead.
- Your deployment has one-shot setup steps (create directories, chmod mounts, install packages) that should run automatically before/after the container comes up.
- Config files (nginx.conf, application.yml, …) currently have hand-edited values per environment that you'd like to parameterize.

## Inputs

Ask the user for:

1. **`docker-compose.yaml`** (or `.yml`) — the source of truth for the services.
2. **App name** — becomes the template directory name (`.ins/app/<name>/`) and the `name` field in qa.yaml. Must match `[a-zA-Z0-9_-]+`.
3. **Install / teardown scripts** (optional) — anything like `install.sh`, `setup.sh`, `pre-deploy.sh`, `post-deploy.sh`, `cleanup.sh`.
4. **Config files referenced by the compose file** (optional) — e.g. `nginx.conf`, `redis.conf`, `application.yml`. List them or extract from `volumes:` bind-mounts.

## Output layout

```
.ins/app/<name>/
├── qa.yaml                         # app metadata + parameterized values + volume deps
├── docker-compose.yaml.j2          # templated compose (values → {{ vars.* }})
├── before.sh                       # optional — runs on the node before `docker compose up`
├── after.sh                        # optional — runs on the node after `docker compose up`
├── <config>.j2                     # any config file with parameterized values
└── <static files as-is>            # anything that doesn't need parameterization
```

## Workflow

### 1. Identify parameterization candidates

Scan the source `docker-compose.yaml` and config files for values that vary across deployments. Common categories:

| Category | Examples | qa.yaml type |
|---|---|---|
| Image / version | `nginx:1.25.0`, `redis:7.2-alpine` | `string` |
| Ports | `8080:80`, `3306:3306` | `number` |
| Environment values | `SERVER_NAME=example.com`, `DB_PASSWORD=…` | `string` |
| Resource limits | `mem_limit: 512m`, `cpus: 2` | `string` / `number` |
| Hostnames / URLs | `REDIS_URL=redis://…` | `string` |
| Feature flags | booleans like `ENABLE_TLS` | `boolean` |

**Do not** parameterize:
- Container names that users never change
- Fixed file paths inside the container (`/etc/nginx/nginx.conf`)
- Service names in the compose file

### 2. Identify volume dependencies

Every **host directory bind** (`/data/www:/usr/share/nginx/html`) or **named volume** (`mydata:/var/lib/mysql`) becomes a candidate for `ins volume`:

- Named volumes → declare in `qa.yaml` under `volumes: [name, ...]` and reference as `{{ volumes.<name>.docker_name }}:/mount/path` in the compose template (or just by name — ins auto-generates the top-level `volumes:` block).
- Bind mounts where the host path is environment-specific → replace with a named volume and let `ins volume add filesystem --node <n> --path <host-path>` record the per-node backing.
- Bind mounts where the host path is a sibling of the compose file (`./nginx.conf`) → leave alone, copied verbatim into the workspace.

If the app uses every volume configured for a node, set `all_volume: true` and omit the `volumes:` list.

### 3. Generate `qa.yaml`

Template:

```yaml
name: <app-name>
version: "1.0.0"
description: "<one-liner describing what this app does>"
author_name: <your name>
author_email: <your email>

# Declare required volumes here. ins injects the top-level `volumes:` block into
# compose at deploy time and runs `docker volume create` on the target node.
volumes:
  - <volume-name-1>
  - <volume-name-2>
# Or: all_volume: true

# Optional — other installed services this app consumes at deploy time.
# Their env vars get injected as INS_SERVICE_<NAME>_* into this app's container.
dependencies: []

values:
  - name: image_tag
    type: string
    description: "Container image tag"
    default: "1.25.0"
  - name: port
    type: number
    description: "Host port to publish"
    default: 8080
  - name: server_name
    type: string
    description: "Server hostname"
    default: "localhost"
  # Password/secret values: no default, prompted at deploy time
  - name: admin_password
    type: string
    description: "Admin password (leave empty to generate)"
    default: ""

# Optional lifecycle hooks
before:
  shell: bash
  script: ./before.sh
after:
  shell: bash
  script: ./after.sh
```

Rules:
- Every value extracted in step 1 becomes a `values:` entry.
- Secrets should have `default: ""` or no default at all so `ins deploy` prompts the user.
- If the source has multiple choices for a field (like an engine mode), use the `options: [...]` list.

### 4. Convert `docker-compose.yaml` → `docker-compose.yaml.j2`

Replace each parameterized literal with a Jinja expression reading from `vars`:

```yaml
# Before
services:
  web:
    image: nginx:1.25.0
    ports:
      - "8080:80"
    environment:
      - SERVER_NAME=example.com

# After (docker-compose.yaml.j2)
services:
  web:
    image: nginx:{{ vars.image_tag }}
    ports:
      - "{{ vars.port }}:80"
    environment:
      - SERVER_NAME={{ vars.server_name }}
```

Remove the top-level `volumes:` block from your template — ins generates it based on `qa.yaml`'s `volumes:` list at deploy time. Service-level `volumes:` entries that reference those names stay as-is.

### 5. Convert install / teardown scripts

- Rename to `before.sh` (runs before `docker compose up`) or `after.sh` (runs after).
- Reference them from `qa.yaml` under `before:` / `after:`.
- Keep them idempotent — `ins deploy` runs them on every invocation.
- They run on the **target node**, not the ins host — any paths, binaries, or network access must be valid there.
- Available env vars: `INS_APP_NAME`, `INS_SERVICE_NAME`, `INS_NODE_NAME`, `INS_VERSION`, plus `<VALUE_NAME>` for each app value (uppercased, non-alphanumeric → `_`) and `INS_SERVICE_<DEP>_*` for each dependency.

### 6. Convert config files with parameterized values

If a bind-mounted config file (e.g. `nginx.conf`, `redis.conf`, `my.cnf`) contains values that should match the app's parameters, rename it to `<name>.j2` and replace the literals with `{{ vars.<name> }}` expressions.

Example:

```
# Before (nginx.conf)
server {
    listen 80;
    server_name example.com;
    root /usr/share/nginx/html;
}

# After (nginx.conf.j2)
server {
    listen 80;
    server_name {{ vars.server_name }};
    root /usr/share/nginx/html;
}
```

Update the compose bind mount to reference the rendered file:

```yaml
# Before
volumes:
  - ./nginx.conf:/etc/nginx/nginx.conf

# After — ins renders nginx.conf.j2 → nginx.conf in the workspace before the bind mount resolves
volumes:
  - ./nginx.conf:/etc/nginx/nginx.conf
```

(The bind path in compose stays the same; ins writes the rendered file at that location inside the workspace.)

### 7. Record per-node volumes

For every name in `qa.yaml`'s `volumes:` list, make sure the target node has a backing configured via `ins volume`:

```bash
ins volume add filesystem --name <volume> --node <node> --path /host/path
# or for SMB
ins volume add cifs --name <volume> --node <node> \
  --server //server/share --username <u> --password <p>
```

If the node isn't registered yet: `ins node add --name <n> --ip <ip> --port 22 --user <u> --password <p>` first.

### 8. Verify

```bash
ins check --workspace /tmp/ws-verify --node <node> <app-name>
```

Expect `Check completed.` with no errors. The rendered files under `/tmp/ws-verify/<app-name>/` should look right:

- `docker-compose.yaml` has concrete values (no `{{ }}` left)
- `nginx.conf` (or whatever) has interpolated config
- Labels `ins.node_name`, `ins.service`, `ins.name`, etc. present on each service
- Top-level `volumes:` block has `external: true, name: ins_<n>` for each declared volume

If a placeholder remains unresolved, either the variable name is misspelled in the template or missing from `qa.yaml.values`. Run `ins deploy` (not just `check`) on a test node to confirm hooks and volume creation work end to end.

## Worked example: nginx

**Source `docker-compose.yaml`:**

```yaml
services:
  nginx:
    image: nginx:1.25.0
    ports:
      - "8080:80"
    volumes:
      - ./nginx.conf:/etc/nginx/nginx.conf
      - wwwdata:/usr/share/nginx/html
    environment:
      - SERVER_NAME=example.com
volumes:
  wwwdata:
```

**Source `install.sh`:**

```bash
#!/bin/bash
mkdir -p /data/www
chmod 755 /data/www
```

**Source `nginx.conf`:**

```
server {
    listen 80;
    server_name example.com;
    root /usr/share/nginx/html;
    index index.html;
}
```

**Migrated `.ins/app/nginx/qa.yaml`:**

```yaml
name: nginx
version: "1.0.0"
description: "Static nginx server"
volumes:
  - wwwdata
values:
  - name: image_tag
    type: string
    default: "1.25.0"
  - name: port
    type: number
    default: 8080
  - name: server_name
    type: string
    default: "example.com"
before:
  shell: bash
  script: ./before.sh
```

**Migrated `.ins/app/nginx/docker-compose.yaml.j2`:**

```yaml
services:
  nginx:
    image: nginx:{{ vars.image_tag }}
    ports:
      - "{{ vars.port }}:80"
    volumes:
      - ./nginx.conf:/etc/nginx/nginx.conf
      - wwwdata:/usr/share/nginx/html
    environment:
      - SERVER_NAME={{ vars.server_name }}
```

(No top-level `volumes:` block — ins generates it from qa.yaml's `volumes:` list.)

**Migrated `.ins/app/nginx/nginx.conf.j2`:**

```
server {
    listen 80;
    server_name {{ vars.server_name }};
    root /usr/share/nginx/html;
    index index.html;
}
```

**Migrated `.ins/app/nginx/before.sh`:**

```bash
#!/bin/bash
mkdir -p /data/www
chmod 755 /data/www
```

**Register the volume on the target node, then verify:**

```bash
ins volume add filesystem --name wwwdata --node local --path /data/www
ins check --workspace /tmp/nginx-verify --node local nginx
# Check completed.
```

## Common pitfalls

- **Forgetting to remove the top-level `volumes:` block in the template** — ins will merge but duplicate keys get noisy. Remove it.
- **Using `{{ }}` inside a shell hook** — `before.sh` / `after.sh` are **not** Jinja-rendered; they're copied verbatim. Use `$VALUE_NAME` env vars instead.
- **Hard-coding host paths in compose** — anything environment-specific (like `/data/www`) should be a named volume declared in qa.yaml + registered with `ins volume`.
- **Parameterizing too much** — if a value is the same across every deployment, leave it as a literal. qa.yaml `values` should only hold things that actually vary.
- **Secrets in `default:`** — defaults end up in the template on disk. For passwords / API keys, set `default: ""` or omit default entirely so the deploy prompt (or `--value key=...`) forces an explicit choice.
