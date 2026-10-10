# 配置 xsos

首次安装使用[部署手册](server-release-readme.md)的最小配置。修改生产参数时编辑 `/etc/isarmg/xsos.env`，随后重启服务并检查 `/readyz`。

## 环境变量



产品运行参数使用 `XSOS_` 前缀。下表的普通变量名省略此前缀；`XSOC_AUTHORIZATION_KEY` 和共享开发变量 `XCSS_DEV_WEB_DIR` 列出完整名称，按原样使用。

| 变量 | 默认/要求 | 说明 |
|---|---|---|
| `DATABASE_URL` | 必填 SQLite URL | 生产例 `sqlite:///var/lib/isarmg/xsos/db/xsos.sqlite3` |
| `BIND` | `127.0.0.1:18105` | 非开发模式必须保持安全部署边界 |
| `XCSS_DEV_WEB_DIR` | 开发可选 | 仅未绑定开发构建可选目录热更新；正式 Web 嵌入可执行文件，不允许目录覆盖 |
| `DEVELOPMENT` | `false` | 仅本机开发可开启 |
| `BOOTSTRAP_ADMIN_USERNAME` | `admin` | 仅在显式 `init` 为全新实例创建首个管理员；按 xcss 规则规范化，不是 email，也没有旧变量别名 |
| `BOOTSTRAP_ADMIN_PASSWORD` | 仅显式 `init` 必填 | 12..1024 字节且无 ASCII control；创建后保存 xcss 当前 Argon2id hash，不保存明文 |
| `XSOC_AUTHORIZATION_KEY`（完整名称） | 必填 | 标准 Base64 编码的 32 个随机字节；生成一次并持久保存，用于加密每实例长期授权码；这是服务端保存的客户端实例授权码密钥 |
| `TELEMETRY_QUEUE_CAPACITY` | 256，最大 1024 | 内存报告队列 |
| `TELEMETRY_BATCH_SIZE` | 64，范围 1..min(512, queue) | 单事务候选报告数 |
| `TELEMETRY_FLUSH_MILLISECONDS` | 25，范围 1..1000 | 低流量 batch 最长聚合等待 |
| `TELEMETRY_ENQUEUE_WAIT_MILLISECONDS` | 10，范围 1..250 | HTTP 请求等待进入 writer queue 的预算 |
| `TELEMETRY_REQUEST_TIMEOUT_MILLISECONDS` | 10000，范围 100..30000 | 从提交到 writer 回应的总预算；必须大于 enqueue + flush |
| `TELEMETRY_SHUTDOWN_DRAIN_MILLISECONDS` | 15000，范围 100..60000 | Server 关机时 writer drain 期限 |
| `RAW_RETENTION_DAYS` | 7，范围 1..365 | 原始报告保留；latest 指向的 raw 行不被清理 |
| `AGGREGATE_RETENTION_DAYS` | 365，最大 3650 | 小时聚合保留，必须严格大于 raw |
| `RETENTION_INTERVAL_SECONDS` | 300，范围 1..86400 | 维护周期；进程启动后也会先运行一次 |
| `RETENTION_BATCH_SIZE` | 256，范围 1..512 | 单个保留事务的有界行批次 |
| `RETENTION_MAX_TRANSACTIONS_PER_RUN` | 12，范围 3..30 | 每轮聚合/删 raw/删 aggregate 的总事务预算 |
| `RETENTION_MAX_RUN_MILLISECONDS` | 2000，范围 100..10000 | 单轮时间预算，并且必须短于 maintenance interval |
| `RETENTION_YIELD_MILLISECONDS` | 10，范围 1..100 | 相邻维护事务之间主动让出执行权的时间 |

例如表中的 `DATABASE_URL` 对应 `XSOS_DATABASE_URL`，而 `XSOC_AUTHORIZATION_KEY` 和 `XCSS_DEV_WEB_DIR` 不再追加 `XSOS_`。程序接受命令行、显式环境映射、当前 JSON 配置和默认值，但不直接解析
`/etc/isarmg/xsos.env` 文本文件；该文件由 systemd 的 `EnvironmentFile` 加载为进程环境。未知环境变量不会被
Server 拒绝，因此应通过配置管理审查拼写，不能把“进程能启动”当作未知变量已生效。

管理员 username 的精确合同是：登录候选 1..64 字节 printable ASCII；trim ASCII whitespace、ASCII
lowercase 后，canonical 值必须为 3..64 字节，首尾是 `[a-z0-9]`，字符仅 `[a-z0-9._-]`，禁止 `@`，
允许相邻分隔符。数据库只存 canonical `username`。`admin` 是默认值，固定 `admin` 的是 role；系统没有
viewer/operator/RBAC。普通 `run` 逐行验证已存在的管理员 username 与当前 Argon2id hash，不创建或覆盖账户；坏行会阻止 `run`/`config validate`，`config validate` 同时验证 Schema/integrity/FK、管理员行与授权凭据，可用于离线检查。


## 修改后验证

使用[运维命令](administration.md)以服务账户执行 `config validate --json`，检查字段来源和状态路径，再启动服务。JSON 文件示例与 CLI 优先级见[命令参考](cli.md)。

`XSOS_DATA_DIR` 可指定数据目录，数据库须为该目录的直接子文件；默认安装从 `XSOS_DATABASE_URL` 定位到 `/var/lib/isarmg/xsos/db`。初始化密码仅在首次 init 使用；实例授权码密钥长期保留。
