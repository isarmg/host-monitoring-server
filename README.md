# xsos

xsos 是自托管的主机监控服务，与 xsoc 客户端配合，集中接收遥测并提供内置 Web 管理台。

## 项目功能

- 查看 CPU、内存、磁盘、网络与可用硬件传感器数据
- 查看主机状态、历史趋势和按接收日期归档的上报日志
- 管理客户端实例、配对和授权码

## 适用平台

服务端仅支持 Linux x86_64 / AMD64、glibc，生产部署使用 systemd。Web 页面通过浏览器访问，已内嵌到服务端程序中。

## 快速部署

以下用于全新主机。先从 [Release](https://github.com/isarmg/xsos/releases) 下载同版 Linux 归档及 `.sha256`，在下载目录执行；遇到已有目录、账户或配置时停止，不覆盖现有安装。

```sh
set -eu
sha256sum --check --strict xsos-1.0.0-x86_64-unknown-linux-gnu.tar.gz.sha256
sudo test ! -e /opt/isarmg/xsos
sudo test ! -e /etc/isarmg/xsos.env
sudo test ! -e /etc/systemd/system/xsos.service
sudo test ! -e /var/lib/isarmg/xsos
sudo install -d -m 0755 -o root -g root /opt/isarmg /opt/isarmg/xsos /opt/isarmg/xsos/releases
sudo tar -xzf xsos-1.0.0-x86_64-unknown-linux-gnu.tar.gz \
  -C /opt/isarmg/xsos/releases --same-permissions --delay-directory-restore
sudo chown -R root:root /opt/isarmg/xsos/releases/1.0.0
sudo /opt/isarmg/xsos/releases/1.0.0/bin/xsos \
  verify-release --root /opt/isarmg/xsos/releases/1.0.0
sudo groupadd --system xsos
sudo useradd --system --gid xsos --home-dir /var/lib/isarmg/xsos \
  --no-create-home --shell /usr/sbin/nologin xsos
sudo install -d -m 0700 -o xsos -g xsos /var/lib/isarmg/xsos/db
sudo install -d -m 0755 -o root -g root /etc/isarmg
sudo sh -c 'umask 077; set -C; : > /etc/isarmg/xsos.env'
openssl rand -base64 32
sudoedit /etc/isarmg/xsos.env
```

填写下列配置：将密码替换为至少 12 字节的独立强密码，密钥替换为上一步生成的 Base64 值；密钥需长期保存。

```dotenv
XSOS_DATABASE_URL=sqlite:///var/lib/isarmg/xsos/db/xsos.sqlite3
XSOS_BIND=127.0.0.1:18105
XSOS_DEVELOPMENT=false
XSOS_BOOTSTRAP_ADMIN_USERNAME=admin
XSOS_BOOTSTRAP_ADMIN_PASSWORD=REPLACE_WITH_A_UNIQUE_LONG_PASSWORD
XSOC_AUTHORIZATION_KEY=REPLACE_WITH_BASE64_ENCODED_32_RANDOM_BYTES
```

先初始化，再启动服务：

```sh
sudo ln -sT /opt/isarmg/xsos/releases/1.0.0 /opt/isarmg/xsos/current
sudo systemd-run --wait --collect -p User=xsos -p Group=xsos \
  -p EnvironmentFile=/etc/isarmg/xsos.env \
  /opt/isarmg/xsos/releases/1.0.0/bin/xsos init
sudo install -m 0644 -o root -g root \
  /opt/isarmg/xsos/current/systemd/xsos.service /etc/systemd/system/xsos.service
sudo systemctl daemon-reload
sudo systemctl enable --now xsos.service
curl --fail http://127.0.0.1:18105/readyz
```

就绪响应应为 `{"ready":true}`。通过 HTTPS 反向代理转发到 `127.0.0.1:18105`，使用配置的管理员账号登录；初始化成功后从环境文件移除 `XSOS_BOOTSTRAP_ADMIN_PASSWORD`。客户端须另行安装并配对。

## 编译部署

在 Linux AMD64 GNU 主机准备 Git、Rust `1.99.0`、Node.js `26.7.0`、npm、Python `3.11+` 和 C 编译工具。从干净源码、与版本号一致且精确指向 HEAD 的 annotated tag 构建发行包；输出目录必须已存在、位于仓库外且不含同名制品：

```sh
git clone https://github.com/isarmg/xsos.git
cd xsos
git checkout v1.0.0
mkdir -p "$HOME/xsos-output"
python3 scripts/package-server-release.py "$HOME/xsos-output"
```

脚本构建 Web 和 Rust、生成校验信息并验证发行包。将输出的归档和 `.sha256` 复制到目标主机，按“快速部署”安装。

[详细文档](docs/README.md)
