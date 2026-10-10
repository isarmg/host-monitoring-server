# xsos 运维文档

## 1. 服务端部署布局

Server 的唯一正式平台/target 是 x86_64 glibc Linux / `x86_64-unknown-linux-gnu`。ARM Linux、musl、
Windows 和 macOS 只可能属于 Client 交付，不得部署 `xsos`。

```text
/opt/isarmg/xsos/releases/1.0.0/   root 持有、只读发行树
/etc/isarmg/xsos.env              0600 生产配置
/var/lib/isarmg/xsos/db/xsos.sqlite3    SQLite 当前数据库
/run/isarmg/xsos/                 systemd runtime
```

systemd 以 `xsos` 运行：

```text
ExecStart=/opt/isarmg/xsos/current/bin/xsos \
  run --release-root /opt/isarmg/xsos/current
```

安装目录允许唯一受控的 `current` 指针；仅 `run` 按当前版本实体路径解析。发行树不能由服务账户、group 或 world 写入，也不能包含 symlink、特殊
文件或硬链接别名。

## 2. 构建 Server 发行物

在 x86_64 glibc Linux 上，从干净、annotated `v1.0.0` 精确指向 HEAD 的 checkout，向仓库外已存在目录
构建：

```bash
python3 scripts/package-server-release.py /absolute/output-directory
```

脚本先拒绝其他 OS、架构或 libc，再以显式 `--target x86_64-unknown-linux-gnu` 构建 Web 和 Rust、写严格
manifest、生成 deterministic archive/checksum，随后解包、重定位、真实启动、读取 hashed asset，并执行
篡改拒绝。已有归档或 checksum 不会被覆盖。`build.rs` 还会拒绝非目标编译，二进制在读取配置、打开
SQLite 或监听端口前通过 `uname` 再确认 Linux/x86_64；三层检查均为 fail-closed。

