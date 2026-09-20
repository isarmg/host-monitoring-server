# Host Monitoring 完整功能与取舍清单

## 0. 开发者决策台账

本表覆盖 Client、Server、Web、协议、打包和运维。分类只能取“核心、保障、可选、建议保留、开发运维”；
复杂度是删除或重构时需要同步修改并验证的闭包，而不是实现代码行数。每行的“实现/主要依赖”都是当前
源码锚点；若锚点不存在或只在文档出现，该项就不能标为已实现。隐藏客户端或 Web 入口不代表后端能力
已删除；反过来，只有表/route 而没有调用方或读取面，也不能宣称端到端能力完成。

| ID | 功能/特性与当前实现 | 实现/主要依赖 | 分类 | 复杂度 | 删除后的确定后果 | 最低验证 |
|---|---|---|---|---|---|---|
| HOST-001 | CPU、内存、磁盘、网络基础遥测采集 | `host-monitoring-client/src/collectors/mod.rs::SystemSampler`、`protocol/src/report.rs` | 核心 | 高 | 失去主机健康的基本可观测性 | Linux/Windows/macOS 单位、集合上限、缺失值 |
| HOST-002 | Linux hwmon 温度采集（当前不采集风扇转速） | `host-monitoring-client/src/collectors/linux_hwmon.rs` | 建议保留 | 中 | 基础负载仍在，但热故障不可见；删除不会影响不存在的 fan 指标 | 无传感器、权限、越界 symlink、设备变动、milli°C 换算 |
| HOST-003 | 编译期可选的 NVIDIA/NVML GPU 指标 | `host-monitoring-client/src/collectors/nvidia.rs`、`nvidia` feature、Linux GPU drop-in | 可选 | 高 | 非 NVIDIA 主机无影响；NVIDIA 温度/负载/显存/功耗/时钟/PCIe 指标消失，AMD/Intel sysfs 与 Windows 路径不受影响 | 无 NVML/驱动、权限、GPU lost 后重试初始化、部分设备失败、单位 |
| HOST-004 | Windows PDH/GPU 采集与 recovery | PDH buffer、Windows collector | 可选 | 高 | Windows 基础指标可保留但 GPU/特定计数器缺失 | counter 重建、缓冲增长、locale |
| HOST-005 | 周期采集和结构化 report contract | monitor runtime、`protocol/` | 核心 | 高 | Client 无法形成 Server 可接受的报告 | 版本、时间戳、重复/缺字段 |
| HOST-006 | 报告、OTLP、配对正式环境固定 HTTPS；HTTP 仅允许 loopback 开发 | `host-monitoring-client/src/config.rs::validate_endpoint/validate_pairing_endpoint`、`transport.rs` | 保障 | 高 | 放宽会暴露设备 credential 和遥测 | 远程 HTTP 无条件拒绝、错 CA/主机名、loopback |
| HOST-007 | 有界磁盘 spool | `host-monitoring-client/src/spool.rs`、`private_fs.rs`、`atomic_file.rs` | 保障 | 高 | Server 短暂不可达时丢数据；无界实现会写满磁盘 | 满额、重启、损坏/隔离条目、FIFO、链接/权限 |
| HOST-008 | delivery retry/backoff、jitter 与严格错误机器码分类 | `monitor_app/delivery/{runtime,spool}.rs`、`transport.rs::classify_host_monitoring_response` | 保障 | 高 | 瞬时错误造成持续丢数或请求风暴；错误凭据可能无界重试 | 401 `unauthorized`、403 `client_host_mismatch`、429/5xx/网络错误 |
| HOST-009 | at-least-once 投递和基于全局 report ID 的 Server 去重 | report ID、spool、`client_metric_reports.report_id` 主键 | 保障 | 高 | 重试会重复写入或错误丢弃；当前同 Host 重放不比较正文 fingerprint | 同 Host 重放 `accepted=false`、跨 Host 409、乱序、响应丢失 |
| HOST-010 | Client 配置严格解析与显式文件路径 | `config/host-monitor.json.example`、`host-monitoring-client/src/config.rs` | 保障 | 中 | 拼错配置可能静默生效；自动搜索会读取意外文件 | unknown field、非当前 application version、权限、相对 state path |
| HOST-011 | state directory 私有化和原子状态写入 | `private_fs.rs`、`atomic_file.rs`、`pairing/state_storage.rs` | 保障 | 高 | token/spool 可被其他用户读取或状态半写 | mode/owner/symlink/hardlink/断电 |
| HOST-012 | Client 本地单实例锁 | `host-monitoring-client/src/state_lock.rs`、`monitor_app/mod.rs` | 保障 | 中 | 两个 Client 会重复采集和投递、争用 spool | 双启动、异常锁文件、不同 state dir |
| HOST-013 | 配对 create/poll/activate/commit 状态机 | pairing modules、Server API | 核心 | 高 | 新 Client 无法取得绑定身份 | 过期、取消、重复、重启 |
| HOST-014 | 配对凭据只在 commit 后持久化 | pairing state/storage、atomic file | 保障 | 高 | 半完成 pairing 可能留下可用凭据或丢失绑定 | 每个崩溃点故障注入 |
| HOST-015 | 配对按来源/request/设备/实例管理准入，并使用每实例长期 authorization code | `pairing_admission.rs`、`http.rs` pairing routes、`store.rs` invite/pairing tables | 保障 | 高 | 请求可耗尽内存/数据库，authorization code 可被错误绑定；轮换必须撤销旧凭据 | 过期请求、重复提交、每设备 4 pending、全局 4096 pending、轮换后重配 |
| HOST-016 | Client `status`/`doctor` 与 delivery doctor | `monitor_app/diagnostics.rs`、`config.rs::ClientCommand` | 建议保留 | 中 | 运维只能查日志，无法快速确认绑定与队列 | 离线、损坏状态、无权限、默认 doctor 零网络、显式 delivery 写入 |
| HOST-017 | Linux Client systemd service 生命周期 | packaging/linux、unit、scripts | 开发运维 | 高 | Linux Client 需手工保持进程和账户权限 | 当前安装、同版本重装、卸载、清理 |
| HOST-018 | Windows Service、CLI 和本地状态通道 | Windows service host、CLI、WiX | 可选 | 高 | Windows 后台 Client 或受支持的配置入口消失 | ACL、service/CLI IPC、MSI lifecycle |
| HOST-019 | macOS LaunchDaemon 和 Installer pkg | packaging/macos、launchd | 可选 | 高 | macOS 无受支持安装/卸载路径 | account safety、pkg、失败回滚 |
| HOST-020 | 移动宿主库边界 | `mobile.rs`、共享 protocol | 可选 | 中 | 不能嵌入移动宿主，桌面 Client 不受影响 | 生命周期、字段、线程边界 |
| HOST-021 | Server 本地 username/password 管理员认证 | `schema/generated/current_schema.sql::_sarmg_administrators`、`host-monitoring-server/src/{http,store}.rs`、Foundation 管理员组件 | 核心 | 高 | 管理 UI/API 无身份边界 | 正误密码、未知 username、规范化、预算、current hash |
| HOST-022 | Session/CSRF/Origin 保护 | browser sessions、`@sarmg/admin-web`、`@sarmg/http-client` same-origin 请求边界 | 保障 | 高 | 浏览器登录可被窃取、跨站利用或无法撤销 | TTL、撤销、unsafe method、跨源 URL、401 清理 |
| HOST-023 | `/api/v2/host-monitor` 唯一 Client/API 合同 | Server router、protocol crate、client | 核心 | 高 | Client 与 Server 无稳定交互；alias 会扩大攻击与测试面 | current path、其他 path 404、unknown field |
| HOST-024 | Server ingest 严格验证和事务写入 | telemetry writer、SQLite | 核心 | 高 | 不可信 Client 数据可污染库或整批丢失 | size、单位、timestamp、rollback |
| HOST-025 | 数据保留与有界清理 | retention config、maintenance task | 保障 | 中 | 数据库无限增长；删得过激则趋势数据消失 | 时间边界、批量、锁竞争 |
| HOST-026 | 管理 API 按名称稳定排序并提供全部实例与 Host、同快照 latest 详情、raw 原始历史及 raw/hourly 有界聚合历史 | `http.rs::{list_instances,list_hosts,host_detail,host_history}`、`store.rs::{list_invites,list_hosts,get_host,history,history_series}` | 核心 | 高 | 采集虽仍落库，但管理员无法读取当前状态或长期趋势 | 完整有序实例列表、from/to、点数/跨度预算、raw/hourly 无重复、权限 |
| HOST-027 | React/Vite 管理页：登录/恢复/登出、实例与配对、Host 列表/详情、历史趋势 | `web/src/{App,Instances,HostDetails,HostHistory}.tsx`、`api.ts`、Foundation admin hook/Vite/TS baseline | 建议保留 | 中 | API 保留，但仓库没有任何内置浏览器管理与趋势视图；删除不影响 Client 摄取 | 精确工具链、clean build、auth、响应 guard、退出清空、实例/配对与历史浏览器测试 |
| HOST-028 | SQLite 当前 Schema identity/doctor | 产品文件预检/DDL/锁、`sarmg-sqlite`、`sarmg-schema-identity` | 保障 | 高 | 错库/漂移库可被误用 | wrong SHA/version、sidecar、corruption、连接 PRAGMA |
| HOST-029 | Server 单实例和 maintenance lock | runtime lock、数据库锁 | 保障 | 高 | 双 Server 可重复清理/写入并破坏一致性 | 双启动、维护冲突 |
| HOST-030 | source-bound release 与 Web fingerprint | package script、release manifest | 开发运维 | 高 | 二进制和 Web 可能混代，来源不可证明 | missing/extra/tamper/relocate |
| HOST-031 | Linux/macOS/Windows Client 安装生命周期测试 | packaging tests | 开发运维 | 高 | 权限、账户、卸载回归可能进入发行 | 各平台 clean fixture |
| HOST-032 | CI Rust/Web/protocol/supply-chain 门禁 | `.github/workflows/ci.yml` | 开发运维 | 中 | 跨组件合同漂移无法提前发现 | clean checkout 全门禁 |
| HOST-033 | 中文学习、流程、功能和运维文档 | README、`docs/` | 开发运维 | 低 | 开发者难以理解跨平台边界 | 链接和命令抽查 |
| HOST-034 | 明确不做远程执行、配置下发和多版本 Client 协商 | 不存在对应 route/command | 核心 | 高 | 新增会把只读遥测 Client 变成远控系统并扩大威胁面 | 独立威胁模型与协议设计 |
| HOST-035 | Server 共享原语固定 Foundation `=0.8.8` + 完整 revision `946bbb5f4bf3e06d421b6f463bd637b83617a5be`；八个 Web 包固定同版 Release URL + lock integrity；Client 使用独立上游 | 根 `Cargo.toml`、`web/{package.json,package-lock.json}`、Foundation 门禁 | 保障 | 高 | 认证、构建及 SQLite 基线漂移；sibling/path 会破坏独立 checkout | Foundation 合同、完整 rev/八个 URL/integrity、Router→Client、Web clean build、SQLite reopen |
| HOST-036 | Server 仅 x86_64 GNU/Linux 的三层平台门禁 | `build.rs`、启动 `uname`、release script/CI | 保障 | 中 | 意外产生或运行未支持的 ARM/musl/Windows/macOS Server | 非目标编译拒绝、打包宿主拒绝、运行身份检查 |
| HOST-037 | 管理 role 只有 `admin`；数据库不存 role 列，`admin` 只是默认 username | `sarmg-contracts::AdministratorRole/AdministratorSession`、`schema/generated/current_schema.sql::_sarmg_administrators` | 核心 | 中 | 加回角色会扩大授权矩阵并使各产品管理语义重新分叉；把默认 username 当唯一 identity 会错误拒绝合法管理员名 | schema 列检查、响应 exact-shape、非 `admin` role 拒绝、其他 canonical username 正例 |
| HOST-038 | 管理员 username 使用 Foundation 唯一 current 规范化：candidate 1..64 printable ASCII，trim ASCII + lowercase 后 canonical 3..64、首尾字母数字、字符 `[a-z0-9._-]`、禁止 `@` | `sarmg-admin-auth::{normalize_administrator_username,require_canonical_administrator_username}`、`store::normalize_username`、`_sarmg_administrators.username` | 保障 | 中 | 大小写/空白别名会拆分限流和唯一约束；放入 email 语义会重新引入跨产品身份分叉 | ` Admin `→`admin`，内部空格/`@`/非 ASCII/首尾分隔符/过短过长拒绝，相邻分隔符允许，启动存量扫描 |
| HOST-039 | 密码只接受 Foundation 当前 Argon2id 参数 | `sarmg-admin-auth`、`sarmg-admin-core`、`host-monitoring-server/src/store.rs` | 保障 | 高 | 弱参数或多套验证路径会降低抗破解性并增加兼容负担 | PHC 参数、错误密码、非当前 hash 启动拒绝 |
| HOST-040 | 未知账户执行 Foundation 固定 dummy hash 验证 | `sarmg-admin-core::AdministratorService` 的登录准入 | 保障 | 中 | 响应时间会泄露账号是否存在；CPU 也可能被无界占满 | 已知/未知账号成本、并发 permit、超时 |
| HOST-041 | 登录按 TCP 来源和规范 username 分别限流，username bucket 不保存明文 | Foundation 管理员服务与 Axum adapter、`host-monitoring-server/src/http.rs` | 保障 | 中 | 单来源可爆破多个账号，或单账号可被分布式爆破；直接存明文 key 会增加内存披露 | 两类 bucket、规范化拼写共享预算、容量回收、`Retry-After` |
| HOST-042 | 登录正文严格 JSON、字段固定且有上限 | Foundation login DTO、Axum body limit | 保障 | 低 | 拼写错误会被静默忽略，超大密码可消耗解析/哈希资源 | unknown/missing/duplicate 语义、超限、Content-Type |
| HOST-043 | `Origin`、全部 `Host`、URI authority、`Sec-Fetch-Site` 原始值共同裁决 | Foundation origin policy 与 Axum adapter | 保障 | 高 | 代理或 HTTP/2 歧义可绕过同源检查 | 缺失、重复、逗号合并、Host/:authority 冲突 |
| HOST-044 | 生产只接受 HTTPS origin；HTTP 仅回环开发 | `AdministratorOriginMode`、Server 配置 | 保障 | 中 | 非回环明文登录会暴露密码和 Session | HTTPS、localhost/127.0.0.1/::1、非回环 HTTP |
| HOST-045 | Cookie 字段行与同名 Cookie 都必须唯一 | `sarmg-admin-auth::parse_cookie_value` 与 Foundation Axum adapter | 保障 | 中 | 代理/框架解析差异可造成 Session fixation 或选择器歧义 | 两个 Cookie header、同名 pair、坏字符、错形 token |
| HOST-046 | Session token 与 CSRF token 均为 32-byte 随机、URL-safe 43 字符 | Foundation token primitives 与 SQLite store | 保障 | 中 | 可预测或宽松 token 形状会降低熵并扩大解析面 | 长度/字符集、碰撞负例、只存 SHA-256 |
| HOST-047 | Session 同时具有 idle 与 absolute TTL；使用时最多按 60 秒 touch 节流刷新，且永不越过 absolute | Foundation 管理员服务、`auth_sessions` 与 SQLite store | 保障 | 高 | 无 absolute TTL 会形成长期凭据；无 idle TTL 会扩大遗留会话窗口；每请求写 touch 会放大数据库竞争 | 边界时刻、刷新不越 absolute、过期请求拒绝、短间隔请求不写；当前没有全局过期行清理 worker |
| HOST-048 | 每个管理员会话只保存一个当前 CSRF 摘要，恢复会话时以 compare-and-swap 轮换 | `_sarmg_admin_sessions.csrf_hash`、Foundation SQLite store | 保障 | 中 | 保留旧摘要会扩大被窃 token 的使用窗口；非原子轮换会让并发恢复覆盖新状态 | 轮换、旧 token 拒绝、并发恢复、撤销 |
| HOST-049 | 登录、Session、退出及含敏感配对材料的成功响应禁止缓存；Foundation Web client 请求统一 `cache=no-store` | `http.rs` 的显式 `Cache-Control`、`@sarmg/admin-web` | 保障 | 低 | 共享缓存可能重放 Session/CSRF/authorization 数据或显示错误身份 | 各成功响应 `Cache-Control: no-store`、客户端请求；当前全局错误响应未统一加该 header，不能宣称已覆盖 |
| HOST-050 | 修改管理员账号会在 Foundation SQLite 事务中提升 `session_version` 并撤销已有会话 | Foundation `update_own_account` / SQLite store、`store.rs::reset_admin_password` | 保障 | 高 | 泄露的旧 Session 在改密后仍可使用 | 改名/改密事务、当前会话失效、并发请求 |
| HOST-051 | `serve`/`admin-create` 在已有账户时扫描每个持久 canonical username 与 current hash；`doctor` 不做这项账户扫描 | `store::ensure_admin_user/validate_stored_administrator_users`、`main.rs` | 保障 | 中 | 问题记录会等到登录时才暴露，并诱发隐式兼容路径；若误称 doctor 已覆盖会形成错误运维证据 | 任一坏 username/hash 阻止 serve，doctor 的现有范围单独验证，数据库字节不变 |
| HOST-052 | 浏览器认证 API 固定为 Foundation 三条 `/api/v2/auth/*` | `sarmg-contracts` 常量、`http.rs`、Web client | 核心 | 中 | 路径别名会形成多套安全策略和长期维护面 | 三条正例、其他版本/尾斜杠/子路径 404 |
| HOST-053 | 所有 API 错误使用 Foundation 严格 envelope | `error.rs`、Client `transport.rs` | 保障 | 高 | Client 无法可靠区分永久/瞬时错误，UI 只能解析字符串 | status/code/retryable、unknown field、非 JSON 上游 |
| HOST-054 | Client 只依据严格错误码删除或保留凭据/Spool | `transport::classify_host_monitoring_response` | 保障 | 高 | WAF/代理伪响应可能导致凭据或遥测永久丢失 | 401/403/409/429/5xx、错 MIME、坏 envelope |
| HOST-055 | 协议结构统一 `deny_unknown_fields` | `protocol/src/report.rs`、`pairing.rs` | 保障 | 中 | 拼错字段可能被当成功，双方合同悄然漂移 | 每一层 unknown field、缺字段、错误枚举 |
| HOST-056 | 跨语言 `u64` 采用规范十进制字符串 | `protocol/src/json_u64.rs` | 核心 | 中 | JavaScript 超过 2^53 后丢精度，计数器和字节量错误 | 0、`u64::MAX`、前导零、符号、小数、溢出 |
| HOST-057 | UUID 必须使用解析后重新格式化一致的小写连字符文本 | protocol DTO、`model.rs::canonical_uuid` | 保障 | 低 | 同一标识可有多种文本，唯一约束与日志关联失真 | 大写、无连字符、缺段、非十六进制；代码不额外限定 UUID version |
| HOST-058 | 报告以稳定 `report_id` 在 raw retention 窗口内由数据库全局唯一约束去重；窗口到期后不承诺永久 exactly-once | `ClientReport`、telemetry schema/writer、`retention.rs` | 保障 | 高 | 窗口内删除依据会让确认丢失后的重投重复聚合；无限期 tombstone 又会无界增长 | 同 Host 重投、跨 Host 复用、旧样本立即维护、窗口到期边界、ack identity |
| HOST-059 | Server acknowledgement 回显 Host/report identity 且形状严格 | `ClientReportAck`、Client transport | 保障 | 中 | 代理缓存或错配响应可能误删错误 Spool 条目 | identity mismatch、缺 `accepted`、unknown field |
| HOST-060 | HTTP 接收与 SQLite 写入由有界 telemetry queue 解耦 | `telemetry.rs`、`http.rs` | 保障 | 高 | 直接写库会让慢事务占住连接；无界队列会耗尽内存 | 队列满、入队超时、worker 停止、503/429 |
| HOST-061 | Writer 使用有限 batch 与每报告 savepoint | `telemetry.rs` | 保障 | 高 | 单坏报告可能回滚整批；无限 batch 会形成长事务 | 中间一条失败、前后报告提交、事务时限 |
| HOST-062 | Latest 状态只被明确更新规则推进 | telemetry SQL、latest query | 核心 | 高 | 迟到报告可能覆盖较新事实，控制台倒退 | 乱序、相同时间、重复报告 |
| HOST-063 | Raw 与小时聚合采用 UTC 和幂等纳入 | `retention.rs`、聚合表 | 保障 | 高 | 崩溃重跑会双计或先删后丢；本地时区会遇到 DST 歧义 | 同小时重跑、崩溃点、UTC 边界 |
| HOST-064 | Retention 分批、限时并主动 yield；积压时短间隔续作、失败退避并接入 worker 健康 | `retention.rs` 配置与 worker、`main.rs` | 保障 | 中 | 大清理事务会阻塞摄取；固定慢周期会持续积压；静默失败无法被发现 | 大量过期行、锁竞争、预算耗尽续作、连续失败与恢复 |
| HOST-065 | Readiness 同时检查数据库、保留结构与 writer 活性 | `http.rs::ready` | 开发运维 | 中 | 进程存活会被误当可接收遥测，负载均衡继续送入失败实例 | 关闭 writer、坏 schema、数据库不可用 |
| HOST-066 | 新库只在目标文件不存在时创建，已有库只读预检 | `database_schema.rs`、`store.rs` | 保障 | 高 | 启动可能暗改未知数据库，违背 current-only | 空文件、未知表、非当前 metadata、byte snapshot |
| HOST-067 | Schema identity 同时绑定 application/version/revision/SHA | `sarmg-schema-identity`、`database_schema.rs` | 保障 | 高 | 只看版本或只看 hash 都可能把错产品库当当前库 | 四字段逐项漂移、实际 schema 重算 |
| HOST-068 | SQLite 连接统一 foreign keys、WAL、busy timeout、synchronous 基线 | `sarmg-sqlite`、`database_schema.rs` | 保障 | 中 | 各连接 PRAGMA 漂移会导致约束失效或耐久性不同 | 每连接 PRAGMA、FK/integrity、checkpoint |
| HOST-069 | 数据库文件创建失败会清理不完整产物并同步目录 | `database_schema.rs` | 保障 | 高 | 半初始化文件会阻止下次启动或被误认为有效库 | DDL/hash/fsync 故障注入、sidecar 清理 |
| HOST-070 | 同库 Server 取得 instance 排他锁 + maintenance 共享锁；doctor 取 maintenance 共享锁；admin-create/reset 取 maintenance 排他锁 | `database_lock.rs::{ApplicationLock,MaintenanceLock}`、`main.rs` | 保障 | 高 | 第二 Server、在线只读检查与离线写维护会失去明确协调；把所有操作误写成排他又会阻止受支持的在线 doctor | 双 Server 拒绝、Server+doctor 成功、Server+admin 失败、两个 doctor 成功、异常退出后重新获取、sibling lock 类型/mode/nlink |
| HOST-071 | Server 可提交配置样例固定在 `config/host-monitoring.env.example`，生产安装路径固定 `/etc/isarmg/host-monitoring.env` | `config/host-monitoring.env.example`、`host-monitoring-server/src/config.rs`、`deploy/host-monitoring-server.service` | 开发运维 | 低 | 示例散落会让文档和部署读取不同值；删掉环境文件会使 systemd 启动失败 | 样例唯一路径、username env、必填变量、数值边界、环境文件 0600；进程不解析文件本身，也不拒绝无关环境变量 |
| HOST-072 | Server systemd 源资产固定在顶层 `deploy/` | `deploy/host-monitoring-server.service` | 开发运维 | 低 | 嵌套副本会漂移；错误 unit 可能绕过环境或平台门禁 | package source 唯一、`systemd-analyze verify` |
| HOST-073 | systemd unit 声明 `ConditionArchitecture=x86-64` 和安全沙箱 | `deploy/host-monitoring-server.service` | 保障 | 中 | 错架构启动或过宽文件/系统调用权限扩大故障面 | unit 静态验证、读写路径、环境文件 |
| HOST-074 | Server release 固定 `README.md`、`bin/`、`web/`、`systemd/` 树；生产配置不进入归档 | `package-server-release.py`、`release_bundle.rs::validate_exact_layout` | 开发运维 | 高 | 文件缺失/多余或模式漂移仍可能被安装；若把配置打包会诱发 Secret/站点值复用 | missing/extra/symlink/hardlink/mode/size，确认无 `config/` |
| HOST-075 | Release identity 绑定当前 target、schema、source revision | `release_contract.rs`、binary self-report | 保障 | 高 | 不同提交或数据合同的组件可被拼装在一起 | 40-hex revision、target、schema、unbound release 拒绝 |
| HOST-076 | Release 验证后仍支持重定位而不依赖 checkout | `release_bundle.rs`、package tests | 开发运维 | 中 | 包只能在构建目录运行，无法作为正式工件部署 | 移动目录、只读树、无仓库 sibling |
| HOST-077 | Server Rust 工具链固定 1.98.0 | `rust-toolchain.toml`、CI | 开发运维 | 低 | 编译器/格式化结果漂移，安全门禁不可复现 | `rustc --version`、fmt/clippy/check/test |
| HOST-078 | Web 工具链精确固定 Node 26.7、React 19.2.8、Vite 7.3.6、TS 5.8.3 | `.node-version`、package manifest、Foundation assertion | 开发运维 | 中 | 本地与 CI bundle 不同，类型与运行语义漂移 | clean `npm ci`、toolchain assertion、build |
| HOST-079 | Web 认证状态只在 Foundation client closure/React hook 中 | `@sarmg/admin-web`、`App.tsx` | 保障 | 中 | local/sessionStorage 会延长 Secret 生命周期并产生多套状态 | storage scan、reload restore、logout 清理 |
| HOST-080 | 登录/恢复/退出按 generation 和队列消除异步竞态 | `@sarmg/admin-web`、Host Web | 保障 | 高 | 旧 401 或迟到 restore 可覆盖新登录 | login/logout overlap、stale 401、unmount |
| HOST-081 | Web 对 Host 列表响应及每个 Host/capability 执行 exact-key 产品 guard | `web/src/api.ts::isHostListResponse` | 保障 | 中 | 后端漂移会以 `undefined` 或错误 JSON 静默传播 | 缺/多字段、UUID/UTC、非有限数、capability error kind、空列表 |
| HOST-082 | Foundation design token、reset、accessibility CSS 为唯一共享视觉基线 | `web/src/main.tsx`、`@sarmg/design-tokens` | 建议保留 | 低 | 页面仍能运行，但跨项目视觉和焦点/动效规则重新分叉 | `data-sarmg-scope`、键盘、reduced-motion |
| HOST-083 | Web 对匿名态清空业务数据并用 effect cancellation 阻止先前请求回填 | `web/src/App.tsx` | 保障 | 中 | 退出后可能短暂显示上一管理员看到的主机数据 | logout/401、慢请求完成、重新登录 |
| HOST-084 | Client 配置只从显式文件读取且包含当前 application version | `config.rs`、`config/host-monitor.json.example` | 保障 | 中 | 自动发现或宽松版本会误读其他安装的数据 | 缺文件、unknown field、非当前 version、权限 |
| HOST-085 | Client 报告端点禁止 URL 内嵌 credential、禁用 redirect，并将远程明文 HTTP 置于显式高风险开关后 | `config.rs::validate_endpoint`、`transport.rs::build_client` | 保障 | 中 | redirect 可跨端点重放 bearer/body；放宽默认会明文泄露主机画像 | userinfo 拒绝、30x 不跟随、默认远程 HTTP 拒绝、显式持久开关、HTTPS 成功 |
| HOST-086 | Client 支持系统 trust、自定义 CA；Linux 客户端身份只收 PEM，Windows/macOS native TLS 只收 PKCS#12，identity password 不能脱离 PKCS#12 | `transport.rs::build_client`、平台 TLS backend | 可选 | 高 | 私有 PKI/mTLS 部署不可用；混用格式会产生只在部分平台失败的配置；放宽会威胁证书校验 | 错 CA/主机名/期限、Linux PEM 正反例、Windows/macOS PKCS#12 正反例、孤立 password 拒绝、确认没有 danger-accept-invalid 入口 |
| HOST-087 | 网络响应先验证 status、MIME、大小和严格 DTO | `transport.rs` | 保障 | 中 | HTML 代理页或超大正文可能被当协议数据 | 错 MIME、超限、坏 UTF-8/JSON、unknown fields |
| HOST-088 | Spool 采用配置字节上限、4096 条上限、文件名 FIFO、跨实例 mutation lock 和私有路径 | `spool.rs`、delivery spool adapter | 保障 | 高 | 无界磁盘/inode 耗尽或乱序投递；并发确认与淘汰会破坏所有权 | 容量/4096 边缘、重启、损坏隔离条目、链接/权限、并发 acknowledge |
| HOST-089 | 成功 acknowledgement 后才删除 Spool 条目 | delivery reporter/runtime | 保障 | 高 | 发送返回前删除会丢数；未验证即删会受代理假响应影响 | 断线点、identity mismatch、重复发送 |
| HOST-090 | 采集、落盘、发送和 shutdown 使用明确所有权转移 | `monitor_app/delivery` | 保障 | 高 | 取消任务时报告可能同时被遗失或重复持有 | Ctrl-C、worker panic、队列 drain、超时 |
| HOST-091 | 单次模式 `once` 与长期 service 复用同一合同 | `monitor_app/delivery/once.rs`、runtime | 建议保留 | 中 | 无法做低风险探测；若分叉则一次与常驻结果不同 | 同输入报告、退出码、网络/Spool 行为 |
| HOST-092 | systemd readiness 使用 `Type=notify` | Client unit、`monitor_app/systemd.rs` | 保障 | 中 | 服务管理器会在初始化/锁/状态校验前宣布成功 | READY 时机、启动超时、错误不通知 |
| HOST-093 | Linux 包只管理有证据的专用账户和当前标记 | packaging lifecycle scripts | 保障 | 高 | purge 可能删除无关同名账户，或残留敏感状态 | UID/GID 绑定、NSS 失败、symlink、partial marker |
| HOST-094 | Windows Service、maintenance helper、CLI 权限边界分离 | Client Windows modules、WiX | 保障 | 高 | 交互用户可能获得服务权限，或服务无法安全配置 | LocalService、ACL、named-pipe IPC、rollback |
| HOST-095 | macOS 包只操作可证明归属的账户、LaunchDaemon 和日志资产 | packaging/macos | 保障 | 高 | 安装失败或卸载可破坏其他本机资源 | clean install、失败回滚、卸载/purge、owner/mode |
| HOST-096 | Client Linux 打包器可接受经 ELF machine/version marker 验证的 amd64 或 arm64 payload；当前 CI 只静态验证两种打包分支，不发布 arm64 Client | `packaging/linux/build-packages.sh`、`tests/test-build-packages.sh` | 可选 | 高 | 删除 arm64 分支只影响 Client 潜在交付；误写成已发布 arm64 artifact 会制造不存在的支持承诺；误用于 Server 会扩大服务端矩阵 | 两种 ELF machine/包架构、错架构拒绝、版本 marker、Server target 独立门禁 |
| HOST-097 | 移动适配层只接受宿主快照，不拥有后台、网络和 Secret | `mobile.rs` | 可选 | 中 | 删除后移动宿主不能复用协议；扩权会引入平台生命周期 | 空架构、字段映射、线程/生命周期、无 token 持久化 |
| HOST-098 | 可选 OTLP 是遥测旁路，不参与 Server 交付裁决 | `otlp.rs`、配置 | 可选 | 高 | 删除只失去外部观测；若耦合则 collector 故障会阻塞主路径 | endpoint/auth、失败隔离、u64/arch 语义 |
| HOST-099 | 日志与诊断对 token、Authorization、主机敏感内容做最小披露 | transport/logger/diagnostics | 保障 | 中 | support bundle 或日志可成为凭据泄漏面 | header redaction、错误正文截断、公开报告脱敏 |
| HOST-100 | 依赖 action、Rust/Node 版本和 release 工具被供应链门禁固定 | CI、workflow checks、lockfiles | 开发运维 | 中 | 上游 mutable 引用或未锁依赖使构建不可复现 | full SHA action、locked install、clean checkout |
| HOST-101 | 公共 health 分离 live 与 ready；ready 同时裁决当前 Schema、retention 结构和 writer 通道 | `http.rs::{live,ready}` | 开发运维 | 低 | 负载均衡无法区分进程存在与可持续接收报告 | `/health/live`、`/health/ready`、关闭 writer、坏 retention 表 |
| HOST-102 | 管理 API 创建/列出/取消 Client invite，创建时只返回一次 activation code | `http.rs::{create_instance,list_instances,cancel_instance}`、`store.rs` | 核心 | 高 | 没有受控 invitation 就无法把 pairing request 绑定到管理员创建的实例 | 201/no-store、最多 200 列表、pending cancel、过期、重复状态 |
| HOST-103 | activation code 可由 Client 激活端点提交，或由已认证管理员端点提交；两者进入同一事务状态机 | `CLIENT_ACTIVATE_PATH`、`CLIENT_ADMIN_ACTIVATE_PATH`、`http.rs::activate` | 核心 | 高 | 激活路径删除后 pending 永远不能成为 Host；分叉实现会造成绑定差异 | code 错误/过期/重放、request/invite 一致、CSRF 管理端、幂等 active |
| HOST-104 | 管理 API 可修改 Host 备注或永久删除 Host；当前是 last-write-wins，没有 revision/ETag 乐观并发 | `http.rs::{update_remark,delete_host}`、`store.rs` | 核心 | 高 | 删除备注更新只影响命名；删除 Host 删除会使凭据、报告、聚合、pairing/invite 一并丢失 | 404、255-byte 备注、CSRF、并发写覆盖语义、删除级联与 audit |
| HOST-105 | invite create/cancel、激活、备注、删除五类动作在同一 SQLite 事务写 `audit_events`；当前没有 audit 读取/导出/保留 API | `schema/product.sql::audit_events`、`store.rs::{audit,create_invite,cancel_invite,activate,update_remark,delete_host}` | 保障 | 中 | 去掉写入会失去这些变更的本地责任证据；误宣称查询会造成合规预期落空 | 动作与业务事务同成败、管理员 actor=user_id、公开激活 actor=`client-capability`、确认路由表无 audit GET |
| HOST-106 | 图表模式在同一只读快照中合并未归档 raw 与 hourly aggregate，按各指标有效样本数加权并返回实际粒度/来源 | `retention.rs`、`store.rs::history_series`、`http.rs::host_history` | 核心 | 高 | 简单相加会在归档过渡期重复计数；平均各小时均值会产生统计偏差 | raw/aggregate 共存、不同 count 加权、空值、边界对齐、最多 1000 点 |
| HOST-107 | 路由分层 body 上限：Foundation 管理员接口 16 KiB、产品管理 API 16 KiB、Client 512 KiB | `sarmg_admin_core::ADMIN_BODY_MAX_BYTES`、`http.rs::router`、`CLIENT_REPORT_MAX_BODY_BYTES` | 保障 | 中 | 统一放大上限会扩大内存/解析攻击面；统一缩小会拒绝合法设备集合 | 上限±1、Content-Length/流式正文、413 Foundation envelope |
| HOST-108 | 报告按 Host 使用 burst 64、16/s refill、有界 16384 entry/15 min TTL token bucket | `http.rs::ReportBuckets` | 保障 | 中 | 单 Host 可淹没 writer；无界 bucket map 可被 Host 标识耗尽内存 | burst/refill、TTL 清理、容量无可淘汰项、`Retry-After` |
| HOST-109 | 开发/生产静态目录必须是绝对真实树且根仅有 `index.html`/`assets`；生产再限制 owner/mode/hardlink | `config.rs::validate_static_dir/validate_static_tree` | 保障 | 中 | ServeDir 可能暴露意外文件、链接目标或服务账户可改内容 | 相对路径、根多余项、symlink/special、depth 32、10000 entries、生产 owner/mode/nlink |
| HOST-110 | 产品 CLI 没有 migration/backup/restore 命令，外部 `sarmg-upgrade` 当前也没有 Host 转换边 | `config.rs::Command` 与 CLI 负例、外部仓当前 edge 表 | 核心 | 低 | 添加隐式入口会绕过 current-only 身份；误删该边界会让运维尝试未支持的数据操作 | CLI 子命令拒绝、文档无可执行迁移步骤、外部 edge 清单为空 |
| HOST-111 | 管理登录请求恰好 `{username,password}`，Session 恰好 `{authenticated,user_id,username,role,csrf_token}` | `sarmg-contracts::{AdministratorLoginRequest,AdministratorSession}`、Foundation 平台路由与 JSON Schema/TS guard | 核心 | 高 | 多余 email/权限字段或缺字段会让 Rust/Web 对身份事实产生分叉 | exact keys、unknown/missing 字段、Rust/TS/Schema fixture、响应不含 email |
| HOST-112 | 空库只 bootstrap 一个当前管理员；已有账户后不提供新增/列表/角色管理，当前登录管理员可通过 Foundation 账号设置修改自己的 username/password | `store::ensure_admin_user`、Foundation 平台路由与 `web/src/App.tsx` | 核心 | 中 | 删除 bootstrap 会让全新实例无法登录；误称完整账户管理会让运维依赖不存在的生命周期 | 空/非空 `_sarmg_administrators`、第二次 admin-create 零新增、当前账号修改、默认 username `admin` |
| HOST-113 | `_sarmg_administrators` 使用 `administrator_id/username/password_hash/active/session_version/created_at_micros/updated_at_micros/last_login_at_micros`，无 email/role；username UNIQUE + canonical CHECK，hash 非空，active 只能 0/1，session_version 必须大于 0 | `schema/generated/current_schema.sql::_sarmg_administrators`、`database_schema.rs` | 保障 | 高 | 加 alias/旧列会扩大存储与查询合同；删除 UNIQUE/CHECK 会让重复或非 canonical identity 绕过应用入口 | `pragma_table_info` exact 列、各 DDL CHECK 负例、唯一冲突、启动 Foundation 二次验证、Schema fingerprint |
| HOST-114 | 当前 DDL 已进入 Schema identity：revision 6、SHA `dc97f6526439673f7a633a15a2557e758e922bc49749f8b9562ee8ee3ed7048d` | `database_schema.rs::{SCHEMA_REVISION,SCHEMA_SHA256}`、`release.json` | 保障 | 高 | 常量/manifest/实际 DDL 任一漂移都会让新库初始化或发行验证失败 | 三处 identity 一致、现场 fingerprint、旧或漂移 DDL 拒绝 |
| HOST-115 | React 登录表单使用 username text/`autocomplete=username`，默认显示 `admin`；认证后展示当前 Session username 与产品管理页面 | Foundation React 登录组件、`web/src/App.tsx` | 建议保留 | 低 | 改回 email input 会与 Foundation request guard 冲突；删除 username 显示会损失身份提示 | 表单 payload、canonical 正例、Session username、登出清空产品数据 |
| HOST-116 | 管理员密码重置只提供排他维护 CLI，密码从单行有界标准输入读取，不进入 argv | `config.rs::AdminResetPassword`、`main.rs::AdminResetPassword` | 开发运维 | 中 | 删除 reset 会失去受支持的当前改密入口 | maintenance lock、错误 username、Session 全撤销 |
| HOST-117 | Server 本身监听 HTTP socket，生产浏览器安全依赖可信 TLS reverse proxy；生产 auth policy仍强制 HTTPS Origin 和 Secure `__Host-` Cookie | `config.rs` 的管理员 origin 模式、Foundation HTTP adapter、systemd `BIND` | 保障 | 高 | 绕过 TLS 代理直出会使登录不可用或暴露其他非 Cookie 流量；把 Server 写成内置 TLS 会误配证书 | production HTTPS Origin、Secure Cookie、loopback development、非回环 development 拒绝 |
| HOST-118 | Web/Server admin 范围与 Client 范围严格分离：Server+内置 Web 只随 AMD64 GNU/Linux release，Client 保留 Linux/Windows/macOS/mobile 合同 | `host-monitoring-server/build.rs`、`scripts/package-server-release.py`、`web/`、Client CI matrix | 核心 | 中 | 把 Client 跨平台能力套到 Server 会产生未验证制品；把 Server 限制套到 Client 会误删现有平台 | Server 非目标拒绝、release target、Windows Client build、macOS/Android/iOS library checks |
| HOST-119 | 遥测 retention 不清理控制面表：audit 无保留任务；过期/撤销 Session 行无全局删除任务；active/cancelled invite/pairing 多数长期保留，只有创建 pairing 时有界清理 expired pending/旧 denied | `retention.rs` 只操作 report/aggregate、`store.rs::create_pairing` cleanup SQL、Foundation SQLite 管理员存储 | 建议保留 | 高 | 直接删控制面历史会破坏责任/会话语义；完全忽略会导致长期数据库增长 | 大量登录/invite/pairing 的增长测试、password reset CSRF 清理、明确新增保留策略前的合规评审 |
| HOST-120 | Linux Client 除 NVIDIA/NVML 外，还从有界 `/sys/class/drm` 只读发现 AMD/Intel GPU，读取可用的 utilization/VRAM/温度/功耗/频率并分别报告 capability | `host-monitoring-client/src/collectors/linux_gpu.rs`、`collectors/mod.rs::GpuRuntime` | 可选 | 高 | AMD/Intel 主机仍有基础遥测，但 GPU 画像消失；若把缺文件填零会伪造利用率/容量 | AMD `0x1002`、Intel `0x8086`、无 DRM、越界 symlink、枚举上限、部分 sysfs 缺失、Intel 无 utilization 时为 unsupported 而非 0 |
| HOST-121 | 官方 Client 在落入 spool/发网前按共享合同规范化、排序、去重、截断并压到 512 KiB；发生收敛时只添加一次 `client.report.truncated` capability | `host-monitoring-client/src/report_contract.rs::{bound_report,encode_report_body}`、`protocol/src/report.rs` | 保障 | 高 | 异常系统枚举可持续制造 Server 400/413 和永久丢点；非幂等截断会让同 report ID 重试正文变化 | 各集合/文本/数值上限、超大 body、优先保留有指标项、两次 bound 字节稳定、logical_count 同步、truncated 标记/错误计数只增加一次 |
| HOST-122 | 原生包的“构建”与“发布者信任”分开：Linux/Windows 当前无仓库内签名步骤；macOS 可校验预签 Mach-O 并签 pkg，但不 notarize/staple，未给 identity 时只产 unsigned prerelease | nFPM/WiX 构建脚本、`packaging/macos/build-pkg.sh` | 开发运维 | 高 | 把结构测试当签名证据会分发无法建立发布者信任的制品；删除 macOS 签名前置校验会形成“已签容器、未签 payload” | 搜索 GPG/signtool 缺口、macOS Developer ID Application/Installer 正反例、`pkgutil --check-signature`、unsigned 标记、确认无 notarization/stapling |
| HOST-123 | 三桌面安装生命周期都是 current-only：Linux 拒绝跨版本 replacement，Windows 无 UpgradeCode/MajorUpgrade，macOS 脚本只接受包内当前 numeric version；均不迁移 Client 状态 | Linux `preremove.sh`/lifecycle tests、Windows `Package.wxs`/authoring tests、macOS pre/postinstall/build tests | 核心 | 高 | 引入隐式替换会让另一版本二进制读取未声明状态；删除同版本生命周期又会使当前安装无法安全重装/卸载 | 同版本重装、不同版本拒绝、无 UpgradeCode/Upgrade/MajorUpgrade、状态保留与显式 purge、无 migration/recovery alias |

