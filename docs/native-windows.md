# Windows 原生版（Rust）

原生版保留现有统计和内嵌面板，由 Rust EXE 管理安装、Codex 启动、托盘、DPAPI、自启动及更新安装。发布运行包不包含 PowerShell 或 VBS；用户无需安装 Node.js，Node 随包提供。源码仅保留原生版，开发检查使用 Cargo 和 Node.js。

## 安装与使用

推荐双击 `CodexUsageMonitor-Setup-<版本>-x64.exe`，保留完成页“立即启动”选项。也可完整解压原生运行包，双击 `CodexUsageMonitor.exe` 确认安装；确认后立即启动安装的工具。

- 安装目录：`%LOCALAPPDATA%\Programs\CodexUsageMonitor`，运行文件按版本及内容指纹隔离。
- 桌面和开始菜单快捷方式直接指向原生 EXE，使用内嵌的项目图标。
- 登录 Windows 后启动监视器，再启动仅绑定 `127.0.0.1` 的 CDP Codex，并自动加载面板。
- 已有 Codex 开放 CDP 时直接连接；没有开放时提示正常退出，再点击新快捷方式。不会强制中断会话。
- 托盘菜单提供打开 Codex、重新连接、启停登录自启动、打开数据目录、恢复原 Codex 自启动、退出监视器。
- API 账户和 API Key 继续在展开面板中配置，密钥通过子进程匿名管道交给 DPAPI，不经过命令参数或日志。

原生入口会自动发现当前用户注册的 Store Codex 和常见非 Store 安装；失败才打开文件选择器。Store CLI 先复制到用户状态目录的版本镜像再执行，绝不直接执行 WindowsApps 内的 CLI。

## 自启动与恢复

启用原生自启动时，只移除当前用户 Run 项或启动文件夹中**可确认指向所发现 Codex EXE** 的条目。修改前备份到 `%LOCALAPPDATA%\CodexUsageMonitor\codex-startup-backup.json` 和 `startup-backup`。

不会按名称模糊删除其他软件，不修改机器级启动项，也不写 Windows 未公开的 StartupApproved 二进制状态。若其他版本使用 Store StartupTask 或机器级自启动，需要用户在 Windows“启动应用”中关闭 Codex；托盘的恢复操作仅恢复本工具实际备份的条目，不覆盖用户后来设置的新条目。

已运行且未开放 CDP 的 Codex 不能无中断接管；工具显示原因，等待用户正常退出。退出监视器仅结束其自己的 Node 子进程，保留 Codex。

## 兼容与更新

复用现有 `%LOCALAPPDATA%\CodexUsageMonitor` 状态、DPAPI entropy、凭据及 Token 账本。安装不重置累计或每日 Token。重新配置账户基准要求可精确表示的非负整数，当前 JS 统计引擎上限为 `9007199254740991`，超出会明确拒绝。

自动更新仍需用户开启，沿用每天检查、正式 Release、精确包名、GitHub SHA-256、受限下载及安全解压。原生安装器只接受包含原生 EXE、私有 Node 和原生白名单的运行包；上游纯脚本包会被拒绝，当前版本继续运行。

压缩包最多 64 MiB、512 个条目，解压总量最多 128 MiB；单文件保持 64 MiB，仅明确的 `runtime/node.exe` 允许最大 100 MiB，以容纳随包 Node。拒绝路径穿越、ADS、保留设备名、重名和符号链接。更新切换版本目录，不覆盖运行中的 EXE，不重启 Codex。

Windows“已安装的应用”中提供卸载入口。卸载恢复备份的自启动，移除原生版白名单程序文件，保留用户数据、用户添加的文件和旧脚本版目录。清理助手在系统临时目录中运行，不需要 PowerShell 或 VBS。

## 开发与验证

需要 Rust MSVC 工具链、Windows SDK、Node.js 22+。不需要 C# 或 .NET。

```text
node scripts/build-native.mjs
node scripts/build-installer.mjs
cargo test --locked --manifest-path native/Cargo.toml
node tests/native-host.mjs
npm test
```

运行包输出到 `dist/native/codex-usage-monitor-windows-<版本>`；Setup、ZIP 和校验文件在 `dist/release`。分发运行包时必须包含整个目录；Setup 是单个安装文件，安装后仍使用私有 Node 和统计引擎。Node 版本及许可需一致，Rust 依赖许可在构建时生成。安装包要求 Inno Setup 6，发布步骤见 [发布指南](publishing.md)。

诊断、安装和恢复命令：

```text
CodexUsageMonitor.exe --diagnose
CodexUsageMonitor.exe --install --enable-autostart
CodexUsageMonitor.exe --disable-autostart
CodexUsageMonitor.exe --restore-codex-startup
```

`--diagnose` 只报告路径、自启动与 CDP 可达状态，不启动 Codex，不读取或返回凭据。
