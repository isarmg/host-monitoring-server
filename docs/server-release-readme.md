# Host Monitoring Server 0.9.29 发行包部署手册

本文只面向已经生成的正式 Server 发行物：

```text
host-monitoring-server-0.9.29-x86_64-unknown-linux-gnu.tar.gz
host-monitoring-server-0.9.29-x86_64-unknown-linux-gnu.tar.gz.sha256
```

它不是源码构建指南，也不描述各平台 Client 的安装。下文所有相对路径都必须在当前发行包内存在；源码
仓库不是运行时依赖。

## 1. 支持边界

- Server 唯一支持 **x86_64 GNU/Linux（AMD64 + glibc）**，target 固定为
  `x86_64-unknown-linux-gnu`。ARM、musl、Windows 和 macOS 不支持运行 Server。
- 正式服务由 systemd 管理，以专用非 root 账户 `isarmg-host` 运行。
- 每个版本都是全新、精确的当前合同。本发行不读取旧 Schema、旧字段、旧路径、旧身份或旧发行别名，
  也不提供兼容 fallback。
- 产品内没有升级、迁移、备份或恢复命令。未来只有 `sarmg-upgrade` 为精确输入/输出版本声明并验证
  转换边后，才存在受支持的数据操作。
- 正式发行目录固定为 `/opt/isarmg/host-monitoring/releases/0.9.29`。禁止创建 `current`、`latest`
  等可变别名。

## 2. 主机前置条件

| 项目 | 要求 | 说明 |
|---|---|---|
| OS/CPU/libc | Linux、x86_64、glibc | 二进制在读取配置、打开 SQLite 和监听前会再次检查 |
| 服务管理 | systemd | 包内 unit 是唯一正式启动合同 |
| 管理权限 | 可使用 `sudo` 成为 root | 用于账户、发行树、配置、状态目录和 unit |
| 基础工具 | GNU tar、gzip、sha256sum、systemctl、systemd-analyze、curl、getent、groupadd、useradd | 校验、安装和验收 |
| TLS 入口 | 同机或可信受控网络中的反向代理 | Server 只监听 HTTP；公网 HTTPS 由代理提供 |
| 存储 | 为 SQLite、WAL 和增长预留容量 | 需要容量、inode、延迟与备份监控 |

生产主机不应允许非管理员写 `/opt/isarmg`、`/etc/isarmg`、`/etc/systemd/system` 或
`/var/lib/isarmg`。不要把 SQLite 放在用户可写挂载或语义不明确的共享文件系统。

## 3. 包布局与信任关系

归档只有一个顶层目录 `0.9.29/`：

```text
0.9.29/
├── bin/host-monitoring-server
├── systemd/host-monitoring-server.service
├── web/
│   ├── index.html
│   └── assets/
│       ├── *.js
│       └── *.css
├── README.md
└── RELEASE-MANIFEST.json
```

信任链分三层：

1. 发布渠道提供的 `.sha256` 文件确认下载归档未被替换或损坏。
2. `RELEASE-MANIFEST.json` 绑定产品、版本、完整源码 revision、二进制 identity，以及每个文件/目录的
   路径、类型、精确权限、大小和 SHA-256。
3. 发行树内真实的 `bin/host-monitoring-server` 必须由自身执行 `verify-release`；复制到树外的二进制
   不能替别的树完成正式校验。

允许的目录只有 `bin`、`systemd`、`web`、`web/assets`。固定文件之外，仅允许 `web/assets` 中的当前
JavaScript/CSS 产物。缺失、额外、符号链接、硬链接别名、特殊文件、权限变化、大小变化或摘要变化都会
导致校验失败。目录和子目录为 `0555`，Server 二进制为 `0555`，其他 payload 与 manifest 为 `0444`。

不要在线编辑 Web、README、unit 或 manifest。唯一可编辑配置位于 `/etc/isarmg/host-monitoring.env`，
业务状态位于 `/var/lib/isarmg/host-monitoring`。

## 4. 下载与预安装验证

先从可信发布渠道确认 checksum 文件来源，再执行：

