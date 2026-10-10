# 管理 xsos 服务

本文使用发行包的默认路径。所有 shell 命令在服务端主机执行，账户具备 sudo 权限。

## 查看服务和日志

```sh
sudo systemctl status xsos.service --no-pager
sudo journalctl -u xsos.service -n 100 --no-pager
curl --fail http://127.0.0.1:18105/readyz
```

预期 `/readyz` 返回 `{"ready":true}`。持续跟踪日志使用 `sudo journalctl -u xsos.service -f`，Ctrl+C 只结束查看。
应用还在数据目录的 `logs/` 写入 JSON 日志，默认活动文件 8 MiB、四个归档，总上限 40 MiB。

## 停止、启动与修改配置

```sh
sudo systemctl stop xsos.service
sudoedit /etc/isarmg/xsos.env
sudo systemctl start xsos.service
curl --fail http://127.0.0.1:18105/readyz
```

只改环境文件即可重新启动加载；修改 unit 才需要 `systemctl daemon-reload`。`systemctl enable` 设置开机自启，`start` 启动当前进程，`enable --now` 同时执行两者。参数含义见[配置参考](configuration.md)。

## 检查数据库与运行状态

以下示例让诊断进程使用与正式服务相同的身份和环境。只读 `config validate` 可以在服务运行时执行：

```sh
sudo systemd-run --wait --pipe --collect -p User=xsos -p Group=xsos \
  -p EnvironmentFile=/etc/isarmg/xsos.env \
  /opt/isarmg/xsos/current/bin/xsos config validate --json
```

预期退出码为 0，并返回配置来源、结构身份和状态路径。`status --json` 使用相同前缀检查监听地址上的服务身份和就绪状态。

深入数据库检查使用 `doctor`，先停服务，让它取得维护锁：

```sh
sudo systemctl stop xsos.service
sudo systemd-run --wait --pipe --collect -p User=xsos -p Group=xsos \
  -p EnvironmentFile=/etc/isarmg/xsos.env \
  /opt/isarmg/xsos/current/bin/xsos doctor
```

检查成功后再 `sudo systemctl start xsos.service`。doctor 会打开当前数据库检查完整性、外键、保留结构及授权码密文，故与运行服务互斥；失败时按错误修复配置或权限，保留业务文件供排查。

## 维护管理员账号

正常修改用户名或密码使用管理台右上角人物图标，见[账号设置](account-settings.md)。忘记密码时，在停服窗口运行 `admin-reset-password --username admin`，将 admin 替换为当前用户名；密码从标准输入读取。

```bash
sudo systemctl stop xsos.service
read -r -s -p 'New password: ' new_password; printf '\n'
printf '%s\n' "$new_password" | sudo systemd-run --wait --pipe --collect \
  -p User=xsos -p Group=xsos -p EnvironmentFile=/etc/isarmg/xsos.env \
  /opt/isarmg/xsos/current/bin/xsos admin-reset-password --username admin
unset new_password
```

成功后启动服务并重新登录。密码须为 12–1024 字节且不含 ASCII 控制字符；重置会撤销该账号已有会话。上例用隐藏输入，避免把密码写进命令历史。

## 容量与健康

原始报告默认保留 7 天，小时聚合默认 365 天，latest 引用的报告会保留。趋势查询使用仍可用的原始点和小时桶。调节保留期见[配置参考](configuration.md)。

巡检关注数据库/WAL 占用、磁盘与 inode、HTTP 429/503、客户端积压和报告接收时间。retention 连续三次失败会使就绪检查降级。控制面的 audit/session 等记录不随遥测保留期统一清理；把它们计入长期容量。当前健康接口返回最小状态，队列深度和延迟没有 metrics API。

## 处理凭据泄露

先隔离受影响的外部入口，保留脱敏时间线、请求 ID 和日志；轮换管理员密码与受影响实例授权码，再验证连接。主密钥 `XSOC_AUTHORIZATION_KEY` 与数据库密文绑定，确认泄露后需要停用受影响状态并重新建立受信任实例。漏洞报告使用仓库的私密安全报告入口，公开工单仅附脱敏诊断。
