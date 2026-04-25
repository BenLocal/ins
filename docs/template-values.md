# 模板变量与函数参考

本文列出 `ins` 在渲染 `.j2 / .jinja / .jinja2 / .tmpl` 文件时可以直接使用的所有变量和函数。模板引擎是 [minijinja](https://docs.rs/minijinja)，其内置过滤器（`| default(...)`, `| upper`, `| join(',')`, `| length` 等）全部可用；本文只列出 `ins` 特有的内容。

触发渲染的场景：

- `ins check` / `ins deploy` 拷贝 `app/<name>/` 到 workspace 的过程
- 文件名以 `.j2` / `.jinja` / `.jinja2` / `.tmpl` 结尾的文件会被渲染，去掉后缀后写入 workspace
- `docker-compose.yml(.yaml)` 无论是否加后缀都会被处理（只是没有值替换，仅注入 `ins.*` labels 和 `volumes:` 块）

实现位置：[src/pipeline/template.rs](/root/workspace/master/ins/src/pipeline/template.rs)

---

## 1. 顶层变量

| 变量          | 来源                                         | 说明                                                                                     |
| ------------- | -------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `app`         | `AppRecord` 序列化                           | 当前 app 的元信息（`name` / `version` / `description` 等）                               |
| `vars`        | 每个 `values[]` 条目的解析结果               | 以 `name` 为 key，提供 `.<name>` 和 `.<name>_meta` 两种访问方式                          |
| `volumes`     | `volumes:` 列表 + `ins volume` 记录合并结果  | 以卷的逻辑名为 key，返回该节点上卷的 docker 侧信息                                       |
| `service`     | 部署目标的 service 名                        | 通常等于 app 名，可在 `ins deploy --service <name>` 时被覆盖                             |
| `namespace`   | `--namespace` 参数（默认 `default`）          | 当前部署的 namespace 字符串，可用于在生成文件里标注归属                                  |
| `node`        | 部署目标节点的 `NodeRecord`                   | `node.name` / `node.ip` / `node.extern_ip`，本地节点固定 `name=local` / `ip=127.0.0.1`；`extern_ip` 取自 `[defaults].local_extern_ip` |

### 1.1 `app`

`app` 是 `AppRecord` 完整序列化后的 JSON，主要字段：

| 字段              | 类型              | 备注                                                                 |
| ----------------- | ----------------- | -------------------------------------------------------------------- |
| `app.name`        | string            | qa.yaml 里的 `name`                                                  |
| `app.version`     | string \| null    | qa.yaml 里的 `version`                                               |
| `app.description` | string \| null    | qa.yaml 里的 `description`                                           |
| `app.order`       | number \| null    | 可选排序 key，仅影响 `ins app list` 展示顺序                         |
| `app.author_name` | string \| null    |                                                                      |
| `app.author_email`| string \| null    |                                                                      |
| `app.dependencies`| string[]          | 声明的依赖 service 名                                                |
| `app.volumes`     | string[]          | 逻辑卷名列表                                                         |
| `app.all_volume`  | boolean           | 为 true 时注入节点上所有卷                                           |
| `app.values[]`    | object[]          | 所有 qa.yaml `values[]` 条目，每个包含 `name/type/value/default/...` |
| `app.files[]`     | object[]          | app 目录同级的其他文件/子目录列表                                    |
| `app.before`      | `{shell, script}` | before hook 配置                                                     |
| `app.after`       | `{shell, script}` | after hook 配置                                                      |

### 1.2 `vars`

对每个 `values[i]` 条目：

- `vars.<name>` — 解析后的**实际值**（`value` > `default` > prompt）
- `vars.<name>_meta` — 原始条目加一个 `resolved` 字段，结构如下：

```json
{
  "name": "port",
  "type": "number",
  "description": "...",
  "default": 3306,
  "value": null,
  "options": [...],
  "resolved": 3306
}
```

常用写法：

```jinja
image: nginx:{{ vars.image_tag }}
ports:
  - "{{ vars.port | default(80) }}:80"
{% if vars.enable_tls %}
  - "443:443"
{% endif %}
```

### 1.3 `volumes`

只包含 qa.yaml `volumes:` 列表中、且在 `ins volume` 里已配置过该节点的卷。每条形如：

```json
{
  "docker_name": "ins_mysql_data",
  "driver": "local",
  "driver_opts": {
    "type": "none",
    "o": "bind",
    "device": "/home/mysql/data"
  }
}
```

- `volumes.<name>.docker_name` — 实际的 docker volume 名，通常是 `ins_<name>`
- `volumes.<name>.driver` — docker driver 类型（一般是 `local` 或 `cifs`）
- `volumes.<name>.driver_opts.*` — 传给 docker 的 mount 参数

> 注意：`docker-compose.y(a)ml(.j2)` 里**不需要**自己写 `volumes:` 顶层块，`ins` 会根据 `qa.yaml` 的 `volumes` 列表自动追加 `external: true, name: ins_<name>` 的条目。服务级的 `volumes:` 直接引用逻辑名即可。

### 1.4 `service`

部署时的 service 名字符串。通常用不到，需要区分同一 app 的多个实例时才有价值：

```jinja
labels:
  com.example.service: "{{ service }}"
```

### 1.5 `namespace`

当前部署的 namespace 字符串（默认 `default`）。可用于在生成文件里标注当前归属：

```jinja
# generated for {{ app.name }} ({{ namespace }})
```

### 1.6 `node`

部署目标节点的基本信息：

| 字段             | 类型     | 备注                                                                                      |
| ---------------- | -------- | ----------------------------------------------------------------------------------------- |
| `node.name`      | string   | 节点名。本地节点固定为 `local`                                                            |
| `node.ip`        | string   | 节点 IP。本地节点固定为 `127.0.0.1`                                                       |
| `node.extern_ip` | string   | 节点的对外可访问 IP / 域名。本地节点取 `[defaults].local_extern_ip`；远程节点等于 `node.ip` |

不暴露 `port` / `user` / `password` / `key_path`（前两个用处不大，后两个是凭证不能泄漏到模板里）。需要 `INS_NODE_NAME` 环境变量请见 [env-vars.md](./env-vars.md)。

**注意**：本地节点部署时，`config.toml` 的 `[defaults] local_extern_ip` 必须事先配置；未配置会直接报错并提示在哪个文件加哪个 key。远程节点不需要这一项，`extern_ip` 自动等于 `node.ip`。

```jinja
# 生成的容器镜像里写一句"我从哪个节点来的"
LABEL ins.deployed_to_node="{{ node.name }} ({{ node.ip }})"
# 对外公开的访问地址（本地节点取 config.toml 的 local_extern_ip）
ENV PUBLIC_HOST="{{ node.extern_ip }}"
```

---

## 2. 函数

### 2.1 `system_info()`

远程节点的系统信息，通过一次 SSH（或本地 shell）探测得到。**懒执行 + 按部署缓存**：同一次 `deploy` 中多次调用只执行一次 SSH；模板里不写就永不触发。

| 返回字段              | 示例                        | 来源命令                                  |
| --------------------- | --------------------------- | ----------------------------------------- |
| `.os`                 | `"Linux"`                   | `uname -s`                                |
| `.arch`               | `"x86_64"` / `"aarch64"`    | `uname -m`                                |
| `.kernel`             | `"5.15.0-25-generic"`       | `uname -r`                                |
| `.hostname`           | `"node1"`                   | `hostname`                                |
| `.cpus`               | `"8"` (字符串)              | `nproc`                                   |

用法示例：

```jinja
image: "myapp:{% if system_info().arch == 'aarch64' %}arm64{% else %}amd64{% endif %}"
```

### 2.2 `gpu_info()`

GPU 探测（目前仅支持 NVIDIA）。同样**懒执行 + 缓存**。

| 返回字段       | 示例                                        | 说明                                    |
| -------------- | ------------------------------------------- | --------------------------------------- |
| `.vendor`      | `"nvidia"` / `"none"`                       | 无 GPU 或无 `nvidia-smi` 时返回 `none`  |
| `.count`       | `2`                                         | 数字                                    |
| `.models`      | `["NVIDIA A100 80GB PCIe", ...]`            | 数组                                    |
| `.driver`      | `"550.54.15"` / `null`                      | 驱动版本                                |

用法示例：

```jinja
{% if gpu_info().count > 0 %}
    environment:
      NVIDIA_VISIBLE_DEVICES: all
      CUDA_VERSION: "{{ gpu_info().driver }}"
{% endif %}
```

### 2.3 失败/超时行为

两个函数都有 10 秒 SSH 超时；出错时返回空对象（`system_info`）或 vendor=`none` 的默认结构（`gpu_info`），模板里用 `| default(...)` 过滤器做兜底。

---

## 3. `qa.yaml` 中的环境变量替换

这是和模板**独立的**一层：`qa.yaml` 自身被加载前会做一次 shell 风格的变量替换，然后再交给 YAML 解析器。

语法：

- `${VAR}` — 必须存在；未设置时会报错
- `${VAR:-fallback}` — 未设置则使用 `fallback`
- `$$` — 字面量 `$`
- `$foo`（无大括号） — 原样保留，方便与 Jinja 的 `{{ }}` 混用

查找顺序：

1. `config.toml` 里 `[nodes.<n>.env]`（仅 pipeline 路径）
2. `config.toml` 里 `[defaults.env]`
3. 进程环境变量
4. `${VAR:-fallback}` 中的 fallback
5. 全部落空 → 加载报错

示例：

```yaml
# qa.yaml
values:
  - name: mysql_password
    type: string
    default: "${MYSQL_PWD:-Iflyssemysql!#2023}"
```

实现位置：[src/app/parse.rs](/root/workspace/master/ins/src/app/parse.rs)（`expand_env_vars`）

---

## 4. Provider 环境变量（Jinja 模板里**不可见**）

下列变量只会被注入到 `docker compose` 运行时和 `before.sh` / `after.sh` 钩子，在 `.j2` 模板里**不能直接引用**（模板渲染发生在 provider 启动之前）。

| 变量                            | 值                                                                 |
| ------------------------------- | ------------------------------------------------------------------ |
| `INS_APP_NAME`                  | 当前 app 的 `name`                                                 |
| `INS_SERVICE_NAME`              | 当前部署的 service 名                                              |
| `INS_NODE_NAME`                 | 节点名（`local` 或 remote 的 `name`）                              |
| `INS_NAMESPACE`                 | 当前部署的 namespace（默认 `default`）                             |
| `INS_VERSION`                   | ins 版本号                                                         |
| `<VALUE_NAME>`                  | 每个 qa.yaml value 对应的解析结果（name 会被大写、非字母数字转 `_`）|
| `INS_SERVICE_<DEP>_*`           | `dependencies[]` 里每个已安装依赖 service 的元信息（hybrid namespace 规则） |
| `[defaults.env]` / `[nodes.<n>.env]` | `config.toml` 中用户配置的环境变量                            |

详见 [qa-yaml-dependencies-env.md](./qa-yaml-dependencies-env.md)。

---

## 5. 完整示例

`app/mysql/docker-compose.yaml.j2`：

```yaml
services:
  mysql:
    image: "mysql:{{ vars.image_tag | default('8.0') }}-{{ system_info().arch | default('amd64') }}"
    environment:
      MYSQL_ROOT_PASSWORD: "{{ vars.mysql_password | default('changeme') }}"
      TZ: "{{ vars.timezone | default('Asia/Shanghai') }}"
    ports:
      - "{{ vars.port | default(3306) }}:3306"
    volumes:
      - mysql_data:/var/lib/mysql          # volumes.mysql_data 由 ins 自动生成 top-level block
      - ./my.cnf:/etc/mysql/my.cnf:ro      # my.cnf.j2 渲染后的文件
    {% if gpu_info().count > 0 %}
    runtime: nvidia                         # 有 GPU 时才加
    {% endif %}
    labels:
      ins.service: "{{ service }}"
```

`app/mysql/qa.yaml`：

```yaml
name: mysql
order: 10
volumes:
  - mysql_data
values:
  - name: mysql_password
    type: string
    default: "${MYSQL_PWD:-Iflyssemysql!#2023}"
  - name: port
    type: number
    default: 3306
  - name: image_tag
    type: string
    default: "8.0"
```

运行 `ins check --node <node> mysql` 会在渲染前打印出当前的 `app` / `vars` / `volumes` / `service` 值，以及 `system_info() / gpu_info()` 的探测结果，方便排查。