```bash
sha256sum --check --strict \
  host-monitoring-server-0.9.29-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -tzf host-monitoring-server-0.9.29-x86_64-unknown-linux-gnu.tar.gz
```

预安装验证必须把 `0.9.29` 解压到名为 `releases` 的真实父目录，因为这是发行 root 合同的一部分：

```bash
temporary_root="$(mktemp -d)"
mkdir -m 0755 "$temporary_root/releases"
tar --extract --gzip \
  --file host-monitoring-server-0.9.29-x86_64-unknown-linux-gnu.tar.gz \
  --directory "$temporary_root/releases" \
  --no-same-owner --same-permissions --delay-directory-restore

"$temporary_root/releases/0.9.29/bin/host-monitoring-server" identity
"$temporary_root/releases/0.9.29/bin/host-monitoring-server" verify-release \
  --root "$temporary_root/releases/0.9.29"
```

`identity` 必须是单行 JSON：

| 字段 | 当前值或形状 |
|---|---|
| `manifest_format` | `host-monitoring-release-v2` |
| `application` | `host-monitoring` |
| `version` | `0.9.29` |
| `schema_application_version` | `0.9.26`，仅在数据库格式迁移时改变 |
| `api_prefix` | `/api/v2` |
| `schema_revision` | `7` |
| `schema_sha256` | 64 位小写十六进制当前 Schema 摘要 |
| `target` | `x86_64-unknown-linux-gnu` |
| `source_revision` | 40 位小写十六进制 Git revision，不能是 `unbound` |

任何错误都应停止交付并重新获取发行物；不得修改 manifest、权限或内容让坏包“通过”。验证完删除临时
目录。

## 5. 全新安装发行树

以下流程只适用于确认没有现有 Host Monitoring Server 的全新主机：

```bash
sudo test ! -e /opt/isarmg/host-monitoring/releases/0.9.29
sudo test ! -e /etc/systemd/system/host-monitoring-server.service
sudo test ! -e /etc/isarmg/host-monitoring.env
```

任一检查失败都应停止。不要覆盖、合并、改名或复用未知内容，也不要把同版本重装当成升级。

```bash
sudo install -d -m 0755 -o root -g root \
  /opt/isarmg \
  /opt/isarmg/host-monitoring \
  /opt/isarmg/host-monitoring/releases

sudo tar --extract --gzip \
  --file host-monitoring-server-0.9.29-x86_64-unknown-linux-gnu.tar.gz \
  --directory /opt/isarmg/host-monitoring/releases \
  --same-owner --same-permissions --delay-directory-restore
sudo chown -R root:root /opt/isarmg/host-monitoring/releases/0.9.29

sudo /opt/isarmg/host-monitoring/releases/0.9.29/bin/host-monitoring-server \
  verify-release --root /opt/isarmg/host-monitoring/releases/0.9.29
```

不要给 `isarmg-host`、其他 group 或 world 发行树写权限。生产静态目录检查还会拒绝由服务账户拥有的
Web 文件。

## 6. 创建专用服务账户

全新主机创建固定组和不可登录账户：

```bash
sudo groupadd --system isarmg-host
sudo useradd --system \
  --gid isarmg-host \
  --home-dir /var/lib/isarmg/host-monitoring \
  --no-create-home \
  --shell /usr/sbin/nologin \
  isarmg-host
```

若名称已存在，不可直接忽略命令失败。必须确认账户不是 root、primary group 精确为 `isarmg-host`、home
精确为 `/var/lib/isarmg/host-monitoring`、shell 为系统 `nologin`，且该身份确由本部署管理：

```bash
getent group isarmg-host
getent passwd isarmg-host
id isarmg-host
```

不要复用普通用户、登录账户、Web 管理员或 Client 身份。

## 7. 创建生产配置

创建 root 控制的配置目录与排他配置文件：

```bash
sudo install -d -m 0755 -o root -g root /etc/isarmg
sudo sh -c 'umask 077; set -C; : > /etc/isarmg/host-monitoring.env'
sudo chown root:root /etc/isarmg/host-monitoring.env
sudo chmod 0600 /etc/isarmg/host-monitoring.env
sudoedit /etc/isarmg/host-monitoring.env
```

