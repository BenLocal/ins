# Volume Command

`ins volume` stores per-node Docker volume backings. A single logical volume name can resolve to different storage drivers depending on the target node.

## Model

Configuration lives in `.ins/volumes.json`. Each record has a `(name, node)` primary key.

- `filesystem` — the node mounts a host directory via `docker volume create --driver local --opt type=none --opt o=bind --opt device=<path>`.
- `cifs` — the node mounts an SMB share via `docker volume create --driver local --opt type=cifs --opt o=username=<u>,password=<p> --opt device=<//server/share>`.

On the node, the actual Docker volume is named `ins_<name>` so it does not collide with volumes created by other tooling.

## Configuring

```bash
ins volume add filesystem --name data --node node1 --path /mnt/data

ins volume add cifs --name data --node node2 \
  --server //10.0.0.5/share --username alice --password secret

ins volume set filesystem --name data --node node1 --path /mnt/new
ins volume set cifs --name data --node node1 \
  --server //10.0.0.5/share --username alice --password secret

ins volume delete --name data --node node1

ins volume list
ins --output json volume list
```

Passwords are stored in plaintext in `volumes.json`, consistent with how SSH passwords are stored in `nodes.json`.

## Using volumes in an app

App templates use standard Docker Compose volume syntax:

```yaml
services:
  web:
    image: nginx
    volumes:
      - data:/var/lib/app
volumes:
  data: {}
```

On `ins deploy`, `ins` rewrites the top-level `volumes:` block for the target node:

```yaml
volumes:
  data:
    external: true
    name: ins_data
```

Before `docker compose up -d`, `ins` runs `docker volume inspect ins_data`; if absent, it runs `docker volume create --driver local --opt type=... --opt o=... --opt device=... ins_data`.

## Error behavior

- If an app references a top-level volume that is not configured on the current node, both `ins check` and `ins deploy` abort with `volume '<name>' is not configured on node '<node>'`.
- If `docker volume create` fails on the node (for example, missing kernel CIFS module, wrong credentials, unreachable server), the error from `docker` is surfaced and the deploy aborts before any service starts.
- If the Docker volume already exists on the node, `ins` reuses it without comparing `driver_opts`. To pick up a configuration change on an already-created volume, remove it manually on the node: `docker volume rm ins_<name>`.

## Troubleshooting CIFS

- Kernel CIFS module missing — most minimal Linux images do not include CIFS. Install `cifs-utils` (Debian/Ubuntu: `apt-get install cifs-utils`; RHEL/CentOS: `yum install cifs-utils`).
- Special characters in CIFS passwords — `,` or `=` inside the password will break the `o=username=...,password=...` form. Avoid those characters, or escape per the kernel's `cifs.ko` option syntax.
- Version negotiation — some servers require `vers=3.0` or higher. This version of `ins volume` does not expose an option for that; if needed, create the docker volume manually on the node (use the `ins_<name>` naming convention) and `ins` will reuse it.
