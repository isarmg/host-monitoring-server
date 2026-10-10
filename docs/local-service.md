# 本地管理服务

仅启动 xsos 及其管理 Web，不自动安装或启动采集 Client。
开发服务监听 `http://127.0.0.1:18105`，使用当前认证和 loopback HTTP Cookie。
不用于生产部署，不修改系统服务，也不占用 xscs 本地管理服务的 18104 端口。

在仓库根执行以下命令：安装锁定的 Web 构建依赖，构建 Web 和开发二进制，显式初始化私有数据，随后启动、查看就绪状态并停止本地进程。`init` 只执行一次；已有实例不再初始化。

```sh
npm --prefix web ci
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 web/node_modules/.bin/xcss-build-server --config xcss-web-build.json --mode development --no-install
node scripts/local-service.mjs init
node scripts/local-service.mjs start
node scripts/local-service.mjs status
node scripts/local-service.mjs stop
```

显式 `init` 生成随机管理员密码，账号为 `admin`，并创建权限为 0600 的
`.runtime/local-service/login.txt` 供本机读取；不要提交或分享。用于加密实例长期授权码的随机密钥
保存在同目录的私有配置 `server.json` 中，不另建 `credentials.json`。该配置与数据库必须一起保留；
已有配置不会被 `init` 覆盖，`start` 不生成替代密钥，密钥不匹配会使已保存授权码验证失败。
数据库位于 `.runtime/local-service/data/xsos.sqlite3`，有界轮转日志位于
`.runtime/local-service/data/logs/`。所有运行状态由 Git 忽略，重复启动复用原有配置、数据库和凭据。
重新构建二进制前先停止本服务；默认使用内嵌的管理页面。

Web 与 Server 使用 xcss 同一构建入口。默认二进制自带管理页面；本地启动默认使用内嵌资源；执行 `node scripts/local-service.mjs start --directory-web` 显式选择开发目录 `web/dist`，修改后重新构建 Web 即生效，无需重编译 Rust。也可运行 Vite 开发服务器获得源码热更新。正式 source-bound 二进制拒绝目录资源模式。

构建产物默认位于 `target/x86_64-unknown-linux-gnu/debug/`。若设置自定义 `CARGO_TARGET_DIR`，启动/状态/停止时使用 `XCSS_LOCAL_SERVER_BINARY` 指定同一绝对二进制路径。

本地 `init` 创建 `.runtime/local-service/server.json`（0600）与 `data/`（0700），已有配置拒绝覆盖。`start` 只读取当前配置及已初始化状态，缺失时失败；`status` 不创建目录。运行日志位于 `data/logs/`，默认有界轮转，配置中的秘密不输出到日志。