`set -C` 使已存在路径失败，避免无意覆盖。配置必须保持 root 所有、`0600`、普通文件且不能经链接代换。
Server 只读取进程环境；此文件由 systemd unit 的 `EnvironmentFile` 加载。

### 7.1 当前配置模板

```text
HOST_MONITORING_DATABASE_URL=sqlite:///var/lib/isarmg/host-monitoring/db/host-monitoring.sqlite3
HOST_MONITORING_BIND=127.0.0.1:18105
HOST_MONITORING_DEVELOPMENT=false

HOST_MONITORING_BOOTSTRAP_ADMIN_USERNAME=admin
HOST_MONITORING_BOOTSTRAP_ADMIN_PASSWORD=REPLACE_WITH_A_UNIQUE_PASSWORD_OF_AT_LEAST_12_BYTES
HOST_MONITORING_CLIENT_AUTHORIZATION_KEY=REPLACE_WITH_BASE64_ENCODED_32_RANDOM_BYTES

HOST_MONITORING_TELEMETRY_QUEUE_CAPACITY=256
HOST_MONITORING_TELEMETRY_BATCH_SIZE=64
HOST_MONITORING_TELEMETRY_FLUSH_MILLISECONDS=25
HOST_MONITORING_TELEMETRY_ENQUEUE_WAIT_MILLISECONDS=10
HOST_MONITORING_TELEMETRY_REQUEST_TIMEOUT_MILLISECONDS=10000
HOST_MONITORING_TELEMETRY_SHUTDOWN_DRAIN_MILLISECONDS=15000

HOST_MONITORING_RAW_RETENTION_DAYS=7
HOST_MONITORING_AGGREGATE_RETENTION_DAYS=365
HOST_MONITORING_RETENTION_INTERVAL_SECONDS=300
HOST_MONITORING_RETENTION_BATCH_SIZE=256
HOST_MONITORING_RETENTION_MAX_TRANSACTIONS_PER_RUN=12
HOST_MONITORING_RETENTION_MAX_RUN_MILLISECONDS=2000
HOST_MONITORING_RETENTION_YIELD_MILLISECONDS=10
```

unit 会把 `HOST_MONITORING_STATIC_DIR` 固定为
`/opt/isarmg/host-monitoring/releases/0.9.29/web`；不得指向源码、旧 dist、软链接或在线编辑目录。

### 7.2 配置约束

| 变量 | 当前约束 |
|---|---|
| `DATABASE_URL` | 必填 SQLite URL；生产指向固定 state path |
| `BIND` | 默认 `127.0.0.1:18105`；推荐仅 loopback |
| `STATIC_DIR` | 绝对路径，正式发行精确等于已验证 root 的 `web` |
| `DEVELOPMENT` | 生产 `false`；`true` 只允许 loopback bind |
| `BOOTSTRAP_ADMIN_USERNAME` | 默认 `admin`；必须是当前 canonical username |
| `BOOTSTRAP_ADMIN_PASSWORD` | 空库时必填，12..1024 bytes、无 ASCII control |
| `CLIENT_AUTHORIZATION_KEY` | 必填；标准 Base64 编码的 32 个随机字节；用于加密每实例长期授权码 |
| `TELEMETRY_QUEUE_CAPACITY` | 默认 256，最大 1024 |
| `TELEMETRY_BATCH_SIZE` | 默认 64，1..min(512, queue) |
| `TELEMETRY_FLUSH_MILLISECONDS` | 默认 25，1..1000 |
| `TELEMETRY_ENQUEUE_WAIT_MILLISECONDS` | 默认 10，1..250 |
| `TELEMETRY_REQUEST_TIMEOUT_MILLISECONDS` | 默认 10000，100..30000，且大于 enqueue + flush |
| `TELEMETRY_SHUTDOWN_DRAIN_MILLISECONDS` | 默认 15000，100..60000 |
| `RAW_RETENTION_DAYS` | 默认 7，1..365 |
| `AGGREGATE_RETENTION_DAYS` | 默认 365，最大 3650，严格大于 raw |
| `RETENTION_INTERVAL_SECONDS` | 默认 300，1..86400 |
| `RETENTION_BATCH_SIZE` | 默认 256，1..512 |
| `RETENTION_MAX_TRANSACTIONS_PER_RUN` | 默认 12，3..30 |
| `RETENTION_MAX_RUN_MILLISECONDS` | 默认 2000，100..10000，且短于维护 interval |
| `RETENTION_YIELD_MILLISECONDS` | 默认 10，1..100 |

