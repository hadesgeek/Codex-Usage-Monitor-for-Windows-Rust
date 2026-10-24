import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
function run(args) {
  const result = spawnSync(process.execPath, args, { cwd: root, stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Failed: ${args.join(" ")}`);
}
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
for (const file of files.filter(file => /\.(?:js|mjs)$/.test(file))) run(["--check", file]);
run(["tests/source-contract.mjs"]);
const packagedTests = new Set(["run-tests.mjs", "source-contract.mjs", "native-package.mjs", "native-setup.mjs"]);
for (const file of (await fs.readdir(path.join(root, "tests"))).sort()) {
  if (file.endsWith(".mjs") && !packagedTests.has(file)) run([path.join("tests", file)]);
}
for (const file of ["cctq.example.json", "custom.example.json"]) run(["scripts/validate-provider.mjs", path.join("config/providers", file)]);
console.log("PASS: complete native-edition JavaScript suite and provider examples.");
