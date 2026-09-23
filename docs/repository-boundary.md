# Server / Client 独立构建

本仓库只保留 `host-monitoring-server/`、`protocol/` 和服务端管理 Web `web/`。
`web` 名称仅指浏览器源码，不代表独立安装客户端。
原生客户端与安装脚本位于 https://github.com/isarmg/host-monitoring-client 。
唯一协议源码在本仓库，Client 通过完整 Git 提交固定依赖，Server 不反向依赖 Client 源码。

错误契约测试分工：Server 测试真实路由的状态码、错误码及 retryable 标记；Client 测试这些响应如何
影响凭据和重试行为。双方通过同一产品协议依赖连接，分别构建和验证各自实现。

当前数据库身份由 `host-monitoring-server/src/database_schema.rs` 定义：Schema revision 为 `7`，
`schema_application_version` 为 `0.9.26`。启动只接受匹配的身份与 DDL 指纹；格式转换须有独立验证流程。
每个发行标签固定对应源码与制品，开发分支的行为由当前提交标识。
