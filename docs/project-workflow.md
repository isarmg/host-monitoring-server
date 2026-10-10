# xsos 端到端流程

## 部署到首次使用

1. 在 Linux AMD64 GNU 安装发行树，准备私有配置和数据目录。
2. 显式 init 创建管理员与数据库，再由 systemd 启动服务。
3. 检查 ready，通过 HTTPS 登录管理台。
4. 创建实例，在主机安装 xsoc 并运行 setup。
5. 确认管理台收到最新报告。

实际命令见[安装](server-release-readme.md)、[实例管理](instance-management.md)和[使用指南](usage.md)。

## 正常运行

采集器 → 本地 spool → HTTPS 报告 → 设备认证与验证 → 有界队列 → SQLite 提交 → 202 确认 → Client 删除已确认队列项。

报告 ID 在重试中保持不变。同一 Host 的重复 ID 返回 accepted=false；跨 Host ID 冲突拒绝。latest 按采集时间选择，在线状态按最近接收时间估算，补传不覆盖更新的读数。

原始数据默认保留 7 天，小时聚合 365 天，latest 关联的报告继续保留。聚合按 UTC 小时产生独立 count/min/max/avg；图表合并 raw 与 hourly 时排除重复计数。

## 维护与开发

日常先查看服务、日志与业务时间，再按具体错误定位。写入维护前停止服务，配置校验可并行执行。修改协议时同时更新类型、生产者、消费者和测试；运行[开发检查](development.md)后核对最终制品。
