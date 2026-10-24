# 架构与代码导航

## 运行链路

桌面快捷方式和当前用户登录自启动直接运行 `CodexUsageMonitor.exe`。稳定入口读取 `current.json`，转交给经过验证的版本目录。Rust 宿主发现 Codex、探测本机 CDP、管理托盘和私有 Node 子进程。Node 通过 CDP 挂载面板、采集统计并响应设置操作。

Codex 尚未运行时，宿主携带仅限 `127.0.0.1` 的 CDP 参数启动它；已经开放 CDP 时直接连接。已运行但没有 CDP 时，等待用户正常退出后从新入口启动。端口冲突会自动回退，实际端口记录在用户数据目录的 `state.json`。

## 代码入口

| 位置 | 职责 |
| --- | --- |
| `native/src/main.rs` | 原生命令入口、稳定启动器和单实例协调。 |
| `native/src/platform.rs` | Store/非 Store 发现、Win32/COM、DPAPI、快捷方式。 |
| `native/src/runtime.rs` | CDP 探测、Codex 启动、Node 生命周期、托盘。 |
| `native/src/install.rs` | 白名单安装、版本切换、自启动备份恢复、卸载和 ZIP 验证。 |
| `scripts/injector.mjs` | CDP 会话、采集调度、面板注入与配置命令。 |
| `scripts/native-host.mjs`、`scripts/native-config.mjs` | 原生模式路由和配置验证，替代运行时 PowerShell 调用。 |
| `scripts/usage/` | 数据源客户端、归一化、Token 账本与历史。 |
| `assets/` | 内嵌面板的脚本和样式。 |
| `scripts/auto-updater.mjs` | 可选更新检查、下载和验证。 |
| `installer/native-setup.iss` | 单文件安装引导器，调用 Rust 安装逻辑。 |

## 生命周期与数据边界

- 程序放在 `%LOCALAPPDATA%\Programs\CodexUsageMonitor`；版本目录不可变，完整校验后才切换指针。
- 用户数据统一放在 `%LOCALAPPDATA%\CodexUsageMonitor`，与程序更新分开。重新安装与卸载保留凭据、历史和 Token 账本。
- DPAPI 使用当前 Windows 用户范围；密钥通过匿名管道传给原生程序，不放入命令参数或日志。
- Store 官方 CLI 复制到用户目录镜像后执行；不修改 WindowsApps、app.asar、Codex 登录或模型配置。
- 退出监视器只结束其拥有的 Node 子进程，并尝试移除面板；Codex 继续运行。
- 更新只切换本工具版本并重启本工具；任何失败保留当前版本。卸载清理白名单程序文件，保留用户追加文件。

## 安装包与验证

`scripts/build-native.mjs` 根据 `config/native-package-files.json` 组装运行包，并生成 Node/Rust 依赖许可。`scripts/build-installer.mjs` 将同一白名单打入 Setup EXE。Inno Setup 仅负责安装向导和解包，Windows 中唯一的卸载入口由 Rust 注册。

隔离测试使用临时安装根目录，避免改动真实用户快捷方式、自启动和当前版本。`tests/native-package.mjs` 验证复制、哈希、重复安装、拒绝损坏版本及卸载保留文件；`tests/native-setup.mjs` 实际执行 Setup，验证安装、覆盖升级与卸载。真实 Codex CDP、订阅和界面需要另行运行验证，文件测试不能替代它们。

此源码目录仅维护原生版；不包含旧版部署脚本及其回退分支。配置、重连与更新安装通过原生 EXE 完成。
