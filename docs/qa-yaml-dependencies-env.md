# `qa.yaml` 中 `dependencies` 与环境变量使用说明

本文说明 `ins` 在 `check` / `deploy` 阶段，如何根据 `qa.yaml` 里的 `dependencies` 和 `values` 生成 provider 环境变量，以及这些字段在 `qa.yaml` 中该怎么写。

适用场景：

- 你想在 `docker-compose.yml.j2` 中引用依赖服务的信息
- 你想在 `before.sh` / `after.sh` 里读取依赖服务参数
- 你想确认 `check` 打印出来的 provider 环境变量分别代表什么

## 1. 生成规则概览

`ins` 会为每个待执行的 service 生成一组 provider 环境变量，来源分成两类：

- 当前 app 自己的元信息与 `values`
- 当前 app 在 `dependencies` 中声明的已安装依赖 service 信息

实现位置见 [src/env.rs](/root/workspace/master/ins/src/env.rs)。

## 2. `qa.yaml` 相关字段说明

下面只列出和环境变量生成直接相关的字段。

### `name`

含义：
当前应用模板的名字。

是否必填：
是。

示例：

```yaml
name: webapp
```

对应环境变量：

- `INS_APP_NAME=webapp`

说明：
这是 app 名，不一定等于最终部署的 service 名。

### `version`

含义：
应用版本号，用于描述当前模板或应用版本。

是否必填：
否。

示例：

```yaml
version: 1.0.0
```

对应环境变量：

- `INS_VERSION=1.0.0`

### `description`

含义：
应用说明。

是否必填：
否。

示例：

```yaml
description: 示例 Web 应用
```

对应环境变量：

- `INS_DESCRIPTION=示例 Web 应用`

### `author_name`

含义：
作者名称。

是否必填：
否。

示例：

```yaml
author_name: Alice
```

对应环境变量：

- `INS_AUTHOR_NAME=Alice`

### `author_email`

含义：
作者邮箱。

是否必填：
否。

示例：

```yaml
author_email: alice@example.com
```

对应环境变量：

- `INS_AUTHOR_EMAIL=alice@example.com`

### `dependencies`

含义：声明当前 app 依赖哪些”已安装的 service”。每个条目可选地带 namespace 前缀。

是否必填：否。

格式：

| 写法 | 解析为 |
|---|---|
| `redis` | (default, redis) |
| `:redis` | (default, redis) |
| `staging:redis` | (staging, redis) |
| `prod:mysql-main` | (prod, mysql-main) |

只有依赖 service 在指定 namespace 下已安装时才注入对应环境变量。

环境变量前缀规则（hybrid）：

- 默认 namespace（`redis` / `:redis`）→ `INS_SERVICE_<SERVICE>_*`
- 显式非默认 namespace（`<ns>:<service>`）→ `INS_SERVICE_<NS>_<SERVICE>_*`

举例 ——

```yaml
dependencies:
  - redis
  - staging:redis
```

会注入：

```text
INS_SERVICE_REDIS_*           # 来自 default 命名空间
INS_SERVICE_STAGING_REDIS_*   # 来自 staging 命名空间
```

`dependencies` 本身不会生成单独变量，但会触发依赖 service 的变量注入。

### `values`

含义：
定义当前 app 需要的参数。参数值会参与模板渲染，也会转成 provider 环境变量。

是否必填：
否，但通常会配置。

示例：

```yaml
values:
  - name: image
    type: string
    description: 镜像地址
    default: nginx:latest

  - name: port
    type: number
    description: 服务端口
    default: 8080
```

对应环境变量：

- `IMAGE=nginx:latest`
- `PORT=8080`

变量名转换规则：

- 转成大写
- 非字母数字字符会转成 `_`
- 如果第一个字符是数字，会自动在前面补 `_`

示例：

- `image-tag` -> `IMAGE_TAG`
- `db.port` -> `DB_PORT`
- `8086_port` -> `_8086_PORT`

## 3. `dependencies` 会注入哪些环境变量

如果当前 app 写了：

```yaml
dependencies:
  - redis
```

并且 `redis` 这个 service 已经安装过，那么当前 app 会额外获得以下环境变量。

### 依赖 service 的基础信息

- `INS_SERVICE_REDIS_SERVICE`
  含义：依赖 service 的名字
  示例：`redis`

- `INS_SERVICE_REDIS_NAMESPACE`
  含义：依赖 service 所在的 namespace
  示例：`default` / `staging`

- `INS_SERVICE_REDIS_APP_NAME`
  含义：这个 service 对应的 app 名
  示例：`redis`

- `INS_SERVICE_REDIS_NODE_NAME`
  含义：依赖 service 部署在哪个节点
  示例：`node-a`

