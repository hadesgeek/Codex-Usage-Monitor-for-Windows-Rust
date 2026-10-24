import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
async function walk(directory) {
  const files = [];
  for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
    if (["node_modules", "target", "dist", ".git"].includes(entry.name)) continue;
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await walk(file));
    else if (entry.isFile()) files.push(file);
  }
  return files;
}
const files = await walk(root);
assert.ok(!files.some(file => /\.(?:ps1|vbs|cmd|bat|sh|py)$/i.test(file)), "Native source must not contain shell deployment scripts");
for (const file of files.filter(file => file.includes(`${path.sep}scripts${path.sep}`) && file.endsWith(".mjs"))) {
  assert.ok(!/pwsh(?:\.exe)?|powershell(?:\.exe)?|wscript(?:\.exe)?|\.ps1\b|\.vbs\b/i.test(await fs.readFile(file, "utf8")), `Legacy runtime dependency in ${path.relative(root, file)}`);
}
const version = (await fs.readFile(path.join(root, "VERSION"), "utf8")).trim();
for (const file of ["package.json", "package-lock.json"]) assert.equal(JSON.parse(await fs.readFile(path.join(root, file), "utf8")).version, version);
assert.match(await fs.readFile(path.join(root, "assets/usage-constants.js"), "utf8"), new RegExp(`VERSION:\\s*"${version.replaceAll(".", "\\.")}"`));
for (const file of files.filter(file => file.endsWith(".md"))) {
  const content = await fs.readFile(file, "utf8");
  for (const match of content.matchAll(/\]\(([^)]+)\)/g)) {
    const link = match[1];
    if (/^(?:https?:|#)/.test(link)) continue;
    assert.ok(await fs.stat(path.resolve(path.dirname(file), link.split("#")[0])).catch(() => null), `Broken link in ${path.relative(root, file)}: ${link}`);
  }
}
const manifest = JSON.parse(await fs.readFile(path.join(root, "config/native-package-files.json"), "utf8"));
for (const file of manifest) {
  assert.ok(!/\.(?:ps1|vbs|cmd|bat)$/i.test(file));
  if (!["CodexUsageMonitor.exe", "runtime/node.exe", "runtime/RUST-LICENSES.txt"].includes(file)) assert.ok((await fs.stat(path.join(root, file))).isFile(), file);
}
console.log("PASS: native-only source, script-free runtime, versions, document links and package inputs.");
