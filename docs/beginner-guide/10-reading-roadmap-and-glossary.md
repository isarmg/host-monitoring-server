# 10. 按问题阅读源码

先读[架构](../architecture.md)找到模块，再带着一个问题从 HTTP 入口跟到telemetry/store/retention。

常用术语：binding 是当前设备绑定，Session 是浏览器会话，spool 是本地待发队列，latest 是最新有效报告，maintenance lock 协调停服维护。

掌握主链路后按[开发指南](../development.md)运行相关测试，最后检查当前[发行说明](../releases/1.0.0.md)。

[学习路线](README.md) · [文档首页](../README.md)