## 1. Server 能力

| 功能 | 当前实现 | 取舍/限制 |
|---|---|---|
| 管理身份 | 本地 canonical username、当前 Argon2id、随机 Session/CSRF、Foundation 精确登录与 Session 形状 | 固定 `role=admin`，默认 username `admin`；没有 email、viewer/operator/RBAC，也不依赖中央账户或共享 Session |
| 配对 | 每实例长期 code、Client request/poll、管理员或 Client 激活端点、分维度限流 | React 可查看/更换加密保存的授权码；更换会撤销旧 Client 并要求重新配对 |
| 报告 API | `/api/v2/host-monitor` 当前协议 | 不注册任何平行版本或 alias |
| API 错误 | Foundation `ErrorEnvelope`：`code/message/retryable/request_id?/details?` | 所有 `/api` 非 2xx（含 extractor/404/405）使用同一严格顶层结构 |
| 写入 | 有界队列、单 writer、batch、savepoint | 单库单活进程，不是分布式写集群 |
| 历史 | raw 标量查询及 raw/hourly 自动粒度趋势、两级保留 | 图表查询最长 31 天、最多 1000 点；不是任意时序查询引擎 |
| Web | Foundation 管理员 client/hook + Host 列表 exact guard，编译进发行物 | 当前提供主机列表、采集详情及实例邀请/配对；图表和 audit 等界面仍未补齐 |
| 诊断 | health/readiness、doctor、事务内 audit 写入 | 不提供数据库修复或 audit 读取 API |
| 发布 | source-bound binary、全树 manifest、固定目录 | 同版本不可原地覆盖 |
| 平台 | 仅 `x86_64-unknown-linux-gnu` 构建、发行和运行 | 不提供 ARM Linux、musl、Windows 或 macOS Server；跨平台只属于 Client |

