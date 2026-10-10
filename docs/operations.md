# xsos 运维入口

- [首次安装](server-release-readme.md)：校验发行包、配置、初始化、启动与 HTTPS
- [日常运维](administration.md)：启停、日志、数据库检查、容量与账号
- [配置参考](configuration.md)：完整环境变量
- [排查问题](troubleshooting.md)：就绪、登录、报告与存储问题
- [HTTP 与存储参考](reference.md)：接口、身份和保留细节
- [客户端安装](https://github.com/isarmg/xsoc/blob/main/docs/platform-setup.md)：在被监控主机安装 xsoc

服务端使用 Linux AMD64 GNU、systemd 和一个 SQLite 数据库。运行与维护命令使用相同服务账户和环境；初次部署完成后，日常工作从服务状态与最新报告开始。
