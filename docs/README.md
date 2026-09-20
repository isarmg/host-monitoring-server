# Host Monitoring 文档总览

本目录描述当前 Server `0.9.19` 开发源码及跨端产品概念；独立 Client 实现和命令须在
[host-monitoring-client](https://github.com/isarmg/host-monitoring-client) 仓库阅读和执行。
事实优先级依次为协议类型和当前 Schema、运行时校验、测试、发行 manifest、
本文档。更改版本身份、HTTP 路由、报告字段或安装布局时，应在同一提交中同步对应文档。

范围必须先分清：Server 与随包管理 Web 只属于 AMD64 GNU/Linux，Web 使用 React/Vite 与 Foundation
admin-only username 合同；`host-monitor` Client 继续拥有 Linux/Windows/macOS 与移动宿主边界。产品只
接受当前状态，不提供旧字段/路由/Schema 兼容；`sarmg-upgrade` 目前没有 Host 转换边，因此本文档没有
可执行的 Host 备份、恢复或迁移步骤。

| 分类 | 文档 | 内容 |
|---|---|---|
| 初学者学习指南 | [beginner-guide/README.md](beginner-guide/README.md) | 从组件、遥测、配对、SQLite 到平台打包的学习路径 |
| 工作流程与流程树 | [project-workflow.md](project-workflow.md) | 启动、配对、报告、聚合、移动宿主和发行流程 |
| 完整功能与取舍 | [feature-inventory-and-tradeoffs.md](feature-inventory-and-tradeoffs.md) | Server、Client、平台能力以及明确边界 |
| 必要 README | [../README.md](../README.md) | 项目定位、仓库入口和最短质量门 |
| Server 发行包部署手册 | [server-release-readme.md](server-release-readme.md) | 正式归档的校验、全新安装、配置、启动、监控与故障处理 |
| 运维 | [operations.md](operations.md) | 服务端部署、Client 安装、配置、诊断、安全和卸载 |
