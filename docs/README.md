# xsos 使用文档

xsos 为管理员提供自托管 Web 管理台，服务端运行于 Linux AMD64 GNU。每台受管主机安装 [xsoc](https://github.com/isarmg/xsoc/blob/main/docs/README.md)。本文档对应当前 1.0.0。

## 第一次使用

1. [安装服务端](server-release-readme.md)：下载发行包、配置、初始化并启动。
2. [连接一台主机](instance-management.md)：创建实例，安装并配对客户端。
3. [使用管理台](usage.md)：查看状态并完成日常操作。

## 按任务查找

- [配置](configuration.md)：环境变量、默认值与修改方法
- [日常运维](administration.md)：服务、日志、容量和账号维护
- [排查问题](troubleshooting.md)：根据现象选择检查
- [命令参考](cli.md)：初始化、校验、状态和诊断
- [本地运行](local-service.md)与[开发指南](development.md)：构建、测试、打包
- [实现概览](architecture.md)与[学习路线](beginner-guide/README.md)：理解协议和持久化
- [功能范围](feature-inventory-and-tradeoffs.md)、[公共库](common-support.md)、[仓库关系](repository-boundary.md)与[原生接口审查](unsafe-audit.md)
- [1.0.0 发行说明](releases/1.0.0.md)

指标含义见[硬件监控](hardware-monitoring.md)，接口与数据细节见[参考](reference.md)。
