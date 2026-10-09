# xsos

当前工作树为 `1.0.0` 发行候选；正式源码、标签与资产以通过 CI 的精准 Source 和 Release manifest 为准。

当前启动入口和初始化边界见 [服务命令](docs/cli.md)。部署须先显式 `init`，再 `run`；配置验证和状态查询失败会返回非零退出码。

xsos `1.0.0` 是集中接收和展示主机遥测的管理服务。Rust/Axum 服务端负责管理员登录、Client 实例、指标接收与聚合；内置 React Web 用于查看主机状态、历史趋势、按服务器日期归档的上报日志和实例配置。

正式 Server 仅支持 Linux AMD64 GNU（`x86_64-unknown-linux-gnu`）。Client 位于独立的 [xsoc](https://github.com/isarmg/xsoc) 仓库。

设备侧从安装、配对/重新配对到服务或后台任务管理、诊断与卸载，见独立 [Client 分平台部署指南](https://github.com/isarmg/xsoc/blob/main/docs/platform-setup.md)。

## 配置概览

生产发行树使用 `/etc/isarmg/xsos.env`。从模板创建受保护的环境文件：

```sh
sudo install -d -m 0750 /etc/isarmg
sudo install -m 0600 config/xsos.env.example /etc/isarmg/xsos.env
openssl rand -base64 32
sudoedit /etc/isarmg/xsos.env
```

至少替换管理员密码和 `XSOC_AUTHORIZATION_KEY`，并检查数据库、静态资源与监听地址。发行包中的服务启动命令为：

```sh
/opt/isarmg/xsos/current/bin/xsos \
  run --release-root /opt/isarmg/xsos/current
```

建议只监听 loopback，由 HTTPS 反向代理对外提供 Web。完整部署、账号维护、备份和诊断见[运维文档](docs/operations.md)。

## 开发验证

```sh
cargo +1.99.0 fmt --all -- --check
cargo +1.99.0 clippy --locked --target x86_64-unknown-linux-gnu --all-targets -- -D warnings
cargo +1.99.0 test --locked --target x86_64-unknown-linux-gnu
(cd web && npm ci && npm run build)
```

## 文档

- [文档总览](docs/README.md)
- [初学者指南](docs/beginner-guide/README.md)
- [项目工作流程](docs/project-workflow.md)
- [功能范围与取舍](docs/feature-inventory-and-tradeoffs.md)
- [部署与运维](docs/operations.md)

代码采用 [Apache License 2.0](LICENSE-APACHE)。

硬件监控扩展、平台支持与当前协议要求见 [硬件监控说明](docs/hardware-monitoring.md)。

当前发布版本：**1.0.0**。参见 [1.0.0 发布说明](docs/releases/1.0.0.md)和[项目命名](docs/naming.md)。
