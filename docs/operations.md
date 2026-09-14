# Host Monitoring 运维文档

## 1. 服务端部署布局

Server 的唯一正式平台/target 是 x86_64 glibc Linux / `x86_64-unknown-linux-gnu`。ARM Linux、musl、
Windows 和 macOS 只可能属于 Client 交付，不得部署 `host-monitoring-server`。

```text
/opt/isarmg/host-monitoring/releases/0.9.12/   root 持有、只读发行树
/etc/isarmg/host-monitoring.env              0600 生产配置
/var/lib/isarmg/host-monitoring/db/host-monitoring.sqlite3    SQLite 当前数据库
/run/isarmg/host-monitoring/                 systemd runtime
```

systemd 以 `isarmg-host` 运行：

```text
ExecStart=/opt/isarmg/host-monitoring/releases/0.9.12/bin/host-monitoring-server \
  serve-release --root /opt/isarmg/host-monitoring/releases/0.9.12
```

不创建 `current` 或 `latest`。发行树不能由服务账户、group 或 world 写入，也不能包含 symlink、特殊
文件或硬链接别名。

## 2. 构建 Server 发行物

在 x86_64 glibc Linux 上，从干净、annotated `v0.9.12` 精确指向 HEAD 的 checkout，向仓库外已存在目录
构建：

```bash
python3 scripts/package-server-release.py /absolute/output-directory
```

脚本先拒绝其他 OS、架构或 libc，再以显式 `--target x86_64-unknown-linux-gnu` 构建 Web 和 Rust、写严格
manifest、生成 deterministic archive/checksum，随后解包、重定位、真实启动、读取 hashed asset，并执行
篡改拒绝。已有归档或 checksum 不会被覆盖。`build.rs` 还会拒绝非目标编译，二进制在读取配置、打开
SQLite 或监听端口前通过 `uname` 再确认 Linux/x86_64；三层检查均为 fail-closed。

