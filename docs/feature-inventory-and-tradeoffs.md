# Host Monitoring 完整功能与取舍清单

## 0. 实现与验证入口

本仓库负责 Server、管理 Web 和 Host 协议；Client 的采集、安装、状态目录和传输实现由
[独立 Client 文档](https://github.com/isarmg/host-monitoring-client/blob/main/docs/README.md)维护。
以下源码路径均相对于本仓库，描述当前入口与验证范围。

| 能力 | 实现入口 | 主要验证 |
|---|---|---|
| 配对与长期授权码 | `host-monitoring-server/src/{http,store,crypto,pairing_admission}.rs` | 过期、取消、轮换、准入预算、重复和并发绑定 |
| 管理员、Session 与 CSRF | `http.rs`、`store.rs`、Foundation admin 依赖 | `tests/browser_sessions.rs` 的真实路由合同 |
| 实例与 Host 管理 | `model.rs`、`store.rs`、`web/src/{Instances,HostDetails}.tsx` | 名称边界、排序、备注、取消/删除与权限 |
| 报告协议与宽整数 | `protocol/src/`、`model.rs`、`hardware_validation.rs` | 字段/集合/单位、未知字段、错误信封、GPU 聚合计数 |
| 有界摄取与持久 ACK | `telemetry.rs`、`store.rs` | `tests/telemetry_writer.rs` 的队列、事务、超时和关闭 |
| 报告去重与最新值 | `store.rs` | 同 Host 重投、跨 Host ID 冲突、乱序与重复确认 |
| 历史、小时聚合与保留 | `retention.rs`、`store.rs` | `tests/retention.rs` 的幂等聚合、窗口和 latest 保护 |
| SQLite 身份、锁与完整性 | `database_schema.rs`、`database_lock.rs` | `tests/{sqlite_connection,database_locking,sqlite_store}.rs` |
| HTTP 状态与恢复语义 | `error.rs`、`http.rs` | `tests/client_error_contract.rs` 的真实 Router 响应 |
| 发行物与目标平台 | `release_bundle.rs`、`release_contract.rs`、`scripts/` | release identity、工具脚本测试、固定 GNU/Linux AMD64 目标 |
| 管理页面 | `web/src/`、Foundation Web 依赖 | Web 类型检查与构建；原生浏览器交互另行验收 |

## 1. Server 能力

| 功能 | 当前实现 | 取舍/限制 |
|---|---|---|
| 管理身份 | 本地 canonical username、当前 Argon2id、随机 Session/CSRF、Foundation 精确登录与 Session 形状 | 固定 `role=admin`，默认 username `admin`；没有 email、viewer/operator/RBAC，也不依赖中央账户或共享 Session |
| 配对 | 每实例长期 code、Client request/poll、管理员或 Client 激活端点、分维度限流 | React 可查看/更换加密保存的授权码；更换会撤销旧 Client 并要求重新配对 |
| 报告 API | `/api/v2/host-monitor` 当前协议 | 不注册任何平行版本或 alias |
| API 错误 | Foundation `ErrorEnvelope`：`code/message/retryable/request_id?/details?` | 所有 `/api` 非 2xx（含 extractor/404/405）使用同一严格顶层结构 |
| 写入 | 有界队列、单 writer、batch、savepoint | 单库单活进程，不是分布式写集群 |
| 历史 | raw 标量查询、raw/hourly 自动粒度趋势、按服务器接收日期读取原始报告日志及两级保留 | 图表查询最长 31 天、最多 1000 点；日志一次读取所选日全部记录，受 64 MiB / 120 秒 Web 请求预算约束 |
| Web | Foundation 管理员 client/hook + Host 列表 exact guard，编译进发行物 | 提供主机列表、采集详情、历史图表、上报日志及实例邀请/配对；没有 audit 读取界面 |
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

- Server 只接受 `0.9.31` 配置与发行身份以及 `0.9.26` 数据库结构身份；不包含转换器或平行 alias。Client 配置格式版本独立冻结为 `0.9.4`。
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

Linux、Windows、macOS 原生安装与移动宿主能力由独立 Client 的
[平台文档](https://github.com/isarmg/host-monitoring-client/blob/main/docs/platform-setup.md)说明。
每个平台分别记录安装、权限、服务生命周期、签名和原生测试证据；Server 的 GNU/Linux 测试
不替代 Client 平台验收。Client 版本、配置格式与 Server 数据库身份分别维护。

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
| 通用审计管理台 | 当前不提供 | 当前 React 提供完整有序实例列表、配对、完整最新详情、自动更新、趋势、备注和删除；审计查询仍需单独合同 |
| audit 查询/导出 | 当前不提供 | 只有事务写入；需定义授权、保留、脱敏、分页与完整性证据 |
| 同 ID 正文 fingerprint | 当前不提供 | 同 Host 同 report ID 不比较正文；若要检测错误重放，需新增规范编码/hash、列和冲突合同 |

## 14. 功能完成定义

一项 Server/Client 功能必须同时具备协议合同、所有必要端实现、容量与失败语义、管理员/设备身份边界、
持久恢复、平台差异、指标/doctor、正负测试、安装/发行证明和中文文档。持久格式变化还必须明确声明当前
身份并拒绝其他输入；只有外部仓真的增加具体转换边时才增加对应转换验收。仅有路由、数据表或半成品页面
都不能算端到端功能完成。
