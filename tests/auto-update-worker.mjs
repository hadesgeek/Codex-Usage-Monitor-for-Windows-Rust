import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { EventEmitter } from "node:events";
import { spawn } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { runAutoUpdateWorker } from "../scripts/auto-update-worker.mjs";

const root = await fs.mkdtemp(path.join(os.tmpdir(), "codex update worker "));
const version = "9.8.7";
const statePath = path.join(root, "update-state.json");
const pending = { schemaVersion: 1, status: "installing", currentVersion: "3.1.6", latestVersion: version };
const writeState = value => fs.writeFile(statePath, JSON.stringify(value));
const readState = async () => JSON.parse(await fs.readFile(statePath, "utf8"));
try {
  for (const scenario of ["zero-without-state", "nonzero", "signal", "missing-executable", "script-error", "installed", "newer-state"]) {
    await writeState(pending);
    const result = await runAutoUpdateWorker({
      statePath, version, nativeHost: "fixture", args: ["--apply-update"],
      spawnImpl(command, args, options) {
        assert.equal(command, "fixture");
        assert.deepEqual(args, ["--apply-update"]);
        assert.equal(options.detached, false);
        assert.equal(options.windowsHide, true);
        assert.equal(options.stdio, "ignore");
        const child = new EventEmitter();
        setImmediate(async () => {
          if (scenario === "script-error") await writeState({ ...pending, status: "error", error: "SHA-256 failed" });
          if (scenario === "installed") await writeState({ status: "installed", currentVersion: version });
          if (scenario === "newer-state") await writeState({ ...pending, latestVersion: "9.8.8" });
          if (scenario === "missing-executable") child.emit("error", new Error("ENOENT"));
          child.emit("close", ["nonzero", "script-error"].includes(scenario) ? 17 : 0, scenario === "signal" ? "SIGTERM" : null);
        });
        return child;
      },
    });
    assert.equal(result, scenario === "installed");
    const state = await readState();
    if (scenario === "installed") assert.equal(state.status, "installed");
    else if (scenario === "newer-state") assert.equal(state.latestVersion, "9.8.8");
    else {
      assert.equal(state.status, "error");
      assert.match(state.error, {
        "zero-without-state": /code 0 without confirming installation/,
        nonzero: /code 17/,
        signal: /SIGTERM/,
        "missing-executable": /could not start.*ENOENT/,
        "script-error": /SHA-256 failed/,
      }[scenario]);
    }
  }

  const workerPath = fileURLToPath(new URL("../scripts/auto-update-worker.mjs", import.meta.url));
  const fixture = path.join(root, "fixture-installer.mjs");
  await fs.writeFile(fixture, `import fs from 'node:fs/promises';
await new Promise(resolve=>setTimeout(resolve,1500));
await fs.writeFile(process.argv[2],JSON.stringify({status:'installed',currentVersion:process.argv[3]}));`);
  await writeState(pending);
  const parentCode = `import {spawn} from 'node:child_process';
const child=spawn(process.execPath,${JSON.stringify([workerPath,statePath,version,process.execPath,fixture,statePath,version])},{detached:true,windowsHide:true,stdio:'ignore'});
await new Promise((resolve,reject)=>{child.once('spawn',resolve);child.once('error',reject)});
child.unref();`;
  const parent = spawn(process.execPath, ["--input-type=module", "-e", parentCode], { windowsHide: true, stdio: "ignore" });
  const exitCode = await new Promise((resolve,reject)=>{parent.once("error",reject);parent.once("close",resolve)});
  assert.equal(exitCode,0);
  assert.equal((await readState()).status,"installing", "Worker must outlive its parent");
  const deadline=Date.now()+12000;
  while ((await readState()).status==="installing" && Date.now()<deadline) await delay(100);
  assert.equal((await readState()).status,"installed");
  assert.equal((await readState()).currentVersion,version);
  console.log("PASS: update worker failure states and installation after parent exit (native command contract).");
} finally {
  await fs.rm(root, { recursive: true, force: true });
}
