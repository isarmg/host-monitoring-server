# xsos 1.0.0 发行包部署手册

## 安装与初始化


在一台尚未安装本服务的 Linux x86_64 / glibc 主机上，以可使用 sudo 的管理员执行。需要 systemd、GNU tar、sha256sum、curl、OpenSSL 以及账户管理工具。

先从 [Release](https://github.com/isarmg/xsos/releases) 下载同版 Linux 归档及 `.sha256`，在下载目录执行；以下检查用于确认安装位置为空；已有服务的管理见本文后面的运行检查。

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


## HTTPS 入口

在同机反向代理配置域名与系统信任的证书，将 HTTPS 请求转发到 `http://127.0.0.1:18105`，保留原始 Host、Origin 与 Sec-Fetch-Site。外部访问使用这个 HTTPS 地址。后台端口保持回环监听。

## 运行检查

```sh
sudo systemctl status xsos.service --no-pager
sudo journalctl -u xsos.service -n 100 --no-pager
curl --fail http://127.0.0.1:18105/readyz
```

预期服务为 active，`/readyz` 返回 `{"ready":true}`；`/healthz` 的正常状态为 HTTP 204。然后在浏览器打开 HTTPS 地址并登录，创建一个实例，按 [xsoc 安装说明](https://github.com/isarmg/xsoc/blob/main/docs/platform-setup.md)完成配对。管理台应收到该主机的新报告。

修改环境文件后执行 `sudo systemctl restart xsos.service`，再检查就绪状态。`XSOC_AUTHORIZATION_KEY` 与数据库内密文配套，日常修改密码或实例授权码时保持它不变；替换该密钥会使已有密文不可读。

## 安装布局

| 路径 | 用途 |
|---|---|
| `/opt/isarmg/xsos/releases/1.0.0/` | root 所有的只读发行目录 |
| `/opt/isarmg/xsos/current` | 指向该版本的绝对链接 |
| `/etc/isarmg/xsos.env` | root 0600 配置，由 systemd 加载 |
| `/var/lib/isarmg/xsos/db/xsos.sqlite3` | 服务账号所有的当前数据库 |
| `/var/lib/isarmg/xsos/db/logs/` | 私有轮转运行日志 |

归档的顶层是 `1.0.0/`，包含 `bin/xsos`、`systemd/xsos.service`、`web-assets.json`、`README.md` 和 `RELEASE-MANIFEST.json`。管理页面已内嵌到二进制。修改运行参数使用环境文件；发行目录保留下载时的内容与权限，供启动校验使用。

`identity` 输出产品、版本、源码提交和结构身份；`verify-release` 核对完整文件树。SHA-256 用于核对下载完整性，仍需从可信发布页取得归档及校验文件。

| 身份字段 | 当前值 |
|---|---|
| `application` | `xsos` |
| `version` | `1.0.0` |
| `schema_revision` | `1` |
| `target` | `x86_64-unknown-linux-gnu` |

## 遇到问题

- 校验失败：重新核对下载来源、文件名及完整性；保留原始错误。
- `init` 失败：检查环境文件中的密码、Base64 密钥、数据目录所有者与 0700 权限。
- 服务无法启动：先读 journal，确认已初始化，发行树校验通过且配置和数据库配套。
- 本机 ready 正常但无法登录：检查 HTTPS 代理、浏览器地址、Host/Origin 和主机时间。
- 客户端离线：检查客户端日志、HTTPS 信任和配对；再确认报告投递结果。

[配置参考](https://github.com/isarmg/xsos/blob/main/docs/configuration.md) · [日常运维](https://github.com/isarmg/xsos/blob/main/docs/administration.md) · [故障排查](https://github.com/isarmg/xsos/blob/main/docs/troubleshooting.md)