未知 `HOST_MONITORING_*` 变量不会自动被拒绝，必须通过配置审查捕获拼写错误，不能把“进程已启动”解释
为每个未知变量都生效。

### 7.3 唯一管理员身份

Server 与 React 管理 Web 只保留 `admin` role；不存在 viewer、operator、RBAC 或邮箱登录。

- 登录请求精确为 `{username,password}`。
- username 候选必须是 1..64 bytes printable ASCII；规范化会 trim ASCII whitespace 并转为 ASCII
  小写。
- canonical username 必须是 3..64 bytes，首尾为 `[a-z0-9]`，字符只允许 `[a-z0-9._-]`。
- `@`、Unicode、内部空白、控制字符和首尾分隔符均被拒绝。
- 成功 Session 精确包含 `authenticated`、`user_id`、`username`、固定 `role:"admin"` 与
  `csrf_token`。

首次成功启动并创建管理员后，从长期配置删除 `HOST_MONITORING_BOOTSTRAP_ADMIN_PASSWORD` 明文。已有
管理员时启动不会覆盖账户，而会验证 username 与当前 Argon2id hash；坏行会阻止服务。

`HOST_MONITORING_CLIENT_AUTHORIZATION_KEY` 应在首次部署时生成一次并持久保存到独立秘密系统，例如
`openssl rand -base64 32`。它不是实例授权码；更换或遗失该密钥会使数据库中已加密的授权码无法读取，
不能作为日常轮换实例授权码的方式。

## 8. 安装 systemd unit

```bash
sudo test ! -e /etc/systemd/system/host-monitoring-server.service
sudo install -m 0644 -o root -g root \
  /opt/isarmg/host-monitoring/releases/0.9.29/systemd/host-monitoring-server.service \
  /etc/systemd/system/host-monitoring-server.service
sudo systemd-analyze verify /etc/systemd/system/host-monitoring-server.service
sudo systemctl daemon-reload
```

unit 的关键边界包括：

- `ConditionArchitecture=x86-64`；
- `User=isarmg-host`、`Group=isarmg-host`、`UMask=0077`；
- `StateDirectory=isarmg/host-monitoring/db`、`RuntimeDirectory=isarmg/host-monitoring`；
- 固定 `serve-release --root /opt/isarmg/host-monitoring/releases/0.9.29`；
- `ProtectSystem=strict`、`ProtectHome=true`、`NoNewPrivileges=true`、空 capability；
- 限制 address family、namespace、proc、设备、内核接口与可执行内存。

不要用 drop-in 改二进制路径、发行 root、运行用户或 static directory。监听与队列参数只通过受控环境
文件修改，并在维护窗口验证。

## 9. 首次启动与验收

```bash
sudo systemctl enable --now host-monitoring-server.service
sudo systemctl status host-monitoring-server.service --no-pager --full
curl --fail http://127.0.0.1:18105/health/live
curl --fail http://127.0.0.1:18105/health/ready
```

- `/health/live` 返回 `200 {"status":"ok"}`，只证明进程路由可响应。
- `/health/ready` 同时报告 database、retention schema 和 telemetry writer；任一未就绪应返回 503，不能
  接入业务流量。

再次验证运行中的物理发行：

```bash
sudo /opt/isarmg/host-monitoring/releases/0.9.29/bin/host-monitoring-server \
  verify-release --root /opt/isarmg/host-monitoring/releases/0.9.29
```

完成 TLS 代理后，还要从外部可信客户端验证 DNS/证书链、hashed Web assets、username 登录、Session、
CSRF、退出，以及 Client pairing/activation/report 链路。

## 10. HTTPS 反向代理

```text
Browser / Client --HTTPS--> 可信反向代理 --loopback HTTP--> 127.0.0.1:18105 Server
```