## 2. Client 能力

| 平台/领域 | 能力 | 边界 |
|---|---|---|
| Linux | CPU、内存、磁盘、网络、hwmon、NVIDIA/NVML、AMD/Intel DRM sysfs；systemd/deb/rpm | NVIDIA 设备访问通常需要显式放宽 PrivateDevices drop-in；sysfs 字段缺失按 capability 表达，不填零 |
| Windows | 系统指标、PDH/GPU、Windows Service、CLI、WiX MSI | Service 与 maintenance helper 不弹出控制台；交互 CLI 保持 console subsystem |
| macOS | 系统指标、LaunchDaemon、pkg、newsyslog | 账户和卸载遵循平台安全检查 |
| Android/iOS/iPadOS | 宿主提供快照的 Rust contract library | 无 App 外壳、签名、权限或 APK/IPA |
| 可靠性 | 单实例状态锁、原子凭据、64 MiB 默认 spool | 有界队列会在持续故障时施加容量压力 |
| 网络 | 正式环境固定 HTTPS、可选 mTLS 材料、自定义 CA、可选 OTLP | pairing/report/OTLP 只允许 HTTPS 或 loopback HTTP，不存在远程明文开关 |
| 操作 | run/once/probe/pair/status/doctor | Client 不提供远程命令执行 |

