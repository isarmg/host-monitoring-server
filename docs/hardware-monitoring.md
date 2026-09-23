# 硬件数据、展示与历史

报告协议为 schema **2**。服务端校验并只接受该协议。扩展位于 `system.hardware`：CPU 硬件详情、网卡信息、硬件传感器、磁盘健康。客户端保留系统基础指标，不采集进程。

硬件读数有明确单位和采集时间；服务端限制设备数量、字符串长度、重复传感器标识及数值范围。风扇不接受负转速；电压、电流可为负；NVMe 寿命消耗允许达到 255%；缺失数值是 null。JSON u64 超过 JavaScript 安全整数范围时使用十进制字符串。

实例详情显示 CPU 型号和频率、网卡 IP/MAC/链路信息、风扇/电压/电流/功率/能量、SMART/NVMe 健康和 GPU 信息。设备是否可读、权限不足、缺少驱动或工具由采集能力诊断展示。

硬件历史标量经过报告校验、SQLite 写入、小时聚合，并由历史 API 和图表提供：

| 指标 | 含义 |
| --- | --- |
| cpu_frequency_mhz | 有效逻辑核心频率的平均值 |
| gpu_power_watts | 所有可读 GPU 中功耗最大值，W |
| gpu_core_clock_mhz | 所有可读 GPU 中核心频率最大值，MHz |
| max_fan_rpm | 所有可读风扇中的最大转速 |
| max_disk_temperature_celsius | 所有可读 SMART 磁盘中的最高温度 |
| max_disk_percentage_used | 所有可读 NVMe 磁盘中最高寿命消耗比例，可超过 100 |

这些是主机级汇总，不是逐设备历史。报告保存策略为：完整 payload 用于最新状态，长期数据为标量与小时聚合；不声称已存储所有设备的每次原始读数。小时聚合使用各指标独立 count/min/max/avg，缺失读数不变成零。慢采样数据会在其刷新前随基础报告重复携带，历史表示当时最近已知状态；磁盘详情展示独立实际采集时间。

当前数据库 schema revision 为 **7**，schema_application_version 为 **0.9.26**。启动校验数据库身份
及 DDL 指纹，拒绝缺失或不匹配的结构。部署使用匹配该数据库身份和报告协议的 Server、Client 与 Web。
自动转换、备份和恢复不在当前支持范围，详见[运维文档](operations.md)。

客户端平台支持矩阵和 smartmontools 配置见相邻客户端仓库 `docs/hardware-monitoring.md`。Windows x64 已通过只读 ADLX/IGCL 补充 AMD/Intel 的温度、功耗、频率、风扇和电压，并按 Windows LUID 与 DXGI/PDH 合并。详细驱动要求与缺失数据处理见客户端 `docs/windows-gpu-vendors.md`。使用现有 GPU、温度和硬件传感器字段，不增加任何命令下发或设备控制接口。

GPU 显存使用率按所有同时提供 used/total 的 GPU 汇总：`sum(used) / sum(total) × 100`。
旧版 Windows AMD 驱动若无法提供 ADLX LUID，且 DXGI 对同型号显卡给出一条动态读数和至多一条相同显存容量的静态读数，管理页会在单张卡片中展示互补数值及全部来源标识。此处理仅影响展示，不修改原始报告或指标汇总；不能唯一匹配的读数继续分别显示。
网络接口与网络硬件若有唯一且相同的接口名，管理页会将流量计数和 IP/MAC/链路信息放在同一张卡片中；无法唯一匹配的网络硬件仍单独显示。物理磁盘的 SMART 健康信息与文件系统挂载卷不是一一对应关系，因此分别展示。
聚合使用可容纳全部设备计数的整数宽度；缺少任一计数的设备不参与，合计 total 为零时返回 `null`。
