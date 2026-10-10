# Rust 原生接口审查

Server 与协议的产品 Rust 源码使用 workspace 级 `unsafe_code = "forbid"`，包括构建脚本、示例和测试。当前产品代码没有 unsafe 块、unsafe 函数或实现；底层原生能力由受控依赖提供。

## 复核方法

从根 Cargo.toml 检查 workspace lint 与内部包继承关系；对 crates、构建脚本和 scripts 中的 Rust 文件搜索原生边界，再运行[开发指南](development.md)的格式、Clippy 和测试。依赖来源由 Cargo 清单与锁文件核对。

编译期 lint 覆盖产品代码约束。Linux 文件系统、锁、systemd 与发行包启动在原生 Linux 环境分别验证；协议或浏览器测试记录其自身覆盖范围。
