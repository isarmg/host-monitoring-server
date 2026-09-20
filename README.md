# Host Monitoring Server

Host Monitoring Server `0.9.23` 是集中接收和展示主机遥测的管理服务。Rust/Axum 服务端负责管理员登录、Client 实例、指标接收与聚合；内置 React Web 用于查看主机状态、历史趋势和实例配置。

正式 Server 仅支持 Linux AMD64 GNU（`x86_64-unknown-linux-gnu`）。Client 位于独立的 [host-monitoring-client](https://github.com/isarmg/host-monitoring-client) 仓库。

## 配置概览

生产发行树使用 `/etc/isarmg/host-monitoring.env`。从模板创建受保护的环境文件：

```sh
sudo install -d -m 0750 /etc/isarmg
sudo install -m 0600 config/host-monitoring.env.example /etc/isarmg/host-monitoring.env
openssl rand -base64 32
sudoedit /etc/isarmg/host-monitoring.env
```

至少替换管理员密码和 `HOST_MONITORING_CLIENT_AUTHORIZATION_KEY`，并检查数据库、静态资源与监听地址。发行包中的服务启动命令为：

```sh
/opt/isarmg/host-monitoring/releases/0.9.23/bin/host-monitoring-server \
  serve-release --root /opt/isarmg/host-monitoring/releases/0.9.23
```

建议只监听 loopback，由 HTTPS 反向代理对外提供 Web。完整部署、账号维护、备份和诊断见[运维文档](docs/operations.md)。

## 开发验证

```sh
cargo +1.98.0 fmt --all -- --check
cargo +1.98.0 clippy --locked --target x86_64-unknown-linux-gnu --all-targets -- -D warnings
cargo +1.98.0 test --locked --target x86_64-unknown-linux-gnu
(cd web && npm ci && npm run build)
```

## 文档

- [文档总览](docs/README.md)
- [初学者指南](docs/beginner-guide/README.md)
- [项目工作流程](docs/project-workflow.md)
- [功能范围与取舍](docs/feature-inventory-and-tradeoffs.md)
- [部署与运维](docs/operations.md)

代码采用 [Apache License 2.0](LICENSE-APACHE)。
