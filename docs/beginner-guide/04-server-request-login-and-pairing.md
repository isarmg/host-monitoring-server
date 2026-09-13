# 04. 服务端请求、登录与配对链路

## 4.1 启动先于请求

正式 Server 在绑定端口前验证物理发行路径、全树 manifest、源码 revision、target、API/Schema 身份、
Web 文件和权限；再解析环境、取得数据库 instance 排他锁与 maintenance 共享锁、验证当前数据库，最后
启动 writer、retention 和 listener。失败发生得越早，越不容易产生半启动状态。

## 4.2 浏览器登录

```text
TCP peer + bounded body
 -> exact {username,password} DTO
 -> username candidate normalize/canonical check
 -> source/account/global admission
 -> bounded Argon2 verification
 -> SQLite Session digest
 -> 生产 `__Host-` Secure/HttpOnly Cookie + 响应内 CSRF plaintext
```

登录候选 username 为 1..64 字节 printable ASCII。Foundation primitive 先 trim ASCII whitespace、转
ASCII 小写，再要求持久 canonical username 为 3..64 字节，首尾 `[a-z0-9]`，所有字符只允许
`[a-z0-9._-]`，禁止 `@`，但不禁止相邻分隔符。默认 username 是 `admin`。账户 bucket 使用规范化值，
因此 ` Admin ` 与 `admin` 共享同一预算和唯一数据库行。

密码 wire candidate 上限为 1024 字节，真正验证前还必须满足当前 12..1024 字节、无 ASCII control 的
Foundation 密码策略。未知账户仍执行固定 current-policy dummy hash，降低用户名枚举侧信道；全局
Argon2 semaphore 防止无界 CPU 并发。写请求同时验证 Session、CSRF、Origin/Host/Sec-Fetch-Site；
forwarded address 默认不是可信来源事实。

登录和 Session 查询响应固定为 Foundation `AdministratorSession` 精确五字段：
`authenticated=true`、`user_id`、`username`、`role=admin`、`csrf_token`。不存在 email、权限数组或额外
角色字段；本产品没有 viewer/operator/RBAC。
Web 的恢复/登录/登出、401 清理与内存 Session/CSRF 状态统一由
`@sarmg/admin-web` client/hook 负责；页面和 Host API 响应 guard 仍在产品仓库。

## 4.3 配对的参与者

Client、管理员 API 和 Server 共同完成配对。管理员 API 创建 invite 并只在创建响应返回一次 activation
code。Client 在本地生成长期 bearer secret 与独立 polling secret，只发送两者的 SHA-256；Server 从不把
bearer plaintext 发回浏览器。实例授权码与 pairing request 成功激活后，Server 把已提交的 bearer
摘要绑定到 Host；Client 轮询 active 并原子写入本地 secret、Host identity 与 active binding。授权码不会
因这次激活被消耗；管理员更换授权码时，Server 会撤销旧 bearer 并要求 Client 重新配对。

当前 React 页面只做管理员认证和 Host 列表，不实现 invite/activation UI，也不处理 Client 给出的
`/activate/{request_id}`。因此“浏览器打开链接即可批准”目前不是已完成能力；真正存在的是受保护管理
API、持有实例授权码的客户端可调用的 activation API 和 Windows Tray code 提交通路。公开 activation
端点依靠 code 本身作为 capability，不应把任意调用方描述为已经认证的管理员或设备。

## 4.4 配对状态机

```text
local none -> creating -> pending -> activating -> active
                         |             |
                         |             + local commit crash: journal convergence
                         + Server waiting -> active / denied / expired

Server invite:  pending -> active / cancelled / expired
Server request: pending -> active / expired（当前没有管理员拒绝路由）
```

网络中断后必须恢复同一 generation/request，而非自动生成多个身份。替换未完成请求需要显式用户动作。
Server 对来源、设备、request、invite 和管理员分别限流，防止一个维度绕过另一个维度的预算。单设备
最多四个 live pending request，全库最多 4096 个；创建请求时会有界清理过期 pending。协议枚举和本地
Client 状态仍包含 `denied`，数据库清理 SQL 也识别它，但当前 Server 没有把 request 转为 denied 的路由；
因此不能把“拒绝 pairing”列为已实现的管理员能力。

## 4.5 报告请求链路

设备 Bearer credential 通过摘要查找和撤销检查后，Server 严格解码报告，验证 Host 绑定、report ID、
版本、时间和字段边界，再尝试在短时预算内进入有界队列。未入队返回 429/503；入队后 writer 拥有任务。

## 4.6 为什么不能直接在 handler 写 SQLite

每个请求独占写事务会放大 SQLite 竞争，并允许网络并发直接决定内存和连接数。单 writer 加有界队列把
并发转为可观察的背压，batch 提高吞吐，per-report savepoint 隔离单个冲突或撤销。

## 4.7 响应的精确含义

- `202`：报告事务已提交，不只是进入队列。
- `401 + code=unauthorized + retryable=false`：Server 严格确认 credential 无效，Client 进入
  `reauth_required`，等待用户显式重新配对。
- `403 + code=client_host_mismatch + retryable=false`：credential 属于另一 Host；当前报告永久无效，
  Client 丢弃这一条但不撤销 credential。
- `400/413`：报告字段或大小不符合当前合同，同一报告不再重试。
- `409`：`report_id` 与另一 Host 冲突，同一报告永久无效。
- `422`：框架无法提取当前 API 请求；Web 调用方修正请求，Client 报告通路不把它当作当前永久裁决。
- `429`：准入容量不足，遵循 `Retry-After`。
- `503`：writer 或依赖不可用，保留同一报告后重试。

所有 `/api` 失败都使用 Foundation 严格顶层 `{code,message,retryable,request_id?,details?}`；未知字段、
缺字段、错误 Content-Type 或代理生成的 HTML 都不是可信机器合同。Web 与 Client 按 `code` 分支，
`message` 只用于展示和受限诊断。

## 4.8 调试顺序

先以 request/report ID 串联日志，再按认证、严格解码、准入、队列等待、writer savepoint、事务提交定位。
禁止打印 credential 或整份报告来换取便利；应记录受限字段摘要和分类错误。

## 4.9 变更检查

新增路由需明确公开/管理员/设备身份、body 上限、超时、来源事实、CSRF、错误 envelope 和负例。当前 API
发生破坏性变化时直接更新所有调用方与测试，不注册平行路径。