## 3. 关键架构取舍

- SQLite 适合单机独立控制面，部署简单；代价是必须通过单 writer 和有界任务控制写竞争，不能水平多写。
- raw + hourly aggregate 控制数据库增长；代价是过期原始样本无法逐点查询。
- Client 先落本地 spool，网络故障不立即丢数据；代价是本机需要保护和监控状态容量。
- 移动端采用宿主驱动 library，而非强行常驻 daemon；符合平台限制，但采样完整性和周期由 OS 决定。
- Client 在三平台提供原生安装资产，而非容器；能接触真实主机传感器和服务管理，运维矩阵更大。Server
  刻意收敛到 x86_64 GNU/Linux，降低数据库、文件安全与发行验证矩阵。
- Server 仓库拥有共享协议，独立 Client 仓库通过完整 Git revision 固定依赖；协议变化需要同步更新 Client
  依赖并完成两仓验证，不能复制 DTO 或形成 Server 对 Client 的反向依赖。
- Foundation 共享 username/password/hash/token/origin primitive、严格登录/Session/ErrorEnvelope 合同、
  浏览器状态机、same-origin HTTP、React/Vite/TS baseline、SQLite PRAGMA 与 Schema identity；产品继续
  拥有账户/准入、服务端 Session/CSRF 持久生命周期、Cookie、页面、产品响应 guard、DDL、数据库文件/锁
  和业务状态机，避免基础层反向拥有 Host 生命周期。

