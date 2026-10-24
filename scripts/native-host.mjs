import path from "node:path";

export function panelCommand(root, environment = process.env, node = process.execPath) {
  if (!environment.CODEX_USAGE_NATIVE_HOST) throw new Error("Start the monitor through CodexUsageMonitor.exe.");
  return { command: node, args: [path.join(root, "scripts", "native-config.mjs")] };
}

export function reloadCommand(root, port, environment = process.env) {
  if (!environment.CODEX_USAGE_NATIVE_HOST) throw new Error("Start the monitor through CodexUsageMonitor.exe.");
  return { command: environment.CODEX_USAGE_NATIVE_HOST, args: ["--reload"] };
}
