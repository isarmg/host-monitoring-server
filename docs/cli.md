# xsos 命令参考

在安装主机运行 `/opt/isarmg/xsos/current/bin/xsos`。下面以程序名简写；生产环境应使用[运维示例](administration.md)加载与服务相同的环境和账户。

| 命令 | 用途 | 执行时机 |
|---|---|---|
| `--help` / `--version` | 查看命令和版本 | 随时 |
| `identity` | 查看产品、源码、平台和结构身份 | 随时 |
| `verify-release --root PATH` | 校验完整发行目录 | 安装后、启动问题排查时 |
| `init` | 创建数据库和首个管理员 | 配置完成后的首次安装；私有数据目录应为空 |
| `config validate --json` | 通过私有快照验证配置、当前数据库和凭据 | 可与服务并行 |
| `run --release-root PATH` | 运行正式服务 | 初始化完成后，由 systemd 启动 |
| `status --json` | 验证当前监听地址的服务身份及 ready 状态 | 运行期间 |
| `doctor` | 深入检查数据库与业务结构 | 先停止服务 |
| `admin-reset-password --username NAME` | 从 stdin 读取新密码并撤销账号会话 | 先停止服务 |

## 配置选择

`--config /absolute/server.json` 选择 JSON 配置；`--data-dir /absolute/data` 选择私有数据目录。优先级为命令行、显式映射的环境变量、文件、默认值。文件中的未知/重复字段或错误类型会报错，即使更高优先级覆盖了该字段。

`config validate --json` 返回 `sources`、`schema_identity` 和 `state_paths`。JSON 模式输出单个记录，失败为稳定的 `code/message/details` 并返回非零退出码。身份和帮助查询无需数据库。字段与环境变量见[配置参考](configuration.md)。

## 文件与并发

数据目录属于服务账户，目录权限 0700、私有文件 0600。运行持有实例锁与共享维护锁；doctor 和密码重置需要排他维护锁。维护待决标记 `.state-maintenance-pending.json` 存在时，先排查未完成维护操作。配置校验使用独立快照，保持原数据库文件不变。

正式启动校验只读发行树。`current` 是安装目录下指向当前版本实体目录的单跳绝对链接，安装父目录与 releases 目录使用相同所有者和 0755 权限。