- `INS_SERVICE_REDIS_WORKSPACE`
  含义：依赖 service 的工作目录
  示例：`/srv/apps/prod`

- `INS_SERVICE_REDIS_CREATED_AT_MS`
  含义：依赖 service 安装记录创建时间，毫秒时间戳
  示例：`1711111111111`

- `INS_SERVICE_REDIS_QA_YAML`
  含义：依赖 service 当时保存下来的 `qa.yaml` 原文
  示例：多行 YAML 文本

### 依赖 service 的参数值

如果 `redis` 自己的 `qa.yaml` 有：

```yaml
values:
  - name: port
    type: number
    value: 6379

  - name: password
    type: string
    value: secret
```

那么当前 app 还能拿到：

- `INS_SERVICE_REDIS_PORT=6379`
- `INS_SERVICE_REDIS_PASSWORD=secret`

也就是说，依赖 service 的每个 `value`，都会按下面的格式注入：

```text
INS_SERVICE_<DEPENDENCY_SERVICE>_<VALUE_NAME>
```

## 4. 完整 `qa.yaml` 示例

下面是一个依赖 Redis 的 Web 应用示例：

```yaml
name: webapp
version: 1.0.0
description: 示例 Web 应用
author_name: Alice
author_email: alice@example.com

dependencies:
  - redis

before:
  shell: bash
  script: ./before.sh

after:
  shell: bash
  script: ./after.sh

values:
  - name: image
    type: string
    description: Web 镜像
    default: mycorp/web:latest

  - name: http_port
    type: number
    description: 对外端口
    default: 8080
```

在该 app 执行 `check` 或 `deploy` 时，常见可见的 provider 环境变量会类似：

```text
INS_APP_NAME=webapp
INS_SERVICE_NAME=webapp
INS_NODE_NAME=local
INS_VERSION=1.0.0
INS_DESCRIPTION=示例 Web 应用
INS_AUTHOR_NAME=Alice
INS_AUTHOR_EMAIL=alice@example.com
IMAGE=mycorp/web:latest
HTTP_PORT=8080
INS_SERVICE_REDIS_SERVICE=redis
INS_SERVICE_REDIS_APP_NAME=redis
INS_SERVICE_REDIS_NODE_NAME=node-a
INS_SERVICE_REDIS_WORKSPACE=/srv/apps/prod
INS_SERVICE_REDIS_CREATED_AT_MS=1711111111111
INS_SERVICE_REDIS_QA_YAML=name: redis
INS_SERVICE_REDIS_PORT=6379
INS_SERVICE_REDIS_PASSWORD=secret
```

## 5. 在模板文件里如何使用

所有模板文件的渲染规则都一样，不只 `docker-compose.yml.j2`。

只要文件名后缀是下面任意一种，就会被当成模板渲染：

- `.j2`
- `.jinja`
- `.jinja2`
- `.tmpl`

例如：

- `docker-compose.yml.j2`
- `nginx.conf.j2`
- `app.env.tmpl`
- `scripts/start.sh.jinja`

可用上下文只有两类：

- `app`
- `vars`

实现位置见 [src/pipeline.rs](/root/workspace/master/ins/src/pipeline.rs)。

### 5.1 `app` 如何使用

`app` 对应整个 `qa.yaml` 解析后的 app 对象，适合读取元信息。

可直接使用的常见字段：

- `app.name`
- `app.version`
- `app.description`
- `app.author_name`
- `app.author_email`
- `app.dependencies`
- `app.values`

示例：

```jinja
# generated for {{ app.name }}
# version={{ app.version }}
# author={{ app.author_name }} <{{ app.author_email }}>
```

### 5.2 `vars` 如何使用

`vars` 对应 `qa.yaml` 里 `values` 解析后的结果，适合在模板中直接取值。

假设 `qa.yaml` 里有：

```yaml
values:
  - name: image
    type: string
    default: nginx:latest

  - name: http_port
    type: number
    default: 8080
```

那么模板中可以这样取：

```jinja
image={{ vars.image }}
port={{ vars.http_port }}
```

另外每个 value 还会额外生成一个 `*_meta` 对象，保留原始定义和最终解析值：

- `vars.image_meta`
- `vars.http_port_meta`

示例：

```jinja
# image description: {{ vars.image_meta.description }}
# image resolved: {{ vars.image_meta.resolved }}
```

### 5.3 在 `docker-compose.yml.j2` 中使用

这里通常会同时用到两类东西：

- Jinja 渲染期变量：`{{ app.xxx }}`、`{{ vars.xxx }}`
- 容器运行期环境变量：`${INS_SERVICE_...}`

示例：

