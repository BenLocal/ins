# `ins check` / `ins deploy` 参数参考

`check` 和 `deploy` 共享同一组参数（来自 `PipelineArgs`）。`check` 渲染 + 校验，`deploy` 渲染 + 校验 + 真正启动。除此之外两条命令行为几乎一样，所以本页一次覆盖。

- 实现入口：[src/cli/check.rs](/root/workspace/master/ins/src/cli/check.rs)、[src/cli/deploy.rs](/root/workspace/master/ins/src/cli/deploy.rs)
- 参数定义：[src/pipeline/mod.rs](/root/workspace/master/ins/src/pipeline/mod.rs) 的 `PipelineArgs`
- 准备流程：[src/pipeline/prepare.rs](/root/workspace/master/ins/src/pipeline/prepare.rs) 的 `prepare_deployment`

## 基本用法

```bash
ins check  [FLAGS] [APPS...]
ins deploy [FLAGS] [APPS...]
```

缺省情况下两条命令都是交互式的：会弹选择 node、选择 app、填 value、决定是否复用上次部署的设置。非交互环境（CI、`stdin` 不是 TTY）会跳过提示、使用默认或报错。

## 位置参数

### `[APPS...]`

可选，多个 app 名字，空格分隔。不传会从 `app_home` 下已有的 app 中弹多选框让你选。

```bash
ins check nginx mysql redis
```

## 参数（按常用度排序）

### `-n` / `--node <NAME>`

部署目标节点名。`local` 表示本机；其他值对应 `ins node add` 注册过的远程节点。不传会在 TTY 环境下弹选择列表。

```bash
ins check --node gpu-01 nginx
```

### `-w` / `--workspace <PATH>`

渲染后的 app 文件放到哪里。ins 会把每个 app 复制到 `<workspace>/<service>/`，docker compose 在那儿跑。

- 支持相对路径，会转绝对路径后保存
- **第一次**指定某节点的 workspace 时，路径会自动写入 `config.toml` 的 `[nodes.<n>].workspace`，以后不用再传
- 如果不传也没有 config 默认值，命令会直接报错

```bash
ins deploy --node gpu-01 --workspace /srv/deploys nginx
```

### `-p` / `--provider <NAME>`

deployment provider。目前只支持 `docker-compose`（默认）。设计上预留了将来接其他 provider 的接口。

优先级：`--provider` > `config.toml` 的 `[nodes.<n>].provider` > `[defaults].provider` > `docker-compose`。

### `-v` / `--value <KEY=VALUE>`

覆写 qa.yaml 里的某个 value。可以传多次。

```bash
ins deploy --node prod -v image_tag=8.0.35 -v max_connections=10000 mysql
```

优先级（每个 value 逐个决定）：

1. `--value KEY=VALUE` CLI 覆写
2. 交互式提示里用户输入（`-d` 时跳过）
3. 上一次成功部署记录（TTY 里会问"是否复用"；`-d` 时跳过）
4. qa.yaml 的 `value` 字段
5. qa.yaml 的 `default` 字段
6. 都没有 → 报错（non-interactive 或 `-d`）或继续弹提示（interactive）

### `-d` / `--defaults`

一键跳过所有交互，所有 value 都用 qa.yaml 的 `default`。适合 CI、脚本部署。

- 任何没有 `default` 且没被 `-v` 覆写的 value 都会让命令报错，一次性列出所有缺项
- 忽略上一次部署的历史，service 名直接用 app 名
- 不影响 `-v`：命令行覆写依然生效

```bash
ins deploy -d --node prod mysql redis nginx    # CI 场景
ins check  -d --node prod mysql               # 本地快速校验
```

## 全局参数（顶层 `ins` 上，不是 PipelineArgs）

### `--home <PATH>`

覆盖 ins 的状态目录（默认：当前目录的 `.ins/`，否则 `~/.ins`）。里面放 nodes.json、volumes.json、config.toml、store/deploy_history.duckdb。

### `--output <FORMAT>`

仅影响 `ins <x> list` 这类结构化输出的格式（`table` / `json`），对 `check` / `deploy` 无效。

## 两条命令的差异

| 维度                             | `check`             | `deploy`            |
| -------------------------------- | ------------------- | ------------------- |
| 拷贝 app 到 workspace            | ✅                   | ✅                   |
| 渲染 `.j2` 模板                   | ✅                   | ✅                   |
| 写 `<workspace>/<service>/.env`   | ✅                   | ✅                   |
| docker compose config -q          | ✅                   | ❌                   |
| docker compose up -d              | ❌                   | ✅                   |
| 运行 `before` / `after` hook      | ❌                   | ✅                   |
| 打印 Template values for app      | ✅                   | ❌                   |
| 打印 Probe function values        | ✅                   | ❌                   |
| 打印 Provider Environment Variables | ✅                 | ❌                   |
| 写入 `store/deploy_history.duckdb`  | ❌                 | ✅                   |

换句话说：`check` 是干跑（无副作用、多打印）；`deploy` 是真跑（会改节点状态 + 写入历史）。

## 常见场景

### 第一次在新节点上部署

```bash
# 1. 注册节点
ins node add --name prod --ip 10.0.0.1 --user root --password ...
# 2. 注册卷
ins volume add filesystem --name mysql_data --node prod --path /srv/mysql
# 3. 干跑看看
ins check --node prod --workspace /srv/deploys mysql
# 4. 真跑
ins deploy --node prod mysql        # --workspace 已被 step 3 记住了
```

### CI 跑部署

```bash
ins deploy -d --node prod mysql redis nginx learn-platform-backend-api
```

### 交互确认 + 覆盖单个密码

```bash
ins deploy --node prod -v mysql_password=$(op read ...) mysql
```

### 快速切换 provider（假设以后支持）

```bash
ins deploy --provider k8s --node prod-k8s mysql
```

## 相关文档

- [env-vars.md](./env-vars.md) — qa.yaml 里 `${VAR}` 怎么解析、`INS_*` 变量怎么生成、`.env` 文件里写什么
- [template-values.md](./template-values.md) — `.j2` 模板能用的 `{{ vars.* }}` / `{{ system_info() }}` 等
- [qa-yaml-dependencies-env.md](./qa-yaml-dependencies-env.md) — `dependencies` 怎么生成 `INS_SERVICE_<DEP>_*`
- [volume-command.md](./volume-command.md) — `ins volume` 命令