## 4. 当前版本与明确不做

- Server 只接受 `0.9.21` 配置、数据库与发行身份；不包含转换器或平行 alias。Client 配置格式版本独立冻结为 `0.9.4`。
- 服务端只初始化不存在的当前库，拒绝 metadata-free、非当前 identity 和 Schema drift。
- 产品不包含 migration、backup、restore；`sarmg-upgrade` 当前也没有 Host 转换边，所以这些操作暂不受支持。
- Client 不执行远程 Shell、配置修改、补丁管理或自动修复。
- 移动库不持久化 Token，不实现网络客户端或后台调度。
- 不通过共享运行时、中央网关、共享数据库或 CDN 依赖其他 Sarmg 项目。

## 5. 安全取舍

服务端默认回环监听，由可信 TLS 代理公开；浏览器与 Client 身份分离。Client 凭据、spool 和可选 OTLP
Token 是敏感数据。管理员操作和遥测不应记录 Secret。只有当前发布版本和 `main` 接受安全修复；漏洞
应使用 GitHub Private Vulnerability Reporting，公开 issue 不得包含凭据或生产数据。

## 6. 端到端能力地图

| 阶段 | Client 行为 | Server 行为 | 操作者看到的证据 |
|---|---|---|---|
| 安装 | 原生包创建服务、账户、配置和状态边界 | 安装不可变 Server release | package 生命周期测试、release verify |
| 采集验证 | `probe` 读取当前平台指标 | 无网络参与 | 有界报告摘要与分类错误 |
| 配对 | 保存 pending、轮询并原子提交 binding；CLI/Client 可提交 code | invite/request/activation 事务与 credential 发放 | 浏览器创建/核对/激活、真实 Server 状态与 Client 协议轮询均有隔离验收；不替代真实设备采集 |
| 日常报告 | 采集 -> spool -> HTTPS batch | 认证 -> queue -> writer -> commit | `once`/服务日志、latest 时间 |
| 历史查询 | 无 | latest 完整详情、raw 原始 history、raw/hourly 聚合趋势 | 管理 Web 提供 15 分钟至 30 天范围和 CPU/内存均值曲线 |
| 诊断 | `status`/`doctor`/delivery doctor | health/readiness/doctor | 机器可读结果、request/report ID |
| 外部转换 | 产品不执行 | 仅在外部仓存在明确支持边时离线转换 | 外部 verify + product doctor |

