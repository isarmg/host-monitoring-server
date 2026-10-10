# xsos 实现概览

xsos 将业务服务和管理 Web 放在一个 Linux AMD64 GNU 可执行程序中，以单个 SQLite 数据库保存状态。xsoc 在受管主机独立运行，协议类型由本仓库维护。

## 从哪里读源码

| 目录 | 职责 |
|---|---|
| `crates/protocol/` | 报告、配对与宽整数编码类型 |
| `crates/server/src/` | HTTP、配置、遥测写入和保留 |
| `crates/server/tests/` | 路由、存储、认证、并发与发行行为测试 |
| `schema/` | 当前 DDL 与生成的结构定义 |
| `web/src/` | 产品页面、API 调用与响应校验 |
| `scripts/` | 本地启动、构建、检查和打包 |
| `deploy/` | systemd示例 |

根 Cargo.toml 统一 workspace 依赖和 lint，Cargo.lock 锁定依赖图。公共配置、认证、日志、SQLite 和生命周期使用 xcss；管理页面使用单个 @xcss/web 包。产品继续维护自己的业务模型和持久条件，见[公共库](common-support.md)。

## 一条请求经过什么

报告经 HTTP 认证、模型验证、有界队列、单 writer 批次事务后返回 202。每条报告用 savepoint 隔离失败。保留任务先幂等汇总，再分批删除到期 raw 数据，latest 依赖行继续保存。

管理页使用管理员 Session/CSRF；报告使用独立设备凭据。Web 显示完整最新详情、趋势和按接收日期分页的原始日志。[端到端流程](project-workflow.md)连接这些模块。

## 状态与资源

`init` 创建私有数据库与首个管理员；`run` 验证当前结构后运行。配置校验使用独立只读快照，doctor 与密码维护在停服窗口取得排他维护锁。正式发行通过 manifest 绑定源码、二进制、资源和权限。

构建、测试、发行步骤及验证范围见[开发指南](development.md)。Rust 原生接口结论见[unsafe 审查](unsafe-audit.md)。
