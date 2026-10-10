# 02. 开发环境与第一次运行

## 2.1 工具链

仓库固定 Rust `1.99.0` 与 `.node-version` 中的 Node `26.7.0`。Server Web 使用 lockfile 对应的 npm。
这个 Node 版本同时满足 xcss 1.0.0 包的 engine 合同；React `19.3.0`、Vite `8.3.3` 与 TypeScript
`5.8.3` 由 `@xcss/web/admin-web` 的 `ADMIN_WEB_TOOLCHAIN` 精确门禁。Linux 常规开发可覆盖协议和服务端
逻辑；Server 的唯一目标是 `x86_64-unknown-linux-gnu`，所以完整 workspace 门禁和 Server 启动必须在
x86_64 glibc Linux 执行。Client 源码、Windows MSI、macOS pkg 以及真实平台采集在独立
`xsoc` 仓库及其 CI 中验证。

```bash
rustup toolchain install 1.99.0
npm --prefix web ci
web/node_modules/.bin/xcss-build-server --config xcss-web-build.json --mode development --no-install
cargo +1.99.0 metadata --no-deps
cargo +1.99.0 check --workspace --locked --target x86_64-unknown-linux-gnu --all-targets --all-features
```

先使用 `--locked` 验证锁图，不要在普通功能修改中顺手升级依赖。

## 2.2 第一次启动 Server

为实验建立仅当前用户可访问的临时状态目录，准备 SQLite URL、初始管理员和显式
开发模式。开发监听必须保持回环。不要复制生产数据库作为练习数据。

```text
XSOS_DATABASE_URL=sqlite:///tmp/xsos/app.db
XCSS_DEV_WEB_DIR=/absolute/repository/web/dist
XSOS_BOOTSTRAP_ADMIN_USERNAME=admin
XSOS_BOOTSTRAP_ADMIN_PASSWORD=<local-only-secret>
XSOS_DEVELOPMENT=true
```

```bash
install -d -m 0700 /tmp/xsos
cargo +1.99.0 run --target x86_64-unknown-linux-gnu -p xsos -- init
cargo +1.99.0 run --target x86_64-unknown-linux-gnu -p xsos -- run
```

先将环境变量导出到当前 Shell，并配置有效的 `XSOC_AUTHORIZATION_KEY`（标准 Base64 的 32 个随机字节）；秘密不得放入命令参数。`init` 只接受全新私有空目录，创建首个管理员和数据库；后续启动只执行 `run`，不会再次初始化。

开发 `run` 与正式 `run --release-root` 是不同安全边界。source-bound 正式二进制必须从验证过的发行树启动；
所有 Server 命令（包括 `identity`、`doctor` 和维护命令）都会先拒绝非 Linux/x86_64 运行环境。

显式 `init` 时，username 默认 `admin`，密码必须为 12..1024 字节且不含 ASCII control。username 登录候选
必须是 1..64 字节 printable ASCII；Server trim ASCII whitespace、转 ASCII 小写后要求 canonical 值为
3..64 字节、首尾 `[a-z0-9]`、字符仅 `[a-z0-9._-]`，明确禁止 `@`。请求只接受
`{"username":"admin","password":"..."}`，不接受 email 字段。成功响应严格含
`authenticated/user_id/username/role/csrf_token`，其中 role 恒为 `admin`。

## 2.3 第一次运行客户端

切换到独立 `xsoc` 仓库，从其 `config/xsoc.json.example` 创建仅用于临时环境的
配置，设置当前 `application_version`、Server HTTPS 地址和独立 state directory。按以下顺序理解命令：

```bash
cargo +1.99.0 run -p xsoc -- probe --config /absolute/config.json
cargo +1.99.0 run -p xsoc -- pair --config /absolute/config.json
cargo +1.99.0 run -p xsoc -- status --config /absolute/config.json
cargo +1.99.0 run -p xsoc -- once --config /absolute/config.json
```

`probe` 只证明采集；`pair` 创建/恢复请求并等待 activation；`status` 读取本地绑定；`once` 才同时覆盖
采集和主通路投递。当前 React 页面可以创建实例、查看长期授权码，并在
`/activate/{request_id}` 页面读取和核对设备后提交激活。Client 也可携带授权码调用 capability
activation 端点。不要跳过配对后把 401 当作采集器故障。

## 2.4 安全地观察状态

配置、active binding、pending pairing、spool 和锁文件都在 state directory 边界内。检查文件名、mode、
大小和时间可以帮助排障，但不要输出 credential 内容。测试结束后删除临时实验目录，不要让它与系统包
的生产目录重叠。

## 2.5 第一次成功的验收定义

一次当前代码可完成的练习应证明：Server readiness 正常；管理员能登录；Client `probe` 返回受限合法
报告；通过管理 Web 创建实例并完成 activation；`once` 返回成功；Host 列表 API 和 React 页面中出现
同一 Host 的 latest 摘要；重启 Client 后继续使用同一当前绑定。浏览器验收还应覆盖授权码显示、设备
核对与激活，不能只调用 API 后跳过页面流程。

## 2.6 常见失败

| 现象 | 首查 |
|---|---|
| Web 404 | 二进制资源清单；若选择开发目录，检查 `XCSS_DEV_WEB_DIR` 与已构建 dist |
| Server 拒绝数据库 | metadata、Schema、文件类型或实例锁 |
| Pair 一直 pending | invite/code 是否有效、activation 是否调用；当前 React 页本身不能批准 |
| TLS 失败 | CA、主机名、证书时间；Client 没有关闭证书校验的开关 |
| `once` 429/503 | Server 准入或 writer，Client 应保留报告 |
| 第二实例失败 | state directory 锁，这是预期保护 |

## 2.7 练习后质量门

在 x86_64 GNU/Linux 运行 `cargo fmt`、workspace check/test 和 Web build；其他平台运行 Client 自己的目标
门禁。此时只建立基线，不改 fingerprint、数据库或配置来“让测试通过”。如果基线失败，先记录环境与
错误层次。

开发默认使用二进制内嵌 Web；`XCSS_DEV_WEB_DIR` 可省略。显式设置它时选择 xcss 开发目录 provider，Web 重新构建即生效。正式 source-bound 发行始终内嵌 Web，拒绝此变量。
