# 环境变量完整指南

`ins` 里和环境变量相关的功能横跨了三个互不相同的层次。它们生效时机、能看到的变量、能被谁读取都不一样，混起来容易踩坑。本文把它们拆开讲清楚，再在最后给出一张一眼就能查的速查表。

| 层次                        | 生效时机                          | 语法                                  | 被什么读取                                           |
| --------------------------- | --------------------------------- | ------------------------------------- | ---------------------------------------------------- |
| ① qa.yaml 文本替换          | 加载 `qa.yaml` 时（最早）         | `${VAR}` / `${VAR:-fb}` / `$$`        | qa.yaml 本身（被 YAML 解析前）                       |
| ② Jinja 模板渲染            | 拷贝 `.j2` 文件到 workspace 时     | `{{ vars.xxx }}` / `{{ system_info()}}`| `docker-compose.yaml.j2` 等模板文件                  |
| ③ Provider/hook 运行时 env  | `docker compose up` + hook 执行时  | shell 变量 `$INS_APP_NAME`            | 容器内进程、`before.sh` / `after.sh` 脚本            |

相关实现：[src/app/parse.rs](/root/workspace/master/ins/src/app/parse.rs)（层 ①）、[src/pipeline/template.rs](/root/workspace/master/ins/src/pipeline/template.rs)（层 ②）、[src/env.rs](/root/workspace/master/ins/src/env.rs)（层 ③）

---

## 层 ①：qa.yaml 文本替换

`qa.yaml` 在被 YAML 解析器看到之前，会先做一次 shell 风格的变量替换。这一层是纯文本级的，所以对 qa.yaml 里的**任何字符串字段**都生效（`name`、`description`、`default`、`value`、`options[].value`、`volumes` 等）。

### 语法

| 写法             | 行为                                                  |
| ---------------- | ----------------------------------------------------- |
| `${VAR}`         | 查 `VAR`；找不到会在 `load_app_record` 报错           |
| `${VAR:-fallback}` | `VAR` 未设置时使用 `fallback`（字面量，不再递归展开） |
| `$$`             | 字面量 `$`（防止意外替换）                            |
| `$foo`（无大括号）| 原样保留，方便和 Jinja 的 `{{ }}` 或 shell 脚本共存   |

### 查找顺序

从前往后第一个命中即停：

1. `config.toml` 里的 **`[nodes.<n>.env]`**（仅 `check` / `deploy` 走 pipeline 路径时生效）
2. `config.toml` 里的 **`[defaults.env]`**
3. 进程环境变量（`std::env::var`）
4. `${VAR:-fallback}` 里的 fallback
5. 全部落空 → 报错退出

### 示例

```yaml
# qa.yaml
name: mysql
values:
  - name: mysql_password
    type: string
    default: "${MYSQL_PWD:-Iflyssemysql!#2023}"
  - name: registry
    type: string
    default: "${REGISTRY}/base/mysql"   # 必填；REGISTRY 没设置会报错
```

对应的 `config.toml`：

```toml
[defaults.env]
REGISTRY = "172.29.100.58:2025/higher-education"

[nodes.gpu-01.env]
MYSQL_PWD = "NodeSpecific!#2026"         # 只在部署到 gpu-01 节点时生效
```

### 什么情况下**用不到** `[nodes.<n>.env]`？

- `ins app list` / `ins app inspect` 这类**没选择节点**的命令只合并 `[defaults.env]`
- TUI 浏览 app 列表时同理
- 只有 `ins check <node>` / `ins deploy <node>` / `ins service install <service>` 这些选定了节点的命令会把 `[nodes.<n>.env]` 叠加进来

---

## 层 ②：Jinja 模板渲染

拷贝 `app/<name>/` 到 workspace 的过程中，所有 `.j2 / .jinja / .jinja2 / .tmpl` 结尾的文件都会走 minijinja 渲染。模板里能访问 `app` / `vars` / `volumes` / `service` 四个对象，以及 `system_info()` / `gpu_info()` 两个函数。

**这一层看不到任何 OS 环境变量，也看不到 `INS_*`**。只能用 qa.yaml 里声明的值和探测到的节点信息。

详见 [template-values.md](./template-values.md)。

典型写法：

```jinja
image: mysql:{{ vars.image_tag | default('8.0') }}
environment:
  MYSQL_ROOT_PASSWORD: "{{ vars.mysql_password }}"
  REGISTRY: "{{ vars.registry }}"
```

### 为什么 Jinja 模板里看不到 `INS_APP_NAME`？

因为 Jinja 渲染发生在 provider 启动**之前**——模板被渲染成 `docker-compose.yaml` 时，docker compose 还没跑、hook 脚本也还没跑。`INS_*` 环境变量是 provider 在跑 docker compose 时才注入的，所以只有容器和 hook 能看到。

如果你想在 compose 文件里把 `INS_APP_NAME` 等写进容器的 environment 段落，两种写法都行：

```yaml
# 方式 A：用 Jinja 从 app 对象拿
environment:
  APP_NAME: "{{ app.name }}"

# 方式 B：借 docker compose 的变量替换，读 provider 注入的 env
environment:
  APP_NAME: "${INS_APP_NAME}"
```

方式 A 在渲染时确定；方式 B 在 `docker compose up` 时由 compose 自己展开，指向 provider 注入的值。大多数场景下两个等价。

---

## 层 ③：Provider / hook 运行时环境变量

`docker compose up` 启动容器时，以及 `before.sh` / `after.sh` 执行时，会看到一组由 `ins` 生成的环境变量。它们来自三个源头，合并后一起传下去：

### ③.1 固定的 `INS_*` 变量

