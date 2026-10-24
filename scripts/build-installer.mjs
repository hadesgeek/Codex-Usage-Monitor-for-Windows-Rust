import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${path.basename(command)} failed with ${result.status}`);
}

async function main() {
  if (!process.argv.includes("--skip-runtime-build")) run(process.execPath, ["scripts/build-native.mjs"]);
  const version = (await fs.readFile(path.join(root, "VERSION"), "utf8")).trim();
  const runtime = path.join(root, "dist/native", `codex-usage-monitor-windows-${version}`);
  const files = JSON.parse(await fs.readFile(path.join(runtime, "config/native-package-files.json"), "utf8"));
  const candidates = [process.env.ISCC_PATH,
    process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, "Programs/Inno Setup 6/ISCC.exe"),
    process.env["ProgramFiles(x86)"] && path.join(process.env["ProgramFiles(x86)"], "Inno Setup 6/ISCC.exe"),
    process.env.ProgramFiles && path.join(process.env.ProgramFiles, "Inno Setup 6/ISCC.exe")].filter(Boolean);
  let compiler;
  for (const candidate of candidates) if (await fs.stat(candidate).then(stat => stat.isFile()).catch(() => false)) { compiler = candidate; break; }
  if (!compiler) {
    const result = spawnSync("where.exe", ["ISCC.exe"], { encoding: "utf8", windowsHide: true });
    compiler = result.status === 0 ? result.stdout.trim().split(/\r?\n/)[0] : null;
  }
  if (!compiler) throw new Error("Install Inno Setup 6 or set ISCC_PATH to its real ISCC.exe.");
  const output = path.join(root, "dist/release");
  await fs.mkdir(output, { recursive: true });
  const entries = path.join(output, "native-files.iss");
  const lines = files.map(relative => {
    if (!relative || relative.includes("\\") || relative.includes('"') || relative.split("/").some(part => !part || part === "." || part === "..")) throw new Error("Unsafe native file entry");
    const parent = path.posix.dirname(relative);
    const directory = "{tmp}\\CodexUsageMonitorPayload" + (parent === "." ? "" : `\\${parent.replaceAll("/", "\\")}`);
    return `Source: "${path.join(runtime, relative)}"; DestDir: "${directory}"; Flags: deleteafterinstall`;
  });
  await fs.writeFile(entries, `\ufeff${lines.join("\r\n")}\r\n`);
  run(compiler, ["/Q", `/DNativeRoot=${runtime}`, `/DNativeFiles=${entries}`, `/DAppVersion=${version}`, "installer/native-setup.iss"]);
  const installer = path.join(output, `CodexUsageMonitor-Setup-${version}-x64.exe`);
  const archive = path.join(output, `codex-usage-monitor-windows-${version}.zip`);
  await fs.copyFile(`${runtime}.zip`, archive);
  const checksums = [];
  for (const file of [installer, archive]) checksums.push(`${createHash("sha256").update(await fs.readFile(file)).digest("hex")}  ${path.basename(file)}`);
  await fs.writeFile(path.join(output, "SHA256SUMS.txt"), `${checksums.join("\n")}\n`);
  run(process.execPath, ["tests/native-setup.mjs", installer, runtime]);
  console.log(`Release installer: ${installer}`);
}

main().catch(error => { console.error(error.message); process.exitCode = 1; });
