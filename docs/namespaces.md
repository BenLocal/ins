# Namespaces

`ins` 的 namespace 是一个逻辑标签，附加在每次 `check` / `deploy` 上，影响：

1. `deploy_history` 的存储维度（按 `(node, namespace, service)` 唯一）
2. `qa.yaml` `dependencies` 的查找目标（`<ns>:<svc>` 语法）
3. provider 环境变量的命名（hybrid 规则）
4. compose 文件里注入的 `ins.namespace` label

## CLI

```bash
ins deploy --namespace staging --node prod web api    # staging 命名空间
ins deploy --node prod redis                          # 不传 → default
```

参数详情见 [check-and-deploy.md](./check-and-deploy.md)。

## 命名规则

正则：`^[a-z0-9][a-z0-9_-]{0,63}$`

理由：namespace 文本会进入环境变量 key，必须是 ASCII 大小写无歧义可转换的形态。

## 同节点 service name 唯一

> 同一台机器上不能部署相同的 service name 且 namespace 不同的服务。

如果节点上已经有 `default:web`，再执行 `ins deploy --namespace staging web` 会报错：

```text
service 'web' already exists on node 'prod' under namespace 'default'; \
cannot deploy under namespace 'staging'. Either redeploy under namespace \
'default' or manually remove the existing record from the deploy history \
(`<home>/store/deploy_history.duckdb`).
```

跨节点不受这个限制。

## 依赖 namespace 前缀

```yaml
dependencies:
  - redis            # default 命名空间
  - :mysql           # default 命名空间（写法等价于 `mysql`）
  - staging:cache    # staging 命名空间
```

env 注入：

| dependency | env 前缀 |
|---|---|
| `redis` / `:redis` | `INS_SERVICE_REDIS_*` |
| `staging:redis` | `INS_SERVICE_STAGING_REDIS_*` |

每个前缀下都会带：`_SERVICE`、`_NAMESPACE`、`_APP_NAME`、`_NODE_NAME`、`_WORKSPACE`、`_CREATED_AT_MS`、`_<VALUE>` ...

## 重新部署已装服务

`ins service list` 已带 NAMESPACE 列。从 TUI 触发的"重新部署"会沿用记录里的 namespace，不需要再传 `--namespace`。

## 模板变量

模板上下文里有 `{{ namespace }}`，可用于在生成的文件中标注当前归属：

```jinja
# {{ app.name }} ({{ namespace }})
```

详见 [template-values.md](./template-values.md)。

## Compose label

每个 service 自动注入：

```yaml
labels:
  ins.namespace: <namespace>
```

## 相关文档

- [check-and-deploy.md](./check-and-deploy.md) — `--namespace` 参数说明
- [qa-yaml-dependencies-env.md](./qa-yaml-dependencies-env.md) — `<ns>:<svc>` 语法与 hybrid env-key 规则
- [env-vars.md](./env-vars.md) — `INS_NAMESPACE` + 依赖 hybrid 前缀规则
- [template-values.md](./template-values.md) — `{{ namespace }}` 模板变量