## 7. 指标覆盖与缺失语义

| 类别 | 典型内容 | 平台差异 | 不提供/注意 |
|---|---|---|---|
| CPU | 总体/核心利用、负载相关事实 | OS 计数器来源不同 | 不把采样间隔差异伪装成同一瞬时值 |
| 内存 | 总量、可用、使用 | cache/available 定义依平台 | 单位必须明确，不用负值/溢出 |
| 磁盘 | 卷容量和使用 | mount/drive 模型不同 | 不是磁盘健康/S.M.A.R.T. 管理器 |
| 网络 | 接口计数与速率基础 | 接口命名和重置不同 | 不抓包、不检查用户内容 |
| 温度/传感器 | Linux hwmon 等可用事实 | 设备/权限依赖强 | 缺失表示未支持/不可用，不填零 |
| GPU | NVIDIA/NVML、Linux AMD/Intel DRM sysfs、Windows PDH 等受支持来源 | 驱动、sysfs 字段和 sandbox 影响 | 不是通用 GPU 调度或诊断工具；macOS 当前无专用 GPU collector |

报告只表达当前协议定义的有界字段。新增传感器必须说明单位、采样成本、缺失/重置语义、集合上限、隐私
影响、三桌面平台策略以及聚合方式。

## 8. 配对与凭据功能明细

