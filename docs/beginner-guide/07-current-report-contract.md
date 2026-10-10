# 07. 理解协议与结果

当前报告 schema 为 1。认证、字段验证和入队后，由单 writer 用事务与 savepoint 写入。latest 保护最新报告，retention 按 UTC 小时聚合再分批清理。

读[HTTP 与存储参考](../reference.md)核对 202、429、503，以及原始保留和聚合窗口。

[学习路线](README.md) · [文档首页](../README.md)
