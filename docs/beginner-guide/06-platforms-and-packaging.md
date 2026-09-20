# 06. Client 的 Linux、Windows、macOS 与移动宿主

## 6.1 共同产品核

本章的跨平台矩阵只描述 Client。三种桌面 Client 共享协议、配置语义、配对、spool 和投递；平台层只实现
采集、服务管理、路径权限、用户交互与安装生命周期。不能以平台便利为理由改变 wire contract。
Server 不属于该矩阵，只构建、发行和运行于 `x86_64-unknown-linux-gnu`。

## 6.2 Linux Client

deb/rpm 安装二进制、0600 配置、专用账户和 systemd unit。默认服务沙箱应保持收紧；NVIDIA 等设备访问
通过明确 drop-in 放宽，而不是默认给所有设备权限。普通卸载保留状态，显式 purge 才清除当前身份和
spool。

## 6.3 Windows Client

Windows Service 承担长期采集，交互配置与配对由管理员终端中的 `host-monitor` CLI 完成；维护 helper
处理受保护的服务/文件事务。Service helper 使用 GUI subsystem 避免后台弹出控制台，交互 CLI 保持
console subsystem。WiX 安装失败必须回滚本次创建的文件、服务和权限。

## 6.4 macOS Client

pkg 创建不可登录服务账户、LaunchDaemon 和日志轮转。安装/卸载脚本验证目标路径、账户身份和资源归属，
不能用宽泛递归删除。安装失败测试要证明无关账户、同名外部文件和已有状态不受影响。

## 6.5 移动宿主库

Android、iOS、iPadOS 只使用无 daemon 外壳的 Rust library contract。宿主提供 `SystemSnapshot`，Rust
执行边界收敛和 JSON 编码；宿主负责系统权限、后台窗口、HTTPS、队列和 Keystore/Keychain。仓库证明
target 可编译，不等于交付完整移动 App。

## 6.6 安装权限与运行权限

安装器可能需要管理员/root 创建账户和服务；运行时应使用最小权限账户。二者必须区分。安装脚本不得
把临时高权限、宽目录 mode 或 Secret 留给运行进程。

## 6.7 日志

各平台使用原生日志设施，但字段语义应一致：版本、Host 摘要、report/request ID、采集器名、错误分类。
credential、Token、完整配置和可能敏感的主机数据不能进入日志。

## 6.8 包测试矩阵

| Client 平台 | 静态检查 | 生命周期检查 | 运行检查 |
|---|---|---|---|
| Linux | unit/nfpm/权限 | install/remove/purge | systemd、采集、投递 |
| Windows | WiX authoring/PE | install/rollback/uninstall | Service、CLI、本机 IPC |
| macOS | pkg/plist/script | install failure/uninstall safety | LaunchDaemon、日志轮转 |
| Mobile | Rust target/API contract | 宿主工程集成 | 权限、后台调度由 App 验收；当前无稳定 C ABI/FFI 包装 |

这些检查证明包结构与生命周期，不自动证明发布者身份。仓库当前没有 Linux 包签名或 Windows
Authenticode/MSI 签名步骤。macOS 打包器可在输入 Mach-O 已有 Developer ID Application 签名时，用
Developer ID Installer 签 pkg；未提供 identity 时只能生成 unsigned prerelease，而且脚本不做
notarization/stapling。正式分发必须把缺失的签名链作为独立发布门禁，不能用 smoke test 替代。

## 6.9 名称变更原则

客户端当前唯一名称是 `host-monitor`。binary、crate、服务、配置样例、安装器、包、日志标签、IPC、测试
和文档必须同步使用这一身份。不要保留另一个可执行名、服务 alias 或读取另一目录的 fallback。