```yaml
services:
  web:
    image: {{ vars.image }}
    container_name: {{ app.name }}
    environment:
      APP_NAME: {{ app.name }}
      HTTP_PORT: {{ vars.http_port }}
      REDIS_HOST: ${INS_SERVICE_REDIS_SERVICE}
      REDIS_PORT: ${INS_SERVICE_REDIS_PORT}
      REDIS_PASSWORD: ${INS_SERVICE_REDIS_PASSWORD}
```

说明：

- `{{ ... }}` 是 `ins` 在拷贝模板到 workspace 时渲染
- `${...}` 是后续 provider 或容器运行时再读取

### 5.4 在其他 `.j2` / `.tmpl` 文件中使用

注意：

- 只有模板后缀文件会被 Jinja 渲染
- 普通文件会原样拷贝，不会解析 `{{ ... }}`
- 如果你希望脚本文件也使用 Jinja，请把它命名成 `*.j2`、`*.jinja`、`*.jinja2` 或 `*.tmpl`

例如 `nginx.conf.j2`：

```jinja
server {
    listen {{ vars.http_port }};
    server_name {{ app.name }};

    location / {
        proxy_pass http://127.0.0.1:{{ vars.http_port }};
    }
}
```

例如 `app.env.tmpl`：

```jinja
APP_NAME={{ app.name }}
APP_VERSION={{ app.version }}
HTTP_PORT={{ vars.http_port }}
IMAGE={{ vars.image }}
```

例如 `scripts/start.sh.j2`：

```jinja
#!/usr/bin/env bash
set -euo pipefail

echo "starting {{ app.name }}"
echo "port={{ vars.http_port }}"
```

### 5.5 模板里不能直接用什么

下面这种写法通常是错的：

```jinja
{{ INS_SERVICE_REDIS_PORT }}
{{ vars.INS_SERVICE_REDIS_PORT }}
```

原因：

- `INS_SERVICE_*` 是 provider 环境变量，不是 Jinja 模板上下文
- 模板渲染时只提供 `app` 和 `vars`

如果你需要在生成后的文件里保留 provider 环境变量占位符，应写成 shell / compose 风格：

```text
${INS_SERVICE_REDIS_PORT}
```

而不是 Jinja 表达式。

### 5.6 什么时候该用 `vars.xxx`，什么时候该用 `${INS_SERVICE_...}`

- 当前 app 自己在 `qa.yaml` 里定义的参数：用 `{{ vars.xxx }}`
- 当前 app 的元信息：用 `{{ app.xxx }}`
- 已安装依赖 service 注入的 provider 环境变量：用 `${INS_SERVICE_...}` 或 shell 中的 `$INS_SERVICE_...`

一个经验法则：

- 渲染模板时就能确定的值，用 Jinja
- 依赖 provider / shell / 容器环境注入的值，用环境变量占位符

### 5.7 在 shell 脚本中使用

`before.sh`：

```bash
#!/usr/bin/env bash
set -euo pipefail

echo "service=${INS_SERVICE_NAME}"
echo "redis_service=${INS_SERVICE_REDIS_SERVICE}"
echo "redis_port=${INS_SERVICE_REDIS_PORT}"
```

适合场景：

- 部署前后脚本里读取依赖信息
- 普通 shell 文件不走 Jinja，只读取运行时环境变量

如果你想让脚本也参与 Jinja 渲染，可以写成 `start.sh.j2`：

```jinja
#!/usr/bin/env bash
set -euo pipefail

echo "starting {{ app.name }}"
echo "port={{ vars.http_port }}"
echo "redis_port=${INS_SERVICE_REDIS_PORT}"
```

## 6. 常见误区

### 误区 1：`dependencies` 写 app 名

错误示例：

```yaml
dependencies:
  - redis-app
```

只有当安装记录里的 service 名就叫 `redis-app` 时，这样才有效。通常应该写实际 service 名。

### 误区 2：依赖没安装就想拿变量

如果依赖 service 还没有安装记录，对应的 `INS_SERVICE_*` 变量不会出现。

### 误区 3：把依赖变量当成模板变量 `vars.xxx`

`values` 会进入模板变量 `vars.xxx`。

`dependencies` 注入的是 provider 环境变量，读取方式是：

- shell 中使用 `$INS_SERVICE_...`
- compose 中使用 `${INS_SERVICE_...}`

而不是 `{{ vars.xxx }}`。

## 7. 排查建议

可以先运行 `check`，确认本次 provider 环境变量是否符合预期：

```bash
cargo run --features duckdb-bundled -- check \
  --provider docker-compose \
  --workspace ./workspace \
  --node local \
  webapp
```

如果依赖映射成功，`check` 输出里会打印当前 service 的 provider 环境变量。
