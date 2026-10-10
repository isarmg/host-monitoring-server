# xsos 开发与验证

## 工具链与构建

使用 `rust-toolchain.toml` 固定的 Rust `1.99.0`、`.node-version` 固定的 Node.js `26.7.0`、npm、Python `3.11+` 和本机 C 构建工具。Server 只在 Linux x86_64 GNU 上构建、运行；客户端各平台能力不改变这个限制。依赖由 Cargo/npm 锁文件和固定 Git 来源约束。

在仓库根目录执行：

```sh
npm --prefix web ci
web/node_modules/.bin/xcss-build-server --config xcss-web-build.json --mode development --no-install
cargo +1.99.0 fmt --all -- --check
cargo +1.99.0 clippy --locked --target x86_64-unknown-linux-gnu --all-targets -- -D warnings
cargo +1.99.0 test --locked --target x86_64-unknown-linux-gnu
python3 scripts/check-workflow-supply-chain.py
```

单独构建管理页面可执行 `npm --prefix web run build`。本地服务初始化、启动和停止见[本地开发服务](local-service.md)，生产归档与安装见[部署手册](server-release-readme.md)。正式打包要求干净源码和精确 annotated tag；开发构建通过不代表正式制品已通过发布验收。

## 运行边界

部署先显式 `init`，再 `run`；普通运行不创建管理员或覆盖状态。配置校验和状态检查失败返回非零退出码，完整命令与配置来源见[服务命令](cli.md)。生产配置由 systemd `EnvironmentFile` 加载，秘密保存在受保护文件中，服务只监听受控地址，并通过 HTTPS 反向代理提供管理入口。

首次初始化后移除长期配置中的管理员明文密码；加密密钥需要稳定保存，不作为日常密码或授权码轮换项。账号维护、锁、诊断和安全事件处理保留在[运维](operations.md)。

## 架构与许可

工程结构、依赖来源和服务端约束见[架构](architecture.md)；公共支撑的职责、单体依赖与验证见[公共支撑](common-support.md)。代码采用 [Apache License 2.0](../LICENSE-APACHE)。

## Web 和打包检查

```sh
npm --prefix web run test:unit
npm --prefix web run build
npm --prefix web run test:browser
python3 scripts/test-server-release-tooling.py
```

浏览器测试需要本机已安装的 Playwright Chromium/Firefox；首次准备使用 `cd web && npx playwright install --with-deps chromium firefox`。常规浏览器测试使用受控 API 数据；真实服务和 Client 联调另行记录。原生安装、硬件和真实 Sunshine 的结果按目标环境单独验证。

## 编辑文档

安装页围绕一条可完成的工作流，使用页解释正常结果，参考页集中字段和精确语义。命令注明执行平台、目录、权限和预期输出；风险放在相关步骤旁。参考 [GNU 用户手册组织建议](https://www.gnu.org/prep/standards/html_node/GNU-Manuals.html)和 [Linux 文档的读者分类](https://docs.kernel.org/)。改动后检查相对链接、标题锚点、示例语法及打包器实际带入的文档。