| 变量                | 值                                                             |
| ------------------- | -------------------------------------------------------------- |
| `INS_APP_NAME`      | qa.yaml 里的 `name`                                            |
| `INS_SERVICE_NAME`  | 当前部署的 service 名（通常等于 app 名）                       |
| `INS_NODE_NAME`     | 节点名（`local` 或 remote 的 `name`）                          |
| `INS_VERSION`       | ins 版本                                                       |

### ③.2 每个 qa.yaml value 对应的变量

规则：value 的 `name` 字段被大写、非字母数字字符替换成 `_`，取该值的最终解析结果（`value` → `default` → 交互输入）。

```yaml
values:
  - name: mysql_password
    type: string
  - name: max-connections
    type: number
```

生成：

- `MYSQL_PASSWORD=...`
- `MAX_CONNECTIONS=...`

### ③.3 依赖服务的变量 `INS_SERVICE_<DEP>_*`

qa.yaml 里每个 `dependencies[]` 条目，会从已安装服务表里读出对应 service 的 `app_values`，按 `INS_SERVICE_<DEP_NAME_UPPER>_<VALUE_NAME_UPPER>` 的格式注入。典型用途是 app A 要访问 app B 的密码：

```yaml
# app/backend/qa.yaml
dependencies:
  - mysql     # 依赖已安装的 mysql 服务
```

运行时 `backend` 的容器里会有：

- `INS_SERVICE_MYSQL_MYSQL_PASSWORD=...`
- `INS_SERVICE_MYSQL_PORT=3306`
- …（取决于 mysql 的 `values` 内容）

详细规则见 [qa-yaml-dependencies-env.md](./qa-yaml-dependencies-env.md)。

### ③.4 用户自定义的变量

`config.toml` 里 `[defaults.env]` + `[nodes.<n>.env]` 也会合并进来（node 键覆盖 defaults）：

```toml
[defaults.env]
REGISTRY = "172.29.100.58:2025"
HTTP_PROXY = "http://10.0.0.1:3128"

[nodes.gpu-01.env]
HTTP_PROXY = ""                  # 在 gpu-01 上清掉代理
CUDA_VERSION = "12.3"
```

### 读取方式

**在 docker-compose.yaml 里（推荐）：**

```yaml
services:
  web:
    environment:
      DB_PASSWORD: "${MYSQL_PASSWORD}"
      CUDA: "${CUDA_VERSION:-cpu}"
```

**在 before.sh / after.sh 里：**

```bash
#!/usr/bin/env bash
echo "deploying $INS_APP_NAME to $INS_NODE_NAME"
docker exec iflyssemysql mysql -uroot -p"$MYSQL_PASSWORD" < init.sql
```

hook 脚本**不会**被 Jinja 渲染 —— 它们按原样拷贝到 workspace 再在目标节点上运行，所以直接用 shell 变量即可。

### `ins check` 时怎么看到这些变量？

`ins check <node> <app>` 在执行 docker compose validate 前会把每个 service 要拿到的 env 全打印出来：

```
--------------------------------
Provider Environment Variables:
  [mysql]
    CUDA_VERSION=12.3
    INS_APP_NAME=mysql
    INS_NODE_NAME=gpu-01
    INS_SERVICE_NAME=mysql
    INS_VERSION=0.1.1
    MYSQL_PASSWORD=***
    ...
--------------------------------
```

`deploy` 不会打印这段，只有 `check` 会。

---

## 速查表

| 我想做什么                                                 | 用哪层                       | 怎么写                                        |
| ---------------------------------------------------------- | ---------------------------- | --------------------------------------------- |
| qa.yaml 里的密码不想明文写                                 | 层 ①                         | `default: "${MYSQL_PWD}"`                     |
| qa.yaml 的默认值在不同节点想不一样                         | 层 ①                         | `[nodes.<n>.env]` + `${VAR}`                  |
| docker-compose 里引用用户选的端口                          | 层 ②                         | `{{ vars.port }}`                             |
| docker-compose 里引用**已安装的**依赖服务的地址            | 层 ③                         | `"${INS_SERVICE_MYSQL_HOST}"`                 |
| 容器里的进程要拿到一个运行时常量                           | 层 ③（`[defaults.env]`）     | `environment: FOO: "${FOO}"` + config.toml    |
| hook 脚本要知道当前部署在哪个节点                          | 层 ③                         | `$INS_NODE_NAME`                              |
| 模板根据节点架构选镜像                                     | 层 ②（`system_info()`）     | `{% if system_info().arch == 'aarch64' %}...` |

## 常见踩坑

- **把 `${VAR}` 写在 `.j2` 模板里但希望 Jinja 展开它** —— 不会。`${VAR}` 只在 qa.yaml 层替换；`.j2` 用 `{{ vars.var }}`。
- **把 `{{ vars.x }}` 写在 `before.sh` 里** —— 不会渲染。hook 脚本不走 Jinja，直接照抄。用 `$VAR_X` 走层 ③。
- **依赖没安装就引用 `INS_SERVICE_<DEP>_*`** —— 那个 service 没装时 `dependencies` 段的变量不会生成。先 `ins service list` 确认。
- **`${VAR}` 在 qa.yaml 里未设置导致加载失败** —— 要么在 `[defaults.env]` 里填；要么改成 `${VAR:-fallback}`；要么写个 `export VAR=...` 再跑 ins。
- **期望 OS 环境变量被自动注入到容器** —— 不会。只有 `config.toml` 里显式写过的才会进 provider env。
- **`ins app list` 读 qa.yaml 时 `${NODE_ONLY_VAR}` 找不到** —— 因为 app list 不选节点，`[nodes.<n>.env]` 不生效。要让 app list 不报错，把变量放到 `[defaults.env]` 或提供 fallback。
