# 09. 部署、安全与生产运维

## 9.1 生产拓扑

Server 只部署在 x86_64 glibc Linux，使用 root 持有的不可变版本目录，专用服务账户读取 0600 环境并写
独立 SQLite 状态；默认回环监听，由可信 TLS reverse proxy 暴露。Client 使用各平台服务管理器和受保护
本地状态，仅出站访问 Server。内置管理页是与 Server 同包的 React/Vite 页面，并通过 Foundation
admin-only username Session 合同访问 API。Windows Service 与 macOS LaunchDaemon 都是 Client，不是
Server 部署方式。

## 9.2 上线前证据

保存代码 tag/revision、归档与 checksum、manifest verification、工具链、测试结果、配置摘要和全新部署计划。
不要保存 Secret 本身。正式包从干净 annotated tag 在 x86_64 glibc Linux 构建，不能从开发工作树手工
复制 binary，也不能把 Client 的其他平台产物改名为 Server 制品。

## 9.3 Server 上线顺序

1. 停止或确认不存在使用同一数据库的其他实例。
2. 安装到新的精确版本目录，禁止覆盖。
3. 设置 root ownership 和不可写发行树。
4. 创建全新的当前数据库，或使用已经被当前 `doctor` 证明身份完全匹配的当前库；不要输入其他身份。
5. 运行 `verify-release` 与 `doctor`。`doctor` 取得共享 maintenance lock，可在线只读检查，但不是备份、
   修复或转换命令。
6. 启动后检查 readiness、静态资源、登录和写入 smoke。

## 9.4 Client 上线顺序

通过原生包安装，校验配置/状态权限，运行 `probe`，完成显式配对，再运行 `once`，最后启用长期服务。
不要把一台 Host 的 state directory 制作进镜像或批量复制。

## 9.5 日常监控

Server 应关注 readiness、认证失败、配对准入、429/503、SQLite/WAL、磁盘和 inode。当前产品没有
metrics API，也没有暴露 writer queue 深度/延迟或 backlog 数量；retention worker 的连续失败会进入 readiness，其他项目只能通过有限日志、HTTP
结果与数据库离线观察间接判断，不能在监控配置里引用不存在的指标。Client 关注服务状态、采集错误、
spool 容量/最老条目、TLS、认证撤销和系统时钟。

## 9.6 当前没有备份、恢复或迁移流程

产品不实现 backup/restore/migration，`sarmg-upgrade` 当前也没有 Host Monitoring 转换边。通用
generation/journal 引擎本身不能证明它会识别本产品，所以当前不得用它制作或恢复 Host 数据。Client 本地
身份若丢失，只能通过新的 invite/pairing 建立全新当前身份；这不恢复历史数据。需要备份/恢复时，应先在
外部仓定义精确输入身份、SQLite WAL 一致性、锁、输出 identity、失败原子性和验证证据，再实现并发布
具体 edge；在此之前运维清单必须标记“不支持”。

## 9.7 故障分级

| 现象 | 立即动作 |
|---|---|
| readiness 失败 | 从代理摘除，保全日志，检查 writer/数据库 |
| 大量 429 | 降低摄取、查队列与 writer，勿简单放大无界容量 |
| 大量 401 | 检查撤销/时间/配置，避免自动重新配对风暴 |
| spool 增长 | 查网络、TLS、Server 和磁盘，保持本地有界 |
| Schema/manifest 不符 | 停止、不手改并保全；当前没有可调用的 Host 转换 edge |
| Secret 疑似泄露 | 隔离、撤销/轮换、重新配对并人工核对影响范围；产品 `audit_events` 目前无读取/导出 API |

## 9.8 安全边界

TLS 私钥、管理员密码、Session/CSRF、设备 credential、OTLP Token 和数据库都按 Secret/敏感数据管理。
管理员身份是 canonical username，不是 email；role 只有 `admin`。日志与 support 包做字段级脱敏。只
信任明确配置的代理和 CA；Client 没有“关闭 TLS 证书校验”开关。远程明文 HTTP 不受配置开放；
report/OTLP/pairing 的生产基线固定为 HTTPS，HTTP 仅限 loopback。

## 9.9 不提供跨版本回滚

每个产品版本按全新当前身份交付，Server 不读取另一个版本的配置、数据库或状态，也没有产品内 rollback
命令。当前外部仓没有 Host edge，因此不能把其他制品与本库拼接来“回滚”。失败部署只能停止、保全证据，
重新创建全新当前实例并重新配对；历史保留不属于当前受支持结果。未来若新增具体外部 edge，届时只能按
该 edge 明示的输入/输出和 journal 语义操作。

## 9.10 安全事件记录

记录时间线、受影响 Host、发行 SHA、撤销/轮换动作和验证结果；公开 issue 不附生产配置、IP、报告、
数据库或 Secret。恢复服务前证明新凭据、当前发行和当前状态三者一致。
