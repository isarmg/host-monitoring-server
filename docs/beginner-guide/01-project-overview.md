# 01. 项目全景与版本边界

## 1.1 一句话理解

Host Monitoring 是独立部署的主机遥测系统：每台受管主机运行 `host-monitor`，控制面运行
`host-monitoring-server`。客户端只主动向服务端建立 HTTPS 连接，服务端不会借监控功能在主机上执行
命令或修改配置。跨平台客户端与单平台控制面是两个不同边界：Server 只支持
`x86_64-unknown-linux-gnu`，Client 继续支持 Linux、Windows、macOS 与移动宿主合同。

## 1.2 四个组成部分

```text
平台采集器 -> host-monitor -> 当前报告协议 -> Server -> SQLite -> React Host 列表
                       \-> 可选 OTLP
```

| 部分 | 责任 | 不拥有的责任 |
|---|---|---|
| `host-protocol` | 配对和报告的唯一 wire contract | 网络、磁盘和 UI |
| `host-monitor` | 采集、配对、本地状态、spool、投递 | 管理员会话和历史查询 |
| Server | 认证、准入、持久化、聚合、保留 | 远程 Shell 和主机修复 |
| Web | 管理员认证与 Host 列表 JSON | pairing 管理、详情/history、图表、备注/删除、audit 与 Secret 保存 |

## 1.3 当前版本是一个整体

当前 `0.8.0` 配置、状态、数据库、API 和发行 manifest 是同一个版本身份。任何一项不匹配都应在监听
或业务写入前拒绝。产品不读取非当前配置，不注册平行路由，不转换其他数据库身份，也不增加字段
fallback；备份、恢复和具体版本转换只有在 `sarmg-upgrade` 明确实现后才属于支持范围。

## 1.4 两类身份

浏览器管理员以本地 username/password 登录，通过 Foundation 精确 Session 与 CSRF 调用受保护 API；
设备通过配对发放的 credential 提交报告。二者不能互换。管理员激活 pairing 并不意味着设备获得浏览器
权限；设备 credential 也不能调用管理 API。

管理 role 只有 `admin`，没有 viewer/operator/RBAC。`admin` 同时是默认 username，但 username 可以是
其他符合 Foundation canonical 规则的值。登录 wire object 恰好含 `username/password`；Session 恰好含
`authenticated/user_id/username/role/csrf_token`。Server 和 Web 不保存、不返回也不接受 email 字段。

## 1.5 两条数据通路

主通路是 `host-monitor -> Server -> SQLite`，决定控制台看到的当前与历史数据。可选 OTLP 是额外导出，
其失败不能伪造主通路成功，也不能让无界缓存拖垮 Client。每条通路均有独立超时、容量和错误分类。

## 1.6 仓库地图

```text
host-monitoring/
├─ protocol/                  wire contract
├─ clients/host-monitor/      跨平台客户端、库和安装资产
├─ clients/web/               React/Vite 管理员认证与 Host 列表
├─ host-monitoring-server/    API、SQLite 与发行合同
├─ config/                    Server env 与 Client JSON 的当前样例
├─ deploy/                    Server systemd 源资产
├─ scripts/                   发布和供应链检查
└─ docs/                      教程、流程、取舍与运维
```

阅读代码时先看 `protocol`，再分别跟踪 Client 和 Server；否则容易把同名字段的传输语义与存储语义混为
一谈。

## 1.7 主要架构取舍

- SQLite 换取单机部署简单，但只允许一个写入控制面。
- 本地 spool 换取短期断网可恢复，但必须严格限制容量并保护目录。
- Client 原生三平台安装换取真实传感器与服务管理，代价是更大的测试矩阵；Server 不进入该矩阵。
- 移动端只提供宿主库，尊重后台限制，代价是采样周期由平台决定。
- 严格当前版本减少长期多版本负担，代价是任何外部转换都必须有明确停机流程和已实现的支持边。

## 1.8 新手常见误解

1. “收到报告”不等于已经持久化；Server 只有事务提交后才返回 `202`。
2. HTTP 超时不等于服务端未处理；Client 必须依据可重试分类保留原报告 ID。
3. 实例授权码长期存在但不是报告 credential；管理员更换它会撤销当前 Client credential，并要求显式重新配对。
4. 卸载程序默认保留本地状态不表示该状态可被另一发行读取；再次安装前必须验证当前 identity。
5. 当前 Web 没有图表和 history 视图；页面中的列表 JSON 只反映 Host summary/latest 标量。

## 1.9 本章检查

能用自己的话回答以下问题再进入下一章：谁拥有 wire contract；何时报告才算接受；为什么客户端不监听
公网端口；为什么数据库转换不能进入 Server；移动库为何不实现常驻 daemon。