| 能力 | 当前保证 | 明确边界 |
|---|---|---|
| Pending 持久化 | 网络中断恢复同一请求 | 不静默生成多套身份 |
| 管理员授权 | 管理 API 创建实例；authorization code 可由管理端点或 Client capability 端点提交 | 当前 React 页面可创建实例、查看/更换 code 并提交激活 |
| 实例授权码 | 长期绑定实例，加密保存；轮换时撤销现有 Client credential | 不作为日常报告 credential，不因一次配对而删除 |
| Active binding | 临时文件、sync、原子替换 | 不从半写文件“尽量恢复” |
| 撤销 | Server 拒绝后续报告 | Client 不无界重试被撤销 credential |
| 重新配对 | 用户明确动作建立新当前身份 | 不读取另一个版本状态 fallback |
| 准入 | 来源/设备/请求/邀请/管理员独立预算 | 不能只靠单 IP 限流 |

## 9. 报告可靠性与容量

| 层 | 有界资源 | 满/失败语义 | 为什么这样取舍 |
|---|---|---|---|
| 采集 | 字段、集合、字符串、执行时间 | 缺失或分类错误 | 防异常系统接口制造无界 JSON |
| Client spool | 默认 64 MiB、条目/单报告边界 | 保持可观察压力，不无限占盘 | 短期断网恢复与主机安全平衡 |
| HTTP | body、timeout、认证/速率 | 4xx/429/503 精确分类 | 防慢请求与风暴 |
| Server queue | 默认 256、短入队等待 | 429/503 + Retry-After | 显式 backpressure |
| Writer batch | 默认 64、等待/事务预算 | 单报告 savepoint 隔离 | 降低 fsync 同时限制长事务 |
| Retention | 行/事务/时间/yield | 分批下次继续 | 不让维护阻塞实时摄取 |

