# 硬件数据、展示与历史

报告协议为 schema **2**。服务端只校验当前协议，不接收旧报告，不转换历史协议。扩展位于 `system.hardware`：CPU 硬件详情、网卡信息、硬件传感器、磁盘健康。客户端保留系统基础指标，不采集进程。

硬件读数有明确单位和采集时间；服务端限制设备数量、字符串长度、重复传感器标识及数值范围。风扇不接受负转速；电压、电流可为负；NVMe 寿命消耗允许达到 255%；缺失数值是 null。JSON u64 延续超过 JavaScript 安全整数范围时使用十进制字符串的规则。

实例详情显示 CPU 型号和频率、网卡 IP/MAC/链路信息、风扇/电压/电流/功率/能量、SMART/NVMe 健康和 GPU 信息。设备是否可读、权限不足、缺少驱动或工具由采集能力诊断展示。

新增六组历史标量，贯通报告校验、SQLite 写入、原始历史查询、小时聚合、历史 API 和图表：

| 指标 | 含义 |
| --- | --- |
| cpu_frequency_mhz | 有效逻辑核心频率的平均值 |
| gpu_power_watts | 所有可读 GPU 中功耗最大值，W |
| gpu_core_clock_mhz | 所有可读 GPU 中核心频率最大值，MHz |
| max_fan_rpm | 所有可读风扇中的最大转速 |
| max_disk_temperature_celsius | 所有可读 SMART 磁盘中的最高温度 |
| max_disk_percentage_used | 所有可读 NVMe 磁盘中最高寿命消耗比例，可超过 100 |

这些是主机级汇总，不是逐设备历史。原始报告保存策略沿用现有实现：完整 payload 用于最新状态，长期数据为标量与小时聚合；不声称已存储所有设备的每次原始读数。小时聚合使用各指标独立 count/min/max/avg，缺失读数不变成零。慢采样数据会在其刷新前随基础报告重复携带，历史表示当时最近已知状态；磁盘详情展示独立实际采集时间。

当前数据库 schema revision 为 **7**，schema_application_version 为 **0.9.26**，DDL 与指纹同步更新。按照本次不兼容旧版本的要求，未添加迁移、兼容读写或旧数据库自动处理。已有 revision 6 数据库会被现有严格启动校验拒绝。部署时使用符合 revision 7 的数据库，并同步部署新版客户端和网页；本次开发不会修改实际运行中的数据库。

客户端平台支持矩阵和 smartmontools 配置见相邻客户端仓库 `docs/hardware-monitoring.md`。Windows x64 已通过只读 ADLX/IGCL 补充 AMD/Intel 的温度、功耗、频率、风扇和电压，并按 Windows LUID 与 DXGI/PDH 合并。详细驱动要求与缺失数据处理见客户端 `docs/windows-gpu-vendors.md`。使用现有 GPU、温度和硬件传感器字段，不增加任何命令下发或设备控制接口。
