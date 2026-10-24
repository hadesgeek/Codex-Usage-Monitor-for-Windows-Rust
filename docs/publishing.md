# 构建与手动发布

## 准备源码

先阅读根目录 `AGENTS.md` 和 `README.md`。本工作区来自下载的源码；仓库地址由发布者自行决定，不会自动创建远程仓库或上传文件。

提交源码及锁文件，保留 `LICENSE` 和 `NOTICE.md`。不要提交 `node_modules`、`native/target`、`dist`、`artifacts`、本机路径、日志、账本或 `*.local.json`。不要把 API 密钥写入 Git、JSON、聊天或构建参数。

## 本机构建

要求 Windows x64、Rust MSVC 工具链与 Windows SDK、Inno Setup 6、`runtime/node-version.txt` 指定版本的 Node。`runtime/NODE-LICENSE.txt` 必须匹配该 Node 版本。更换运行时应同步版本与许可。

```text
npm ci
cargo test --locked --manifest-path native/Cargo.toml
cargo clippy --locked --manifest-path native/Cargo.toml --all-targets -- -D warnings
node tests/native-host.mjs
npm test
npm run build
```

编译器不在常见位置时，将 `ISCC_PATH` 设置为实际 `ISCC.exe`。构建脚本执行原生运行包与 Setup 的隔离安装/卸载测试，不启动或终止真实 Codex。源码不包含旧版部署脚本；开发测试使用 Node.js 和 Cargo，安装器构建使用 Inno Setup。

产物位于 `dist/release/`：

| 文件 | 用途 |
| --- | --- |
| `CodexUsageMonitor-Setup-<版本>-x64.exe` | 普通用户安装入口；默认完成安装后立即启动。 |
| `codex-usage-monitor-windows-<版本>.zip` | 原生运行包、手动安装及原生自动更新资产。必须完整解压。 |
| `SHA256SUMS.txt` | 上述两个文件的 SHA-256。 |

不要上传中间文件 `native-files.iss`，其中包含构建机路径。当前产物未签名；可在自己的签名流程完成后重新生成校验值。

## 上传到 GitHub

1. 将源码推送到本仓库的远程地址 `hadesgeek/Codex-Usage-Monitor-for-Windows-Rust`，推送前审核待提交文件。README 使用相对文档链接。
2. 按 `VERSION` 创建对应 `v<版本>` 标签，手动创建 Release。
3. 上传上表三个文件，写明已验证系统、Codex 版本、功能和已知限制。
4. 在新用户或干净虚拟机中验证发现 Codex、安装后启动、面板、订阅、重装和卸载。已有 Codex 未开放 CDP 时仍需用户正常退出一次。

`.github/workflows/release.yml` 仅支持手动触发，生成可下载的构建产物，不创建 Release、不推送代码。GitHub 托管环境可能需要下载编译依赖；用户安装时不下载运行依赖。

## 版本与更新源

当前版本信息存在于 `VERSION`、`package.json`/锁文件、Rust Cargo 清单/锁文件、原生版本常量/资源和面板版本标记中。升级时同步这些已有位置并运行完整测试，避免包名、资源和显示版本不同。不要只改安装包文件名。

`scripts/auto-updater.mjs` 中的 `RELEASE_API_URL` 和 `REPOSITORY_PATH` 已指向本仓库 `hadesgeek/Codex-Usage-Monitor-for-Windows-Rust`。再次分叉发布时需一起修改它们，以及 `tests/auto-updater.mjs` 内对应测试地址，再运行完整测试。不能只改 API 地址而保留旧资产路径校验，也不能放宽为任意下载 URL。

更新仍必须满足：用户主动开启、至多每 24 小时检查、正式非草稿 Release、更高语义版本、精确 ZIP 包名、GitHub 资产 `digest` 中的 SHA-256、安全 ZIP 路径和体积限制。原生版拒绝纯脚本包。更新失败保留当前工具，不能重启或终止 Codex。

在对应的正式 Release 发布之前，保持自动更新关闭，以手动 Setup 升级。`SHA256SUMS.txt` 供人工校验，不能代替自动更新要求的 GitHub 资产 digest。
