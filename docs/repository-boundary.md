# Server / Client 独立构建

本仓库只保留 `host-monitoring-server/`、`protocol/` 和服务端管理 Web `web/`。
`web` 名称仅指浏览器源码，不代表独立安装客户端。
原生客户端与安装脚本位于 https://github.com/isarmg/host-monitoring-client 。
唯一协议源码在本仓库，Client 通过完整 Git 提交固定依赖，Server 不反向依赖 Client 源码。

错误契约测试分工：Server 测试真实路由的状态码、错误码及 retryable 标记；Client 测试这些响应如何
影响凭据和重试行为。双方通过同一产品协议依赖连接，不再用跨仓库相对路径编译另一端实现。

Client 命名改变后的 Server Schema revision 为 4；只接受精确当前 Schema，不兼容或迁移旧状态。
已有发行标签不改写，本分支源码变化不能冒充旧标签对应的发行包。
