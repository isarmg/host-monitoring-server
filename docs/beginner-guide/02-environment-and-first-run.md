# 02. 开发环境与第一次运行

## 2.1 工具链

仓库固定 Rust `1.98.0` 与 `.node-version` 中的 Node `26.7.0`。Server Web 使用 lockfile 对应的 npm。
这个 Node 版本同时满足 Foundation Server 0.8.2 包的 engine 合同；React `19.2.8`、Vite `7.3.6` 与 TypeScript
`5.8.3` 由 `@sarmg/admin-web` 的 `ADMIN_WEB_TOOLCHAIN` 精确门禁。Linux 常规开发可覆盖协议、服务
端和大部分 Client 逻辑；Server 的唯一目标是 `x86_64-unknown-linux-gnu`，所以完整 workspace 门禁和
Server 启动必须在 x86_64 glibc Linux 执行。Windows MSI、macOS pkg 以及真实平台采集仍由相应系统和
CI 验证，Windows `x86_64-pc-windows-msvc` Client 不因 Server 的单平台边界而移除。

```bash
rustup toolchain install 1.98.0
cargo +1.98.0 metadata --no-deps
cargo +1.98.0 check --workspace --locked --target x86_64-unknown-linux-gnu --all-targets --all-features
cd web
npm ci
npm run build
```

先使用 `--locked` 验证锁图，不要在普通功能修改中顺手升级依赖。

## 2.2 第一次启动 Server

为实验建立仅当前用户可访问的临时状态目录，准备 SQLite URL、Web `dist` 绝对路径、初始管理员和显式
开发模式。开发监听必须保持回环。不要复制生产数据库作为练习数据。

```text
HOST_MONITORING_DATABASE_URL=sqlite:///tmp/host-monitoring/app.db
HOST_MONITORING_STATIC_DIR=/absolute/repository/web/dist
HOST_MONITORING_BOOTSTRAP_ADMIN_USERNAME=admin
HOST_MONITORING_BOOTSTRAP_ADMIN_PASSWORD=<local-only-secret>
HOST_MONITORING_DEVELOPMENT=true
```

```bash
cargo +1.98.0 run --target x86_64-unknown-linux-gnu -p host-monitoring-server -- serve
```

开发命令与正式 `serve-release` 是不同安全边界。source-bound 正式二进制必须从验证过的固定发行树启动；
所有 Server 命令（包括 `identity`、`doctor` 和维护命令）都会先拒绝非 Linux/x86_64 运行环境。

首次空库中，username 默认 `admin`，密码必须为 12..1024 字节且不含 ASCII control。username 登录候选
必须是 1..64 字节 printable ASCII；Server trim ASCII whitespace、转 ASCII 小写后要求 canonical 值为
3..64 字节、首尾 `[a-z0-9]`、字符仅 `[a-z0-9._-]`，明确禁止 `@`。请求只接受
`{"username":"admin","password":"..."}`，不接受 email 字段。成功响应严格含
`authenticated/user_id/username/role/csrf_token`，其中 role 恒为 `admin`。

## 2.3 第一次运行客户端

从 `config/host-monitor.json.example` 创建仅用于临时环境的配置，设置当前 `application_version`、Server
HTTPS 地址和独立 state directory。按以下顺序理解命令：

```bash
cargo +1.98.0 run -p host-monitor -- probe --config /absolute/config.json
cargo +1.98.0 run -p host-monitor -- pair --config /absolute/config.json
cargo +1.98.0 run -p host-monitor -- status --config /absolute/config.json
cargo +1.98.0 run -p host-monitor -- once --config /absolute/config.json
```

`probe` 只证明采集；`pair` 创建/恢复请求并等待 activation；`status` 读取本地绑定；`once` 才同时覆盖
采集和主通路投递。当前 React 页面没有 invite/activation 操作，所以不能只打开 activation URL 完成
配对；开发联调需直接覆盖受保护 invite/activation API，Windows Tray 路径还可把实例授权码交给 Client
提交。不要跳过配对后把 401 当作采集器故障。

## 2.4 安全地观察状态

配置、active binding、pending pairing、spool 和锁文件都在 state directory 边界内。检查文件名、mode、
大小和时间可以帮助排障，但不要输出 credential 内容。测试结束后删除临时实验目录，不要让它与系统包
的生产目录重叠。

## 2.5 第一次成功的验收定义

一次当前代码可完成的练习应证明：Server readiness 正常；管理员能登录；Client `probe` 返回受限合法
报告；通过管理 API 创建 invite 并完成 activation；`once` 返回成功；Host 列表 API 和 React JSON 中出现
同一 Host 的 latest 摘要；重启 Client 后继续使用同一当前绑定。把 React pairing 页面列为尚未满足的
验收项，而不是手工跳过后记为成功。

## 2.6 常见失败

| 现象 | 首查 |
|---|---|
| Web 404 | `STATIC_DIR` 是否为已构建绝对路径 |
| Server 拒绝数据库 | metadata、Schema、文件类型或实例锁 |
| Pair 一直 pending | invite/code 是否有效、activation 是否调用；当前 React 页本身不能批准 |
| TLS 失败 | CA、主机名、证书时间；Client 没有关闭证书校验的开关 |
| `once` 429/503 | Server 准入或 writer，Client 应保留报告 |
| 第二实例失败 | state directory 锁，这是预期保护 |

## 2.7 练习后质量门

在 x86_64 GNU/Linux 运行 `cargo fmt`、workspace check/test 和 Web build；其他平台运行 Client 自己的目标
门禁。此时只建立基线，不改 fingerprint、数据库或配置来“让测试通过”。如果基线失败，先记录环境与
错误层次。