## 10. Client 平台交付清单

### 10.1 Linux

提供 deb/rpm、systemd unit、专用账户、0600 配置、显式 GPU drop-in、卸载保留状态与 purge 工具。生命周期
测试覆盖当前安装、同版本重装、remove、purge、失败清理与路径权限。没有容器替代，因为 Client 需要观察真实 Host。

### 10.2 Windows

提供原生 Windows Service、交互 CLI、无控制台维护 helper 与 WiX MSI。Service 长期采集，CLI 负责配置与
配对并通过受保护本机通道读取运行状态。测试检查 WiX authoring、PE subsystem、安装/回滚/卸载和服务状态。

### 10.3 macOS

提供 pkg、不可登录账户、LaunchDaemon 与 newsyslog。脚本验证账户/路径归属，失败不得删除无关同名资源。
测试覆盖 pre/postinstall、失败回滚、卸载证明和日志配置。

### 10.4 移动库

仅提供 Android/iOS/iPadOS Rust target 的纯宿主 contract。没有 App UI、签名、权限、网络、Secret 存储或
后台保证；这些必须由真实移动产品实现后才能宣称“移动客户端”。

## 11. 数据保留与查询取舍

| 数据层 | 用途 | 默认保留 | 精度/限制 |
|---|---|---|---|
| latest | 当前 Host 状态 | 永不被 raw 清理删除 | 每 Host 一条稳定裁决结果 |
| raw | 短期逐次报告 | 7 天 | 可精确查看，但增长快 |
| hourly aggregate | 长期趋势资产 | 365 天 | 只保留支持标量的 count/min/max/avg，由自动粒度历史 API 与 Web 趋势读取 |
| audit/session/pairing/invite | 安全与控制状态 | 没有统一期限 | audit 只写不读且不清理；Session 过期/撤销行没有全局删除 worker；pairing 只在新建请求时有界清部分状态，不能套用 raw/aggregate 保留天数 |

聚合先幂等纳入再删除 raw，崩溃不能双计或丢计。UTC 小时避免时区/DST 歧义；UI 才转换本地时间。项目
不提供任意 PromQL/SQL、多年逐秒数据或分布式时序集群。

## 12. 安全和隐私分类

| 对象 | 分类 | 保护/日志规则 |
|---|---|---|
| 管理员密码/Session/CSRF | Secret | Argon2/随机摘要；不记录明文 |
| 设备 credential/TLS 私钥 | Secret | 受保护状态目录；日志只写受限 ID |
| OTLP Token | Secret | 与设备 credential 分离 |
| Spool 报告 | 敏感主机数据 | 最小权限、有界、成功后清理 |
| SQLite 历史 | 敏感资产/运行画像 | 专用账户和文件访问控制；当前没有受支持的备份流程 |
| Host 名称/硬件/接口 | 可能识别基础设施 | support 包和公开 issue 脱敏 |

## 13. 候选功能决策

| 候选 | 当前决定 | 理由 |
|---|---|---|
| 远程 Shell/修复 | 不提供 | 把只读 Client 变为高危控制代理 |
| 补丁管理 | 不提供 | 需要独立授权、回滚和软件供应链模型 |
| 多 Server active-active | 不提供 | SQLite/credential/report 幂等模型是单控制面 |
| 无限本地缓存 | 不提供 | 网络故障会耗尽主机磁盘 |
| 移动常驻 daemon | 不提供 | 不符合 Android/iOS 后台模型 |
| 多版本 Client API alias | 不提供 | 扩大协议和安全测试矩阵 |
| 自动硬件告警规则 | 当前不提供 | 需明确规则状态、抑制、通知和时钟语义 |
| 通用审计管理台 | 当前不提供 | 当前 React 已覆盖完整有序实例列表、配对、完整最新详情、自动更新、趋势、备注和删除；审计查询仍需单独合同 |
| audit 查询/导出 | 当前不提供 | 只有事务写入；需定义授权、保留、脱敏、分页与完整性证据 |
| 同 ID 正文 fingerprint | 当前不提供 | 现在同 Host 同 report ID 不比较正文；若要检测错误重放，需新增规范编码/hash、列和冲突合同 |

## 14. 功能完成定义

一项 Server/Client 功能必须同时具备协议合同、所有必要端实现、容量与失败语义、管理员/设备身份边界、
持久恢复、平台差异、指标/doctor、正负测试、安装/发行证明和中文文档。持久格式变化还必须明确声明当前
身份并拒绝其他输入；只有外部仓真的增加具体转换边时才增加对应转换验收。仅有路由、数据表或半成品页面
都不能算端到端功能完成。
