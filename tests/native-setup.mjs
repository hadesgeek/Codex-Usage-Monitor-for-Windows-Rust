import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";

const installer = path.resolve(process.argv[2]);
const runtime = path.resolve(process.argv[3]);
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), "codex-native-setup-"));
const target = path.join(temporary, "install");
const existing = process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, "Programs/CodexUsageMonitor/current.json");
const before = existing ? await fs.readFile(existing).catch(() => null) : null;
const digest = async file => createHash("sha256").update(await fs.readFile(file)).digest("hex");
const setup = () => spawnSync(installer, ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/SP-", "/NORESTART", `/NATIVEINSTALLROOT=${target}`, `/LOG=${path.join(temporary, "setup.log")}`], { windowsHide: true, timeout: 120000, encoding: "utf8" });
try {
  assert.equal(setup().status, 0, "Setup EXE failed in the isolated installation directory");
  const pointer = JSON.parse(await fs.readFile(path.join(target, "current.json"), "utf8"));
  const installed = path.join(target, pointer.directory);
  const files = JSON.parse(await fs.readFile(path.join(installed, "config/native-package-files.json"), "utf8"));
  for (const relative of files) assert.equal(await digest(path.join(installed, relative)), await digest(path.join(runtime, relative)), relative);
  const custom = path.join(installed, "private.local.json");
  await fs.writeFile(custom, JSON.stringify({ fixture: "preserve" }));
  assert.equal(setup().status, 0, "Setup upgrade failed");
  assert.equal(await fs.readFile(custom, "utf8"), '{"fixture":"preserve"}');
  const result = spawnSync(path.join(runtime, "CodexUsageMonitor.exe"), ["--uninstall", "--install-root", target, "--quiet"], { windowsHide: true, encoding: "utf8", timeout: 60000 });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  for (const relative of files) await assert.rejects(fs.stat(path.join(installed, relative)), { code: "ENOENT" });
  await assert.rejects(fs.stat(path.join(target, "CodexUsageMonitor.exe")), { code: "ENOENT" });
  assert.equal(await fs.readFile(custom, "utf8"), '{"fixture":"preserve"}');
  assert.deepEqual(existing ? await fs.readFile(existing).catch(() => null) : null, before, "Isolated installer tests must not change the real installation");
  console.log("PASS: single-EXE setup, complete runtime hashes, setup upgrade, native uninstall and user-file preservation; real installation untouched.");
} finally {
  assert.equal(path.dirname(temporary), os.tmpdir());
  assert.ok(path.basename(temporary).startsWith("codex-native-setup-"));
  await fs.rm(temporary, { recursive: true, force: true });
}
