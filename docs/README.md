# xsos 文档总览

本目录描述当前 Server `1.0.0` 开发源码及跨端产品概念；独立 Client 实现和命令须在
[xsoc](https://github.com/isarmg/xsoc) 仓库阅读和执行。
事实优先级依次为协议类型和当前 Schema、运行时校验、测试、发行 manifest、
本文档。更改版本身份、HTTP 路由、报告字段或安装布局时，应在同一提交中同步对应文档。

范围必须先分清：Server 与随包管理 Web 只属于 AMD64 GNU/Linux，Web 使用 React/Vite 与 xcss
admin-only username 合同；`xsoc` Client 继续拥有 Linux/Windows/macOS 与移动宿主边界。产品只
文档描述当前配置、路由和持久化状态。

| 分类 | 文档 | 内容 |
|---|---|---|
| 初学者学习指南 | [beginner-guide/README.md](beginner-guide/README.md) | 从组件、遥测、配对、SQLite 到平台打包的学习路径 |
| 工作流程与流程树 | [project-workflow.md](project-workflow.md) | 启动、配对、报告、聚合、移动宿主和发行流程 |
| 完整功能与取舍 | [feature-inventory-and-tradeoffs.md](feature-inventory-and-tradeoffs.md) | Server、Client、平台能力以及明确边界 |
| 必要 README | [../README.md](../README.md) | 项目定位、仓库入口和最短质量门 |
| Server 发行包部署手册 | [server-release-readme.md](server-release-readme.md) | 正式归档的校验、全新安装、配置、启动、监控与故障处理 |
| 运维 | [operations.md](operations.md) | 服务端部署、配置、诊断、安全与独立 Client 文档入口 |
| 当前发行说明 | [releases/1.0.0.md](releases/1.0.0.md) | 本版本功能和验证范围 |

工程约定与依赖来源见 [架构说明](architecture.md)，Rust unsafe 结论见 [审查记录](unsafe-audit.md)。

公共支撑的职责、单体依赖、平台边界与验证方法见[公共支撑说明](common-support.md)。
