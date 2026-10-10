// Explicit local initialization; normal start/status never create state or credentials.
import { spawn, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { lstatSync, mkdirSync, readFileSync, readlinkSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "node:net";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const runtime = join(root, ".runtime", "local-service");
const binary = process.env.XCSS_LOCAL_SERVER_BINARY ?? join(root, "target/x86_64-unknown-linux-gnu/debug/xsos");
if (!isAbsolute(binary)) throw new Error("XCSS_LOCAL_SERVER_BINARY must be absolute");
const recordPath = join(runtime, "process.json");
const configPath = join(runtime, "server.json");
const address = "http://127.0.0.1:18105";
function privateDirectory(path) {
  mkdirSync(path, { mode: 0o700, recursive: true });
  const metadata = lstatSync(path);
  if (!metadata.isDirectory() || metadata.isSymbolicLink() || (metadata.mode & 0o077) !== 0 || metadata.uid !== process.getuid()) throw new Error(`Not a private owned directory: ${path}`);
}
function readPrivate(path) {
  const metadata = lstatSync(path);
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.nlink !== 1 || (metadata.mode & 0o077) !== 0 || metadata.uid !== process.getuid()) throw new Error(`Not a private owned file: ${path}`);
  return JSON.parse(readFileSync(path, "utf8"));
}
function birth(pid) {
  const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
  return stat.slice(stat.lastIndexOf(")") + 2).split(" ")[19];
}
function running() {
  try {
    const record = readPrivate(recordPath);
    if (!Number.isSafeInteger(record.pid) || record.pid <= 1) throw new Error("Invalid service PID");
    const executable = readlinkSync(`/proc/${record.pid}/exe`);
    const expected = realpathSync(binary);
    // A rebuild may unlink the old executable while its process is still alive.
    // Require the recorded birth time as well as the exact executable path.
    return (executable === expected || executable === `${expected} (deleted)`)
      && birth(record.pid) === record.birth ? record : null;
  } catch (error) {
    if (error.code === "ENOENT" || error.code === "ESRCH") return null;
    throw error;
  }
}
async function ready() {
  const response = await fetch(`${address}/readyz`, { signal: AbortSignal.timeout(1500), redirect: "error" });
  if (!response.ok || response.headers.get("x-service") !== "xsos" || JSON.stringify(await response.json()) !== '{"ready":true}') throw new Error("Service is not ready");
}
const command = process.argv[2] ?? "status";
const options = process.argv.slice(3);
const directoryWeb = options.length === 1 && options[0] === "--directory-web";
if (!["init", "start", "status", "stop"].includes(command) || (options.length && (!directoryWeb || command !== "start"))) throw new Error("Usage: node scripts/local-service.mjs init|start [--directory-web]|status|stop");
const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith("XSOS_") && name !== "XCSS_DEV_WEB_DIR"));
if (directoryWeb) env.XCSS_DEV_WEB_DIR = join(root, "web/dist");
function invoke(arguments_) {
  const result = spawnSync(binary, arguments_, { cwd: root, env, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Server command failed with exit ${result.status}`);
  return result.stdout;
}
const existing = running();
if (command === "init") {
  if (existing) throw new Error("Stop the local service before initialization");
  privateDirectory(join(root, ".runtime"));
  privateDirectory(runtime);
  const config = {
    "data_dir": join(runtime, "data"),
    "bind": "127.0.0.1:18105",
    "bootstrap_admin_username": "admin",
    "bootstrap_admin_password": randomBytes(24).toString("base64url"),
    "client_authorization_key": randomBytes(32).toString("base64"),
    "development": true
};
  writeFileSync(configPath, JSON.stringify(config, null, 2) + "\n", { mode: 0o600, flag: "wx" });
  invoke(["init", "--config", configPath, "--json"]);
  writeFileSync(join(runtime, "login.txt"), `URL: ${address}\nUsername: ${config.bootstrap_admin_username}\nPassword: ${config.bootstrap_admin_password}\n`, { mode: 0o600, flag: "wx" });
  console.log(`Local xsos initialized; private login details: ${join(runtime, "login.txt")}`);
} else if (command === "stop") {
  if (existing) {
    process.kill(existing.pid, "SIGTERM");
    for (let attempt = 0; attempt < 150 && running(); attempt++) await new Promise(done => setTimeout(done, 100));
    if (running()) throw new Error("Service has not stopped; no forced termination was performed");
  }
  console.log("Local xsos stopped");
} else if (existing) {
  readPrivate(configPath);
  await ready();
  console.log(`xsos ready: ${address} (PID ${existing.pid})`);
} else if (command === "status") {
  console.log("Local xsos is not running");
  process.exitCode = 1;
} else {
  readPrivate(configPath);
  invoke(["config", "validate", "--config", configPath, "--json"]);
  const listener = createServer();
  await new Promise((done, fail) => { listener.once("error", fail); listener.listen(18105, "127.0.0.1", done); });
  await new Promise(done => listener.close(done));
  const child = spawn(binary, ["run", "--config", configPath], { cwd: root, env, detached: true, stdio: ["ignore", "ignore", "inherit"] });
  await new Promise((done, fail) => { child.once("spawn", done); child.once("error", fail); });
  const record = { pid: child.pid, birth: birth(child.pid) };
  writeFileSync(recordPath, JSON.stringify(record) + "\n", { mode: 0o600 });
  child.unref();
  let started = false;
  for (let attempt = 0; attempt < 100; attempt++) {
    try { await ready(); started = true; break; }
    catch { if (!running()) break; await new Promise(done => setTimeout(done, 100)); }
  }
  if (!started) {
    if (running()) process.kill(record.pid, "SIGTERM");
    throw new Error(`Local service startup failed; inspect ${join(runtime, "data/logs")}`);
  }
  console.log(`xsos ready: ${address} (PID ${record.pid})`);
  console.log(`Private login details: ${join(runtime, "login.txt")}`);
}
