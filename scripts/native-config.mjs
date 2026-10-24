import fs from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { spawn } from "node:child_process";
import { ApiAccountUsageClient, ApiUsageClient, normalizeCredentialBaseUrl, normalizeApiUsageView, validateApiProviderConfig } from "./usage-client.mjs";

const MAX_INPUT_BYTES = 131072;

async function writeAtomic(file, value) {
  return writeAtomicBytes(file, Buffer.from(JSON.stringify(value)));
}

async function writeAtomicBytes(file, bytes) {
  await fs.mkdir(path.dirname(file), { recursive: true });
  const temporary = `${file}.${process.pid}.tmp`;
  try { await fs.writeFile(temporary, bytes, { flag: "wx" }); await fs.rename(temporary, file); }
  finally { await fs.rm(temporary, { force: true }).catch(() => {}); }
}

async function persist(root, kind, secret, config, counter, save, host) {
  const files = [kind === "account" ? "account-token.dpapi" : "api-key.dpapi", kind === "account" ? "account.json" : "provider.json"];
  if (counter) files.push("account-token-counter.json");
  const previous = await Promise.all(files.map(async name => {
    try { return await fs.readFile(path.join(root, name)); }
    catch (error) { if (error.code === "ENOENT") return null; throw error; }
  }));
  try {
    await save(kind, secret, host);
    await writeAtomic(path.join(root, files[1]), config);
    if (counter) await writeAtomic(path.join(root, files[2]), counter);
  } catch (error) {
    for (let index = 0; index < files.length; index++) {
      const file = path.join(root, files[index]);
      if (previous[index] === null) await fs.rm(file, { force: true });
      else await writeAtomicBytes(file, previous[index]);
    }
    throw error;
  }
}

function credential(value, saved) {
  const result = value || saved;
  if (typeof result !== "string" || !/^[\x21-\x7e]{1,16384}$/.test(result)) throw new Error("凭据必须为完整的单行 ASCII 文本。");
  return result;
}

function saveSecret(kind, secret, host) {
  return new Promise((resolve, reject) => {
    const child = spawn(host, ["--save-secret", kind], { windowsHide: true, stdio: ["pipe", "ignore", "ignore"] });
    const timer = setTimeout(() => { child.kill(); reject(new Error("凭据保存超时。")); }, 10000);
    child.once("error", () => { clearTimeout(timer); reject(new Error("无法启动本机凭据服务。")); });
    child.once("close", code => { clearTimeout(timer); code === 0 ? resolve() : reject(new Error("Windows DPAPI 保存失败。")); });
    child.stdin.on("error", () => {});
    child.stdin.end(secret);
  });
}

export function providerFromPanel(source) {
  if (!source || typeof source !== "object") throw new Error("Provider 配置不能为空。");
  return validateApiProviderConfig({
    schemaVersion: 1, id: source.id, label: source.label,
    baseUrl: normalizeCredentialBaseUrl(source.baseUrl),
    requests: { usagePath: source.usagePath, statusPath: source.statusPath || null },
    auth: { header: source.authHeader, scheme: source.authScheme },
    response: Object.fromEntries(["usageRoot", "statusRoot", "used", "limit", "unlimited", "expiresAt", "quotaPerUnit", "currency", "defaultQuotaPerUnit", "defaultCurrency"].map(key => [key, source[key]])),
  });
}

export async function configureFromPanel(request, {
  environment = process.env,
  save = saveSecret,
  AccountClient = ApiAccountUsageClient,
  KeyClient = ApiUsageClient,
} = {}) {
  if (!/^[a-z0-9-]{8,64}$/.test(request?.requestId || "") || !["api-account", "api-key"].includes(request?.type)) throw new Error("配置请求无效。");
  if (!environment.LOCALAPPDATA || !environment.CODEX_USAGE_NATIVE_HOST) throw new Error("原生配置服务不可用。");
  const root = path.join(environment.LOCALAPPDATA, "CodexUsageMonitor");
  if (request.type === "api-account") {
    const baseUrl = normalizeCredentialBaseUrl(request.baseUrl, "API 账户 Base URL");
    const userId = String(request.userId || "");
    if (!/^[1-9][0-9]{0,19}$/.test(userId)) throw new Error("用户 ID 必须是 1–20 位正整数。");
    if (!/^(0|[1-9][0-9]*)$/.test(String(request.initialTokens))) throw new Error("累计 Token 基准必须是非负完整整数。");
    const initialTokens = Number(request.initialTokens);
    if (!Number.isSafeInteger(initialTokens)) throw new Error("累计 Token 超过当前统计引擎可精确表示的范围。");
    const token = credential(request.token, environment.CODEX_USAGE_ACCOUNT_TOKEN);
    const client = new AccountClient({ baseUrl, userId, token });
    // Build a checkpoint with the existing deduplication algorithm, without touching the live ledger.
    client.counter = null;
    client.counterPath = null;
    const profile = await client.requestJson("/api/user/self");
    if (!profile?.data || typeof profile.data !== "object" || profile.success === false) throw new Error("API 账户响应格式不匹配，请核对服务商文档。");
    const logs = await client.readLogs();
    if (!logs.complete) throw new Error("账户日志未完整读取，未修改原有配置或 Token 账本。");
    client.updateCounter(logs.items);
    const counter = { ...client.counter, baselineConfigured: true, initialTokens, totalTokens: initialTokens };
    await persist(root, "account", token, { schemaVersion: 1, baseUrl, userId }, counter, save, environment.CODEX_USAGE_NATIVE_HOST);
  } else {
    const apiKey = credential(request.apiKey, environment.CODEX_USAGE_API_KEY);
    const provider = providerFromPanel(request.provider);
    const client = new KeyClient({ provider, apiKey });
    const usage = await client.requestJson(new URL(provider.requests.usagePath, `${provider.baseUrl}/`));
    const status = provider.requests.statusPath ? await client.requestJson(new URL(provider.requests.statusPath, `${provider.baseUrl}/`)) : null;
    const normalized = normalizeApiUsageView(usage, status, provider);
    if (normalized.status !== "ready" || normalized.metrics.find(metric => metric.id === "usedAmount")?.value === "--") {
      throw new Error("用量响应与字段映射不匹配，请核对服务商文档。");
    }
    await persist(root, "provider", apiKey, provider, null, save, environment.CODEX_USAGE_NATIVE_HOST);
  }
  return { ok: true, message: "配置已验证，并使用 Windows DPAPI 安全保存。" };
}

async function main() {
  let bytes = 0;
  const chunks = [];
  try {
    for await (const chunk of process.stdin) {
      bytes += chunk.length;
      if (bytes > MAX_INPUT_BYTES) throw new Error("配置内容过大。");
      chunks.push(chunk);
    }
    const request = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    const result = await configureFromPanel(request);
    process.stdout.write(`${JSON.stringify(result)}\n`);
  } catch {
    // Upstream errors may contain URLs or response text; never echo them into pipes/logs/UI.
    process.stdout.write(`${JSON.stringify({ ok: false, message: "配置未成功保存。请检查服务地址、凭据、字段映射及累计 Token，确认接口可用后重试。" })}\n`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) main();
