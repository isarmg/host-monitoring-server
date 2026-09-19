# 08. 测试、调试与安全变更方法

## 8.1 先确定变化落在哪条链

修改前写出从输入到持久化/展示的路径。协议字段涉及 protocol、Client、Server、Web；平台采集涉及目标
系统 fixture；安装涉及服务账户和回滚；Schema 涉及 fingerprint、doctor 与升级仓。只改“最明显文件”
通常会留下不一致合同。

## 8.2 本地基础门禁

完整 workspace/Server 门禁仅在 x86_64 GNU/Linux 执行。Windows/macOS 不尝试构建 Server，而是继续
验证各自 Client；尤其不得删除 CI 中的 `x86_64-pc-windows-msvc` Windows Client release 构建。

```bash
cargo +1.98.0 fmt --all -- --check
cargo +1.98.0 check --workspace --locked --target x86_64-unknown-linux-gnu --all-targets --all-features
cargo +1.98.0 clippy --workspace --locked --target x86_64-unknown-linux-gnu --all-targets --all-features -- -D warnings
cargo +1.98.0 test --workspace --locked --target x86_64-unknown-linux-gnu
cd clients/web
# Node 必须与仓库根 .node-version 的 26.7.0 一致
npm ci
npm run build
```

再运行仓库提供的 workflow、打包和平台静态测试。`cargo check` 成功不能代替测试，单元测试成功也不能
证明安装生命周期。

## 8.3 分层调试

1. `probe` 验证采集。
2. `status` 验证本地状态与配对。
3. `once` 验证 spool 和主投递。
4. Server readiness/日志验证准入与 writer。
5. 数据库/API 验证 latest、raw 及 `resolution=auto` 的 raw/hourly 合并结果。
6. Web 验证 Session、实例分页、配对、自动更新、完整最新详情、趋势和管理变更。

每次只跨一层，避免用“重装一切”掩盖原因。

## 8.4 可靠性测试方法

使用临时目录和受控假 Server 注入：连接拒绝、响应丢失、429/503、认证撤销、磁盘满、部分文件、进程
崩溃和时钟边界。测试重启后状态，而不仅是错误返回当下。

## 8.5 并发测试

覆盖同一 state directory 双实例、重复配对、多个 report 并发、writer queue 满、retention 与摄取并行、
管理员维护锁冲突。断言最终状态、响应分类和资源上限，而非依赖任务恰好执行顺序。

## 8.6 安全测试

包含链接/特殊文件/硬链接、宽权限目录、超大 JSON、unknown fields、CSRF/Origin、forwarded spoof、Secret
redaction、TLS 证书/主机名验证、redirect 拒绝、默认远程 HTTP 拒绝与持久明文开关，以及安装脚本路径
替换。安全负例必须与功能正例同等重要。

管理员合同变更还要跨 Rust DTO、Foundation TS guard/JSON Schema、Host SQLite DDL/Schema SHA、登录
限流 key、Session response、CLI/env 和 React 表单做全文闭包检查。当前正例应覆盖 ` Admin ` 规范化为
`admin`；负例覆盖 `@`、内部空格、首尾分隔符、非 ASCII、control、过短/过长、额外 JSON 字段和非
`admin` role。Host Server/Web 中不应出现 email 字段或兼容 alias。

## 8.7 版本与名称变更

破坏性变更应全量替换 crate/binary/package/service/API/配置/文档/测试/发行 identity，只留下唯一入口。用
全文和文件路径搜索审计，再构建真实包；仅 Cargo metadata 成功不足以证明安装资产已同步。

## 8.8 提交前检查表

- 工作树只包含本问题相关修改。
- 格式、Server x86_64 GNU/Linux、各 Client 目标、Clippy、测试、Web 和脚本全部通过。
- 当前名称与版本全局唯一，忽略构建目录后不存在任何非当前产品身份。
- 文档命令确实存在，链接可解析。
- 没有凭据、真实主机数据、target/node_modules 或临时包。
- 大问题形成可独立回滚的提交。

## 8.9 代码评审提问

失败发生后谁拥有任务？容量是否有上限？客户端超时能否断言未执行？Secret 会流向哪里？崩溃后读取
哪个事实源？当前版本不匹配时是否 fail closed？这些问题比“代码看起来简洁”更能揭示缺陷。
