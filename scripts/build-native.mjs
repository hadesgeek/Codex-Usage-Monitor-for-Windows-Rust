import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// A project-owned icon, embedded at common Windows DPI sizes; no Codex artwork extraction.
export function createIcon() {
  const sizes = [16, 24, 32, 48, 64, 128, 256];
  const images = sizes.map(size => {
    const maskStride = Math.ceil(size / 32) * 4;
    const bitmap = Buffer.alloc(40 + size * size * 4 + maskStride * size);
    bitmap.writeUInt32LE(40, 0); bitmap.writeInt32LE(size, 4); bitmap.writeInt32LE(size * 2, 8);
    bitmap.writeUInt16LE(1, 12); bitmap.writeUInt16LE(32, 14);
    bitmap.writeUInt32LE(size * size * 4, 20);
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const nx = (x + 0.5) / size, ny = (y + 0.5) / size;
      const rounded = Math.hypot(Math.max(0, Math.abs(nx - 0.5) - 0.27), Math.max(0, Math.abs(ny - 0.5) - 0.27)) < 0.16;
      const bar = [[0.25, 0.55], [0.44, 0.37], [0.63, 0.23]].some(([left, top]) => nx >= left && nx <= left + 0.12 && ny >= top && ny <= 0.76);
      const color = bar ? [84, 223, 185, 255] : rounded ? [24, 36, 48, 255] : [0, 0, 0, 0];
      const offset = 40 + ((size - 1 - y) * size + x) * 4;
      bitmap.set([color[2], color[1], color[0], color[3]], offset);
      if (!color[3]) bitmap[40 + size * size * 4 + (size - 1 - y) * maskStride + (x >> 3)] |= 0x80 >> (x % 8);
    }
    return bitmap;
  });
  const header = Buffer.alloc(6 + 16 * sizes.length);
  header.writeUInt16LE(1, 2); header.writeUInt16LE(sizes.length, 4);
  let offset = header.length;
  sizes.forEach((size, index) => {
    const start = 6 + index * 16;
    header[start] = size % 256; header[start + 1] = size % 256;
    header.writeUInt16LE(1, start + 4); header.writeUInt16LE(32, start + 6);
    header.writeUInt32LE(images[index].length, start + 8); header.writeUInt32LE(offset, start + 12);
    offset += images[index].length;
  });
  return Buffer.concat([header, ...images]);
}

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited with ${result.status}`);
}

async function writeRustLicenses() {
  const result = spawnSync("cargo", ["metadata", "--locked", "--format-version", "1", "--manifest-path", "native/Cargo.toml"], { cwd: root, encoding: "utf8", windowsHide: true, maxBuffer: 16 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error("Cannot resolve Rust dependency licenses.");
  const packages = JSON.parse(result.stdout).packages.filter(item => item.source).sort((a, b) => `${a.name}-${a.version}`.localeCompare(`${b.name}-${b.version}`));
  const notices = ["Rust dependency notices (generated from Cargo.lock).\nBuild-time dependencies are included for completeness.\n"];
  for (const item of packages) {
    const directory = path.dirname(item.manifest_path);
    const names = (await fs.readdir(directory, { withFileTypes: true })).filter(entry => entry.isFile() && /^(?:licen[cs]e|copying|copyright|notice)(?:[._-].*)?$/i.test(entry.name)).map(entry => entry.name);
    if (item.license_file && !names.includes(item.license_file)) names.push(item.license_file);
    if (!names.length) throw new Error(`Missing license text for ${item.name} ${item.version}.`);
    notices.push(`\n${"=".repeat(72)}\n${item.name} ${item.version}\nSPDX: ${item.license || "see license text"}\n`);
    for (const name of names) {
      const file = path.resolve(directory, name);
      if (!file.startsWith(`${directory}${path.sep}`)) throw new Error("Dependency license path escapes its crate.");
      notices.push(`\n--- ${name} ---\n${await fs.readFile(file, "utf8")}\n`);
    }
  }
  await fs.writeFile(path.join(root, "runtime/RUST-LICENSES.txt"), notices.join(""));
}

async function main() {
  await fs.writeFile(path.join(root, "native/app.ico"), createIcon());
  if (process.argv.includes("--icon-only")) return;
  run("cargo", ["test", "--locked", "--manifest-path", "native/Cargo.toml"]);
  run("cargo", ["build", "--locked", "--release", "--manifest-path", "native/Cargo.toml"]);
  await writeRustLicenses();
  const licensedNode = (await fs.readFile(path.join(root, "runtime/node-version.txt"), "utf8")).trim();
  if (licensedNode !== process.versions.node) throw new Error(`Node license is pinned to ${licensedNode}; update runtime/NODE-LICENSE.txt and node-version.txt before bundling ${process.versions.node}.`);
  const version = (await fs.readFile(path.join(root, "VERSION"), "utf8")).trim();
  const output = path.join(root, "dist/native", `codex-usage-monitor-windows-${version}`);
  const files = JSON.parse(await fs.readFile(path.join(root, "config/native-package-files.json"), "utf8"));
  const target = process.env.CARGO_TARGET_DIR ? path.resolve(root, process.env.CARGO_TARGET_DIR) : path.join(root, "native/target");
  for (const relative of files) {
    const source = relative === "CodexUsageMonitor.exe" ? path.join(target, "release/CodexUsageMonitor.exe")
      : relative === "runtime/node.exe" ? process.execPath : path.join(root, relative);
    const destination = path.join(output, relative);
    await fs.mkdir(path.dirname(destination), { recursive: true });
    await fs.copyFile(source, destination);
  }
  run(path.join(output, "CodexUsageMonitor.exe"), ["--validate-package"]);
  run(path.join(output, "CodexUsageMonitor.exe"), ["--package", output, `${output}.zip`]);
  run(process.execPath, [path.join(root, "tests/native-package.mjs"), output]);
  console.log(`Native package: ${output}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
