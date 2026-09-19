// Loopback-only development service; all credentials/state stay in ignored .runtime.
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { closeSync, existsSync, lstatSync, mkdirSync, openSync, readFileSync, readlinkSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "node:net";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const runtime = join(root, ".runtime", "local-service");
const binary = join(root, "target/debug/host-monitoring-server");
const recordPath = join(runtime, "process.json");
const address = "http://127.0.0.1:18105";

function validAuthorizationKey(value) {
  if (typeof value !== "string") return false;
  const decoded = Buffer.from(value, "base64");
  return decoded.length === 32 && decoded.toString("base64") === value;
}

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
  if (!response.ok || JSON.stringify(await response.json()) !== '{"ready":true}') throw new Error("Service is not ready");
}
const command = process.argv[2] ?? "status";
if (!["start", "status", "stop"].includes(command)) throw new Error("Usage: node scripts/local-service.mjs start|status|stop");
privateDirectory(join(root, ".runtime"));
privateDirectory(runtime);
const existing = running();
if (command === "stop") {
  if (existing) {
    process.kill(existing.pid, "SIGTERM");
    for (let attempt = 0; attempt < 150 && running(); attempt++) await new Promise(done => setTimeout(done, 100));
    if (running()) throw new Error("Service has not stopped; no forced termination was performed");
  }
  console.log("Local Host Monitoring stopped");
} else if (existing) {
  await ready();
  console.log(`Host Monitoring ready: ${address} (PID ${existing.pid})`);
} else if (command === "status") {
  console.log("Local Host Monitoring is not running");
  process.exitCode = 1;
} else {
  const listener = createServer();
  await new Promise((done, fail) => { listener.once("error", fail); listener.listen(18105, "127.0.0.1", done); });
  await new Promise(done => listener.close(done));
  privateDirectory(join(runtime, "db"));
  const secretsPath = join(runtime, "credentials.json");
  const databasePath = join(runtime, "db", "host-monitoring.sqlite3");
  let credentials;
  try {
    credentials = readPrivate(secretsPath);
    if (!validAuthorizationKey(credentials.clientAuthorizationKey)) {
      if (existsSync(databasePath)) {
        throw new Error("Local credentials are missing the valid client authorization key for the existing database; restore the original key before starting");
      }
      credentials.clientAuthorizationKey = randomBytes(32).toString("base64");
      writeFileSync(secretsPath, JSON.stringify(credentials) + "\n", { mode: 0o600 });
    }
  }
  catch (error) {
    if (error.code !== "ENOENT") throw error;
    credentials = {
      username: "admin",
      password: randomBytes(24).toString("base64url"),
      clientAuthorizationKey: randomBytes(32).toString("base64"),
    };
    writeFileSync(secretsPath, JSON.stringify(credentials) + "\n", { mode: 0o600, flag: "wx" });
    writeFileSync(join(runtime, "login.txt"), `URL: ${address}\nUsername: ${credentials.username}\nPassword: ${credentials.password}\n`, { mode: 0o600, flag: "wx" });
  }
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith("HOST_MONITORING_")));
  Object.assign(env, {
    HOST_MONITORING_BIND: "127.0.0.1:18105", HOST_MONITORING_DEVELOPMENT: "true",
    HOST_MONITORING_DATABASE_URL: `sqlite://${databasePath}`,
    HOST_MONITORING_STATIC_DIR: join(root, "clients/web/dist"),
    HOST_MONITORING_BOOTSTRAP_ADMIN_USERNAME: credentials.username,
    HOST_MONITORING_BOOTSTRAP_ADMIN_PASSWORD: credentials.password,
    HOST_MONITORING_CLIENT_AUTHORIZATION_KEY: credentials.clientAuthorizationKey,
    RUST_LOG: "info",
  });
  const logPath = join(runtime, "server.log");
  try {
    const metadata = lstatSync(logPath);
    if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.nlink !== 1 || (metadata.mode & 0o077) !== 0) throw new Error("Unsafe service log");
  } catch (error) { if (error.code !== "ENOENT") throw error; }
  const log = openSync(logPath, "a", 0o600);
  const child = spawn(binary, ["serve"], { cwd: runtime, env, detached: true, stdio: ["ignore", log, log] });
  closeSync(log);
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
    throw new Error(`Local service startup failed; inspect ${logPath}`);
  }
  console.log(`Host Monitoring ready: ${address} (PID ${record.pid})`);
  console.log(`Private login details: ${join(runtime, "login.txt")}`);
}
