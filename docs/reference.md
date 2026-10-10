# HTTP、存储与报告参考

本文面向接口调用方和维护者。开始使用管理台见[使用指南](usage.md)，修改环境参数见[配置](configuration.md)。

## HTTP 与认证



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
| `GET /api/v1/monitoring/hosts` | 管理员 Session | 可选 `cursor` 或 `host_id`，两者不混用 | 按实例名称字母数字顺序返回最多 50 条 Host summary、全局统计及前后游标；`host_id` 可定位单台主机；React 总览与实例/详情入口共同使用 |
| `GET /api/v1/monitoring/hosts/{host_id}` | 管理员 Session | canonical UUID | Host summary 与可空 latest 原始报告；当前 Web 详情使用列表中的同一投影，端点供独立调用方精确读取 |
| `GET /api/v1/monitoring/hosts/{host_id}/history` | 管理员 Session | 原始模式使用 `from/to/limit`；图表模式使用 `from/to/resolution=auto/max_points`，跨度最多 31 天、点数 100..1000 | 原始点，或在同一快照内无重复合并 raw 与 hourly 的时间桶；响应明确粒度、来源和实际对齐范围 |
| `GET/POST /api/v1/monitoring/client-instances` | 管理员 Session；POST 另需 CSRF/同源 | GET 可选 `cursor` 或 `instance_id`，两者不混用；管理路由组正文上限 16 KiB；POST exact `display_name?`，省略时使用默认名称；授权码不设有效期 | GET 返回最多 50 条实例、关联主机、可查看的授权码及前后游标；`instance_id` 可定位单个实例；新建 `201`；成功响应 `no-store` |
| `PUT /api/v1/monitoring/client-instances/{request_id}/authorization` | 管理员 Session + CSRF + 同源 | canonical UUID；exact `authorization_code`，36 位小写英文字母或数字 | 更新加密密文/摘要、撤销旧 Client credential，并将实例恢复为 pending；Client 需重新配对 |
| `DELETE /api/v1/monitoring/client-instances/{request_id}` | 管理员 Session + CSRF + 同源 | canonical UUID；pending 首次调用转 cancelled，cancelled 再次调用永久删除 | `204`；不存在为 404，active 为 409；Web 分别显示“取消配对”和“删除实例” |
| `POST /api/v1/xsoc/activate-admin` | 管理员 Session + CSRF + 同源 | 16 KiB 管理上限；exact request ID + activation code | 与 capability 激活进入同一事务；React 配对确认流程调用 |
| `DELETE /api/v1/monitoring/managed-instances/{host_id}` | 管理员 Session + CSRF + 同源 | canonical UUID | `204`；永久级联删除关联数据 |
| `POST /api/v1/xsoc/pairing-requests` | 未配对 Client | Client 路由组 512 KiB；strict Host、bearer/polling-secret SHA-256；来源/设备/容量限流 | 创建或幂等恢复 pairing request；返回 activation URL，成功 `no-store` |
| `GET /api/v1/xsoc/pairing-requests/{request_id}` | 持有 request ID 的调用方 | canonical UUID；来源/请求限流 | 只暴露 OS/arch/version/status/expiry 公共摘要；成功 `no-store` |
| `POST /api/v1/xsoc/pairing-requests/{request_id}/status` | Client 的 `Pairing <polling_secret>` | 512 KiB 组上限；secret 32..256 字符且无 whitespace | waiting/active/denied/expired 与可空 instance ID；成功 `no-store` |
| `POST /api/v1/xsoc/activate` | 持有实例 authorization code 的 capability 调用方 | 512 KiB 组上限；来源/请求限流；不是管理员 Session | 激活同一事务状态机；授权码错误/取消/跨实例使用严格失败；短期设备请求仍有超时保护；成功 `no-store` |
| `PATCH /api/v1/monitoring/client-instances/{request_id}` | 管理员 Session + CSRF + 同源 | 实例名称 | `204`，更新名称 |
| `DELETE /api/v1/monitoring/client-instances/{request_id}/delete` | 管理员 Session + CSRF + 同源 | canonical UUID | `204`，永久删除实例 |
| `GET /api/v1/monitoring/logs/calendar` | 管理员 Session | 无业务正文 | 服务器时区的当前日期 |
| `GET /api/v1/monitoring/hosts/{host_id}/reports` | 管理员 Session | date 或 start_date/end_date，加可选 cursor | 按服务器接收日期分页，每页最多 50 条，含报告 ID 和时间 |
| `GET /api/v1/xsoc/credential-status` | 设备 Bearer credential | 32–256 字符凭据 | 返回 authorized、host_id、instance_id、protocol_version，no-store |
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


## 数据库与保留



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
