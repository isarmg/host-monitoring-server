# 排查 xsos 问题

先记录发生时间、软件 `identity`、HTTP 状态和错误 `code` / request ID。以下检查按从本机到代理、再到客户端的顺序进行。

| 现象 | 检查 | 预期与下一步 |
|---|---|---|
| 服务立即退出 | `sudo journalctl -u xsos.service -n 100 --no-pager` | 按首个错误核对发行目录、环境文件、权限与初始化结果 |
| `release root must be releases/1.0.0` | 查看发行实体路径和 current 链接 | 使用 `/opt/isarmg/xsos/releases/1.0.0` 及同安装树的绝对 current 链接 |
| `/healthz` 正常，`/readyz` 返回 503 | 查看运行日志；执行只读 `config validate --json` | 运行依赖全部健康后 ready 才为 true；需要 doctor 时先停服 |
| 本机就绪，浏览器打不开 | 查看 TLS 代理与本机后端连接 | 公网使用有效 HTTPS，后端转发到回环端口 18105 |
| 登录失败或 403 | 核对用户名、主机时钟、Host/Origin 和 Cookie | 使用同一 HTTPS 源站；用户名规则见配置参考 |
| 页面报 `invalid_error_response` | 检查代理返回的状态和 Content-Type | API 错误应保持原始 JSON；修复代理改写 |
| 页面报 `invalid_response_shape` | 核对服务与内嵌 Web 的发行身份 | 使用同一已验证发行树，按 request ID 排查响应 |
| 数据库身份或完整性错误 | 停服后 doctor；核对配置和密钥来源 | 保留数据库和日志；由当前结构验证定位问题 |
| 出现锁冲突 | 检查服务和维护进程 | 同一数据库只运行一个服务；维护前等待它完全停止 |
| 主机离线或数据不更新 | Client `status`、`doctor --network`；停服后 `doctor --delivery` | 真正收到报告确认，再核对页面接收时间 |
| 429 / 503 增多 | 查看 writer/retention 日志、磁盘和客户端队列 | 客户端保留同一报告并退避；先恢复写入能力 |
| 硬件读数为空 | 查看 Client 采集能力和采集时间 | 按驱动、工具和服务权限排查，空值表示不可用，参见硬件文档 |

需要完整诊断命令见[日常运维](administration.md)。分享 `identity`、脱敏错误和所做检查即可，凭据、原始配置与数据库保持私有。
