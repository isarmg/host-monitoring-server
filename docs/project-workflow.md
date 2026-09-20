# Host Monitoring 工作流程与流程树

## 1. 总流程树

```text
Host Monitoring
├─ Server
│  ├─ 验证发行树/配置/当前 Schema
│  ├─ Foundation admin 用户名登录与 Session/CSRF
│  ├─ Client 实例授权码、配对请求与凭据激活
│  ├─ 报告限流 -> 有界队列 -> 单 SQLite writer
│  └─ 原始报告 -> 小时聚合 -> 分层保留
├─ Desktop Client
│  ├─ 读取当前配置并取得状态锁
│  ├─ 采集 CPU/内存/磁盘/网络/传感器
│  ├─ 本地有界 spool
│  └─ HTTPS 报告与可选 OTLP
├─ Mobile library
│  └─ 宿主快照 -> 合约收敛 -> JSON payload
└─ Delivery
   ├─ Server：x86_64 GNU/Linux 不可变 tar release
   └─ Client
      ├─ Linux deb/rpm + systemd
      ├─ Windows x86_64 Service + CLI + MSI
      └─ macOS LaunchDaemon + pkg
```

## 2. Server 启动

Server 进程只允许 `x86_64-unknown-linux-gnu` 构建，并在解析命令/配置前通过 `uname` 确认当前内核是
Linux、机器是 x86_64。正式二进制要求 `--root` 是规范绝对的 `.../releases/0.9.19` 且当前 executable
就是该树的 `bin/host-monitoring-server`；systemd 提供的标准部署根固定为
`/opt/isarmg/host-monitoring/releases/0.9.19`。随后验证 manifest、完整源码 revision、target、API、
Schema、Web 文件集合/Hash/权限，再解析
`HOST_MONITORING_*`。随后取得数据库 instance
排他锁和 maintenance 共享锁；已有库先用独立只读连接验证 `product_metadata` 与实际
`sqlite_schema` 指纹，最后才通过 `sarmg-sqlite` 打开 WAL、foreign keys、5 秒 busy timeout、FULL
synchronous 的 SQLx pool，启动 writer/retention 和 HTTP listener。文件创建/权限、精确产品 DDL、
只读预检和锁仍由产品负责；Foundation 不执行 migration 或初始化产品表。Host 数据库没有可供运维
调用的 generation/journal API；不要把外部通用引擎的术语写成产品现有能力。

当前数据库身份是 application `host-monitoring`、version `0.9.19`、schema revision `6`、SHA
`dc97f6526439673f7a633a15a2557e758e922bc49749f8b9562ee8ee3ed7048d`。`_sarmg_administrators` DDL 先用 CHECK/
UNIQUE 约束 canonical username、非空 hash 和布尔 active；`serve`/`admin-create` 再用 Foundation
primitive 检查已有 username 与完整 Argon2id 参数。`doctor` 检查 Schema/integrity/FK，但不做这项账户
加载检查。

## 3. 管理员登录和配对

```text
{username,password} -> Foundation username 规范化 + Argon2id 校验 -> SQLite Session + CSRF
  -> 受保护管理 API 创建实例并返回该实例的长期 authorization code
  -> Client 创建含 token/polling-secret 摘要的 pairing request
  -> code 经 Client 激活端点或管理员激活端点提交
  -> Server 在一个事务中绑定 invite/request/Host/credential
  -> Client 轮询到 active 后原子提交 active-binding.json
```

授权码在 Server 以摘要和认证加密密文同时保存，首次绑定不会删除；正常报告使用独立 Client credential。管理员更换授权码会撤销旧 credential、把实例恢复为 pending，Client 必须使用新授权码显式执行 `pair recover` 或重新运行 `setup`；只有明确放弃旧身份时才使用 `pair replace`。

来源、设备、请求/邀请和管理员账户分别拥有有界准入预算；TCP peer 是来源事实，默认不信任 forwarded
address。登录请求恰好是 `{username,password}`。候选 username 必须是 1..64 字节 printable ASCII；
Server 使用 Foundation 唯一规则 trim ASCII whitespace 并转 ASCII 小写，然后要求 canonical 值为
3..64 字节、首尾 `[a-z0-9]`、全部字符仅 `[a-z0-9._-]`，明确禁止 `@`，相邻分隔符允许。持久
`_sarmg_administrators.username` 只保存 canonical 值并具有 UNIQUE 约束。

登录与 Session 查询只返回 Foundation 精确 `AdministratorSession`：`authenticated=true`、`user_id`、
`username`、`role=admin`、`csrf_token`，不得出现 email、权限数组或附加字段。`admin` 是默认 username；
固定的是 `role=admin`，不是 username 只能叫 `admin`。产品没有 viewer/operator/RBAC。相同 pairing
request 重放幂等，单设备最多保留四个 live pending 请求。

