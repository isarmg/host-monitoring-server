# Host Monitoring 主机监控

管理 Web 已支持新建实例、查看每实例长期授权码、更换授权码、取消待配对实例和设备配对激活。授权码在服务端加密保存；更换后会撤销该实例的现有客户端凭据，客户端必须使用新码重新配对。
使用说明见 [实例创建与配对](docs/instance-management.md)。

本仓库只包含 Server、服务端管理 Web（`web`）及唯一协议源码（`protocol`）。跨平台
`host-monitor` 已拆到 [host-monitoring-client](https://github.com/isarmg/host-monitoring-client)。
服务端使用本地管理员用户名/密码和 SQLite；Client 只读采集主机状态，通过配对取得凭据，并默认经 HTTPS 发送有界报告。
正式 report/OTLP 与配对都要求 HTTPS；明文 HTTP 仅在 debug 构建允许 loopback 开发地址，release 构建拒绝。

跨平台范围只属于 Client。`host-monitoring-server` 的正式构建、发行与运行唯一支持
`x86_64-unknown-linux-gnu`；不提供 ARM Linux、musl、Windows 或 macOS Server。非目标编译在
`build.rs` 失败，发行脚本和进程启动还会分别复核构建宿主、target 与运行内核/机器架构。Windows
`x86_64-pc-windows-msvc` Client 仍是受支持目标。

项目依赖 `sarmg-foundation-server 0.8.2` 的严格登录/Session/ErrorEnvelope 合同、管理员用户名规范化、当前
Argon2id/Token/same-origin 原语、React Session 状态机、React/Vite 构建基线、SQLite 连接基线和 Schema
identity 算法。Host Monitoring 仍独立拥有账户记录、登录准入、Session/CSRF 持久生命周期、Cookie、
产品页面/响应 guard、产品 DDL、文件安全、数据库初始化/锁、Client 配对与投递状态机。管理角色只有
固定 `admin`，不接受 viewer/operator 或其他多角色形状；`admin` 是默认用户名，不是唯一允许的用户名。

管理登录的当前 wire contract 恰好是 `{username,password}`；成功 Session 恰好是
`{authenticated,user_id,username,role,csrf_token}`。用户名候选为 1..64 字节 printable ASCII，Server
按 Foundation 规则 trim ASCII whitespace、转 ASCII 小写后，只接受 3..64 字节、首尾为字母或数字、
字符仅为 `[a-z0-9._-]` 的 canonical username，并明确禁止 `@`。仓库不保留 email 字段或别名。

产品只接受当前 `0.9.19` 配置、协议、SQLite Schema 和发行身份。服务端与 Client 不解释任何非当前
状态或路由，也不执行迁移、备份或恢复；这些能力只有在 `sarmg-upgrade` 建立明确转换边后才成立。

## 仓库组成

```text
protocol/                  host-protocol：Client/Server 唯一共享 wire contract
host-monitoring-server/    Axum 管理/Client API、SQLite 写入与保留策略
web/                       React/Vite 管理 Web：管理员认证、实例、主机详情与配对
config/                    可提交的当前配置样例；不存放生产 Secret
deploy/                    Server 当前 systemd 源资产
scripts/                   发行打包、manifest 和供应链门禁
```

## 快速验证

本仓库的 workspace/Server 门禁必须在 x86_64 GNU/Linux 执行；其他系统须在独立 Client 仓库运行对应目标门禁：

```bash
cargo +1.98.0 fmt --all -- --check
cargo +1.98.0 check --workspace --locked --target x86_64-unknown-linux-gnu --all-targets --all-features
cargo +1.98.0 clippy --workspace --locked --target x86_64-unknown-linux-gnu --all-targets --all-features -- -D warnings
cargo +1.98.0 test --workspace --locked --target x86_64-unknown-linux-gnu --all-targets --all-features
(cd web && npm ci && npm run build)
python3 scripts/check-workflow-supply-chain.py
python3 scripts/test-server-release-tooling.py
```

## 文档

- [文档总览](docs/README.md)
- [初学者学习指南](docs/beginner-guide/README.md)
- [项目工作流程与流程树](docs/project-workflow.md)
- [完整功能与取舍清单](docs/feature-inventory-and-tradeoffs.md)
- [部署、平台打包、安全和故障运维](docs/operations.md)

## 许可证

代码采用 Apache License 2.0，见 [LICENSE-APACHE](LICENSE-APACHE) 与 [NOTICE](NOTICE)。

账号修改方法见 [账号设置](docs/account-settings.md)。
