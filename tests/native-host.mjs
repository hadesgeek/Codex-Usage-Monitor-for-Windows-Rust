import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import http from "node:http";
import { panelCommand, reloadCommand } from "../scripts/native-host.mjs";
import { configureFromPanel, providerFromPanel } from "../scripts/native-config.mjs";
import { createIcon } from "../scripts/build-native.mjs";

const root = await fs.mkdtemp(path.join(os.tmpdir(), "codex-native-config-"));
const host = path.join(root, "CodexUsageMonitor.exe");
const environment = { LOCALAPPDATA: root, CODEX_USAGE_NATIVE_HOST: host };
const secrets = [];
const save = async (kind, value) => { secrets.push({ kind, value }); };
const token = "unit-test-native-not-a-real-key";
let redirect = false;
let invalidUsage = false;
const server = http.createServer((request, response) => {
  if (redirect) { response.writeHead(302, { Location: "http://localhost:1/blocked" }); response.end(); return; }
  response.setHeader("Content-Type", "application/json");
  if (request.url.startsWith("/api/user/self")) { response.end(JSON.stringify({ success: true, data: { quota: 100 } })); return; }
  if (request.url.startsWith("/api/log/self")) {
    const page = Number(new URL(request.url, "http://localhost").searchParams.get("p"));
    response.end(JSON.stringify({ data: { items: [{ request_id: `request-${page}`, created_at: Math.floor(Date.now() / 1000), prompt_tokens: 10, completion_tokens: 5 }], page_size: 1, total: 2 } }));
    return;
  }
  response.end(JSON.stringify(invalidUsage ? { unknown: true } : { data: { total_used: 10, total_granted: 100 } }));
});
await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
const baseUrl = `http://127.0.0.1:${server.address().port}`;
const provider = { id: "native-test", label: "Native Test", baseUrl, usagePath: "/usage", statusPath: "", authHeader: "Authorization", authScheme: "Bearer", usageRoot: "data", statusRoot: "data", used: "total_used", limit: "total_granted", unlimited: "", expiresAt: "", quotaPerUnit: "", currency: "", defaultQuotaPerUnit: 1, defaultCurrency: "USD" };

try {
  assert.equal(panelCommand(root, environment).command, process.execPath);
  assert.deepEqual(reloadCommand(root, 9335, environment), { command: host, args: ["--reload"] });
  for (const command of [panelCommand(root, environment), reloadCommand(root, 9335, environment)]) {
    assert.doesNotMatch(JSON.stringify(command), /powershell|pwsh|wscript|\.ps1/i);
  }
  assert.throws(() => panelCommand(root, {}), /CodexUsageMonitor.exe/);
  assert.throws(() => reloadCommand(root, 9335, {}), /CodexUsageMonitor.exe/);
  const icon = createIcon();
  assert.equal(icon.readUInt16LE(2), 1);
  assert.equal(icon.readUInt16LE(4), 7);
  assert.equal(icon.readUInt32LE(6 + 6 * 16 + 12) + icon.readUInt32LE(6 + 6 * 16 + 8), icon.length);
  assert.throws(() => providerFromPanel({ ...provider, baseUrl: "http://example.com" }));
  assert.throws(() => providerFromPanel({ ...provider, usagePath: "https://example.com/steal" }));
  await assert.rejects(configureFromPanel({ requestId: "invalid", type: "api-key" }, { environment, save }));

  const request = { requestId: "native-account-123", type: "api-account", baseUrl, userId: "123", initialTokens: "1000", token };
  assert.equal((await configureFromPanel(request, { environment, save })).ok, true);
  const state = path.join(root, "CodexUsageMonitor");
  const counter = JSON.parse(await fs.readFile(path.join(state, "account-token-counter.json"), "utf8"));
  assert.equal(counter.totalTokens, 1000);
  assert.equal(counter.dailyTokens, 30);
  assert.equal(counter.recentLogIds.length, 2);
  assert.equal(counter.baselineConfigured, true);
  assert.equal(secrets[0].kind, "account");
  await assert.rejects(configureFromPanel({ ...request, initialTokens: "9007199254740992" }, { environment, save }));
  await assert.rejects(configureFromPanel({ ...request, userId: "abc" }, { environment, save }));

  const keyRequest = { requestId: "native-provider-123", type: "api-key", provider, apiKey: token };
  await configureFromPanel(keyRequest, { environment, save });
  assert.equal(secrets[1].kind, "provider");
  const savedProvider = await fs.readFile(path.join(state, "provider.json"), "utf8");
  assert.ok(!savedProvider.includes(token));
  assert.equal(JSON.parse(savedProvider).requests.usagePath, "/usage");
  await assert.rejects(configureFromPanel(keyRequest, { environment, save: async () => {
    await fs.writeFile(path.join(state, "api-key.dpapi"), "encrypted-fixture");
    throw new Error("simulated storage failure");
  } }));
  assert.equal(await fs.readFile(path.join(state, "provider.json"), "utf8"), savedProvider);
  await assert.rejects(fs.stat(path.join(state, "api-key.dpapi")), { code: "ENOENT" });
  invalidUsage = true;
  await assert.rejects(configureFromPanel(keyRequest, { environment, save }));
  assert.equal(await fs.readFile(path.join(state, "provider.json"), "utf8"), savedProvider);
  invalidUsage = false;
  redirect = true;
  await assert.rejects(configureFromPanel(keyRequest, { environment, save }));
  assert.equal(secrets.length, 2, "Rejected remote responses must not persist credentials");
  for (const name of await fs.readdir(state)) assert.ok(!(await fs.readFile(path.join(state, name), "utf8")).includes(token), "No plaintext credentials in JSON");
  console.log("PASS: native command routing, icon, provider validation, pagination, token baseline, redirect rejection and credential boundaries.");
} finally {
  await new Promise(resolve => server.close(resolve));
  await fs.rm(root, { recursive: true, force: true });
}