Web 只创建一个 Foundation `AdministratorApiClient`，React 通过共享 hook 以 username 恢复/登录/登出并
响应 401；Session 与 CSRF 仅保存在该 client 的内存闭包。产品代码负责 Host 与实例 API 的精确响应
guard。当前页面提供实例创建、授权码查看/轮换、取消/删除、`/activate/{request_id}` 设备核对与激活、
Host 列表、完整最新详情、备注/删除和历史趋势；audit 查询与导出仍未提供。

## 4. Client 采集与投递

长驻 Client 读取严格 `application_version=0.9.4` 配置并锁定 state directory，初始化 host identity、
采集器和 spool 后才报告服务 ready。按基础/慢速周期生成报告，周期加入受限 jitter；报告先入 spool，
再通过当前 `/api/v2/host-monitor/report` 投递。关机信号停止新采集，并尽力收敛已拥有工作。

## 5. 服务端写入和过载

认证报告经过字段、Host、credential、report ID 和速率验证后，最多等待 10 ms 进入默认 256 容量队列。
单 writer 最多把 64 条放入一次事务，以 per-report savepoint 隔离撤销凭据或冲突 ID。队列满返回 429；
writer 停止、总等待超时或写入失败返回 503；两者带 `Retry-After: 1`。客户端断开不会取消已经入队、
归 writer 所有的工作。

所有 `/api` 失败都输出 Foundation 当前 `ErrorEnvelope`。Client 只有在严格 JSON、正确 Content-Type、
状态码/机器码一致且 `retryable=false` 时才作永久 spool/凭据裁决：`401 + unauthorized` 进入重新授权，
`403 + client_host_mismatch` 只永久丢弃该错误 Host 的报告。代理/WAF 的文本或非合同响应保持可重试，
不得改变凭据。

## 6. 聚合与保留

```text
超过 raw retention 的非 latest 报告
  -> 按 Host + UTC 小时选择有界批次
  -> 幂等聚合 count/min/max/avg/区间
  -> 标记已纳入的 raw rows
  -> 独立事务有界删除
  -> 超过 aggregate retention 的小时行再删除
```

默认 raw 7 天、aggregate 365 天。崩溃在聚合和删除之间不会丢失或双计；每个 Host 的 latest report 永久
避开聚合删除。维护任务启动即运行，随后默认五分钟一次，并受事务数、行数、时间和 yield 预算限制。
历史 API 可查询 raw 标量，并在自动粒度模式中无重复合并未归档 raw 与小时聚合；Web 提供有界历史趋势。

## 7. 平台安装生命周期

- Linux：包创建专用账户、安装 0600 配置和 systemd unit；卸载保留状态，显式 purge 才删除本地状态。
- Windows：MSI 安装原生 Windows Service、交互 CLI 和无控制台维护 helper；管理员在终端中配置/配对，
  CLI 通过受保护的本机状态通道读取运行事实，安装失败必须回滚。
- macOS：pkg 安装不可登录账户、LaunchDaemon 和日志轮转；卸载脚本验证路径和身份后清理，失败测试
  保证不会删除无关账户/文件。
- 移动平台：只交付 Rust 宿主库源码/构建合同，不执行系统安装生命周期。

## 8. Server 发行流程

```text
build.rs 拒绝非 x86_64-unknown-linux-gnu
  -> 打包器确认 x86_64 glibc Linux 宿主
  -> 干净且 annotated v0.9.19 tag == HEAD
  -> npm ci + Web build
  -> 显式 target + source revision 绑定 Rust release build
  -> 严格全树 manifest
  -> deterministic archive + checksum
  -> 解包、重定位、真实启动和静态资源探测
  -> 篡改/额外文件/权限负例
```

归档没有 migration、backup 或 restore。版本目录不可覆盖，也没有 `current` 链接；不生成 ARM Linux、
musl、Windows 或 macOS Server 归档。Windows Client 的 MSVC 构建继续由独立 CI 矩阵负责。

## 9. 维护流程

`doctor` 取得 maintenance 共享锁，并执行 Schema identity、integrity 与 foreign-key 检查；因此它可以与
Server 并行执行，但不验证管理员用户名/hash，也不构成离线快照、修复、备份或恢复。
`admin-create`、`admin-reset-password` 和外部有状态维护取得排他锁，
运行中的 Server 会让排他维护立即失败。产品本身没有跨版本流程；外部工具只有在存在已评审的具体
转换边时才能处理非当前输入。当前安装只读取当前凭据和数据身份。
