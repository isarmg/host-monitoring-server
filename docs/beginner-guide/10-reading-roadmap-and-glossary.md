# 10. 源码阅读路线、练习与术语

## 10.1 三阶段阅读路线

第一阶段读 `protocol` 的报告与配对类型、根 README 和流程树，能画出 Client 到 SQLite 的路径。第二阶段
读 `host-monitoring-client/src` 的 config、pairing、collectors、spool、delivery。第三阶段读 Server auth、telemetry
writer、retention、release 验证和 Web 调用。

## 10.2 按问题找入口

| 问题 | 先读 |
|---|---|
| 字段从哪来 | 平台 collector -> model -> protocol |
| 为什么没发出 | config/status -> spool -> delivery/transport |
| 为什么 401 | pairing state -> credential lookup/revocation |
| 为什么 429/503 | report admission -> queue -> writer readiness |
| 历史为什么缺点 | retention -> aggregate -> latest 保护 |
| 安装为何失败 | 对应平台 packaging tests 与生命周期脚本 |
| 正式启动为何拒绝 | release identity/manifest/path verification |

## 10.3 建议练习

1. 在临时环境完成 probe、通过 API/code 激活 pair、once，并在 Web Host 列表 JSON 中看到 latest 摘要；
   同时记录当前缺少 React activation 页面。
2. 断开 Server，观察 spool；恢复后确认相同 report ID 被接收。
3. 让假 Server 返回 429，确认遵循退避且容量有界。
4. 尝试同时启动两个 Client，确认状态锁拒绝第二实例。
5. 在复制的数据库上改变 DDL，确认 doctor 拒绝。
6. 构建一个平台包并检查 binary、service、配置和卸载语义都使用当前身份。

## 10.4 术语表

| 术语 | 本项目含义 |
|---|---|
| Client / `host-monitor` | 受管主机上的当前客户端产品 |
| Server / `host-monitoring-server` | 只支持 `x86_64-unknown-linux-gnu` 的控制面；不等于跨平台 Client |
| administrator username | Foundation 规范化的本地管理标识；默认 `admin`，不是 email |
| admin role | 唯一管理角色；没有 viewer/operator/RBAC，与 username 文本是两件事 |
| binding | Host identity 与服务端 credential 的当前原子绑定 |
| pairing | invite/code、Client request/poll 和 Server 原子绑定设备 credential 的流程 |
| report ID | 一次逻辑报告的稳定幂等标识 |
| spool | Client 本地有界持久待投递队列 |
| backpressure | 容量不足时显式拒绝/延迟，而非无界积压 |
| savepoint | 一个 SQLite 事务内隔离单报告失败的检查点 |
| latest | 按稳定规则选出的 Host 最新有效报告 |
| raw retention | 原始逐点报告保留期限 |
| aggregate retention | 小时聚合保留期限 |
| OTLP | 可选 OpenTelemetry Protocol 导出通路 |
| source-bound | binary 身份绑定构建源码 revision |
| maintenance lock | 产品与离线工具协调状态访问的锁 |
| fail closed | 无法证明当前身份或安全条件时拒绝运行 |

## 10.5 完成学习的标准

维护者应能独立回答：报告在哪一刻算持久化；断线后为何保留原 ID；同 Host 同 ID 改正文为何当前不会
冲突；配对怎样防止身份分叉以及 React 为何还不能完成激活；latest 为什么不随 raw 清理消失；aggregate
为何当前查不到；Client 三平台安装权限怎样收敛；Schema 变化何时才需要外部仓新增具体 edge；一次名称
变化要检查哪些制品层。

## 10.6 后续文档

掌握本教程后，使用 [工作流程与流程树](../project-workflow.md)核对端到端顺序，用
[功能与取舍清单](../feature-inventory-and-tradeoffs.md)判断需求是否在产品边界内，生产操作只以
[运维文档](../operations.md)和当前命令帮助为依据。