最小 Caddy 示例：

```caddyfile
monitor.example.com {
    reverse_proxy 127.0.0.1:18105
}
```

Server 不终止 TLS。代理负责证书、私钥、续期、HSTS 和公网限制，并必须保留浏览器真实的单一 `Host`、
`Origin` 与 `Sec-Fetch-Site`；重复或冲突字段会 fail closed。

登录限流使用实际 TCP peer，不信任 `Forwarded` 或 `X-Forwarded-For`。若代理复用一个后端源地址，来源
桶看到的是代理而非公网客户端；不能通过信任任意转发头来“修复”。

生产 Cookie 是 `__Host-host_session`，带 `Path=/; Secure; HttpOnly; SameSite=Strict` 且无 Domain。
`DEVELOPMENT=true` 只用于 loopback 调试并改用非 Secure Cookie，不能用于生产。

## 11. 当前 HTTP 与身份面

| 路径 | 身份 | 用途 |
|---|---|---|
| `GET /healthz` | 公开 | 进程存活，成功 204 |
| `GET /readyz` | 公开 | 最小就绪状态，成功体精确为 `{"ready":true}` |
| `POST /api/v2/auth/login` | 公开入口 + 同源/限流 | 管理员登录 |
| `GET /api/v2/auth/session` | 管理员 Cookie | Session 恢复并轮换 CSRF |
| `POST /api/v2/auth/logout` | 管理员 Cookie + CSRF | 撤销当前 Session |
| `/api/v2/monitoring/*` | 管理员 Cookie；写操作另需 CSRF | Host、历史、邀请、备注与删除 |
| `/api/v2/host-monitor/*` | pairing capability 或 Client Bearer | 配对、激活与遥测 |

当前 React 页面覆盖登录、Session 恢复、退出、实例创建、长期授权码查看/轮换、取消配对、已取消实例删除、
激活确认、Host 列表与详情、历史、备注和受控删除。页面显示“已保存”只证明管理写入提交；Client 重新配对、
新报告到达等业务结果仍须在实例状态和 Host `last_seen` 中单独确认。

## 12. 日常运维

### 12.1 服务和日志

```bash
sudo systemctl status host-monitoring-server.service --no-pager --full
sudo journalctl --unit host-monitoring-server.service --since today
sudo journalctl --unit host-monitoring-server.service --follow
```

记录时间、HTTP status、错误 `code`、请求 ID、readiness、磁盘和只读摘要。不要公开密码、Cookie、CSRF、
Client credential、activation code、数据库或遥测正文。

### 12.2 数据库锁

| 命令 | instance lock | maintenance lock | 与运行 Server 并行 |
|---|---|---|---|
| `serve-release` | 排他 | 共享，持有到 writer/retention 停止 | 同库只允许一个 |
| `doctor` | 无 | 共享 | 可以，但观察变化中状态 |
| `admin-create` | 无 | 排他 | 不可以，必须先停 Server |
| `admin-reset-password` | 无 | 排他 | 不可以，必须先停 Server |
| `identity` / `verify-release` | 不访问数据库 | 不访问数据库 | 可以 |

锁文件只是并发协调，不是备份或 journal。不要复制数据库到另一条路径绕过锁。

### 12.3 doctor 与管理员维护

`doctor` 读取与服务相同的环境，检查当前 Schema、readiness、SQLite integrity 和 foreign keys；它不
迁移、不修复，也不验证每个管理员内容。`degraded` 时保持流量隔离并调查。

创建首个管理员使用 `admin-create --database-url ...`，从环境读取 bootstrap username/password。重置
密码通过标准输入交给 `admin-reset-password --database-url ... --username ...`。两者需要 maintenance
排他锁，必须先停服务。

reset 的密码只接受单行标准输入，不能写入命令参数；应在受控维护终端通过秘密管理器或不回显的临时变量提供。

### 12.4 容量与保留

监控 SQLite/WAL/SHM、容量、inode、I/O 延迟、队列饱和、429/503、writer readiness、raw/hourly 表、
systemd 重启、代理 5xx、证书到期和 Client 离线比例。

