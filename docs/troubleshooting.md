# 原生版故障排查

## 工具运行但没有监视栏

先检查托盘状态。已有 Codex 如果没有开放 CDP，不能无中断接管；请正常退出 Codex，再从桌面的 Codex Usage Monitor 启动。不要强制结束当前任务。

若 Codex 已开放 CDP，托盘“重新连接”只重建本工具的统计进程，不重启 Codex。端口冲突会自动回退；实际端口在 `%LOCALAPPDATA%\CodexUsageMonitor\state.json`。

原生诊断命令：

```text
CodexUsageMonitor.exe --diagnose
```

它只报告安装路径、发现的程序、自启动和 CDP 可达状态，不返回凭据。`launcher-error.log` 位于用户数据目录；分享前隐藏本机路径和个人信息。

## 快捷方式或安装文件缺失

重新运行官方来源的原生 Setup。它会创建指向 CodexUsageMonitor.exe 的桌面及开始菜单入口。不要复制旧的启动脚本或修改 Codex 程序文件。

如果无法发现 Codex，选择真实的 ChatGPT.exe；缺少官方 CLI 时选择真实的 codex.exe。请确认安装了完整的 Codex Desktop，不要随意选择同名文件。

## 订阅或 API 数据不可用

- 官方订阅使用 Codex 正常登录态，无需额外 Key。检查官方账户是否已登录并等待刷新。
- API 账户和 API Key 在展开面板中配置。按服务商公开文档核对地址、用户 ID、账户令牌或 Key；不要把凭据发送到聊天。
- 接口不兼容时提供公开文档或脱敏响应，不要猜字段。HTTP 429 会退避；失败时保留上次成功值。
- 第三方重置预测失败不代表官方订阅耗尽，不影响本会话统计。

## 自启动与卸载

托盘可切换登录自启动及恢复备份的 Codex 自启动。Store StartupTask 和机器级启动项不在接管范围内，必要时在 Windows 启动应用中手动调整。

从 Windows“已安装的应用”卸载 Codex Usage Monitor (Rust)。默认保留用户数据和加密凭据，不关闭或卸载 Codex。

## 安全软件提示

当前安装包未签名。核对来源和 SHA-256，并查看具体告警，不要关闭安全防护。误隔离后重新运行可信 Setup。

## 开发验证

```text
npm ci
npm test
cargo test --locked --manifest-path native/Cargo.toml
npm run build
```

Node 测试、Cargo 测试和隔离安装成功，不等于真实 Codex 页面已验证；报告问题时请分别说明。