当前 Server Rust 固定 Foundation 0.7.11 / `8f2a5c888bc5f543186ad58bc5b3cee4dc5b2602`，八个 Web 包使用
同版正式 Release tarball 与 SHA-512 integrity，无相邻 Foundation 路径依赖；独立 CI 已通过，
见[消费者矩阵](https://github.com/isarmg/sarmg-foundation-server/blob/main/consumers/consumer-matrix.json)。Client Foundation 是另一个独立上游，其版本不随 Server 包改写。
React/Vite/TypeScript 基线与配置由 web-toolchain 维护；登录、Session、退出、主题和全局错误由共享 Shell 维护，诊断管理功能已移除。
独立构建通过不等于当前主分支改动已进入产品 Release；发行仍须核对精确 tag、源码和全部门禁，不改写旧资产。
Foundation 变更必须显式发布新版本并替换当前合同，同时通过 Host 的 Rust 全矩阵、Web clean build、
SQLite reopen 与 Router→Client 合同测试；不保留旧版本 fallback。

当前 React 管理台以实例列表和实例详情为主线：列表提供总览、分页、CPU/内存摘要、实例简要信息和
新建实例；详情提供完整采集字段，并可查看或轮换长期授权码、完成待处理配对、取消待配对实例、修改备注
以及删除已配对实例。授权码轮换会撤销旧 Client credential，客户端必须重新配对。当前仍没有历史图表和
audit 查询界面。
`cd clients/web && npm run test:browser` 对实际生产构建执行 Chromium/Firefox 分页、指标详情、移动主题与 WCAG AA 验收；
首次运行需 `npx playwright install --with-deps chromium firefox`。该测试的 API 全部由本机测试数据拦截，不访问真实 Client。

## 3. Server 配置

所有变量使用 `HOST_MONITORING_` 前缀。核心项：

| 变量 | 默认/要求 | 说明 |
|---|---|---|
| `DATABASE_URL` | 必填 SQLite URL | 生产例 `sqlite:///var/lib/isarmg/host-monitoring/db/host-monitoring.sqlite3` |
| `BIND` | `127.0.0.1:18105` | 非开发模式必须保持安全部署边界 |
| `STATIC_DIR` | 必填 | 正式环境必须精确等于发行树 `web/` |
| `DEVELOPMENT` | `false` | 仅本机开发可开启 |
| `BOOTSTRAP_ADMIN_USERNAME` | `admin` | 仅在空 `_sarmg_administrators` 创建首个管理员；按 Foundation 规则规范化，不是 email，也没有旧变量别名 |
| `BOOTSTRAP_ADMIN_PASSWORD` | 空 `_sarmg_administrators` 时必填 | 12..1024 字节且无 ASCII control；创建后保存 Foundation 当前 Argon2id hash，不保存明文 |
| `CLIENT_AUTHORIZATION_KEY` | 必填 | 标准 Base64 编码的 32 个随机字节；生成一次并持久保存，用于加密每实例长期授权码 |
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

变量名在进程环境中必须带完整 `HOST_MONITORING_` 前缀；表中为去前缀后的可读写法。程序只读取环境，
不会解析 `/etc/isarmg/host-monitoring.env` 文件；该路径是 systemd unit 的部署合同。未知环境变量不会被
Server 拒绝，因此应通过配置管理审查拼写，不能把“进程能启动”当作未知变量已生效。

管理员 username 的精确合同是：登录候选 1..64 字节 printable ASCII；trim ASCII whitespace、ASCII
lowercase 后，canonical 值必须为 3..64 字节，首尾是 `[a-z0-9]`，字符仅 `[a-z0-9._-]`，禁止 `@`，
允许相邻分隔符。数据库只存 canonical `username`。`admin` 是默认值，固定 `admin` 的是 role；系统没有
viewer/operator/RBAC。已有任意管理员行时，bootstrap 不创建或覆盖账户，而是逐行验证 username 与当前
Argon2id hash；坏行会阻止 `serve`/`admin-create`，但 `doctor` 目前只检查 Schema/integrity/FK，不能代替
这项启动验证。

### 3.1 Server HTTP 面与身份矩阵

Server 自身只监听 HTTP socket；正式 HTTPS、证书与外部连接限制由可信 reverse proxy 负责。代理必须保留
浏览器发送的单一 `Host`、`Origin` 与 `Sec-Fetch-Site` 事实，不能注入第二个同名字段，也不能把公网请求
直接转给一个可被旁路访问的监听地址。登录来源限流读取 Axum 的 TCP peer `ConnectInfo`，当前不信任
`Forwarded` 或 `X-Forwarded-For`；如果代理复用一个后端源地址，来源桶看到的是代理地址而非公网客户端。

| 路径与方法 | 当前调用方/身份 | 请求边界 | 成功结果与当前限制 |
|---|---|---|---|
| `GET /healthz` | 公开 | 空响应体 | 存活 204，不健康 503 |
| `GET /readyz` | 公开 | 仅最小就绪事实 | `200/503`，精确 `{"ready":bool}`；任务与数据库详情仅在受保护诊断中提供 |
| `POST /api/v2/auth/login` | 浏览器公开入口 | 16 KiB；exact `{username,password}`；同源；TCP peer 与规范 username 双重限流 | `200` + exact Session；设置 Cookie；不知道账户时仍做 dummy Argon2；成功响应 `no-store` |
| `GET /api/v2/auth/session` | 管理员 Session Cookie | 不接受业务正文；不要求 CSRF | 轮换一个 CSRF token 并返回 exact Session；成功响应 `no-store` |
| `POST /api/v2/auth/logout` | 管理员 Session + CSRF + 同源 | 无业务正文 | 撤销当前 Session、删除其 CSRF 摘要、清除 Cookie；成功响应 `204 no-store` |
| `GET /api/v2/monitoring/hosts` | 管理员 Session | query 只有 `limit/offset`；服务端钳到 1..1000，默认 200 | 分页 Host summary；React 总览与实例/详情入口共同使用 |
| `GET /api/v2/monitoring/hosts/{host_id}` | 管理员 Session | canonical UUID | Host summary 与可空 latest 原始报告；当前 Web 详情使用列表中的同一投影，端点供独立调用方精确读取 |
| `GET /api/v2/monitoring/hosts/{host_id}/history` | 管理员 Session | `from/to/limit`；`from <= to`；limit 1..1000，默认 300 | 仍保留的 raw 标量点；不读取 hourly aggregate |
| `GET/POST /api/v2/monitoring/client-instances` | 管理员 Session；POST 另需 CSRF/同源 | 管理路由组正文上限 16 KiB；POST exact `display_name?`，授权码不设有效期 | 列表最多 200 条并返回可查看的实例授权码；新建 `201`；成功响应 `no-store` |
| `PUT /api/v2/monitoring/client-instances/{request_id}/authorization` | 管理员 Session + CSRF + 同源 | canonical UUID；exact `authorization_code`，当前 `uci_` 格式 | 更新加密密文/摘要、撤销旧 Client credential，并将实例恢复为 pending；Client 需重新配对 |
| `DELETE /api/v2/monitoring/client-instances/{request_id}` | 管理员 Session + CSRF + 同源 | canonical UUID；pending 首次调用转 cancelled，cancelled 再次调用永久删除 | `204`；不存在为 404，active 为 409；Web 分别显示“取消配对”和“删除实例” |
| `POST /api/v2/host-monitor/activate-admin` | 管理员 Session + CSRF + 同源 | 16 KiB 管理上限；exact request ID + activation code | 与 capability 激活进入同一事务；React 配对确认流程调用 |
| `PATCH/DELETE /api/v2/monitoring/managed-instances/{host_id}` | 管理员 Session + CSRF + 同源 | canonical UUID；PATCH remark trim 后 1..255 UTF-8 bytes | `204`；PATCH 是 last-write-wins，无 ETag/revision；DELETE 永久级联删除且没有产品内恢复 |
| `POST /api/v2/host-monitor/pairing-requests` | 未配对 Client | Client 路由组 512 KiB；strict Host、bearer/polling-secret SHA-256；来源/设备/容量限流 | 创建或幂等恢复 pairing request；返回 activation URL，成功 `no-store` |
| `GET /api/v2/host-monitor/pairing-requests/{request_id}` | 持有 request ID 的调用方 | canonical UUID；来源/请求限流 | 只暴露 OS/arch/version/status/expiry 公共摘要；成功 `no-store` |
| `POST /api/v2/host-monitor/pairing-requests/{request_id}/status` | Client 的 `Pairing <polling_secret>` | 512 KiB 组上限；secret 32..256 字符且无 whitespace | waiting/active/denied/expired 与可空 instance ID；成功 `no-store` |
| `POST /api/v2/host-monitor/activate` | 持有实例 authorization code 的 capability 调用方 | 512 KiB 组上限；来源/请求限流；不是管理员 Session | 激活同一事务状态机；授权码错误/取消/跨实例使用严格失败；短期设备请求仍有超时保护；成功 `no-store` |
| `POST /api/v2/host-monitor/report` | `Bearer <client credential>` | 512 KiB；单份 strict report；每 Host 速率桶 | 持久事务提交后才返回 `202`；同 Host 同 ID 重放 `accepted=false` |

登录与管理写操作的同源裁决会把所有原始 `Origin`、`Host`/HTTP/2 authority、`Sec-Fetch-Site` 值交给
Foundation；重复、冲突或非当前形状 fail closed。生产 Cookie 名是 `__Host-sarmg-host-monitoring-session`，带
`Path=/; Secure; HttpOnly; SameSite=Strict` 且没有 Domain；开发模式改用非 Secure 的 `sarmg-host-monitoring-session`，但
配置层强制监听 loopback。Session token 和 CSRF token 都是 32-byte 随机值，只以 SHA-256 摘要入库；
Session 同时受 idle/absolute TTL、账户 active 与 `session_version` 约束，每个 Session 只保留当前
CSRF 摘要；恢复会话时轮换，不保留旧 token 的兼容窗口。

所有 `/api` 的 4xx/5xx（包括 JSON extractor、body 过大、方法错误与未知 API 路径）都会规范为
Foundation `ErrorEnvelope`；健康端点和静态文件不在这个 envelope 范围。当前仅部分敏感成功响应显式
设置 `Cache-Control: no-store`，不能把它扩大解释为所有管理 GET 都由 Server 响应头禁止缓存。

### 3.2 数据库锁矩阵

数据库文件旁有两把不同的 0600 regular-file `flock`，通过 Linux `openat2` 拒绝 symlink、magic-link、
路径穿越和多硬链接。它们是并发协调，不是数据备份或 transaction journal。

| 操作 | instance lock | maintenance lock | 能否与运行中 Server 并行 |
|---|---|---|---|
| `serve` / `serve-release` | 排他，阻止第二个 Server | 共享，持有到 writer/retention 全部停止 | 不适用；同库只允许一个 Server |
| `doctor` | 不取得 | 共享 | 可以；只读检查，仍可能观察持续变化的业务数据 |
| `admin-create` | 不取得 | 排他 | 不可以；Server 的共享锁会令它立即失败 |
| `admin-reset-password` | 不取得 | 排他 | 不可以；必须先停止 Server |
| `identity` / `verify-release` | 不访问数据库 | 不访问数据库 | 可以，但只证明 binary/release，不证明数据库健康 |

锁身份来自规范化后的实际数据库路径；数据库本体、锁文件及其父目录仍必须满足当前文件安全检查。不要
用复制数据库到另一路径的方式绕开锁：那既不是一致快照，也不在当前支持范围。

## 4. Server 日常命令

```bash
host-monitoring-server identity
host-monitoring-server verify-release --root /opt/isarmg/host-monitoring/releases/0.9.12
host-monitoring-server doctor
host-monitoring-server admin-create --database-url sqlite:///path/app.db
host-monitoring-server admin-reset-password --database-url sqlite:///path/app.db \
  --username admin --password '<new-secret>'
```

`admin-create` 从 `HOST_MONITORING_BOOTSTRAP_ADMIN_USERNAME/PASSWORD` 读取首个账户；若库已有账户，它
只验证现有记录。`admin-reset-password` 接受 `--username/--password`，先规范化 username，再写新的当前
Argon2id hash；Schema trigger 同时提升 `session_version`、撤销该账户全部 Session 并删除其 CSRF 摘要。
两条管理员维护命令要求 maintenance 排他锁，因此应先停止运行实例。

当前 reset CLI 的密码是 argv 参数，不支持 stdin/文件 Secret provider；这是明确的运维限制。不要把真实
密码字面量写进可持久 Shell history、脚本、工单或日志，并限制同机进程列表与维护终端的访问。首次创建
完成后从长期环境文件移除 bootstrap 明文密码。管理 Web 只允许一个管理员，不提供创建管理员入口。账号名称与密码通过右上角人物图标修改；历史管理员记录不代表当前允许多个管理员。

## 5. Client 配置与诊断

`config/host-monitor.json.example` 是当前完整字段样例；Client 配置的 `application_version` 必须等于冻结格式 `0.9.4`，与 Server 程序版本独立。默认采集
10 秒、慢速采集 30 秒、请求超时 10 秒、jitter 10%、spool 64 MiB。配对端点只接受 HTTPS；仅 debug
构建另允许 loopback HTTP，release 拒绝。远程明文 HTTP 已删除；正式投递固定使用 HTTPS。自定义 CA 和客户端身份仍会执行正常证书、
主机名与有效期验证。

Spool 使用 Foundation 唯一当前二进制容器，不读取旧队列格式。单条 Host payload 上限 512 KiB，
队列上限 4096 条，`spool_max_bytes` 必须在 1–256 MiB；物理容器元数据和隔离记录均计入容量。
采样和网络重试 jitter 由 Foundation 实现，网络最终延迟不超过 5 分钟。
`status` 和只读 `doctor` 使用有界目录清单，不创建队列、不获取写锁、不清理临时文件、不删除隔离记录。
清单不是 payload 校验和检查；运行中写入引起的临时检查错误可重试，隔离记录会明确报告为异常。

```bash
host-monitor probe --config /etc/host-monitor/config.json
host-monitor status --config /etc/host-monitor/config.json --output json
host-monitor doctor --config /etc/host-monitor/config.json
host-monitor doctor --config /etc/host-monitor/config.json --delivery
```

本地 doctor 与 delivery doctor 含义不同；后者会真实发送报告，应在变更窗口使用。

## 6. Linux Client

从工作区根构建并调用打包器：

```bash
cargo build --release -p host-monitor
NFPM_ARCH=amd64 clients/host-monitor/packaging/linux/build-packages.sh
```

包安装 `/usr/bin/host-monitor`、0600 配置、systemd unit 和显式 purge 工具。普通卸载保留身份与 spool；
确认不再需要当前状态后才运行 `host-monitor-purge`。Linux collector 会有界读取 hwmon 与 DRM sysfs，
分别表达 AMD/Intel/NVIDIA capability；字段或驱动不存在时保留缺失/错误分类，不用 0 伪装。NVIDIA
NVML 采集通常需要按包内 `host-monitor-gpu.conf` 明确配置设备访问，不能默认放宽整个服务沙箱。

## 7. Windows Client

WiX 4 MSI 同时安装 Windows Service、Tray 和维护 helper。Tray 是用户交互外壳，Service 是持续采集
主体；两者通过受保护本机控制通道通信。构建/验收使用：

```powershell
clients\host-monitor\packaging\windows\wix\build-msi.cmd 0.9.12 `
  target\x86_64-pc-windows-msvc\release\host-monitor.exe `
  target\x86_64-pc-windows-msvc\release\host-monitor-maintenance.exe `
  target\x86_64-pc-windows-msvc\release\host-monitor-tray.exe
powershell -File clients\host-monitor\packaging\windows\tests\Test-WixAuthoring.ps1
powershell -File clients\host-monitor\packaging\windows\tests\Test-PeSubsystems.ps1
```

当前 MSI 不声明跨版本 UpgradeCode 家族，也不迁移非当前状态。每个发行版本按全新产品安装；需要数据
转换时，必须先在外部仓库建立、评审并验证明确转换边。安装失败必须由 MSI rollback 清理本次创建的
服务和文件。

## 8. macOS Client

`build-pkg.sh` 生成含 LaunchDaemon、配置、日志轮转和专用不可登录账户的 pkg。验证：

```bash
clients/host-monitor/packaging/macos/tests/validate-packaging.sh
clients/host-monitor/packaging/macos/tests/smoke-pkg.sh
clients/host-monitor/packaging/macos/tests/account-safety-test.sh
clients/host-monitor/packaging/macos/tests/postinstall-failure-test.sh
clients/host-monitor/packaging/macos/tests/uninstall-proof-test.sh
```

卸载脚本只删除能证明属于当前包的资源；不能用宽泛递归路径替代这些身份检查。

### 8.1 安装包签名边界

“能生成包”与“可作为受信正式制品分发”是两项不同能力。当前仓库的 Linux nFPM 和 Windows WiX 构建
流程没有 GPG/Authenticode 签名步骤；相关测试证明结构、权限、PE subsystem 和生命周期，不证明发布者
身份。macOS `build-pkg.sh` 有两种明确模式：

- 设置完整 `Developer ID Installer: ...` identity 时，脚本要求输入 Mach-O 已由 Developer ID
  Application 签名，再签 pkg 并用 `pkgutil --check-signature` 验证；
- 未设置 installer identity 时只生成明确标记的 unsigned prerelease；
- 无论哪种模式，脚本都不执行 Apple notarization 或 stapling。

因此正式分发若要求平台信任链，发布流水线还必须补齐仓库当前没有的 Linux/Windows 签名，以及 macOS
notarization/stapling，并保存签名者、时间戳、摘要和验证结果；不能把打包测试通过写成“已签名发布”。

## 9. 数据库身份与当前不支持的数据操作

Server 只创建当前库。`product_metadata` 必须精确绑定 application `host-monitoring`、version `0.9.12`、
schema revision `3` 与 SHA-256
`233c8b12e9b09bc8a4f3dfa57309e5bc268de0aa958e8e0eb45555d75f94c410`；现场 `sqlite_schema` 重新计算也
必须一致。当前 DDL 中管理员列是 `_sarmg_administrators.username`，没有 `email` 或 role 列；DDL 自身约束 canonical
username、非空 password hash、`active IN (0,1)`，`serve`/`admin-create` 加载已有行时再用 Foundation
primitive 验证 username 和完整 current Argon2id 参数；DDL 还要求 `session_version > 0`，形成存储形状与
密码策略双层 fail-closed。数据库/
父目录/锁的
链接、特殊文件和硬链接
别名在 Linux 通过 `openat2` 锚定检查。`doctor` 还通过 Foundation 适配器执行完整
`PRAGMA integrity_check` 与 `foreign_key_check`；失败只报告 degraded 并退出，不在产品内修库。

产品仓不实现一致性备份、恢复或版本转换。`sarmg-upgrade` 当前只保留通用离线机制，尚未声明任何
Host Monitoring 转换边，因此当前没有受支持的 Host 数据迁移、备份或恢复流程。不得把“通用机制存在”
解释为可以处理本库，也不得自行只复制 SQLite 主文件、手改 metadata 或拼接 WAL。未来只有外部仓同时
声明精确输入/输出身份、锁、自己的 journal/原子性语义、验证和失败恢复合同后，对应操作才进入支持范围。

当前 retention worker 只处理 raw report 与 hourly aggregate。`audit_events` 没有读取、导出或清理 API；
过期/撤销的 `auth_sessions` 行没有全局清理 worker；pairing 只有创建新 pairing 时针对 expired
pending/旧 denied 的有界清理和删除 Host 时的定向清理。长期实例必须把这些控制面表的增长视为已知
运维缺口，不能误以为 `RAW_RETENTION_DAYS` 会覆盖它们，也不能在没有新合同/测试时手工删行。

## 10. 监控与故障处理

Run、Once 和 `doctor --delivery` 在加载身份或初始化采样器前获取 Foundation 运行会话锁。
同一状态目录已有投递进程时，新进程直接失败；Spool 及其克隆保留该锁直到退出。
Status、只读 Doctor、Probe 与 Pair 不获取投递会话锁，可以与运行服务并行；配对写入仍须
自己的短事务锁。不要删除 `client.instance.lock` 或 Spool 锁文件来强行启动第二个进程，
锁文件在进程结束后保留是正常行为；应先正常停止原实例。

Unix Client 状态目录必须由当前服务账户拥有、权限为 0700，路径及祖先不能是符号链接。
配对事务锁必须为同 UID/GID 的 0600 普通单链接文件。管理员以 root 配对时，新凭据和锁
继承服务状态目录的 UID/GID；程序拒绝不安全的现有目录或锁，不自动 chmod/chown 修复。
安装器/systemd 负责建立当前部署权限；权限异常时先停止并检查部署及现场，不把重新配对
当作修复权限或覆盖异常文件的手段。配置文件仍保留安装器定义的服务可读权限（如 0640）。
Unix 配置目录只接受 0700/0750/0755，配置文件只接受普通单链接的 0600/0640；读写使用
Foundation 持有目录句柄，拒绝符号链接和硬链接。替换保留原 UID/GID/mode；新文件默认
0600，不会创建缺失的父目录。配置大小上限为 64 KiB（含末尾换行），`status` 只报告配置
错误而不修复文件。安装器负责建立供服务读取的组与权限，不通过放开 world-read 绕过检查。
身份、令牌、配对 journal、授权状态与 active binding 的读取也检查普通单链接文件、0600 和
所属 UID/GID；读取失败不会被当作未配对或触发权限修复。文件读取上限分别为 128 字节、
4 KiB、64 KiB、16 KiB、16 KiB，包含文件中的空白。只读诊断沿用这些边界，不创建锁或状态。

配对 journal 中的 bearer/polling 秘密在内存中使用共享清零秘密类型，克隆状态共享所有权。
私有 journal 仍按当前协议保存明文，必须保护整个状态目录，不能上传到公开日志或 issue。
序列化直接进入 64 KiB 有界清零缓冲区，超限保留原文件；损坏 journal 的错误不回显原值。
轮询 Authorization 头标记为敏感。此边界不等于磁盘加密，也不保证 serde/HTTP 内部副本全部
清零。配置中的 OTLP Token/TLS 身份密码也使用共享秘密类型，克隆配置与 Reporter 不复制
Token 明文；保存直接进入有界清零缓冲区（含换行），超限保留原文件。配置仍是受权限保护
的明文 JSON，不是加密存储；进程环境、解析器内部副本和 TLS 身份文件读取器不在完整清零
保证内。损坏配置只报告错误位置、不回显输入值。凭据加载、轮换/恢复及失效已接入共享事务接口，
Reporter 自带凭据代际；新配对 Pending 期间仍按 active binding 核对旧响应，不因 journal
不再是 Active 而放过代际检查。无效轮换身份先于 token 写入拒绝，未知授权状态直接报错，
不自动覆盖修复。当前配对 wire 保持不变，不保留无条件授权/失效的生产接口。

服务使用同一事务中捕获的 Reporter、Host 身份和报告端点完整快照，构造失败不部分修改
运行配置。若服务错过 B 的 Active、下一轮 C 已进入 Pending，会先加载仍有效的本地 B
凭据，再继续 C 的轮询，不被 C 的网络等待阻塞；旧状态响应不作为回退快照的依据。
切换同时通知采样器更新 Host 身份，并停止持有旧 Reporter 的可选 OTLP 工作线程。
OTLP 始终为主报告持久确认后的尽力导出，旧工作线程取消时不承诺补发其队列。

Host/OTLP 失败诊断不记录远端正文或 ErrorEnvelope 的任意 message/request_id，防止
凭据被响应反射进日志；保留 HTTP 状态与本地已识别的固定机器码标签。确认正文解析错误
只保留位置，身份不匹配不回显远端 ID；传输/读取错误移除 reqwest 附带的 URL。

TLS identity/CA 每个文件上限 1 MiB，空文件拒绝。Unix 使用 Foundation 的受保护输入
目录句柄与单文件名，拒绝符号链接、硬链接、特殊文件、不可信属主和 group/other 写权限。
身份文件可用 0400/0440/0600/0640（允许配置的服务组读取）；CA 还允许 0444/0644。
不要把系统证书符号链接直接作为配置输入；提供受保护的普通文件。CA 文件须解析出至少
一张证书，未知文本不能静默当作空 CA 集合。Windows 当前只保证读取预算，原生句柄/ACL
及 PKCS#12 验收仍未完成。Linux 测试需 OpenSSL CLI 和带 TLS 1.3 支持的 Python `ssl`；
证书/私钥在临时私有目录生成，不提交私钥。除输入构造检查，还用独立 OpenSSL 回环服务
验证实际 Reporter 的 TLS 1.2/1.3 投递、Authorization 和匹配 ACK；两种协议都覆盖未知
CA、主机名不匹配、过期服务端证书、缺失/不受信任的客户端证书，失败不被分类为报告永久
拒绝。可选 OTLP 另验证 TLS 1.3 mTLS、Bearer 与 gzip 请求，以及缺失客户端证书的拒绝。
测试进程有握手/读取/退出期限，并由父测试负责终止和回收。这是 Linux 本机真实握手证据，
不是 Windows/macOS PKCS#12、真实 Collector、生产代理或部署环境验收。

可单独运行握手回归：`cargo test --locked -p host-monitor --lib --all-features transport::tls_tests`。

Report、OTLP 和创建/轮询/激活配对共用 Client Foundation HTTP 工厂，不再保留产品本地
响应读取循环。请求总超时覆盖 DNS 到响应体读完，连接超时为总超时与 10 秒的较小值；
每次请求最多接受 16 个解析地址，验证后绑定实际连接。响应 Header/Body 各限 64 KiB，
拒绝超限 Content-Length 和分块响应。系统/环境代理及重定向禁用；依赖环境代理的部署
不能绕过这一规则，需使用可直接访问的 HTTPS 端点。明文 loopback HTTP 只允许 debug
构建，release 连 localhost 也拒绝。配对失败不再回显任意响应正文。Windows 托盘健康
探测也通过同一工厂的同步适配器：只发无凭据的 `GET /health/live`，不读取 ProgramData
状态或服务身份，Header/Body 各限 16 KiB，要求当前版本且不回显远端不匹配版本字符串。
解析和实际 HTTP 路径已纳入跨平台测试，不代表 Windows 原生 UI/安装器验收完成。
同步适配器持有并回收工作线程与运行时；系统 DNS 解析本身不可强制取消，可能延迟
运行时退出，因此当前不能把 4 秒 HTTP 预算写成所有故障下严格的 4 秒调用返回保证。

`status` 和默认只读 `doctor` 的 `tls` 检查复用实际投递客户端构造：检查平台身份格式、
密码配置、受保护的有界文件读取和证书解析，不读取投递凭据、不获取事务/投递锁、不创建
状态目录，也不做 DNS、连接或握手。失败返回固定 `tls_configuration_invalid`，不会附带
底层解析错误链、证书内容或密码；Status 总体变为 `degraded`，Doctor 为 `unhealthy` 并
返回失败退出码。`ok` 仅说明本地输入和构造通过，不证明证书在远端有效、服务可达或 mTLS
授权成功。只读凭据检查使用同一 StateReader 的 4 KiB 上限与文件安全规则，拒绝特殊文件，
不会因 FIFO 等待写入者；检查可读且非空不等于当前凭据已获 Server 授权。

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