当前 Server Rust 固定 xcss 1.0.0 / `627d988a4ed471469ed4fdce8af0ea6b5c131ce6`，一个 @xcss/web 包使用
同版 Release tarball 与准确 SHA-512 integrity，无相邻 xcss 路径依赖。本轮使用封存的本地候选 Git 对象和真实 tarball 验证；远端发布与独立 CI 仍须以当前精确提交的正式证据核对，
见[本项目当前 CI](https://github.com/isarmg/xsos/actions)与[正式发行资产](https://github.com/isarmg/xsos/releases)。
React/Vite/TypeScript 基线与配置由 web-toolchain 维护；登录、Session、退出、主题和全局错误由共享 Shell 维护。
独立构建通过不等于当前主分支改动已进入产品 Release；发行仍须核对精确 tag、源码和全部门禁，不改写旧资产。
xcss 变更必须显式发布新版本并替换当前合同，同时通过 Host 的 Rust 全矩阵、Web clean build、
SQLite reopen 与 Router→Client 合同测试，使用当前严格响应合同。

当前 React 管理台以实例列表和实例详情为主线：列表提供完整实例集合、同页监控摘要、长期授权码和
新建实例；创建成功后窗口立即关闭。详情每两秒自动读取同一快照的完整最新报告，页面隐藏时停止轮询、返回页面后恢复，
并提供 15 分钟至 30 天的有界自动粒度趋势。授权码轮换会撤销旧 Client credential，客户端必须重新配对。
当前没有 audit 查询界面；所保存的原始报告可在实例「日志」页按服务器接收日期查看，保留期限见下文。
`cd web && npm run test:browser` 对实际生产构建执行 Chromium/Firefox 实例列表、指标详情、移动主题与 WCAG AA 验收；
首次运行需 `npx playwright install --with-deps chromium firefox`。该测试的 API 全部由本机测试数据拦截，不访问真实 Client。

## 3. Server 配置

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

例如表中的 `DATABASE_URL` 对应 `XSOS_DATABASE_URL`，而 `XSOC_AUTHORIZATION_KEY` 和 `XCSS_DEV_WEB_DIR` 不再追加 `XSOS_`。程序只读取环境，
不会解析 `/etc/isarmg/xsos.env` 文件；该路径是 systemd unit 的部署合同。未知环境变量不会被
Server 拒绝，因此应通过配置管理审查拼写，不能把“进程能启动”当作未知变量已生效。

管理员 username 的精确合同是：登录候选 1..64 字节 printable ASCII；trim ASCII whitespace、ASCII
lowercase 后，canonical 值必须为 3..64 字节，首尾是 `[a-z0-9]`，字符仅 `[a-z0-9._-]`，禁止 `@`，
允许相邻分隔符。数据库只存 canonical `username`。`admin` 是默认值，固定 `admin` 的是 role；系统没有
viewer/operator/RBAC。普通 `run` 逐行验证已存在的管理员 username 与当前 Argon2id hash，不创建或覆盖账户；坏行会阻止 `run`/`config validate`，`config validate` 同时验证 Schema/integrity/FK、管理员行与授权凭据，可用于离线检查。

### 3.1 Server HTTP 面与身份矩阵

Server 自身只监听 HTTP socket；正式 HTTPS、证书与外部连接限制由可信 reverse proxy 负责。代理必须保留
浏览器发送的单一 `Host`、`Origin` 与 `Sec-Fetch-Site` 事实，不能注入第二个同名字段，也不能把公网请求
直接转给一个可被旁路访问的监听地址。登录来源限流读取 Axum 的 TCP peer `ConnectInfo`，当前不信任
`Forwarded` 或 `X-Forwarded-For`；如果代理复用一个后端源地址，来源桶看到的是代理地址而非公网客户端。

| 路径与方法 | 当前调用方/身份 | 请求边界 | 成功结果与当前限制 |
|---|---|---|---|
| `GET /healthz` | 公开 | 空响应体 | 存活 204，不健康 503 |
| `GET /readyz` | 公开 | 仅最小就绪事实 | `200/503`，精确 `{"ready":bool}`；任务与数据库详情仅在受保护诊断中提供 |
| `POST /api/v1/auth/login` | 浏览器公开入口 | 16 KiB；exact `{username,password}`；同源；TCP peer 与规范 username 双重限流 | `200` + exact Session；设置 Cookie；不知道账户时仍做 dummy Argon2；成功响应 `no-store` |
| `GET /api/v1/auth/session` | 管理员 Session Cookie | 不接受业务正文；不要求 CSRF | 轮换一个 CSRF token 并返回 exact Session；成功响应 `no-store` |
| `POST /api/v1/auth/logout` | 管理员 Session + CSRF + 同源 | 无业务正文 | 撤销当前 Session、删除其 CSRF 摘要、清除 Cookie；成功响应 `204 no-store` |
| `GET /api/v1/monitoring/hosts` | 管理员 Session | 无查询参数 | 按实例名称字母数字顺序返回全部 Host summary；React 总览与实例/详情入口共同使用 |
| `GET /api/v1/monitoring/hosts/{host_id}` | 管理员 Session | canonical UUID | Host summary 与可空 latest 原始报告；当前 Web 详情使用列表中的同一投影，端点供独立调用方精确读取 |
| `GET /api/v1/monitoring/hosts/{host_id}/history` | 管理员 Session | 原始模式使用 `from/to/limit`；图表模式使用 `from/to/resolution=auto/max_points`，跨度最多 31 天、点数 100..1000 | 原始点，或在同一快照内无重复合并 raw 与 hourly 的时间桶；响应明确粒度、来源和实际对齐范围 |
| `GET/POST /api/v1/monitoring/client-instances` | 管理员 Session；POST 另需 CSRF/同源 | 管理路由组正文上限 16 KiB；POST exact `display_name?`，省略时使用默认名称；授权码不设有效期 | 返回完整实例列表和可查看的实例授权码；新建 `201`；成功响应 `no-store` |
| `PUT /api/v1/monitoring/client-instances/{request_id}/authorization` | 管理员 Session + CSRF + 同源 | canonical UUID；exact `authorization_code`，36 位小写英文字母或数字 | 更新加密密文/摘要、撤销旧 Client credential，并将实例恢复为 pending；Client 需重新配对 |
| `DELETE /api/v1/monitoring/client-instances/{request_id}` | 管理员 Session + CSRF + 同源 | canonical UUID；pending 首次调用转 cancelled，cancelled 再次调用永久删除 | `204`；不存在为 404，active 为 409；Web 分别显示“取消配对”和“删除实例” |
| `POST /api/v1/xsoc/activate-admin` | 管理员 Session + CSRF + 同源 | 16 KiB 管理上限；exact request ID + activation code | 与 capability 激活进入同一事务；React 配对确认流程调用 |
| `PATCH/DELETE /api/v1/monitoring/managed-instances/{host_id}` | 管理员 Session + CSRF + 同源 | canonical UUID；PATCH remark trim 后 1..255 UTF-8 bytes | `204`；PATCH 是 last-write-wins，无 ETag/revision；DELETE 永久级联删除且没有产品内恢复 |
| `POST /api/v1/xsoc/pairing-requests` | 未配对 Client | Client 路由组 512 KiB；strict Host、bearer/polling-secret SHA-256；来源/设备/容量限流 | 创建或幂等恢复 pairing request；返回 activation URL，成功 `no-store` |
| `GET /api/v1/xsoc/pairing-requests/{request_id}` | 持有 request ID 的调用方 | canonical UUID；来源/请求限流 | 只暴露 OS/arch/version/status/expiry 公共摘要；成功 `no-store` |
| `POST /api/v1/xsoc/pairing-requests/{request_id}/status` | Client 的 `Pairing <polling_secret>` | 512 KiB 组上限；secret 32..256 字符且无 whitespace | waiting/active/denied/expired 与可空 instance ID；成功 `no-store` |
| `POST /api/v1/xsoc/activate` | 持有实例 authorization code 的 capability 调用方 | 512 KiB 组上限；来源/请求限流；不是管理员 Session | 激活同一事务状态机；授权码错误/取消/跨实例使用严格失败；短期设备请求仍有超时保护；成功 `no-store` |
| `POST /api/v1/xsoc/report` | `Bearer <client credential>` | 512 KiB；单份 strict report；每 Host 速率桶 | 持久事务提交后才返回 `202`；同 Host 同 ID 重放 `accepted=false` |

登录与管理写操作的同源裁决会把所有原始 `Origin`、`Host`/HTTP/2 authority、`Sec-Fetch-Site` 值交给
xcss；重复、冲突或非当前形状 fail closed。生产 Cookie 名是 `__Host-admin-xsos-session`，带
`Path=/; Secure; HttpOnly; SameSite=Strict` 且没有 Domain；开发模式改用非 Secure 的 `admin-xsos-session`，但
配置层强制监听 loopback。Session token 和 CSRF token 都是 32-byte 随机值，只以 SHA-256 摘要入库；
Session 同时受 idle/absolute TTL、账户 active 与 `session_version` 约束，每个 Session 只保留当前
CSRF 摘要；恢复会话时轮换，不保留旧 token 的兼容窗口。

所有 `/api` 的 4xx/5xx（包括 JSON extractor、body 过大、方法错误与未知 API 路径）都会规范为
xcss `ErrorEnvelope`；健康端点和静态文件不在这个 envelope 范围。当前仅部分敏感成功响应显式
设置 `Cache-Control: no-store`，不能把它扩大解释为所有管理 GET 都由 Server 响应头禁止缓存。

### 3.2 数据库锁矩阵

数据目录使用公共实例锁与维护锁，数据库还保留自身的锁身份检查。锁文件是 0600 regular file，拒绝 symlink、路径穿越和多硬链接。配置要求数据库是数据目录的直接子文件，保证这些锁协调同一份状态。它们是并发协调，不是数据备份或 transaction journal。

| 操作 | instance lock | maintenance lock | 能否与运行中 Server 并行 |
|---|---|---|---|
| `run` / `run --release-root` | 排他，阻止第二个 Server | 共享，持有到 writer/retention 全部停止 | 不适用；同库只允许一个 Server |
| `init` | 不取得 | 排他 | 不可以；仅接受全新私有空目录 |
| `config validate` | 不取得 | 不取得 | 可以；读取私有验证快照，不写原库 |
| `doctor` | 不取得 | 排他 | 不可以；在原库上打开 WAL 连接，须先停止 Server |
| `admin-reset-password` | 不取得 | 排他 | 不可以；必须先停止 Server |
| `identity` / `verify-release` | 不访问数据库 | 不访问数据库 | 可以，但只证明 binary/release，不证明数据库健康 |

公共锁绑定数据目录，数据库锁身份来自规范化后的实际数据库路径；数据库本体、锁文件及其父目录仍必须满足当前文件安全检查。不要
用复制数据库到另一路径的方式绕开锁：那既不是一致快照，也不在当前支持范围。

Host 新建库在初始化事务中写入唯一的 `_common_platform_metadata` 记录。启动和 readiness
要求该记录严格匹配 `server-control-plane`；缺失或不匹配时只读拒绝，服务不补写或修复。
数据库还须满足产品 Schema identity 与 DDL 指纹检查。部署前可通过只读 `config validate --json` 验证这些条件；停服后执行 `doctor` 可检查当前连接与保留任务结构。

## 4. Server 日常命令

```bash
xsos identity
xsos verify-release --root /opt/isarmg/xsos/releases/1.0.0
xsos init --data-dir /absolute/new-private-data
xsos config validate --data-dir /absolute/current-private-data --json
xsos status --data-dir /absolute/current-private-data --json
xsos doctor
printf '%s\n' "$NEW_ADMIN_PASSWORD" | xsos admin-reset-password \
  --database-url sqlite:///path/app.db --username admin
```

`init` 从当前配置中的 bootstrap username/password 创建首个账户，只接受全新私有空目录；已有库或账户不会被覆盖。普通 `run` 只验证当前状态，不创建管理员。`admin-reset-password` 接受 `--username`，从标准输入读取一行有界密码，先规范化 username，再写新的当前
Argon2id hash；xcss SQLite 更新事务同时提升 `session_version` 并撤销该账户全部 Session。
`doctor` 和管理员密码重置要求 maintenance 排他锁，因此应先停止运行实例。

reset CLI 不从 argv 读取密码。不要把真实密码字面量写进可持久 Shell history、脚本、工单或日志。首次创建
完成后从长期环境文件移除 bootstrap 明文密码。管理 Web 只允许一个管理员，不提供创建管理员入口。账号名称与密码通过右上角人物图标修改。

## 5. Client 配置与诊断

Client 配置、命令、网络行为和 xcss 依赖由独立 Client 仓库维护，见
[配置与诊断](https://github.com/isarmg/xsoc/blob/main/docs/configuration.md)。
Client 的产品 transport 使用 reqwest；HTTPS、响应读取和协议分类规则以该仓库的实现和测试为准。
只读诊断与实际投递检查是不同操作，执行命令前按所安装 Client 的版本文档确认其副作用。

## 6. Linux 客户端

Linux 客户端 的 deb/rpm、systemd、专用账户、配置权限和卸载流程见
[平台安装](https://github.com/isarmg/xsoc/blob/main/docs/platform-setup.md)。
构建与安装命令在 Client 仓库执行。

## 7. Windows 客户端

Windows 客户端 的 MSI、交互 CLI、Windows Service 和权限提升流程由 Client 维护，见
[平台安装](https://github.com/isarmg/xsoc/blob/main/docs/platform-setup.md)。
Client 与 Server 版本独立，不使用 Server 版本号代替 MSI 的发行版本。

## 8. macOS 客户端

macOS 客户端 的 pkg、LaunchDaemon、日志、账户与卸载流程见
[平台安装](https://github.com/isarmg/xsoc/blob/main/docs/platform-setup.md)。
原生包签名与平台验收证据随对应 Client 发行物记录。

## 9. 当前数据库身份

Server 仅在显式 `init` 创建当前库。`product_metadata` 必须精确绑定 application `xsos`、格式版本 `1.0.0`、
schema revision `1` 与 SHA-256
`cb768892031e80900b95395aae25397fac4c37f3aac8b912d911d65c6a277afe`；软件补丁版本由发行身份中的 `version` 独立表达，现场 `sqlite_schema` 重新计算也
必须一致。当前 DDL 中管理员列是 `_common_administrators.username`，没有 `email` 或 role 列；DDL 自身约束 canonical
username、非空 password hash、`active IN (0,1)`，`run`/`config validate` 加载已有行时再用 xcss
primitive 验证 username 和完整 current Argon2id 参数；DDL 还要求 `session_version > 0`，形成存储形状与
密码策略双层 fail-closed。数据库/
父目录/锁的
链接、特殊文件和硬链接
别名在 Linux 通过 `openat2` 锚定检查。`doctor` 还通过 xcss 适配器执行完整
`PRAGMA integrity_check` 与 `foreign_key_check`；失败只报告 degraded 并退出，不在产品内修库。



当前 retention worker 只处理 raw report 与 hourly aggregate。`audit_events` 没有读取、导出或清理 API；
过期/撤销的 `auth_sessions` 行没有全局清理 worker；pairing 只有创建新 pairing 时针对 expired
pending/旧 denied 的有界清理和删除 Host 时的定向清理。长期实例必须把这些控制面表的增长视为已知
运维缺口，不能误以为 `RAW_RETENTION_DAYS` 会覆盖它们，也不能在没有新合同/测试时手工删行。

保留任务在仍有积压时按一秒下限继续有界批次，空闲后回到配置的巡检周期；失败指数退避，连续三次失败会让 `retention-worker` 健康检查降级。已经计入小时聚合的 raw 行仍按首次 `received_at` 至少保留一个 raw retention 窗口，因此确认丢失后的同 ID 重试在窗口内继续返回 `accepted=false`。窗口到期后的旧 ID 不承诺永久去重。

图表查询的 `actual_from/actual_to` 会向外对齐时间桶，覆盖包含小数秒的请求范围，并按左闭右开区间取数。
范围与小时聚合桶重叠时，粒度至少为一小时；即使该小时只有少量样本，也按完整小时对齐实际范围。
`source` 根据实际对齐范围内选中的 raw/hourly 数据确定；已计入聚合的 raw 行不会再次计数。

## 10. 监控与故障处理

Client 本机状态、权限、凭据事务、TLS 材料、会话锁与采集诊断见
[Client 配置与诊断](https://github.com/isarmg/xsoc/blob/main/docs/configuration.md)。
Server 运维先区分摄取身份错误、资源饱和、数据库健康及 Client 采集/传输故障。

1. 检查 Server 的 systemd，以及 Client 所在平台的 systemd/Windows Service/LaunchDaemon 状态和最近日志。
2. Server 检查 `/healthz`、`/readyz`；writer 停止时 readiness 必须失败。
3. 查看 429/503 与 `Retry-After`，区分准入限流、队列饱和和 writer 故障。
4. 查看严格错误 `code`：`unauthorized` 表示当前 credential 已失效并需显式重新配对；
   `client_host_mismatch` 表示该报告 Host 与 credential 绑定不一致，只丢弃该报告。不能按 `message` 分支。
5. Client 查看 `status`、spool 数量、当前 active binding、TLS 和系统时间。
6. 运行 Server/Client doctor；Schema 不符时停止服务并保全原件。当前没有 Host 转换边，不要直接调用
   外部通用引擎尝试处理。
7. 容量规划同时监控 SQLite、WAL、spool、磁盘空间和 inode。

## 11. 安全事件与报告

先隔离公网入口和受影响 Client，保全只读日志、发行摘要、数据库四元 identity、SQLite/WAL 文件现场与
状态目录权限，再轮换
管理员、Client、mTLS、OTLP 等凭据。使用 GitHub Private Vulnerability Reporting；公开 issue 不得
包含生产遥测、主机标识、凭据或复现 Secret。安全支持仅覆盖当前发布版本和当前 `main`。

初次安装须先验证 `/opt/isarmg/xsos/releases/1.0.0`，再在同一所有者、实体 `0755` 安装目录与 `releases` 目录下创建绝对链接 `current` 指向该版本。服务执行 `current/bin/xsos run --release-root /opt/isarmg/xsos/current`。首个管理员与数据库仍须通过 `init` 显式创建。

主机、实例与单日上报日志按固定最多 50 条的 keyset 页浏览，支持首页、上一页、下一页以及直接定位实例。游标最长 512 字节，绑定资源、管理员、主机与日期；游标不授予权限。页面仅保留当前页，日期变化、刷新与实例选择重新定位，既有历史仍可逐页访问。全局状态统计由 SQLite 汇总，不加载所有主机。能力较多的合法主机可能使一页少于 50 条；下一页仍能继续访问全部记录。损坏的主机元数据以 `unavailable` 和 `stored_host_data_invalid` 显示，不改写原记录，也不隐藏其它实例。

这些读取共用 4 个准入名额，每个资源种类与管理员、或每台主机的报告读取只允许一个。连接等待最多 2 秒，SQLite 语句共享 3 秒原生执行预算，HTTP 最多等待 5 秒，序列化响应不超过 8 MiB。取消请求不会提早释放仍在工作的数据库名额；名额也覆盖响应正文的传输。单行文本、能力 JSON 与 latest 报告在 SQL 返回前按协议预算截取或标明不可用。此处限制的是服务返回和查询生命周期；对人为改坏的超大 SQLite 单元格，SQLite 读取内部所需的分配仍取决于底层库。

持久写入在同一 SQLite 写事务内先准入：实例邀请最多 4096 条，配对请求保留总量最多 8192 条，原始报告每台主机最多 100000 条、全部主机最多 1000000 条。每台主机原始行与 current 报告按保守 main/WAL 预算不超过 1 GiB；数据库与 WAL 总量预算为 8 GiB，新写入还须留出 1 GiB 可用磁盘与 64 MiB 管理操作余量。上限不会清空既有数据；原始报告和小时聚合继续按既有保留设置清理，已提交回执的重试在保留窗口内继续读取原结果。容量不足时拒绝新邀请或新报告，报告返回可重试 503，建议 60 秒后重试。配对容量不足使用既有容量拒绝响应。

每台主机同时在队列或提交中的报告最多 16 份，该额度直到实际事务结束才释放，取消 HTTP 等待不会移除已经准入的写入。全局队列与批大小仍受既有配置硬上限约束。已排队的报告也必须通过事务内容量检查后才能获成功回执；未提交的写入不会返回成功。管理保留余量支持正常管理操作，不承诺磁盘故障或外部写入耗尽空间后的持久写入成功。

## 当前中立接口与旧版数据处理

当前版本只使用 `.state-instance.lock`、`.state-maintenance.lock`、`.state-maintenance-pending.json` 和 `.state-atomic-` 临时文件前缀；离线升级工具采用 `.release-upgrade` 工作目录。服务身份头为 `x-service`，健康状态中的公共源码修订字段为 `common_revision`。管理会话采用 `__Host-admin-xsos-session`，显式开发模式采用 `admin-xsos-session`；生产 Cookie 的 Secure、HttpOnly、SameSite、Path 和 CSRF 约束继续生效。资源清单格式为 `web-assets-v1`，公共数据库内部表及索引采用 `_common_` 前缀。

这些接口没有旧名称别名或旧版兼容分支。旧版升级前，先按本文的停服步骤停止服务、配套客户端及全部维护工具；确认全部进程退出后，完整备份配置、SQLite 数据库及其 WAL/SHM、业务文件和必要的私有凭据。备份包含敏感数据，应保留原有访问权限并离线保存。

保留旧数据目录，按当前安装步骤配置新的私有数据目录，执行显式 `init` 初始化，随后运行 `config validate`，再启动服务、登录管理页面并重新配对客户端。旧配置应人工审阅后填写当前字段，不能整体覆盖新目录。旧业务数据需要另行处理；当前版本不提供自动迁移。不得让旧、新版本同时写同一目录，不得通过删锁文件或修改数据库 metadata 强制启动；当前结构指纹包含实际表名、索引名和 SQL，仅改名称不能证明数据符合当前合同。