retention worker 只覆盖 raw report 与 hourly aggregate，不会全局清理 `audit_events` 或全部过期/撤销
Session；invite/pairing 也只有有界定向清理。不能把 raw retention 解释为所有控制面表的保留策略。

## 13. 备份、恢复、升级和回滚

当前产品没有受支持的 Host Monitoring 数据备份、恢复或转换边。禁止：

- 只复制 SQLite 主文件而遗漏 WAL/SHM；
- Server 运行时复制并宣称得到一致快照；
- 手改 `product_metadata`、Schema 或摘要；
- 把旧数据库交给当前二进制试跑；
- 增加旧列、旧 API、旧 username、双读或 fallback；
- 用可变软链接切换版本。

需要数据操作时，停止服务、保全 SQLite 与 sidecar、记录发行 identity 和摘要，等待 `sarmg-upgrade` 为
精确版本声明锁、journal、原子性、验证及失败恢复合同。没有显式边就没有受支持的操作。代码回滚也不能
复用当前数据库；回滚必须显式定义完整 generation 与数据一致性流程。

## 14. 故障定位

### checksum 或发行校验失败

重新从可信渠道取得归档与 checksum。检查是否被代理、解压工具或人工编辑改变，不要修补 manifest。

### `release root must be releases/0.9.29`

root 的直接父目录必须名为 `releases`，root 必须名为 `0.9.29`，整条路径必须规范化且不能经过 symlink。

### 服务启动前退出

依次检查：Linux/x86_64/glibc；发行 root/manifest/权限；static path；配置范围与参数关系；数据库父目录
和锁安全；当前数据库 identity；bootstrap username/hash。

### live 正常但 ready 为 503

进程活着，但 database、retention schema 或 writer 未就绪。检查 Journal、SQLite、磁盘、锁和最近配置，
readiness 恢复前不接入流量。

### 登录失败

确认使用 username 而非邮箱；检查 canonical 规则、密码、HTTPS Cookie、单一 Host/Origin、浏览器时间和
TCP peer 限流。不要添加邮箱候选、旧用户名 fallback 或第二 role。

### Client 报告失败

按稳定错误 `code` 分支，不按 `message`。`unauthorized` 表示 credential 已失效，需显式重新配对；
`client_host_mismatch` 表示 Host 与 credential 绑定不一致，应丢弃并调查。同时检查队列、429/503、
`Retry-After`、Client spool、TLS 和时间。

## 15. 安全事件

1. 在代理隔离入口，并隔离受影响 Client；
2. 保全只读 Journal、identity/manifest、摘要、数据库与 WAL/SHM；
3. 轮换管理员、Client、mTLS、OTLP、TLS 私钥和主机凭据；
4. 证据未保全前不要清理数据库、日志或状态；
5. 只修当前版本，不加入旧兼容。

公开报告不得包含生产遥测、主机标识、Cookie、Token、密码、activation code、私钥或数据库。

## 16. 上线验收清单

- [ ] 主机是 Linux x86_64 + glibc，systemd 与基础工具齐全。
- [ ] checksum 来源可信并通过严格校验。
- [ ] 归档只有 `0.9.29` 顶层目录，identity 和 `verify-release` 通过。
- [ ] 发行位于固定版本目录，不存在可变别名或链接父链。
- [ ] 发行树 root 所有、只读，服务账户不能写。
- [ ] `isarmg-host` 是独立非 root、nologin 账户，group/home 精确。
- [ ] 配置 root 所有、`0600`，所有参数已复核。
- [ ] 初始 username canonical，密码是独立长随机秘密。
- [ ] unit 与发行包一致，`systemd-analyze verify` 通过。
- [ ] 固定 `serve-release` 启动，live 与 ready 均为 200。
- [ ] TLS、证书、外部管理登录、Cookie/CSRF 与 Client 链路通过。
- [ ] 监控覆盖 SQLite/WAL、容量/inode、writer、retention、systemd、代理和证书。
- [ ] 首次创建后已移除长期配置中的 bootstrap 明文密码。
- [ ] 已明确当前不存在受支持的数据迁移、备份、恢复或跨版本回滚。

全部通过后，才表示当前 `0.9.29` Server 发行具备上线条件。
