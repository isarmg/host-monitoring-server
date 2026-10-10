# 03. 阅读数据类型

报告含主机身份、稳定报告 ID、采集时间和有单位的指标。缺失读数使用 null，集合有数量与文本上限。大于 JavaScript 安全整数的 u64 使用十进制字符串；管理 Web 读取完整最新报告并按协议解析。

练习：从 crates/protocol 的报告字段跟到 Server 校验与 web/src 的显示，记录单位和缺失表现。

[学习路线](README.md) · [文档首页](../README.md)
